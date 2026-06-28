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

## Rewrite plan (7 batches; ~5–8k LoC; Batch 2/3/4 parallel after Batch 1)

- **Batch 1 (SERIAL):** POD `CoreObjectCell` skeleton — header + butterfly handle + scalar
  fields only; delete the ~18 fat fields. NEW `object/butterfly_handle.rs` (Heap::
  allocate_butterfly + slot get/put) + `object/auxiliary.rs` (per-kind backing). Update the
  layout const-asserts (STRUCTURE_ID_OFFSET==0, STORAGE_PTR_DISP==8). Ratify the final scalar
  field set.
- **Batch 2:** Structure-wire — `StructureIdTable.PropertyTable` becomes the sole offset
  authority; delete property_offsets/next_property_offset; rewire ~100 offset sites
  (`cell.property_offsets.get(k)` → `structure_table.get(cell.structure_id).lookup_offset(k)`).
  ~1–2 kLoC. (Activates the already-ported object/structure_cell.rs PropertyTable.)
- **Batch 3:** per-kind auxiliary relocation (Map/Set, RegExp, Promise, ArrayBuffer,
  BoundFunction, JSFunction; Proxy already inline) — 7 parallel units, ~100–150 LoC each:
  delete the Vec/HashMap/String field → handle + Heap::allocate_*_backing + get/put + barrier.
- **Batch 4:** Butterfly backing for out-of-line properties — move `out_of_line_storage` to
  Heap::allocate_butterfly; refresh storage_ptr at every mutation (putDirectOffset). ~200–300 LoC.
- **Batch 5 (deferred, post-R4):** inline property slots (inlineCapacity>0) — needs arena
  size-classes; only for IC optimization, not parity.

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

value-rep NaN-box (done) → Batch 1 (POD cell, serial) → Batches 2/3/4 (parallel, integrate
serially) → R3 (shadow) → R4 (flip). Composes with the JSStack track independently (different
subsystems). The running collector + the JIT emitting raw pointers sit on top of R4.
