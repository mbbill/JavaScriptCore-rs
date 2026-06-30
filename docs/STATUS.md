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
- [done] R3 shadow oracle (debug-gated, reversible, **R4-GO**): the arena ACCEPTS+STORES a byte-identical
  twin CoreObjectCell through allocate_cell; byte-equal cross-check at find/find_mut + population check held
  SUITE-WIDE (2740 tests, zero fires); release compiles it ALL out (byte-identical to HEAD, zero extra mem).
  First wiring of the S4 arena into the live engine. Caveat: proves ACCEPT+STORE+population, NOT the live
  deref (re-syncs at read) — the self-aliasing live-deref is R4's miri gate.
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
- [next-GC] leaf-cell migration (String/Symbol/BigInt → arena + sweep) = the REMAINING leak (string-heavy
  benches still OOM); the bounded micro-probe then gates a memory-capped real Octane bench. THEN all 15
  benches can run → R becomes measurable. (Per-slab aux already reclaimed via the free-lists; SD-4 done.)

## Baseline JIT / DFG / FTL (parity lives here; ~0% started)
- [done] JIT↔runtime bridge (D1+D5 reborrow shim, Miri-passed; Vm::operation_* split-borrow wrappers; D3
  jit_pending exception word + far-call; docs/design/jit-runtime-bridge.md).
- [done] per-opcode ARITH lowering (each EXECUTES native under W^X, generator-faithful): op_add int32 fast +
  slow far_call + C-ABI trampoline; int32 family sub/mul/bitand/bitor/bitxor/lshift/rshift; op_urshift +
  mul-−0-double deferred (pre-existing evaluator gap, not a JIT defect).
- [done] dispatch Stage 1 (verified): the full-function 3-pass emitter `emit_baseline_function`
  (MAIN/SLOW/LINK, op_enter/mov/ret + arith family + int32 branches; branch-to-bytecode-index resolved
  in LINK forward+backward) — WHOLE FUNCTIONS + native LOOPS execute under W^X (int-sum f(5)=10/f(10)=45).
  S5 one control-flow model; S6 deferred slow cases; fusion deadness-guard + branch bounds-check.
- [done] U3/U4 LIVE tier-up wiring: hot int-arith functions now tier up on the LIVE entry path
  (execute_code_block entry hook bumps the ExecutionCounter; loop back-edges counted at LoopHint) →
  emit_baseline_function (its Err IS the S4 allowlist; only int32 arith/mov/branch admitted) → install to
  RX → execute NATIVE machine code. Verified in RELEASE (sum(5)==10 native). Faithful to JSC LLInt→Baseline
  (prologue + loop_osr counter, addressForCall). Divergences commented: synchronous compile (S8 vs async
  JITWorklist), B5-lite handoff at next entry not mid-loop OSR (S2), entry-only (nested bytecode calls don't
  re-enter the hook). Unsafe reborrow island adversarially verified sound + HARDENED (nested-park & Vm-pin
  debug guards; compare/truthy shims Miri-clean; valueOf-reentry test normal-profile-green). HONEST CAVEAT:
  arith-only allowlist — Octane material R needs property/call ops (R4 / B5-B6); R UNDEFINED until 15-gate.
- [done] double/float ARITH baseline (verified, EXECUTES native FP in release): FP encoding added to the
  ARM64 encoder (fadd/fsub/fmul/fdiv/scvtf/fmov, byte-oracle-proven) + double fast paths add/sub/mul/div
  (JIT{Add,Sub,Mul,Div}Generator-faithful: int32 fast / branchIfNotNumber→slow / double path) + DivNumber
  allowlisted → double-arith functions tier up. Deferred: LoadDouble (double LITERALS — needed for the asm.js
  mandreel/octane-zlib to tier up), div int32-result fold, NaN significand (faithful, same number).
- [done] the live path emits real per-opcode ARM64 via the MacroAssembler encoder + finalize (f139350);
  the old P6/P15 byte-blob lane is now DEAD — retiring it (~22k LoC) is a DEFERRED off-gate cleanup
  (moves neither R nor 15/15; do it in idle integration capacity, never preempting R4/calls).
- [done] op_call EXECUTES (UNLINKED virtual call; U5 adversarially verified SOUND-AND-FAITHFUL) — the
  biggest R-mover (no Octane fn tiered up before — all contain calls) + the call-heavy gate half. K1 (slot-2
  = real CodeBlock* via the registry Rc::as_ptr) + U2 parking (recursion-local RAII save/restore, nesting-safe)
  + emit_op_call (far_call operation_call; operand mapping faithful vs dispatch_call) + the D1/D5 reborrow
  shim (callee runs DirectInterpreter → NO 2nd whole-Vm &mut, so the nested re-park is unreachable today;
  reborrow shape == the miri-clean add-shim). Milestone: f calls g, native==interp incl. boxed-double + throw
  + 2-deep nesting (DEBUG+RELEASE); suite 2781. op_call tests are FFI-blocked under miri (mmap arena) — the
  reborrow miri proof rides the analogous add-shim test. B5-full native bl-chain/direct-link DEFERRED
  (slow-call now). RESIDUAL: a native callee tier-up under op_call needs its own sibling-aliasing re-verify FIRST.
- [done] **GATE-CAPABILITY SET**: int+double arith + LoadDouble + typed-array get/put_by_val (slow-call IC)
  + op_call all EXECUTE native → asm.js functions can tier up WHOLE.
- [BLOCKED — measured 06-29] the baseline JIT is a NET REGRESSION on arm64 (geomean ~0.64x; richards/delta-blue
  ~3x slower; raytrace/earley DNF). DEFAULT FLIP HELD (would move R DOWN). Cause = the CALL path: callee "native
  entries" are x86_64 byte-seqs (HostBlockedX86_64), so ~3.6M generated-direct-call transactions fall back to a
  nested interpreter while paying per-call route/accounting. SUSPECTED LOAD-BEARING DIVERGENCE: the generated-*
  call/tier layer (route/transaction/native-entry-kind — NO JSC counterpart vs CallLinkInfo+thunks). Strategic
  divergence-assessment in flight (CORRECT it, don't build B5-full on it). Faithful fix → arm64-callable
  CallLinkInfo entry + delete route/transaction layer → native breadth → THEN flip default → R moves.
- [missing] bytecode-stream cutover + baseline profiling emission (ValueProfile/ArithProfile, a DFG
  prereq downstream of R4/calls broadening the allowlist).
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
