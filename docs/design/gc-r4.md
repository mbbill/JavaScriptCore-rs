# Design — POD object model + R3 → R4 (arena cell identity)

The GC/value track to a running JIT: the JIT emits raw cell pointers and assumes a
sweeping GC, but the live object cell is a fat Drop-bearing Rust struct that cannot
live in a sweep-freed MarkedBlock. This is the plan to make it POD and flip cell
identity to the raw arena address. R4 is the orchestrator's call, gated on **technical
verification**, not sign-off.

## C++ target: POD JSObject

JSObject is a fixed-size POD cell in a sweep-freed MarkedBlock; ALL variable-size state
is out-of-line. `DestructionMode::DoesNotNeedDestruction` (JSCell.h:105) — the sweeper
calls no destructors, so the cell must be POD (no Rust `Drop`).
- 8-byte JSCell header (StructureID@0 + type@4 + flags + cellState) (JSCell.h:293-302).
- the single `m_butterfly` pointer (JSObject.h:1167).
- inline property slots (Structure::inlineCapacity; JSFinalObject, JSObject.h:1249-1281).
- out-of-line behind the Butterfly (Butterfly.h:134-150): properties grow left, indexed
  elements grow right, IndexingHeader between. Per-kind payloads (Map/Set/Promise/
  ArrayBuffer/BoundFunction state) live in their own cells / auxiliary allocations.

## The live divergence (the blocker)

`CoreObjectCell` (object_store.rs:320-427) carries **~18 fat Drop-bearing fields** —
making it non-POD and un-sweepable. Current workaround: `Vec<Pin<Box<CoreObjectCell>>>`
(object_store.rs:32), hand-pinned, never swept, never freed (leaks). The **two
load-bearing divergences**: `properties: HashMap` (:360) and `property_offsets: HashMap`
(:361) — C++ keeps the property→offset map in `Structure::PropertyTable` (per-shape),
never per-cell. Relocation targets: out_of_line_storage/property values → Butterfly;
deleted_offsets/property_order → PropertyTable; elements → Butterfly indexed; map_entries/
set_values → JSOrderedHashTable; regexp_source/flags → JSString cells; promise_reactions →
reaction cells; array_buffer_data → auxiliary u8 backing; bound_args/captures/instance_fields
→ separate array/function cells.

## The gating hazard: `storage_ptr` self-reference (audit 2026-06-28)

`storage_ptr: *const RuntimeValue` (object_store.rs:347, the butterfly slot @8) points
into the cell's OWN `out_of_line_storage` Vec — a self-referential interior pointer. It
is the SOLE reason the hand-written `Clone`/`Default`/`refresh_storage_ptr` exist, and it
is non-POD-by-aliasing: any move/sweep dangles it, and once the cell IS a raw arena
address (R4) a self-referential interior ptr is immediate UB under Stacked/Tree Borrows.
Eliminating it (point `storage_ptr` at a SEPARATE butterfly allocation) is a HARD R3/R4
precondition, and it lives inside the rank-1 Butterfly-values unit — so that unit is both
highest-value and the R4 gate. **Lead the GC track with Butterfly-values.**

## Rewrite plan — vertical slices, build green every commit (refined 2026-06-28)

NOT the big-bang "delete all ~18 fat fields then refill" — that breaks the build across
the 56 `find_mut` sites + every per-kind path until all units land, so the tree is never
green and no unit is independently committable. Instead each unit is a COMPLETE vertical
slice (add backing module + handle field → rewrite that kind's call sites through the
handle → delete the old Vec/HashMap/String field) that keeps the suite green and is one
reviewable commit. New backing modules are disjoint (parallel-implementable in worktrees);
the struct-def field-deletions + Default/Clone edits all touch object_store.rs:320-590, so
**integrate ONE UNIT AT A TIME on trunk, in rank order.**

- **Prereq:** Structure-wire (offset map → `Structure::PropertyTable`; in flight) lands first.
- **B1a (SERIAL, small):** shared infra only — CONSOLIDATE the existing `object/storage.rs`
  Butterfly/ButterflyHandle/OutOfLineStorage/IndexedStorage skeleton (over
  `gc::ValueBarrier<JsValue>`) into `object/butterfly_handle.rs` (do NOT create a 2nd
  butterfly rep) + scaffold `object/auxiliary.rs` + `Heap::allocate_*` stubs (C++ Auxiliary
  subspace; `HeapCellKind::Auxiliary` gc/cell.rs:108). `needs_drop` assert deferred.
- **Per-kind units, rank order** (each a vertical slice; all share the struct-def edit
  region so integrate serially):
  1. **Butterfly-values** — **DONE (de-self-reference) + a prerequisite for the flip.** B1a (infra)
     + the cutover LANDED the critical R4 UB precondition: `out_of_line_storage` + `elements`
     relocated to the store-owned butterfly slab; the offset-8 slot is now a `ButterflyHandle`
     (slab index, separate allocation) — **`storage_ptr` no longer self-references** (refresh/reset
     helpers gone, grep-proven, verified ACCEPTABLE). Clone-via-store deep-clones the slab.
     **DEFERRED — the full FLIP (delete the per-cell `properties` HashMap → POD) is BLOCKED** on a
     prerequisite the cutover's verify surfaced: NOT just accessors — **both accessor AND
     Symbol-keyed properties have NO Structure offset** (`structure_offset`→None for them), there is
     **no GetterSetter cell**, and **no Accessor attribute bit** in `core_attributes_to_u32`. So
     those values have no butterfly home; deleting the HashMap would orphan them. **PREREQUISITE
     BATCH (the next GC unit):** (a) a minimal GetterSetter cell kind, (b) an Accessor attribute bit
     + Structure plumbing, (c) Structure offsets for Symbol + accessor keys — THEN the HashMap
     deletion (the butterfly becomes the sole value authority) + the `needs_drop` POD assert.
     Until then the cell is NOT POD (the HashMap is Drop-bearing) and R4 stays gated on this.
     **STAGING (spec'd 2026-06-29; B-i/ii/iii additive/dual-write + reversible, B-iv the flip):**
     B-i Accessor bit in `core_attributes_to_u32` (thread is_accessor) + `structure_property(sid,key)
     →Option<(offset,attrs)>` plumbing the live PropertyTable.get's attributes (makes data-vs-accessor
     adds DISTINCT transition edges — bit 1<<4 — so siblings converge correctly); B-ii a
     `CoreObjectKind::GetterSetter` cell + `getter_value`/`setter_value: Option<RuntimeValue>` (Copy,
     POD-safe) — the accessor's butterfly slot holds `from_cell(getter_setter)` like C++'s
     GetterSetter*; B-iii un-gate Symbol+accessor keys to REAL Structure offsets
     (`core_property_key_supports_named_property_offset` + `define_accessor` route through
     `structure_add_property`), DUAL-WRITE the butterfly in lockstep with the still-authoritative
     HashMap; B-iv the FLIP — delete the HashMap (~36 value-authority sites) + property_order (23,
     →Structure entry order) + vestigial deleted_offsets, reads → structure+butterfly. **DECISIONS:**
     symbols transition+converge (faithful); fresh-key accessors get real transitions, but in-place
     data↔accessor CONVERSION keeps the dictionary fallback (defer the faithful attributeChangeTransition);
     GetterSetter = CoreObjectKind variant. The `needs_drop` POD assert flips only after the OTHER
     per-kind Drop fields (Map/Set/RegExp/Promise/ArrayBuffer/Bound/captures) ALSO relocate — this
     batch removes only the PROPERTY Drop fields.
  2. **JSFunction-captures** — `captures`(358)+`instance_fields`(357) → JSLexicalEnvironment
     variables[] / class-field init (JSLexicalEnvironment.h:56-80, JSCallee::m_scope).
  3. **RegExp** — `regexp_source`(397) → RegExp::m_patternString (RegExp.h:219);
     **delete** `regexp_flags_text`(399), recompute from the POD `regexp_flags` bits.
  4. **ArrayBuffer** — `array_buffer_data`(410) → aux u8 backing (ArrayBuffer.h:126); no barrier.
  5. **Map/Set** — `map_entries`(395)+`set_values`(396): relocate to an aux backing for POD-ness
     NOW; the faithful insertion-ordered JSOrderedHashTable (JSOrderedHashTable.h:164) is a
     DEFERRED correctness/perf batch (Map/Set is not Octane-hot). Documented deviation.
  6. **Promise** — `promise_reactions`(402) → reaction records (JSPromise.h:35).
  7. **BoundFunction** — `bound_args`(426) → aux value array (JSBoundFunction.h:133). Smallest;
     good warm-up unit.
- **POD-ness proof:** the final unit's commit flips ON `assert!(!std::mem::needs_drop::<CoreObjectCell>())`
  — sweepability proven atomically (compile-fails if any Drop field is reintroduced). Only
  then R3 → R4.
- **Handle rep decision:** handle/index NOW (R3-reversible); the raw machine-code-dereffable
  arena butterfly pointer (so `storage_ptr@8` is a real `[base+8]→[ptr+off*8]` deref) lands at
  R4. A cross-cutting identity decision, settled this way to keep B1a/Butterfly-values reversible.

## R3 → R4 (refined by the readiness audit 2026-06-28)

R4 is LOW-risk MECHANICALLY — the value ALREADY carries the raw cell pointer
(`RuntimeValue::from_cell`, object_store.rs:3780; `find_mut` round-trips the index map ONLY
because safe Rust can't deref a raw ptr to `&mut`), and the `&mut self` borrow checker ALREADY
forced the copy-out/re-lookup pattern R4 wants (every multi-cell site copies out Copy data,
drops the borrow, re-looks-up). So `find_mut` → `arena.with_cell_mut(addr,|cell|…)` is mostly
mechanical. The SHARP EDGE is below; the REAL work is the collector (next section).

- **R3 (reversible):** the object choke point is `allocate_cell` (object_store.rs:3736-3781) —
  NOT :3660 (that line drifted to `install_native_getter`). Others hold: string_store.rs
  72/99/170 (+ atom ~175), symbol_store.rs 47/179, bigint_store.rs 572 — all the same
  Box::pin→NonNull→payload→push→index-insert→`from_cell` template. R3 KEEPS that path unchanged
  (it stays the ORACLE and keeps publishing the box pointer, so all 56 `find_mut` + read sites
  work untouched → fully reversible) and ADDS a twin POD cell into MarkedSpace keyed by the same
  payload; a `shadow_oracle(payload)` at the top of `find_mut`/`find` asserts the twin is
  byte-equal to the box cell, + a suite-end population cross-check (arena live count == Σ store
  lens). R3 only needs the arena to ACCEPT a POD blob — NOT sweep.
- **R4 (IRREVERSIBLE, 1 atomic commit):** delete the `Vec<Pin<Box>>` stores + `objects`/
  `object_indices_by_payload`; arena address = sole identity; the **56** `find_mut` sites (36
  object_store.rs + 20 mod.rs) → `with_cell_mut` closures; the read-only `find` subset →
  shared-deref closures (lower risk).
- **THE SHARP EDGE — ~3 TWO-DISTINCT-CELL families** that JS lets be the SAME cell, which LOSE
  compile-time aliasing safety at R4 (a naive `with_cell_mut(target,|t|{…with_cell(source)…})`
  on `source===target` = overlapping borrows = instant UB). MUST stay copy-out; author a
  self-aliasing verifier (these are stable JS semantics, authorable now):
  - `Object.assign(o,o)` / `Object.defineProperties(o,o)` — mod.rs:26997/26798/26531 (source
    slots → target slots).
  - Map/Set self-key `m.set(m,v)` — mod.rs:23284/23365 + native_map_* 15504-16364 (copy the
    key-index out FIRST, as native_map_delete:15501 already does).
  - prototype-chain walkers (N-cell, not aliasing but most closures to thread) — 6578/6744/6475/6452.
- **R4's technical gate (NOT human sign-off):** (a) R3 shadow oracle green suite-wide (no
  assert + population cross-check passes) across all 15 benches; (b) miri on the live deref under
  `-Zmiri-permissive-provenance` + `-Zmiri-tree-borrows`, exercising the self-aliasing hotspots
  (`Object.assign(o,o)`, `defineProperties(o,o)`, `m.set(m,…)`, a proto-chain get) — zero UB on
  the raw arena deref, the butterfly second deref (distinct provenance), and no double-`&mut`;
  (c) adversarial verifier — grep zero `find_mut`/`objects.get_mut`/`object_indices_by_payload`
  remain, zero `refresh_storage_ptr` (self-ref interior ptr gone), barrier-before-mutate ordering
  preserved, every two-cell site provably copies out, `needs_drop::<CoreObjectCell>()==false`
  compiles; (d) all 15 Octane benches pass. Orchestrator verifies all four, then merges.

## The collector — the REAL gap (gated on POD-ness / Batch 1)

The audit found the live cell has neither a trace nor a sweep; the only Trace impls are
unwired no-op skeletons. "Make the collector run" = author these, all gated on the same
POD-ness (Batch-1-complete) gate as R4. Author as soon as **Butterfly-values lands** (it
de-self-references `storage_ptr`, unblocking both the trace's butterfly edge and the sweep's
relocation-safety):

- **GAP A — trace [BLOCKER]:** `CoreObjectCell` (and String/Symbol/BigInt cells) impl NEITHER
  Trace NOR TraceCell; `JsCell::trace` is a no-op (gc/cell.rs:632-637); the skeleton `JsObject`
  trace (object/identity.rs:126-137) is a stub. Write `CoreObjectCell::trace` visiting the ~14
  inline `RuntimeValue` GC edges (prototype, super_base/constructor, binding/primitive_value,
  promise_result, view_buffer, proxy_target/handler, bound_target/this, native_bound_*) + the
  butterfly + (per-kind, added as each unit lands) the relocated backings. Mechanism exists
  (`RuntimeValue::as_cell`→append); the visitor is just unwritten. Mirrors JSObject::visitChildren.
- **GAP B — sweep [BLOCKER]:** `marked_block.rs` (the real 16KB S4 block) exposes NO sweep /
  free-list rebuild. Write a sweep that, relying on `needs_drop==false` (the Batch-1 assert is
  exactly what makes this legal — no destructor to run), reclaims unmarked atoms into the FreeList
  (gc/heap/free_list.rs exists). Mirrors MarkedBlock::specializedSweep for DoesNotNeedDestruction.
- **GAP C — JS-stack conservative scan [gated on JSStack track]:** native-stack conservative scan
  IS wired (machine_stack_marker.rs:72-95 captures x19-x28 + spans), but the interpreter JSStack
  is a separate span and not yet a single contiguous conservative span; today roots come from a
  precise `root_snapshot` (mod.rs:928). Wiring the JSStack span into ConservativeRoots firms up
  after the native-thread-stack migration (B4b/B6).
- **GAP D — value-type reconciliation [divergence]:** live edges are `RuntimeValue` (value/repr.rs,
  NaN-box, has as_cell/from_cell); the skeleton Trace/`ValueBarrier` path uses a DIFFERENT
  `JsValue` (object/storage.rs). The trace MUST be written against `RuntimeValue` — the skeleton
  impls target the wrong type and can't be reused as-is. Same reconciliation as the
  storage.rs↔butterfly_handle.rs Butterfly consolidation (B1a): consolidate onto `RuntimeValue`.

## Dependency / ordering

value-rep NaN-box (done) → Structure-wire (offset map → PropertyTable; in flight) → B1a
(shared butterfly/aux infra, serial) → per-kind vertical slices integrated serially in rank
order, Butterfly-values FIRST (it de-self-references `storage_ptr`, the R4 gate) → `needs_drop`
assert flips in the final unit → R3 (shadow oracle, arena==old per access, reversible) → R4
(flip: raw arena address = sole cell identity; the 56 `find_mut` sites → `arena.with_cell_mut`).
Composes with the JSStack track independently (different subsystems). The running collector +
the JIT emitting raw pointers sit on top of R4.

**Residual after Structure-wire (VERIFIED at integration 2026-06-28):** Structure-wire folded
the offset MAP and the `m_deletedOffsets` RECYCLING into the per-shape PropertyTable, but the
cell still carries `deleted_offsets`(object_store.rs:368, now VESTIGIAL — the PropertyTable owns
recycling) and `property_order`(369, still the LOAD-BEARING per-cell enumeration order, used at
~15 enumeration sites). C++ keeps enumeration order in the Structure's PropertyTable entry vector,
not per-cell. FOLLOW-UP (small, before/with Butterfly-values): fold `property_order` → the
Structure entry order + delete the vestigial cell `deleted_offsets`. Correctness is unaffected
(the verify pass confirmed no-wrong-slot + faithful recycling); this is dead-weight + an
enum-order divergence to correct, not a bug.

**GC edges in the POD cell:** ~14 of the STAYS scalar fields are `RuntimeValue` GC edges
(prototype, bound_target/this, proxy_target/handler, promise_result, binding/primitive_value,
…) — POD for sweeping but the collector's `trace()` MUST visit them (inline slots + butterfly)
and every write MUST barrier (gc/barrier.rs).
