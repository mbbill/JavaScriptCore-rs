# Status — per-subsystem (agent working tracker)

The detailed current-state tracker, per subsystem. `README.md` is the human progress
view; this is the agent's working status tree. The *plan* is `docs/ROADMAP.md`; keystone
*designs* are `docs/design/`; the decision *log* is `git log`. Keep this STATUS, not
history; record where each subsystem stands, not what happened.

Legend: `[done]` implemented+verified for the stated scope · `[wip]` partial/expanding ·
`[missing]` not yet reliable · `[risk]` exists, needs fidelity/structure review ·
`[deferred]` intentionally later · `[frozen]` quarantined salvage.

## Octane harness & correctness
- [done] JetStreamDriver load order, shell globals, iteration, validation, scoring, probe surface.
- [done] All 3 original throwers fixed (faithful, C++-verified): regexp (full Yarr engine wired,
  simple_exec deleted, checksum validates), Box2D (Number/Math constants), gbemu (`new Function`),
  pdfjs (abstract-equality ToPrimitive). call-link per-site rewire landed earley-boyer + Box2D.
- [done] typescript value-divergence FIXED (faithful, jsc-verified): Array `length` get + set.
  `arr.length=N` was a no-op (length isn't stored) and `get_by_id "length"` (used by
  `arr.length++/--`) returned undefined; together they broke the ResolutionDataCache
  `.length=0` clear -> spurious overload candidates -> `params[-1].getType()` throw. Now
  setLength resizes (+RangeError) and get_by_id sees exotic Array/TypedArray length.
  runIteration completes zero-throw, parseErrors=192 == jsc. (Suite score still JIT-gated.)

## Faithful foundation (built; mostly unwired behind dead_code)
- [done] value → JSVALUE64 NaN-boxing (lossless double + immediates).
- [done] S4 cell arena (MarkedSpace/MarkedBlock/BlockDirectory/FreeList/PreciseAllocation, miri-proven)
  + SlotVisitor STW marking core — collector RUN-gated on R3/R4.
- [done] Structure leaf ports + Structure cell (StructureID/StructureIdTable/TypeInfoBlob/PropertyTable).
- [done] StringImpl Stage A (8/16-bit Latin-1/UTF-16, O(1) index).
- [done] profiling: ArithProfile + ExecutionCounter (faithful bitfields) + SpeculatedType u64 bitset.
- [done] bytecode: faithful packed instruction-stream core (Vec<u8>, byte-offset index, width-aware).

## Assembler / codegen (PROVEN end-to-end: emit → relocate → execute)
- [done] AbstractMacroAssembler operands + RegisterID + ARM64 encoder (byte-oracle-proven).
- [done] MacroAssemblerARM64 composite-op layer (add/sub/logic/shift/mul/move/load/store/branch over
  RegisterID/Address/BaseIndex; byte-oracle-proven; unwired — B7 emits through it). Deferred: logical-imm
  bitmask forms, CachedTempRegister, cbz/cbnz folds, extended-register cmp for sp operands.
- [done] box/tag layer (baseline-JIT Rank-1, adversarially verified): or64/and64/xor64, branch_mul32
  (smull+cmp-sxtw overflow), branch_test64 + jit/assembly_helpers.rs (AssemblyHelpers: numberTag x27/
  notCellMask x28 model, branchIfInt32/boxInt32/unboxInt32) — value-rep-consistent via the SHARED
  value::{NUMBER_TAG,NOT_CELL_MASK}. Forward: branch_test64 needs an Imm(1) overload for jtrue/jfalse
  (Rank-2 adds it); x28 cell-check ≠ single-mask in the transitional cell encoding (defer to the IC wirer).
- [done] LinkBuffer Label/Jump/Call + byte-exact in-place relocation.
- [done] W^X executable memory (MAP_JIT + pthread_jit_write_protect; emit→finalize→call returns 42);
  unsafe scoped to jit/unsafe_platform_boundary.rs (forbid→deny).

## JSStack execution substrate (native-thread-stack — see docs/design/jsstack.md)
- [done] B1 types/offsets/provenance gate; B2 reservation+seeding+stack guard; B3 dual-write shadow; B4
  READ-FLIP -- the arena IS the live register window (reads served from it; Vec retained as a dual-written
  debug oracle, arena==Vec cross-check holds suite-wide; reversible). Fixed the jit/abi.rs callee-slot defect.
- [wip] B4b/B6 drop the Vec oracle + retire CallFrameId (arena CallFrame* = identity); B5 prologue split;
  B7 wire the encoder to emit ldr/str [x29,#vreg*8] against the arena.

## GC / value cutover (toward R4 — see docs/design, the arena cell identity the JIT emits)
- [done] the arena + marking core (above), unwired.
- [done] Structure-wire: the #1 divergence corrected — per-cell offset map → per-shape
  Structure::PropertyTable (StructureIdTable is the offset authority; offsets flow from
  transitions; inline_cap=6; delete-then-readd recycles faithfully via m_deletedOffsets).
- [done] B1a butterfly infra (additive, dead_code): object/butterfly_handle.rs (ButterflyAllocation
  over RuntimeValue + store slab + allocate/clone/prop/elem API) + object/auxiliary.rs scaffold;
  ButterflyHandle moved out of storage.rs.
- [done] Butterfly-values cutover (verified): storage/elements → the store slab; the offset-8 slot is
  a ButterflyHandle (separate alloc) — **storage_ptr de-self-referenced (the R4 UB hazard, gone)**;
  Clone-via-store; ~74 sites flipped (copy-out pattern). KEEPS the HashMap (cell NOT yet POD).
- [done] GetterSetter infra B-i/ii/iii (verified; additive/dual-write, reversible): Accessor attribute
  bit (1<<4, distinct data/accessor transition edges — provably disjoint) + CoreObjectKind::GetterSetter
  cell (POD Option<RuntimeValue> getter/setter) + Symbol+accessor keys now get REAL Structure offsets +
  dual-write the butterfly in lockstep with the still-authoritative HashMap. IC data-load probe gated to
  miss for accessor shapes (required; reads the shape). HashMap still authoritative; needs_drop NOT flipped.
- [done] B-iv FLIP (irreversible, 66a860a): the per-cell properties HashMap (value authority) DELETED —
  reads route structure offset → butterfly slot; accessors via the butterfly GetterSetter; property_order
  folded into PropertyTable entry order; vestigial deleted_offsets dropped (recycle owned by m_deletedOffsets);
  in-place data↔accessor conversion now offset-stable (corrects a pre-flip offset-vanish defect). Gated by a
  randomized HashMap-oracle equivalence test (per-op get/enum/accessor diff). needs_drop POD assert still
  waits for the OTHER per-kind units.
- [missing] POD object-model rewrite (retire the fat CoreObjectCell) → R3 shadow oracle → R4 flip
  (gate = technical verification: shadow cross-check + miri + adversarial verify) → running collector.
  Audited (gc-r4.md): R4 mostly mechanical (value carries the ptr; copy-out pattern exists), sharp
  edge = ~3 two-cell self-aliasing families. REAL gap = the collector: CoreObjectCell has NO trace
  (GAP A) + NO sweep (GAP B); both gated on POD-ness (Batch 1). Author trace+sweep when Butterfly-values lands.

## Baseline JIT / DFG / FTL (parity lives here; ~0% started)
- [done] JIT↔runtime bridge-infra (adversarially verified): extern-C operation_value_add shim
  (D1+D5 raw-ptr reborrow of vm+real host, Miri-passed) + Vm::operation_* split-borrow wrappers
  (evaluators verbatim) + D3 jit_pending exception word + far-call. docs/design/jit-runtime-bridge.md.
- [done] op_add baseline-JIT lowering (verified; EXECUTES native machine code under W^X): fast int32
  (load64/branchIfNotInt32/branchAdd32 Overflow/boxInt32/store64, JITAddGenerator-faithful) + slow-path
  far_call(operation_value_add) + exception edge + C-ABI trampoline (push_pair prologue, x19=pinned-VM,
  x27/x28 tags). 4 native cases proven (2+3→5; overflow→boxed double; 1.5+2→3.5; throw→bail). TEMPLATE
  conventions: x1=left/x2=right/x0=result (operands pre-placed in op-arg slots → zero slow-path moves);
  x19=canonical pinned-VM reg (shared const). Standalone callable image — NOT yet wired to live dispatch.
- [done] int32 ARITH FAMILY (verified; each EXECUTES): sub/mul/bitand/bitor/bitxor/lshift/rshift — the
  ACTUAL JSVALUE64 generator paths (sub left-right, bitand and64+single-guard-no-box, bitor or64-no-box,
  bitxor xor32+box, mul negative-zero guard); zero new unsafe (shared reborrow island). op_urshift +
  mul-−0-double deferred (the latter a pre-existing engine-wide evaluator gap, not a JIT defect).
  NEXT: dispatch Stage 1 (full-function 3-pass emitter + branch ops, spec'd baseline-dispatch.md) →
  tier-up trigger + B5-lite handoff → the int-sum-loop milestone (R moves there).
- [done] dispatch Stage 1 (verified): the full-function 3-pass emitter `emit_baseline_function`
  (MAIN/SLOW/LINK, op_enter/mov/ret + arith family + int32 branches; branch-to-bytecode-index resolved
  in LINK forward+backward) — WHOLE FUNCTIONS + native LOOPS execute under W^X (int-sum f(5)=10/f(10)=45).
  S5 one control-flow model; S6 deferred slow cases; fusion deadness-guard + branch bounds-check.
- [next-JIT] U3/U4 LIVE tier-up wiring (V0-V5, spec'd baseline-dispatch.md; S7 Box<Vm> / S8 sync / S9
  ExecutionCounter ratified): the trigger + can_baseline_compile allowlist (use the emitter's Err) +
  JitCode install in select_interpreter_entry_plan + B5-lite frame handoff + set_jit_code_block /
  real-TypeError prereqs → the SYNTHETIC hot-arith r_i lift (R first moves). HONEST CAVEAT: Octane
  material R needs the gated property/call ops (R4 / B5-B6) — arith-only tier-up moves Octane R only
  marginally (Crypto at best).
- [missing] wire arm64_baseline to emit per-opcode via the encoder/finalize (retire the byte-blob /
  re-interpreter shim) + the bytecode-stream cutover + profiling wiring + tier-up.
- [missing] DFG (bytecode→SSA→speculation→SpeculativeJIT+OSR); FTL + B3 + Air + register allocation.

## Structural fidelity
- [done] Phase E: interpreter/mod.rs 41k→33k, all 4 runtime-class stores split to interpreter/*_store.rs.
- [wip] vm/mod.rs (74k) still oversized; existing Rust-only files/types need dedicated structure review.

## Runtime semantics (interpreter-level, broadly working for Octane)
- [done] objects/structures/transitions/Butterfly; LLInt monomorphic Get/Put ICs; calls/constructs/
  BoundFunction; typed arrays (8 Number ctors); Math/Number/String/Array breadth; Yarr regexp engine.
- [missing] full AccessCase taxonomy (multi-hop/transition/megamorphic); full ArrayProfile/ArrayMode;
  full String.prototype + ropes; Date, modules/microtasks; [deferred] Wasm.

## [frozen]
- ARM64 native-entry admission-proof cluster (cfg off-by-default; retained as JIT/GC salvage).
