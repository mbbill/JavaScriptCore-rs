# Design — gc-r4 Batch 5: machine-addressable object storage (RATIFIED 2026-06-30)

The measurement-confirmed R-gate (scoreboard 2026-06-30): the native JIT regresses on property/call-heavy
benches because Increment-1 FAR-CALLS every property load. Increment 2 (an inline machine-code load) needs
machine-ADDRESSABLE storage. R4a already made the cell an arena address; Batch 5 flips the two remaining
non-addressable pieces — this is the "raw machine-code-dereffable arena butterfly pointer lands at R4"
that gc-r4.md anticipated.

## The two pieces (today → faithful)
1. **No inline slots** (INLINE_CAPACITY=6 is a constant but unused, object_store.rs:1548) → add
   `inline_storage: [RuntimeValue; 6]` as the `#[repr(C)]` field immediately after `butterfly`, landing at
   **offset 16 = C++ `offsetOfInlineStorage()` exactly** (structure_id@0 + js_type@4 + butterfly@8 → @16).
   `[RuntimeValue;6]` is POD so the `needs_drop::<CoreObjectCell>()==false` assert still holds. Inline read =
   `load [cell + 16 + offset*8]` for offset<6. Const-assert the offset == 16.
2. **Butterfly = slab INDEX** (`ButterflyHandle`, butterfly_handle.rs:37) + OOL = forward-packed
   `Vec<Option<RuntimeValue>>` (16-byte, non-addressable) → cell+8 becomes a RAW base pointer into a single
   stably-addressed `RuntimeValue` buffer laid out C++-faithfully (named OOL at NEGATIVE indices below the
   base, elements above, **in-band empty/undefined sentinel for holes — NOT `Option`**, so 8-byte slots).
   OOL read = `load [cell+8]→bptr; load [bptr + offsetInOutOfLineStorage(offset)*8]` (= `-(offset-64)-1`).
   This is the verbatim `AssemblyHelpers::loadProperty` BaseIndex (AssemblyHelpers.cpp:442-465).

## Sound with the NON-MOVING R4a/R4b collector
A raw base pointer in the cell is valid for the butterfly's whole lifetime (no relocation); the JIT
re-loads `[cell+8]` on EVERY access (never caches it across an op), so only butterfly REALLOCATION on
property-add changes it — and that rewrites cell+8 under a write barrier, exactly as C++
`createOrGrowPropertyStorage` rewrites `m_butterfly`.

## The GC interaction (ratified: the BRIDGE rep, not a new subspace)
- Butterfly = a store-owned, **stably-addressed** `Box<[RuntimeValue]>` (the heap buffer's data pointer is
  stable across the store registry `Vec`'s own growth — only the owning `Box` handle moves, never the buffer).
  cell+8 stores the raw base pointer (the `m_butterfly` analog). Allocated at the `allocate_cell` chokepoint.
- **Mark:** `trace_cell` already appends every butterfly value edge — change it to deref the cell's butterfly
  pointer and append each named-prop + element `RuntimeValue` (the `markAuxiliaryAndVisitOutOfLineProperties`
  value-append). The butterfly allocation needs NO separate mark bit: one owner ⇒ owner liveness = butterfly
  liveness (the non-shared-butterfly invariant; COW arrays are the one exception, out of scope).
- **Free:** `reconcile_dead_cells_before_sweep` already reads each DEAD cell's butterfly from intact pre-sweep
  bytes (type-gated) and frees it BEFORE sweep — only the free MECHANISM changes (drop the owned buffer /
  butterfly free-list instead of `mem::take` a slab slot). The ≥2-collection membership-only reclaim invariant
  is unaffected.
- The full-faithful Auxiliary-subspace MarkedBlock home for butterflies (`markAuxiliary` + sweep) is the
  DEFERRED SD-4 / R2-Auxiliary follow-up — standing it up now would couple Batch 5 to a whole subsystem.

## Structure offset model
Wire `object/property_offset.rs` (inline<64 / negative-OOL, already faithful + tested) as the live storage
authority in `butterfly_prop_get/put` (object_store.rs:2091-2124), RETIRING `offset_storage_index` (the
forward-packed Vec, object_store.rs:1593). Offset PRODUCTION is unchanged — it already flows from
`Structure::PropertyTable` via `offsetForPropertyNumber` (the inline-then-jump-to-64 rule). Only the storage
INDEXING under a fixed offset changes; get/put/transition logic is untouched. Assert Structure offsets obey
`offset_for_property_number(n, INLINE_CAPACITY)` so storage dispatch can never disagree with the table.

## Migration — INCREMENTAL, 3 reversible steps (oracle dual-paths; NOT an atomic flip)
- **Step 1 — inline slots (highest value, growth-free, reversible):** add `inline_storage`@16; route inline-band
  offsets to it in `butterfly_prop_get/put`; DUAL-WRITE the forward Vec band initially (oracle), assert
  inline==oracle, then delete the Vec inline band. Gate: cargo test --lib (2818) + a <6-field object reads
  inline==interpreter==oracle.
- **Step 2 — machine-addressable OOL buffer + raw cell pointer:** replace `ButterflyAllocation{props:Vec,
  elements:Vec<Option>}` with the single stably-addressed buffer (negative-indexed named side, in-band hole
  sentinel); cell+8 → raw base pointer; update `trace_cell` (deref) + reconcile/free (free by pointer) + every
  GROWTH site (realloc + rewrite cell+8 under barrier). Gate: tests + **miri Tree-Borrows on the raw butterfly
  deref** + the **≥2-collection** reclaim/retain test + a release value-encoding probe.
- **Step 3 — retire `offset_storage_index`** (mostly falls out of 1-2; flip `butterfly_prop_*` onto
  property_offset.rs; unify the `PropertyOffset` newtype).
- **Then Increment 2 (gated on 1-3):** emit the verbatim `loadProperty`/`storeProperty` in ARM64
  `function_emitter.rs`, replacing the Increment-1 far-call; the Increment-1 structure-guard + DataIC record
  fill + write barrier are REUSED unchanged.

## Minimum for Increment 2: BOTH bands (the DataIC offset is a runtime field → the emitted code runs the
`offset<64` branch, so both the inline path `cell+16+off*8` and the OOL path `deref cell+8` must be physically
present). Ship inline-first, OOL-second, both before Increment 2 wires.

## Fan-out
- **SERIAL (main-agent, between phases — all touch the cell-rep / GC region):** the cell struct-def edit
  (inline_storage@16 + butterfly slot type + layout asserts); `trace_cell` butterfly-edge; reconcile
  free-by-pointer; `allocate_cell` + every butterfly GROWTH site.
- **PARALLEL:** AUDITOR (the full read/write surface: every `butterfly_prop_*`/`offset_storage_index`/
  `self.butterflies[...]` site); AUDITOR (the growth sites = `createOrGrowPropertyStorage` analogs);
  IMPLEMENTER (property_offset.rs dispatch, Step 3); VERIFIER (miri/TB, ≥2-collection, the inline/OOL boundary
  at offset 5 vs the 7th-property jump to 64); IMPLEMENTER (Increment 2, after Batch 5).

## Risk (top landmines)
1. **Dangling butterfly pointer after growth** — rewrite cell+8 with a barrier at EVERY grow site; the JIT
   re-loads `[cell+8]` per access (no cache) + no collection inside a `with_cell_mut` window → safe.
2. **Inline-vs-OOL boundary** (offset 5 inline vs property-number-6 → offset 64 OOL) — wire the tested
   property_offset.rs as the single authority + the offset_for_property_number assert.
3. **Non-addressable holes** — in-band empty/undefined sentinel (8-byte), not `Option`.
4. **POD violation** — the cell holds only `[RuntimeValue;6]` (POD) + a raw pointer; the owning Box lives in
   the store registry, never on the cell.
5. **Shared COW array butterflies** break the one-owner-liveness invariant — out of scope for Batch 5 (the
   Auxiliary-subspace follow-up).

Authority: C++ JSObject.h/Butterfly.h/PropertyOffset.h/AssemblyHelpers.cpp (cited in the spike); mcts_mem
object-model.md + object-model/property-storage.md (offsets are the stable interface; single bidirectional
m_butterfly). Builds on docs/design/gc-r4.md (R4a/R4b) + baseline-property-ic.md (Increment 2 = SQ2).
