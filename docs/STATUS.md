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
- [done] profiling: ArithProfile + ExecutionCounter (faithful bitfields) + SpeculatedType u64 bitset (canonical
  in DFG/DOMJIT); profile-slot derivation + **POPULATION ROUND COMPLETE** — value (named-loads/scope/by-val/
  lengths/calls), array (by-val reads+writes/lengths/InByVal), and binary+unary arith profiles all record live
  at LLInt-faithful sites (`c650d48`/`8a2b5e7`/`1f53724`/`5f45ab9`; closes the DFG `SpecNone→ForceOSRExit`
  hazard for the wired set). [missing] U8 argument profiles; getter-resume value-profile write; construct-result
  profiles.
- [done] bytecode: faithful packed instruction-stream core (Vec<u8>, byte-offset index, width-aware); mov/ret
  wedge LIVE + hardened (instruction-start gating, constant-index placement, canonical constant bands); W1 real
  generated opcode ids + sub/mul rows (5d455f1). **GENERATOR TRACK COMPLETE (G1-G3):** OperandKind full
  18-variant stream-operand census (`833592d`); JSC's own generator emits all 193 opcode rows (`ee174a7`); that
  generated table IS the crate's live `OPCODE_TABLE` (`7accf10`) — the packed stream DECODES every JSC bytecode.
  [deferred] G4: the CoreOpcode identity cutover (~8k refs) + per-opcode execution admission (separate track).

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

## GC / value cutover (toward R4 — see docs/design/gc-r4.md, the arena cell identity the JIT emits)
- [done] the arena + marking core (S4 blocks/free-list/SlotVisitor STW marking) + Structure-wire (the #1
  divergence corrected — per-cell offset map → per-shape Structure::PropertyTable) + the full POD object
  model (Butterfly + GetterSetter + all 9 per-kind Drop fields relocated to store-owned aux slabs —
  `CoreObjectCell` is POD, `needs_drop::<CoreObjectCell>()==false` compiles) + collector trace/sweep authored
  + the R4a cell-identity flip (IRREVERSIBLE: identity = raw MarkedSpace address) + R4b mark/reconcile/sweep +
  the R4b LIVE DRIVER (byte-counter-triggered at back-edge/VM-entry safepoints). Each stage independently
  verified (miri Stacked+Tree-Borrows 0 UB, adversarial VERIFIER, ≥2-collection survival tests). Full
  R3→R4a→R4b history: `a01d071`/`243b89d`/`4c17801` + docs/design/gc-r4.md.
- [done] **GC ROUND COMPLETE — THE ARENA IS LEAK-FREE END-TO-END.** All four cell kinds now reclaim: object
  (R4b live driver, above), string (U0/U0b/U1), bigint (POD limb slab + weak intern, `354cb89`), symbol (POD
  cell + Symbol.for-registry/well-known/property-key-only root classes, `c9c3227` — the LAST leaking cell
  store). Plus: a faithful weak-finalization seam + WeakMap/WeakSet ephemeron semantics (mark → finalize-
  unconditional → reconcile → sweep, mirroring `Heap::runEndPhase`; repeat-until-empty ephemeron fixpoint over
  marked WeakMaps, `3ad0ab7` — the U7 `visitWeak` unit); CodeBlock constant-pool rooting closed a latent
  constant-cell UAF (`f213265` — live CodeBlocks act as root providers via host_roots, CodeBlocks stay
  non-arena). All independently adversarially verified; 2934 tests + miri TB 0 UB.
- [missing] the scoped native-stack conservative scan (GAP C, gated on the JSStack B4b/B6 migration);
  generational/incremental collection; heap cell-id table cleanup for eager-bound strings (bounded follow-up,
  arena leak already fixed).

## Baseline JIT / DFG / FTL (parity lives here; baseline on the native stack + first native call; DFG/FTL 0%)
- [done] JIT↔runtime bridge (D1+D5 reborrow shim, Miri-passed; Vm::operation_* split-borrow wrappers; D3
  jit_pending exception word + far-call; docs/design/jit-runtime-bridge.md).
- [done] per-opcode ARITH lowering (each EXECUTES native under W^X, generator-faithful): op_add int32 fast +
  slow far_call + C-ABI trampoline; int32 family sub/mul/bitand/bitor/bitxor/lshift/rshift; op_urshift +
  mul-−0-double deferred (pre-existing evaluator gap, not a JIT defect).
- [done] dispatch Stage 1 (verified): the full-function 3-pass emitter `emit_baseline_function`
  (MAIN/SLOW/LINK, op_enter/mov/ret + arith family + int32 branches; branch-to-bytecode-index resolved
  in LINK forward+backward) — WHOLE FUNCTIONS + native LOOPS execute under W^X (int-sum f(5)=10/f(10)=45).
  S5 one control-flow model; S6 deferred slow cases; fusion deadness-guard + branch bounds-check.
- [done] U3/U4 LIVE tier-up wiring: hot functions tier up on the live entry path (ExecutionCounter bumped at
  entry + loop back-edge → emit_baseline_function [its Err IS the S4 allowlist] → install RX → execute native).
  Faithful LLInt→Baseline (sync-compile S8 + B5-lite-handoff S2 divergences commented); reborrow island miri-clean.
- [done] double/float ARITH baseline (verified, EXECUTES native FP in release): FP encoding added to the
  ARM64 encoder (fadd/fsub/fmul/fdiv/scvtf/fmov, byte-oracle-proven) + double fast paths add/sub/mul/div
  (JIT{Add,Sub,Mul,Div}Generator-faithful: int32 fast / branchIfNotNumber→slow / double path) + DivNumber
  allowlisted → double-arith functions tier up. Deferred: LoadDouble (double LITERALS — needed for the asm.js
  mandreel/octane-zlib to tier up), div int32-result fold, NaN significand (faithful, same number).
- [done] the live path emits real per-opcode ARM64 via the MacroAssembler encoder + finalize (f139350);
  the old P6/P15 byte-blob re-interpreter lane is DEAD (retirement = STEP 5/6 off-gate hygiene, see design doc).
- [done] op_call SLOW path EXECUTES (far_call operation_call; U5-verified sound; K1 real CodeBlock* + parking;
  native==interp incl. boxed-double/throw/2-deep). Now the SLOW path beneath A1.2's native fast path (below).
- [partial] GATE-CAPABILITY SET: native arith/LoadDouble/typed-array get/put_by_val/op_call exist, but
  asm.js still DNF under execoff (5 opcode declines + op_urshift fix); flip deferred (dfg-path.md).
- [done] **STACK MODEL DECIDED + A1.0–A1.3 LANDED — the baseline JIT runs on the NATIVE machine stack + the
  FIRST JIT→JIT NATIVE CALL works** (faithful Option A, judge-panel ratified; jsstack.md "B5/STACK MODEL").
  A1.0/A1.1: prologue flipped to `push_pair(fp,lr); mov fp,sp`, entry seeded on the native stack via the sibling
  sp-switch trampoline (no B-fallback; existing tests byte-identical). A1.2/A1.3: native op_call fast path
  (calleeFrame on the native stack, sp=calleeFrame+16, blr to the resolved entry; CallLinkInfo→entry resolution)
  — proof passes (callerFrame@0/returnPC@1 adjacent, contiguous callee frame). A1.4 (prologue stack-overflow
  check → RangeError) + A1.5 (scoped conservative GC scan of native-stack JIT frames → cells) LANDED.
- [done] **BROAD ENGAGEMENT — real op_calls run NATIVE + beat the interpreter**: live op_call resolves the
  callee's installed native entry per call (the registry addressForCall analog) + blr's it when the callee is
  JIT'd (else operation_call slow path; resolver returns 0 for host/constructor/arity-mismatch — faithful). The
  cell-free gate is LIFTED (A1.5 roots native frames). MEASURED ~39× vs interp on a call probe (native ≪ interp;
  the 39× is pre-GC-interp-inflated, not steady-state); native==interp==oracle. BYPASSES the generated-* arbiter.
  LATE-5: execoff/interp geomean ~1.086 (net win, mixed) but still ~1e-3 vs C++; next R-lever is the
  DFG precursor set, not baseline breadth. Deferred local wins: CallLinkInfo cache/opcode admits; cheap
  exception = remaining parked-CodeBlock divergence in global get/put shims.
- [measured 06-29 / CONFIRMED DIVERGENCE — docs/design/baseline-call-tier-divergence.md] the OLD generated-*
  call/tier layer (generated_executor RE-INTERPRETER + VmGeneratedDirectCallTransactionRoute arbiter + P6X86_64
  entry) is a NET REGRESSION (geomean ~0.64x opt-in, default flip HELD) with NO JSC counterpart (C++ grep = 0).
  The faithful native path (A1.x above: CallLinkInfo + native-stack bl) now BYPASSES it. Plan: broaden the
  faithful path → beats interp → flip default; then delete the dead generated-* cluster (STEP 5) + de-megafile
  35k tiering.rs (STEP 6, off-gate). STEP 1 (collapse the slow-path dispatch onto CallLinkInfo) = R-neutral cleanup.
- [wip] DFG precursor set (docs/design/dfg-path.md): packed wedge + SpeculatedType + profile storage/derivation/
  POPULATION all LANDED (foundation section above); JITCodeMap persisted on baseline images (bci→machine-code
  OSR landing map, U1); faithful NodeType/NodeFlags/VariableAccessData/Operands LANDED — abstract Rust-only DFG
  taxonomy DELETED, graph starts LoadStore. Ratified: first OSR exit lands in the INTERPRETER (exitToLLInt
  analog) — bailout hard gate before SPECULATIVE DFG only. **FIRST DFG PARSER LANDED** (`src/dfg/parser.rs`,
  `c164345`): single-BB non-speculative slice (`op_enter {mov|add|sub|mul}* op_ret`), type-agnostic (SpecNone),
  declines every getPrediction opcode — no plan/phases/speculation/codegen yet.
- [missing] DFGPlan analog (graph creation + identity stamping, in flight); DFG speculation
  (bytecode→SSA→SpeculativeJIT+OSR); FTL + B3 + Air + register allocation.

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
