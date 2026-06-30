# Roadmap — to Octane R ≥ 1.0

The plan and the workload tracker for the Rust JavaScriptCore rewrite. `README.md`
is the bounded *current-status* snapshot; this file is the *plan* (unbounded, but
keep it the plan, not history — history is `git log`). Keystone *designs* live in
`docs/design/`. The contract is `CLAUDE.md`.

**The goal restated:** `R = geomean(Rust per-bench)/geomean(C++ jsc per-bench) ≥ 1.0`
on JetStream 3 Octane, same machine/inputs/scoring, **all 15 benches passing first**.
Until all 15 complete+validate the suite yields no geomean and R is undefined.

**The one load-bearing fact:** the measured scoreboard (README) shows compute-bound
`r_i` at 5e-4–2e-3 (~500–6000× slower than C++), and the 3 failing benches are asm.js
(can't complete under the interpreter) + a correctness bug. **Both the parity gap and
the completion gate are gated on the optimizing JIT.** That is why the priority order
below is the JIT dependency chain, not "fix more benches" — and any work off that
chain must justify itself with a hard dependency (Principle #1).

---

## Workload tracker (% of total project effort)

JSC's optimizing tiers alone are ~280k LoC, so the JIT dominates the remaining work
*and* is the only thing that moves R off ~0.001. Percentages are estimates of total
effort, engine-from-scratch to R ≥ 1.0.

| # | Workstream | % total | Done | Notes |
|---|---|---|---|---|
| 1 | Interpreter + parser + bytecompiler + runtime/builtins (run all 15 correctly) | 27% | ~90% | 12/15 validate; typescript value-bug + correctness tail remain |
| 2 | Faithful foundation (value/GC-arena/Structure/strings/profiling/bytecode) + Phase E + throwers + call-link | 13% | ~95% | built, mostly unwired |
| 3 | Assembler codegen (operands→encoder→LinkBuffer→W^X execution) | 3% | 100% | emit→relocate→execute proven |
| 4 | R scoreboard / measurement harness | 1% | 100% | both engines, identical harness |
| 5 | JSStack execution substrate (B1–B7 migration) | 5% | ~55% | B1–B4 done (arena=live register window); B5–B7 to go |
| 6 | GC/value cutover: POD object model + R3 → R4 + running collector | 7% | ~10% | arena+SlotVisitor built, not wired |
| 7 | **Baseline JIT** (per-opcode machine-code codegen + profiling wiring + tier-up) | 10% | ~5% | codegen layer ready, not emitting per-opcode |
| 8 | **DFG** (bytecode→SSA→speculation→SpeculativeJIT+OSR) | 18% | 0% | — |
| 9 | **FTL + B3 + Air** (top tier + optimizer + register allocation) | 15% | 0% | — |
| 10 | Final correctness + perf tuning to hit R ≥ 1.0 | 1% | 0% | the last mile |

**≈ 40% by effort.** But by *measured R* we are near the start: rows 7–9 (43% of the
whole project, ~0% done) are the only rows that lift R from ~0.001 to ≥ 1.0. Rows 5–7
are where R *first becomes defined and moves above the interpreter floor*.

---

## Critical path (dependency order to the running baseline JIT)

A running baseline JIT — the first thing that moves R — needs, in dependency order:

1. **JSStack substrate** (row 5) — the JIT emits FP-relative frame access (`ldr/str
   [x29,#off]`) at the JSC offsets; this is the immovable Register stack it addresses.
   `docs/design/jsstack.md`. **Status: B1–B4 done -- the arena IS the live register window
   (read-flip proven byte-faithful vs the Vec oracle suite-wide; reversible). Next: B4b/B6 drop
   the Vec + retire CallFrameId → B5 prologue split → B7 wire the encoder per-opcode.**
2. **GC / cell identity (R4)** (row 6) — the JIT emits raw cell pointers and assumes a
   real GC. `docs/design/gc-r4.md`. Needs the fat `CoreObjectCell` (HashMaps/Vecs)
   replaced by a POD inline-slots + Butterfly representation first (a sweep-managed
   block can't hold a Drop type), then R3 (shadow-oracle arena allocation, reversible),
   then **R4** (the one irreversible flip: raw address = sole cell identity). R4's gate
   is *technical* — shadow cross-check arena==old-map green suite-wide + miri on the
   live deref + adversarial verify + all gates — **not** human sign-off.
3. **Bytecode cutover** — wire the live dispatch onto the packed instruction-stream (the
   JIT lowers from it) + freeze the type-specialized `CoreOpcode`.
4. **Profiling wiring** — per-CodeBlock ValueProfile/ArithProfile (the DFG's speculation
   fuel), retiring the VM-global observation logs.
5. **Baseline JIT** (row 7) — emit per-opcode machine code via the proven encoder/LinkBuffer/
   W^X path, against the JSStack + real cells. **R lifts off the interpreter floor here.**
6. **DFG → FTL/B3** (rows 8–9) — the optimizing tiers that take R to ≥ 1.0.

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
- **R4 IS DONE** (the live object-cell GC runs; cell identity = raw arena address). **The 15/15 GATE
  (asm.js mandreel + octane-zlib) is K2-FREE** — decoupling audit 2026-06-29: typed-array element
  get_by_val/put_by_val (the asm.js HEAP path) is ArrayMode-dispatched over the SEPARATE
  array_buffer_backings slab, NOT the named-property storage split (K2). **Gate path = typed-array
  element IC + dense-Array ElementLoad (have it) + LoadDouble + op_call — ALL on R4, no K2.**
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
