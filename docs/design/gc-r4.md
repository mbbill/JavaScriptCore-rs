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
  1. **Butterfly-values** — `out_of_line_storage`(385)+`properties` values(360)+`elements`(394)
     → Butterfly (props left, elements right, IndexingHeader between; Butterfly.h:134-150).
     De-self-references `storage_ptr`; deletes the custom Clone/Default/refresh. Accessors =
     2 adjacent slots. The JIT GET/PUT_BY_ID/VAL target. **R3/R4 precondition.**
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

## R3 → R4

- **R3 (reversible):** route the ~7 store allocation choke points (object_store.rs:3660;
  string_store.rs:72/99/170; symbol_store.rs:47/179; bigint_store.rs:572) to ALSO allocate the
  POD cell in the MarkedSpace arena, keeping the old Vec storage as the oracle; a shadow_oracle
  asserts arena==old every access + a suite-end population cross-check. Reversible; ~200–300 LoC.
- **R4 (IRREVERSIBLE, 1 atomic commit):** delete the `Vec<Pin<Box>>` stores + the payload↔cell
  HashMaps; arena address = sole cell identity; `RuntimeValue::from_cell` carries the bare
  address; the ~177 `find_mut(obj)` sites become `arena.with_cell_mut(addr, |cell| …)` closures;
  the collector sweeps the POD cells. ~300–500 deleted + ~1–2 kLoC modified.
- **R4's technical gate (NOT human sign-off):** R3 shadow oracle green suite-wide (no assert,
  population cross-check passes) + miri on the live deref under `-Zmiri-permissive-provenance`
  + an **adversarial verifier** (refute UAF / double-drop / write-barrier-ordering / arena-deref
  validity; grep that zero `find_mut` remain) + all 15 Octane benches pass. Orchestrator verifies
  all four, then merges.

## Dependency / ordering

value-rep NaN-box (done) → Structure-wire (offset map → PropertyTable; in flight) → B1a
(shared butterfly/aux infra, serial) → per-kind vertical slices integrated serially in rank
order, Butterfly-values FIRST (it de-self-references `storage_ptr`, the R4 gate) → `needs_drop`
assert flips in the final unit → R3 (shadow oracle, arena==old per access, reversible) → R4
(flip: raw arena address = sole cell identity; the 56 `find_mut` sites → `arena.with_cell_mut`).
Composes with the JSStack track independently (different subsystems). The running collector +
the JIT emitting raw pointers sit on top of R4.

**Coupling to Structure-wire (verify at its integration):** `deleted_offsets`(392) and
`property_order`(393) are PropertyTable-owned in C++ (m_deletedOffsets PropertyTable.h:292 +
the entry-vector order), NOT per-cell aux — they belong with Structure-wire, not a per-kind
unit. When Structure-wire lands, confirm it relocated them; if not, a small Structure-wire
follow-up owns them before Butterfly-values touches the cell.

**GC edges in the POD cell:** ~14 of the STAYS scalar fields are `RuntimeValue` GC edges
(prototype, bound_target/this, proxy_target/handler, promise_result, binding/primitive_value,
…) — POD for sweeping but the collector's `trace()` MUST visit them (inline slots + butterfly)
and every write MUST barrier (gc/barrier.rs).
