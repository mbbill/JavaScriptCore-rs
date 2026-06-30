# Design — baseline property-access ICs (K2) + native-lowering breadth (RATIFIED 2026-06-30)

The next R-lever after broad engagement (native calls beat the interpreter, but real Octane hot
functions don't tier up — the S4 allowlist lacks property ICs). Port the baseline **DataIC** so
`get_by_id`/`put_by_id`-heavy functions tier up and run native. From a 5-auditor scoping+design pass.

## The model: port the baseline DataIC, NOT the code-patched InlineAccess

Current JSC splits property ICs into `HandlerPropertyInlineCache` (a **DataIC** — Baseline + DFG) and
`RepatchingPropertyInlineCache` (code-patched InlineAccess — **FTL only**). The baseline fast path reads
the cached StructureID + PropertyOffset as **runtime data fields** from a per-site heap IC record loaded
into a register (`bytecode/PropertyInlineCache.h:123,590`; `jit/JITInlineCacheGenerator.cpp:140-183`) —
**nothing is code-patched** (no W^X re-patching). The Rust DATA-SIDE IS ALREADY BUILT:
`HandlerPropertyInlineCacheRecord` (`#[repr(C)]` structure_id@0/offset@4/holder@8, `src/bytecode/ic.rs:1527`),
`BaselineJitData.property_caches` + `record_store_base()`→r13 (`ic.rs:1590-1618`), fill/reset
(`code_block.rs:943-1034`); an x86-64 reference emitter exists (`src/jit/emitter.rs` GetByIdSelf ~:1104).
**The ARM64 `function_emitter.rs` has ZERO get_by_id/put_by_id emission — that is the gap.**

## Ratified serial decisions

- **SQ1 — DataIC.** RATIFIED: port the baseline `HandlerPropertyInlineCache` DataIC; defer RepatchingIC/
  InlineAccess to FTL. (Low risk; the Rust seed already commits to it.)
- **SQ2 — the storage gate (the crux) → TWO-INCREMENT SPLIT.** RATIFIED. A faithful inline machine-code
  property load needs machine-addressable storage — real inline slots on the cell (today INLINE_CAPACITY
  unused, props forward-packed in a `Vec`) + a raw butterfly pointer (today a `ButterflyHandle` slab index).
  That is **gc-r4 Batch 5 + `object/property_offset.rs` wiring** (an object-model cutover, orchestrator-owned).
  - **Increment 1 (NOW, no object-model dependency):** structure-guard + a FAR-CALL load
    (`operation_get_by_id_optimize`) — mirrors the landed typed-array `emit_get_by_val` decision
    (`function_emitter.rs:582-602`). Unblocks COVERAGE (functions tier up); R moves from the surrounding
    native code + native calls (modest).
  - **Increment 2 (GATED on Batch 5):** replace the far-call with inline `loadProperty`/`storeProperty`
    machine code (runtime offset<64 inline vs negative-butterfly OOL, `AssemblyHelpers.cpp:442-465`). The
    genuine property-load speedup. Guard+fill machinery from Increment 1 is reused unchanged.
- **SQ3 — structure-id cache WITHOUT visitWeak.** RATIFIED: the Rust `StructureIdTable` never recycles ids
  (`structure_cell.rs:588-605`), so a bare id-compare is self-validating (a dead structure simply misses +
  re-fills) — no `visitWeak`/U7 dependency. Comment it as a deliberate divergence from JSC's recycled-id +
  visitWeak design; a `visitWeak`-equivalent reset becomes a hard prerequisite ONLY when the GC begins
  freeing/recycling structures (the IC reads ids only, so it does not entrench the non-recycling divergence).
- **SQ4 — monomorphic-only first cut.** RATIFIED: GetByIdSelf / PutByIdReplace monomorphic; a guard miss
  re-fills the same record (`mirror_self_load_data_ic_record`) + a slow-path-count → Megamorphic/Disabled
  churn cap; defer the polymorphic handler chain (a single regenerated BinarySwitch stub, not a linked chain).
- **put_by_id:** REPLACE cached, TRANSITION → slow path (faithful — `Repatch.cpp:1117-1123`); emit
  `emitWriteBarrier(base)` after the IC.

## Coverage — the priority order (evidence: a non-allowlisted opcode declines the WHOLE function)

1. **`GetByName` + `PutByName` (get_by_id/put_by_id) — #1, blocks 5/6 benches**, CO-DOMINANT with
   **`CallWithThis`** (method call = GetByName(callee) + CallWithThis; the allowlist admits only plain
   `Call`). **Ship #1 + CallWithThis as ONE unit** (else neither unblocks). 2. `Construct` (`new X()`).
   3. global/lexical resolves. 4. **`GetClosureCell`/`PutClosureCell` — SOLE blocker for navier-stokes**.
   5. StrictEqual/Equal/NotEqual. 6. `GreaterEqualInt32` (non-fusible, crypto's bignum loop). 7. InstanceOf.

## Fan-out (Increment 1 — the coverage unblock)

- **U-PRE [serial, PRECONDITION — landmine #1]:** verify `operation_get_by_val`/`operation_put_by_val`
  (`operations.rs:305-358`) correctly handle **plain-Array** (non-typed-array) element access. Today the
  allowlist admits GetByValue/PutByValue via the typed-array bridge; richards/deltablue/navier use them on
  plain Arrays but those functions DON'T tier up (blocked by get_by_id). Admitting get_by_id tiers them up
  → plain-Array element access goes LIVE on the native path → **wrong answers if the bridge is typed-array-
  only.** Verify + fix BEFORE/WITH the get_by_id batch.
- **U-A [serial seed]:** the `operation_get_by_id_optimize`/`operation_put_by_id_optimize` extern-C bridge
  signatures + record-index plumbing (mirror `operations.rs:305-358`).
- **U-G1 / U-P1 [parallel impl]:** ARM64 `emit_get_by_id` / `emit_put_by_id` — DataIC structure-guard
  (`load32 structIdReg; branch32(NE, structIdReg, [record+0])→slow`) + far-call load/store + `SlowCase`
  fill linkage + write barrier (put); transition→slow.
- **U-S1 [impl, after U-A]:** the slow-path bridges (resolve + fill the record + exception stamp).
- **U-C1 [serial integration]:** allowlist admission (`function_emitter.rs:1367-1476`) for `GetByName`,
  `PutByName`, AND `CallWithThis` (reuse `emit_op_call_dynamic` with a `this` arg).
- **U-V1 [verify]:** adversarial fidelity + a BOUNDED compile/probe-only test (a tiny property-access
  function tiers up; native==interpreter==oracle). NO heavy benches.
- **Increment 2 [gated on gc-r4 Batch 5]:** U-G2/U-P2 inline load.
- **Adjacent (parallel once the IC pattern lands):** Construct, global/closure (navier), StrictEqual,
  GreaterEqualInt32 fusion, InstanceOf.

## Risk (top landmines)

1. **Plain-Array get_by_val/put_by_val going live** (U-PRE above) — the #1 landmine; verify/fix first.
2. **Stale cached structure after a transition** — transition→slow path (never cached); monotonic ids → no
   false hit.
3. **Inline-vs-OOL offset** — Increment 1 far-calls the load (no baked offset math); Increment 2 ports the
   exact runtime branch under Batch-5 storage.
4. **GC-weak stale structure** — monotonic non-recycling ids make a bare id-compare safe (SQ3).
5. **Three IC representations** (PropertyInlineCache side-table / HandlerPropertyInlineCacheRecord generated
   record / LLIntGetByIdCache interpreter HashMap) — the baseline IC consumes ONLY the record store (r13);
   the slow-path bridge is the single writer; do NOT cross-wire the interpreter's LLIntGetByIdCache.

Authority: mcts_mem property-access/inline-cache nodes; C++ `jit/JITPropertyAccess.cpp`,
`bytecode/PropertyInlineCache.{h,cpp}`, `jit/Repatch.cpp`, `jit/JITInlineCacheGenerator.cpp`,
`jit/AssemblyHelpers.cpp` loadProperty.
