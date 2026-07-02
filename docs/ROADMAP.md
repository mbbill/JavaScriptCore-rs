# Roadmap — to Octane R ≥ 1.0

The plan and the workload tracker for the Rust JavaScriptCore rewrite. `README.md`
is the bounded *current-status* snapshot; this file is the *plan* (unbounded, but
keep it the plan, not history — history is `git log`). Keystone *designs* live in
`docs/design/`. The contract is `CLAUDE.md`.

**The goal restated:** `R = geomean(Rust per-bench)/geomean(C++ jsc per-bench) ≥ 1.0`
on JetStream 3 Octane, same machine/inputs/scoring, **all 15 benches passing first**.
Until all 15 complete+validate the suite yields no geomean and R is undefined.

**The one load-bearing fact:** the native baseline JIT now beats the interpreter on the
5 compute/call benches (LATE-5 geomean execoff/interp ~1.086), proving native execution
is real, but the interpreter is still ~500–6000× slower than local C++ `jsc`, so this
is still around the ~1e-3 floor. **R ≥ 1.0 lives in the optimizing JIT (DFG → FTL/B3).**
That is why the priority order below is the JIT dependency chain, not "fix more benches"
or "flip a still-floor default" — and any work off that chain must justify itself with
a hard dependency (Principle #1).

---

## Workload tracker (% of total project effort)

JSC's optimizing tiers alone are ~280k LoC, so the JIT dominates the remaining work
*and* is the only thing that moves R off ~0.001. Percentages are estimates of total
effort, engine-from-scratch to R ≥ 1.0.

| # | Workstream | % total | Done | Notes |
|---|---|---|---|---|
| 1 | Interpreter + parser + bytecompiler + runtime/builtins (run all 15 correctly) | 27% | ~90% | 13/15 validate; 2 asm.js DNF (JIT-gated); correctness tail remains |
| 2 | Faithful foundation (value/GC-arena/Structure/strings/profiling/bytecode) + Phase E + throwers + call-link | 13% | ~98% | built; profile population round COMPLETE (value+array+binary/unary arith all record live, `c650d48`/`8a2b5e7`/`1f53724`/`5f45ab9`); generated 193-opcode table IS `OPCODE_TABLE` (G1–G3, `833592d`/`ee174a7`/`7accf10`) |
| 3 | Assembler codegen (operands→encoder→LinkBuffer→W^X execution) | 3% | 100% | emit→relocate→execute proven |
| 4 | R scoreboard / measurement harness | 1% | 100% | local C++ `jsc` release build + Rust release, identical harness; C++ baseline is the measuring instrument and must be re-measured |
| 5 | JSStack execution substrate (stack model + entry + frames) | 5% | ~70% | stack model DECIDED (Option A = native stack); B1–B4 + A1.0/A1.1 native-stack entry LANDED; A2 interp-migration + arity/varargs remain |
| 6 | GC/value cutover: POD object model + R4 + running collector | 7% | ~90% | **arena LEAK-FREE end-to-end**: object (R4b) + string (U0/U1) + bigint (`354cb89`) + symbol (`c9c3227`, the last leaking cell store) all reclaim; weak-finalization seam + faithful WeakMap/WeakSet ephemeron semantics (`3ad0ab7`); CodeBlock constant-pool rooting closed a latent UAF (`f213265`). Remaining: scoped native-stack conservative scan (GAP C), generational/incremental |
| 7 | **Baseline JIT** (per-opcode machine code + native calls + profiling + tier-up) | 10% | ~35% | native path measured net-win over interpreter (LATE-5 geomean execoff/interp ~1.086, mixed); further baseline breadth is now local-win/deferred except bailout soundness (JITCodeMap OSR landing map landed) |
| 8 | **DFG** (bytecode→SSA→speculation→SpeculativeJIT+OSR) | 18% | ~4% | precursors landing: packed wedge live+hardened, SpeculatedType canonical, faithful NodeType/flags/VAD skeleton, profile storage+derivation+population all DONE; W1 ids+sub/mul landed; **first bytecode→DFG parser LANDED** (`src/dfg/parser.rs`, `c164345` — single-BB non-speculative slice, no plan/phases/codegen yet) |
| 9 | **FTL + B3 + Air** (top tier + optimizer + register allocation) | 15% | 0% | — |
| 10 | Final correctness + perf tuning to hit R ≥ 1.0 | 1% | 0% | the last mile |

**≈ 55% by effort.** But by *measured R* we are near the start: rows 7–9 (43% of the
whole project, still ~2% done for the optimizing tiers) are the only rows that lift R from
~0.001 to ≥ 1.0. The baseline JIT now proves native execution can beat the interpreter, but
DFG/FTL are what move R by orders of magnitude. R itself did not move this round — this closes
foundational GC/profiling/bytecode dependencies the DFG/JIT need.

---

## Critical path (dependency order to the optimizing JIT)

The baseline JIT now runs and is a net win over the interpreter, but R ≥ 1.0 lives in the DFG/FTL. The next dependency order is:

1. **JSStack / stack model** (row 5) — DECIDED + foundation LANDED. Judge panel ratified
   **Option A: native machine stack = JS stack** (FP/SP unified, callerFrame@0+returnPC@1 adjacent —
   the layout DFG/FTL OSR + stack-walk + GC assume; Option D split was unanimously fatal).
   `docs/design/jsstack.md` "B5/STACK MODEL". **A1.0+A1.1 LANDED (5ec4cef): the baseline JIT runs on
   the native stack (faithful `push_pair(fp,lr); mov fp,sp`; entry via the doVMEntry-analog seed +
   sp-switch trampoline). A1.2 (first JIT→JIT native call = R-lever existence proof) in flight.** Then
   A1.4 stack-check, A1.5 scoped JIT-frame GC scan (before native-call args carry cells), A2 (interpreter
   onto native CallFrames).
2. **GC / cell identity (R4)** (row 6) — object-cell R4a GC is **LIVE/DONE** (POD `CoreObjectCell` in
   the arena, raw-address identity, real mark→reconcile→sweep→free-list, auto-triggered at a byte counter
   polled at interpreter back-edges). **The GC round is now COMPLETE — the arena is LEAK-FREE
   end-to-end**: string (U0/U0b/U1, b73d806), bigint (`354cb89`), and symbol (`c9c3227`, the LAST leaking
   cell store) all reclaim; a faithful weak-finalization seam + WeakMap/WeakSet ephemeron semantics
   landed (`3ad0ab7`, the `visitWeak` unit); CodeBlock constant-pool rooting closed a latent constant-cell
   UAF (`f213265`). `docs/design/gc-r4.md` + `gc-r4-completion.md` (see the 2026-07-02 status-correction
   header — the design doc's prose predates this arc). Remaining: the scoped native-stack conservative
   scan (GAP C), generational/incremental collection — neither gates the DFG.
3. **Packed bytecode stream LIVE cutover** (row 2/8 dependency) — JSC has one flat byte stream consumed
   by LLInt/Baseline/DFG/FTL. First live wedge LANDED (raw mov/ret: byte-offset PC +
   `Fits<VirtualRegister>` constants) and correctness-HARDENED (instruction-start gating — no
   mid-instruction decode, constants placed by constant index, canonical constant bands, ONE opcode
   table, JSC-derived byte fixtures). W1 landed: real generated opcode ids (5d455f1) + sub/mul rows.
   **The generator track is now COMPLETE (G1–G3): the OperandKind stream-operand set is the full
   18-variant census (`833592d`), the JSC-generator-run table emits all 193 opcode rows (`ee174a7`),
   and that generated table IS the crate's live `OPCODE_TABLE` (`7accf10`) — the packed stream can
   DECODE every JSC bytecode.** G4 (the CoreOpcode identity cutover, ~8k refs, + per-opcode execution
   admission) remains, as its own serial track. See `docs/design/dfg-path.md`.
4. **Parallel DFG precursor set** — SpeculatedType canonicalization DONE (canonical u64 bitset in
   DFG/DOMJIT descriptors). Profile STORAGE + derivation + **POPULATION are now ALL DONE**: the four
   parallel population units landed (`c650d48` unary arith, `8a2b5e7` named-loads/scope, `1f53724`
   binary arith, `5f45ab9` by-val/length value+array) — value, array, and arith profiles record live at
   every LLInt-faithful site, closing the `SpecNone→ForceOSRExit` hazard for the wired opcode set.
   Remaining profile units (not hard-gating): U8 argument profiles, the getter-resume write,
   construct-result profiles. Baseline-as-bailout: JITCodeMap LANDED (bci→machine-code landing map
   persisted on baseline images, U1); exit-target RATIFIED — the first OSR exit lands in the
   INTERPRETER (exitToLLInt analog), so the bailout hard gate sits before SPECULATIVE DFG only.
5. **First DFG parser — LANDED** (`src/dfg/parser.rs`, `c164345`): single-basic-block, non-speculative,
   fallback-heavy (matching JSC's first DFG scoping), lowering `op_enter {mov|add|sub|mul}* op_ret`
   into a faithful `DfgBasicBlock` (`LoadStore` form, as JSC's does) over the faithful NodeType/
   NodeFlags/VariableAccessData/Operands skeleton (abstract Rust-only taxonomy DELETED). Lowers
   type-agnostically (SpecNone), declining every getPrediction opcode — no plan/phases/speculation/
   codegen yet. Next: a DFGPlan analog owning graph creation + identity stamping.
6. **DFG speculation + live OSR exit → FTL/B3** — the optimizing tiers that take R to ≥ 1.0.

These can fan out where independent (the JSStack substrate and the GC/POD-cell work are
different subsystems), but each touches megafiles serially at its cutover, so integration
capacity is the bottleneck (Principle #3).

### The baseline JIT is NOT one monolithic gated block (B7 audit 2026-06-28)

A read-only audit of `src/jit/arm64_baseline.rs` (today only a return-seed proof-of-pipeline
lane, emitting via hand-rolled encoders) found the baseline JIT splits into an **unblocked
arith core** and **gated IC/call parts** — so its foundation advances IN PARALLEL with the GC
and substrate tracks, additive in `jit/`+`assembler/` (no megafile conflict):

- **WIREABLE TODAY** (frame arena live post-B4, value rep satisfied, integer/branch ops exist):
  op_mov, int32 op_add/sub/mul/bitand/bitor/bitxor/lshift/rshift (fast path + slow-call),
  op_jless/jgreater/jlesseq/jtrue/jfalse. Needs only: (1) the MacroAssembler **box/tag layer**
  (or64/and64/xor64, branch_mul32, branch_test64, + a `jit/assembly_helpers.rs` AssemblyHelpers
  analog: branchIfNotInt32/boxInt32/numberTag model — the assembler has ZERO JSValue-tag
  awareness today); (2) ARM64 per-LoweredOperation encoders mirroring `jit/emitter.rs`
  (the bytecode→selection contract already models Move/AddInt32/… ); (3) a slow-path C-call
  shim; (4) bytecode→selection→ARM64 dispatch + tier-up.
- **Slow-call mechanism (audited 2026-06-28) — mostly ALREADY BUILT, non-gated on B5/B6** (it's a
  C call returning to the SAME JS frame, not a JS-frame push): the `SlowCaseEntry` linkage
  (record/link slow cases by bytecode index + fast-path-resume) is faithful in the
  `arm64_baseline.rs` control-flow builder, and the ABI clobber policy is faithful in `jit/abi.rs`
  (`BaselineRuntimeCallClobbers`). numberTag x27 / notCellMask x28 are **callee-saved** (preserved
  across the C call — NO re-materialization; corrects the B7 note). The interpreter ALREADY has the
  faithful evaluators (arithmetic/bitwise/compare `*_binary_result`). GAP = a thin `jit/operations.rs`
  (JITOperations analog) with one `extern "C"` shim per op (decode NaN-box u64 → RuntimeValue → call
  the existing evaluator → re-encode; set VM::exception on throw) + a MacroAssembler far-call
  (move_imm64+blr, primitives exist) + arg-marshal + exception-check + topCallFrame/CallSiteIndex
  store. The ONE open design point (R1): the exception-handler jump target — a first cut bails to a
  generic throw/interpreter stub (int-arith ops only throw via valueOf on object operands, rare).
- **R4 IS DONE** (the live object-cell GC runs; cell identity = raw arena address). CORRECTION
  (flip-gate survey 2026-06-30): the asm.js 15/15 gate is **not** currently satisfied and the earlier
  "K2-free → can tier up whole" claim was overbroad. Typed-array HEAP access is separate from K2, but
  mandreel/octane-zlib hot functions still decline under execoff on UnsignedRightShiftInt32 / ModNumber /
  LogicalNot / BitNotInt32 / standalone LessEqualInt32 plus an op_urshift evaluator fix. The flip is
  deferred behind the DFG path because it would only define R at the ~1e-3 floor.
- **op_call** (the call-heavy gate half AND the biggest R-mover — today NO real Octane fn tiers up
  because all have calls): needs K1 (CodeBlock real pointer, localized `Vec<Pin<Box>>`, NOT the
  ~1476-ref CodeBlockId cutover) + B5 (native callee-frame seed; B4 arena-window done) + the
  recursion-local parking save/restore correction. **NOT the owner-gated B6.**
- **K2 — named-property get_by_id/put_by_id ICs** (the inline/out-of-line storage split:
  PropertyInlineCache + INLINE_CAPACITY>0 + negative-indexed butterfly) is **DEFERRED PAST the gate**:
  it is the post-gate R-mover for the property/call-heavy benches, NOT a 15/15 gate item.

So the baseline-JIT arith core is its own parallel track (box/tag layer → per-opcode encoders →
slow-call shim → dispatch), started now; the property-access ICs and calls plug in once R4 and
B5/B6 land. R moves materially only once the IC/call tiers are JIT-compiled, but the arith core
builds the mandatory, reusable scaffolding (dispatch, tier-up, slow-call, box/tag) the higher
tiers all sit on — pipelining, not a rabbit hole.

## Parallel, non-blocking correctness work (protects the gate, schedulable any time)

- **typescript throw — RESOLVED**: the Array-`length` get/set value-divergence was fixed;
  typescript now runs zero-throw (parseErrors=192==jsc) and completes. It's the 13th completer
  (interpreter-slow). See docs/STATUS.md.
- **StringImpl-swap** — wire the faithful StringImpl into the live string store (UTF-8→
  Latin-1/UTF-16); a faithful representation correction that helps string-heavy benches.
- **Structure-wire** — invert property-offset ownership to the Structure (part of the
  POD-cell work for R4); a faithful divergence correction.

## Decisions settled (don't re-litigate; evidence in docs/design + git log)

- **JSStack = native-thread-stack** (immovable Register reservation), reject `Vec<Register>`:
  the JIT bakes FP offsets into instructions (a Vec reallocates and invalidates them), and
  Vec is the mcts-recorded superseded CLoopStack model. See `docs/design/jsstack.md`.
- **Parity is JIT-gated**, proven by the measured scoreboard — not by assertion.
- **No manufactured gates:** R4/JSStack cutovers are the orchestrator's calls to make and
  drive; irreversible steps gate on *technical verification*, not human sign-off.
