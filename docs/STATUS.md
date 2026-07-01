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
  in DFG/DOMJIT); profile-slot derivation for ALL profile-carrying opcodes + Binary/Unary ArithProfile
  storage/record APIs (F0). [wip] population U1-U8 (4 parallel units: named-loads/scope, by-val+length,
  binary arith slow-path-only, unary arith).
- [done] bytecode: faithful packed instruction-stream core (Vec<u8>, byte-offset index, width-aware); mov/ret
  wedge LIVE + hardened (instruction-start gating, constant-index placement, canonical constant bands, ONE
  opcode table, JSC byte fixtures). [done] W1: real generated opcode ids + sub/mul rows (5d455f1).

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
- [done] GetterSetter infra B-i/ii/iii (Accessor attribute bit 1<<4, GetterSetter cell, symbol+accessor
  keys get REAL Structure offsets) — dual-write stage, folded into the B-iv flip below.
- [done] B-iv FLIP (irreversible, 66a860a): the per-cell properties HashMap (value authority) DELETED —
  reads route structure offset → butterfly slot; accessors via the butterfly GetterSetter; property_order
  folded into PropertyTable entry order; vestigial deleted_offsets dropped (recycle owned by m_deletedOffsets);
  in-place data↔accessor conversion now offset-stable (corrects a pre-flip offset-vanish defect). Gated by a
  randomized HashMap-oracle equivalence test (per-op get/enum/accessor diff). needs_drop POD assert still
  waits for the OTHER per-kind units.
- [done] per-kind POD-ification COMPLETE (all 6 units, cheapest-first, serial): bound_args /
  promise_reactions / regexp_source / array_buffer_data / map_entries / set_values / captures /
  instance_fields all relocated to store-owned aux slabs via POD Copy handles, regexp_flags_text deleted
  (recompute from bits). **CoreObjectCell is now POD — `const _ = assert!(!needs_drop::<CoreObjectCell>())`
  COMPILES (atomic sweepability proof).** Documented deferred-faithful deviations: Map/Set JSOrderedHashTable
  (O(1)), captures JSLexicalEnvironment; instance_fields key interned to a POD AtomId. Each aux slab still
  holds GC edges (except ArrayBuffer raw bytes) → the collector trace must visit them.
- [done] collector TRACE (GAP A) authored (unwired, R4-gated): CoreObjectStore::trace_cell visits the 15
  inline RuntimeValue edges + butterfly (props+elements) + the value aux slabs (bound_args/captures/
  instance_fields/map/set/promise_reactions), skips the non-edge slabs (regexp String, ArrayBuffer bytes);
  targets RuntimeValue via as_cell (GAP D honored, NOT the skeleton JsValue path), through a minimal
  CellEdgeVisitor trait. R4's collector driver supplies the adapter (CellValue bits → arena addr → Tracer).
- [done] collector SWEEP (GAP B) authored (unwired, R4-gated): FreeList::sweep_block mirrors
  MarkedBlock::specializedSweep<DoesNotNeedDestruction> — scans the mark bitmap, threads unmarked atoms into
  the FreeList (legal precisely because needs_drop==false → no destructors), retains marked + newly-allocated,
  rebuilds the interval free-list. MIRI-CLEAN (Stacked + Tree Borrows, 0 UB) over the demo POD cell. R4 drives
  it stopAllocating→sweep→resumeAllocating across the directories.
- [done] R4a cell-identity FLIP (IRREVERSIBLE, verified sound-and-complete): CoreObjectCell identity = the
  raw MarkedSpace arena address; DELETED the leaking Vec<Pin<Box>> object stores + object_indices_by_payload
  + the R3 shadow. MarkedSpace::find (isPointerGCObjectJSCell port) is the object-vs-foreign TYPE GATE (leaf
  String/Symbol/BigInt cells stay in their own Vec stores → Box addr ∉ arena block → None → no type-confusion
  deref); cell_at(&self)/with_cell_mut(&mut self) deref islands; ~30 find_mut → with_cell_mut (find() stays,
  132 read sites untouched); self-aliasing copy-out is COMPILER-ENFORCED by the safe API. Gate (TECHNICAL,
  the leak forbids benches pre-R4b): 2750 tests + miri tree-borrows 0 UB (deref/butterfly/self-aliasing/
  type-gate) + release round-trip + INDEPENDENT adversarial verify = sound-and-complete (7/7 refutations
  failed). Decision D (ptr<<8 / ptr<2^41) confirmed in release; B: find_by_object_id uses a store-local
  CellId→addr index (heap unreachable) — **R4b's sweep MUST invalidate stale entries**; C: CoreObjectStore::
  clone deleted. NIT: vestigial shadow fns + dead CoreObjectCell Clone to prune.
- [done] R4b-mark — the marking half (verified, unwired): the MEMBERSHIP-ONLY gate is_arena_cell (= find
  MINUS is_live_cell — the #1 UAF landmine; a test proves it admits a post-sweep survivor that liveness-find
  REJECTS) + clear_all_marks + the CellEdgeVisitor/VisitChildren mark adapter over trace_cell/SlotVisitor +
  gather_all_gc_roots (register file + frame callee + exceptions + the ~25 CoreObjectStore intrinsic roots +
  jit_pending; microtask queue not-yet-a-live-source, lexical_scope transitively rooted via the captures
  slab — both with evidence). 2761 tests + miri TB 0 UB (incl. the ≥2-collection survivor test). MARK-ONLY →
  nothing freed, no UAF surface yet.
- [done] R4b-sweep MECHANISM (force_collect, verified, NOT yet live-wired): for_each_object_cell + a
  store-driven PRE-SWEEP reconcile (reads each DEAD cell's handles via an AUTHORITATIVE live-set — needed
  because a never-allocated zeroed slot decodes Handle(0) aliasing a LIVE slab — frees its butterfly+aux slots
  via 9 per-slab free-lists [allocate_* reuse them], drops the reverse-index, BEFORE sweep_block clobbers it)
  → FreeList::sweep_block (multi-block). force_collect = mark → reconcile → sweep. PROVEN: the bounded no-OOM
  micro-probe returns to EXACTLY baseline (43 cells/43 slots) after every collection with the slab bounded
  (the LEAK IS FIXED); ≥2-collection landmine (s2.reclaimed==0); free-list reuse; self-aliasing under
  collection; miri TB 0 UB; whole suite 2766 green (force_collect explicit/unwired).
- [done] R4b LIVE DRIVER — **the object-cell collector now RUNS**: byte-counter trigger in allocate_blob
  (4MB prod / 16KB cfg(test)) arms a request; collected at the back-edge / VM-entry safepoint
  (DeferToVm-gated; NO inline collection → re-entrancy foreclosed) via gather_all_gc_roots → force_collect,
  STW-flagged. An adversarial verify caught + we FIXED a REAL mass-UAF: the global object (Program/Eval
  this_value) + the host global lexical let/const/class bindings were NOT rooted → top-level functions/
  constructors would be swept on the 2nd collection; now gathered (≥2-collection survival tests). #3 builtin
  callbacks proven sound by construction (DirectInterpreter-inherited → poll suppressed, tested); #1 baseline
  frames confirmed forward-only (arith-only/cell-free) + documented. 2770 tests + miri TB 0 UB on the live
  cycle. **THE OBJECT-CELL LEAK IS FIXED LIVE** (micro-probe returns to baseline).
- [done] STRING-cell GC (U0/U0b/U1) — the string leak CLOSED: U0 type-dispatched marker/reconcile by cell
  js_type; U0b the mutator isObject() gate (the faithful isPointerGCObjectJSCell-then-isObject the port had
  collapsed); U1 CoreStringCell→POD arena cell + string_texts slab + rope fiber edge + weak interning removal.
- [missing] U2/U3 symbol+bigint leaf GC (share U0b); U7 visitWeak (CLEAR/RELINK phase); A1.5 scoped native-stack
  JIT-frame scan (in flight). Heap cell-id table not cleaned for eager-bound strings (follow-up; arena leak fixed).

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
- [wip] DFG precursor set (docs/design/dfg-path.md): packed wedge + SpeculatedType + profile storage/derivation
  LANDED (foundation section above); JITCodeMap persisted on baseline images (bci→machine-code OSR landing map,
  U1); faithful NodeType/NodeFlags/VariableAccessData/Operands LANDED — abstract Rust-only DFG taxonomy DELETED,
  graph starts LoadStore. Ratified: first OSR exit lands in the INTERPRETER (exitToLLInt analog) — bailout hard
  gate before SPECULATIVE DFG only. [wip] P2 src/dfg/parser.rs (launched).
- [missing] DFG proper (bytecode→SSA→speculation→SpeculativeJIT+OSR); FTL + B3 + Air + register allocation.

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
