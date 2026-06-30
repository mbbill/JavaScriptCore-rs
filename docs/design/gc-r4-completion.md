# Design — R4 completion: leaf-cell GC + `visitWeak` (RATIFIED 2026-06-29)

Extends `gc-r4.md` (object-cell R4a, live). Corrects the overstated "R4 IS DONE": object cells are
arena-managed + swept + auto-triggered, but **leaf cells (String/Symbol/BigInt) never enter the arena
and are never swept** (a real leak + faithfulness divergence vs JSC, which GCs strings), and the live
collector has **no `visitWeak`** (gates GC-safe `CallLinkInfo` callee caching = STEP-2's fast path).

## Orienting fact (reshapes the work)

The **object→leaf mark edge already exists and is correct.** `trace_cell` (src/interpreter/object_store.rs
:2495-2616) enumerates every outgoing `RuntimeValue` slot (inline + butterfly props/elements + all aux
slabs) → `trace_value_edge` → `RuntimeValue::as_cell`; a String/Symbol/BigInt value IS already appended
as an edge. The live marker `ObjectEdgeMarker::visit_cell_edge` drops it **only because the membership
gate `MarkedSpace::is_arena_cell` rejects a boxed leaf-cell address** (object_store.rs:2694-2698). So the
leak is fixed by **moving leaf cells into the arena** (so `is_arena_cell` admits them) — NOT by rewriting
the tracer. The danger is that the marker + reconcile **hard-cast every arena cell to `CoreObjectCell`**
(object_store.rs:2728-2733, 3049): that cast MUST become type-dispatched first (SD-2/U0).

Faithful confirmations: property NAMES are NOT GC edges (JSC keeps `UniquedStringImpl`/`SymbolImpl` uids
alive by PropertyTable refcount, never marking — Structure::visitChildren appends only the table cell);
the Rust "DELIBERATELY NOT VISITED: structure_id property names" (object_store.rs:2609-2613) is faithful.
The ONLY missing tracer edge is the **rope fiber edge** (`Substring{base}` string→string, JSString.cpp:113).

## Ratified serial decisions

- **SD-1 — leaf representation:** POD `allocate_blob` arena cells (raw-address identity, R4a), variable
  payloads relocated to store-owned slabs (mirroring `CoreObjectCell` butterfly/aux SD-4): String bytes →
  `string_texts` slab (= JSString handle + out-of-line StringImpl); Symbol description → slab; BigInt
  limbs → slab (faithful-enough vs JSBigInt inline-trailing digits; the established port pattern). Payload
  freed by **store-driven leaf reconcile**, not a cell destructor (the arena runs none).
- **SD-2 — type-dispatched `visitChildren` (THE gate, U0):** marker + reconcile dispatch by cell header
  `js_type` (the faithful `methodTable()->visitChildren` analog): object → `trace_cell`; rope-string →
  `trace_string_cell` (fiber edge); flat-string/symbol/bigint → no edges. Serial collector change; gates
  leaf admission. No behavior change while only object cells live in the arena.
- **SD-3 — leaf-first:** land Part A (rep + U0 dispatch + leaf reconcile/sweep + weak interning removal +
  Symbol-registry roots) first; `CallLinkInfo::visitWeak` (U7) as an independent sibling (needs only the
  post-mark `is_value_marked`).
- **SD-4 — no conservative stack scan in the first cut** (leaf GC stays safepoint-only + no-gc-scope
  bounded, identical to object cells; GAP C deferred). **Drop BigInt `by_value` interning** to match JSC
  (heap JSBigInts are not interned).

## Part A — leaf-cell GC (faithful to JSString/StringImpl, Symbol, JSBigInt)

- The interning maps are an **AtomStringTable analog** = **remove-on-sweep by identity** (JSC: a dying atom
  evicts itself from `~StringImpl` → `AtomStringImpl::remove`, identity-matched; StringImpl.cpp:129). In
  `reconcile_dead_leaf_cells_before_sweep`, for each dead (unmarked) string cell remove its
  `by_text`/`by_payload` entry by identity BEFORE the sweep clobbers the cell.
- Symbol `registry` (`Symbol.for`) = **STRONG ROOT** (JSC SymbolRegistry is a strong HashSet); `well_known`
  = permanent strong roots. Gather both in `gather_all_gc_roots` (the one new precise-root gap).
- Rope fiber edge: add `trace_string_cell` (Substring{base} → base string). Check the for-in enumerator
  name cache if it holds name cells.

## Part B — `visitWeak` (the CLEAR/RELINK phase only; KEEP-ALIVE ephemerons deferred)

JSC splits weak work: KEEP-ALIVE (marks more — WeakMap ephemerons, opaque roots) runs INSIDE the mark
fixpoint; CLEAR/RELINK (marks nothing) runs ONCE after the fixpoint, before sweep (Heap::runEndPhase).
**Target = CLEAR/RELINK.** Phase placement in `force_collect` (object_store.rs:3179-3197): after
`mark_live_set_from_addrs`, before reconcile/sweep. Liveness = `is_value_marked` (the `Heap::isMarked`
analog). Drive it with the existing `src/gc/weak.rs` vocabulary (`WeakSlotState` Live→ClearPending→Dead,
`WeakProcessingPhase::{Validate,Clear}`, `WeakRootPolicyPlan`).
- First-cut referents: (a) string interning entries (lands with Part A); (b) `CallLinkInfo::visitWeak`
  per CodeBlock: `is_linked() && !is_value_marked(callee)` → `flags.cleared_by_gc=true; reset_to_unlinked`
  (ic.rs:1079,1446; faithful to CallLinkInfo.cpp:171-217).
- Deferred (separate track): WeakMap/WeakSet ephemeron keep-alive + dead-key delete (Rust currently traces
  WeakMap keys+values STRONGLY — a known divergence to correct later), IC `PropertyInlineCache::visitWeak`,
  WeakSet finalizers.

## Fan-out (dependency order; parallel-safe vs serial)

- **U0 [SERIAL, gate, no-behavior-change]** — type-dispatched `visitChildren` in marker + reconcile (SD-2).
- After U0, parallel (isolated worktrees): **U1** String cell rep + `string_texts` slab; **U2** Symbol cell
  rep (+ registry/well_known roots); **U3** BigInt cell rep (drop `by_value`); **U4** `trace_string_cell`
  rope edge (coordinate with U0's dispatch hook).
- **U5 [SERIAL]** `reconcile_dead_leaf_cells_before_sweep` (free leaf slabs + weak interning removal);
  **U6 [SERIAL]** root-set extension (Symbol registry/well_known). Touch the collector → main-agent-owned.
- **U7 [independent sibling, gated on U0]** `CallLinkInfo::visitWeak` + the `visit_weak` phase slot in
  `force_collect`. Unblocks STEP-2's monomorphic callee-cache fast path.

## Risk (top UAF landmines)

1. **Marker mis-casts a marked leaf cell to `CoreObjectCell`** (garbage edges / OOB slab reads) — gated by
   SD-2/U0 landing BEFORE any leaf cell enters the arena. The edge is already correct; only the dispatch is new.
2. **Swept-while-interned dangle** — remove-on-sweep by identity in the leaf reconcile, BEFORE the sweep
   clobbers the cell (faithful `~StringImpl → AtomStringImpl::remove`).
3. **Registered/well-known Symbol or rope base swept under a live holder** — strong-root the Symbol
   registry/well_known (U6); add the rope fiber edge (U4). Property-name Symbols stay refcount-strong (faithful).

Relationship to the R-lever: independent of B5 Path-B / STEP 1-2. The first native call (STEP 2, callee
re-resolved each call) does NOT need `visitWeak`; U7 unblocks only the later monomorphic-cache fast path.
So this track is faithfulness + heavy-bench robustness + the call-cache enabler — not the first R move.
