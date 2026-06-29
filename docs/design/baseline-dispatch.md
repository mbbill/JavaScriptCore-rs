# Design — baseline-JIT dispatch / tier-up wiring (where R lifts off)

Status: **ratified (2026-06-29)**; the architecture for wiring the baseline JIT into LIVE
execution so a hot function tiers up to native machine code — the milestone where measured R
first moves off the interpreter floor. The orchestrator's call, from a read-only design audit.

## Strategic finding: the arith-only dispatch is DOABLE NOW (not gated on R4/B5/B6)

The tier-up DECISION + install framework is ALREADY wired (vm/mod.rs:4926-5070):
`select_interpreter_entry_plan` → {EnterOptimized{Baseline}, EnterNative{Baseline}, interpreter}
+ the P15 auto-install descriptor path + the `can_execute_*` gate. What's MISSING: (i) the
trigger is a bytecode-size/IC-count HEURISTIC, not the faithful execution-count countdown;
(ii) the "generated Baseline" arm is a P6 return-seed shim that DECLINES for real bodies and
stays in the interpreter (vm/mod.rs:5016-5018); (iii) NO full-function CodeBlock is lowered to
ARM64 + executed. op_add executes but is STANDALONE/test-only (one op, its own test harness,
the raw `fn(vm,cfr)->u64` lane — not the JS call/return ABI, not driven by the tier-up install).

A first wired function can be **arith/mov/branch-only** — property access (R4 + storage split +
IC machinery) and op_call (B5/B6) are GATED OUT by an opcode allowlist (S4). Most Octane code
has property/calls, so arith-only moves R MARGINALLY — but it proves the tier-up→emit→execute
→return mechanism end-to-end and builds the scaffolding the gated opcodes plug into.

## Ratified serial decisions

- **S1 — faithful tier-up trigger.** Adopt the ported `ExecutionCounter` countdown
  (bytecode/profiling.rs:1204/1241; JSC ExecutionCounter.h:90-96) as THE trigger: a per-CodeBlock
  negative-threshold countdown, +1 per function entry AND per loop back-edge, fires the compile
  check on crossing ≥0 (JSC LLIntSlowPaths jitCompileAndSetHeuristics, entry :388 + loop_osr
  :493). Replaces the bytecode-size heuristic. DIVERGENCE (documented): JSC baseline-compiles
  ASYNC (JITWorklist/BaselineJITPlan); the Rust first cut compiles SYNCHRONOUSLY on the check
  (baseline is the cheap tier) — async deferred.
- **S2 — B5-lite frame handoff** (load-bearing for the milestone). The interpreter SEEDS the
  callee JSStack frame at the JSC CallFrameSlot offsets (reusing the landed B2 doVMEntry-style
  seeding) + jumps to the compiled entry; op_ret's epilogue returns; the x0 return decodes into
  the destination register. DEFER the emitted op_enter / prologue-SP-move (full JSStack B5). This
  is a LOCALIZED divergence: the compiled BODY (which only READS the frame at the offsets) is
  unchanged whether the interpreter or an emitted prologue set it up — so converging to B5
  (emitted prologue) revises only the entry/setup, not the opcode lowerings. Lands the milestone
  now without blocking on B5.
- **S3 — extend the parking discipline** to a third raw pointer: `Vm::set_jit_code_block(*const
  CodeBlock)` parked for the JIT-call region (mirrors set_jit_host, D5), so the slow-path
  `Vm::operation_value_*` builds its DispatchState with the REAL active CodeBlock (object
  operands' ToNumber/valueOf read it). Identical soundness island (dormant during the JIT call).
- **S4 — `can_baseline_compile` opcode-allowlist gate.** A CodeBlock containing ANY unsupported
  opcode (property access, calls) stays in the interpreter. THE invariant that makes incremental
  opcode coverage safe; the allowlist grows toward JSC's full coverage as opcodes land.

## Staged plan (all 4 stages DOABLE NOW — arith-only; gated pieces deferred)

1. **Full-function emit** — port JIT::privateCompile's 3 passes (JIT.cpp:813-815): MAIN
   (per-bytecode loop, record label[bcIndex] :200, dispatch emit_op_X), LINK (resolve forward
   branches via the label table — the arm64_baseline control-flow builder ALREADY models
   record_label + link), SLOW (slow-case linking, already modeled). Generalize op_add.rs's
   per-op pattern (AssemblyHelpers fast path + the bridge slow-call) into a CodeBlock-driven
   emitter over op_enter / op_mov / the arith family / int32 branches (jless/jgreater/jtrue/
   jfalse) / op_ret. Behind the S4 allowlist gate.
2. **Tier-up trigger** (S1) — wire the ExecutionCounter countdown into select_interpreter_entry_plan;
   sync-compile allowlist-clean CodeBlocks on crossing.
3. **B5-lite frame handoff** (S2) — interpreter seeds the callee frame + jumps to the compiled
   entry + decodes the x0 return + resumes. **The milestone: an arith-only function tiers up,
   executes natively, returns correctly, and raises its r_i.**
4. **Close the op_add-flagged prereqs** — `set_jit_code_block` parking (S3) + route an engine
   `Fail(ExecutionError)` to the EXISTING `type_error_outcome_with_heap` (interpreter/mod.rs:18262)
   / `type_error_value` (:15454) so a real TypeError is surfaced (replace the 0x8 sentinel; best
   via `arithmetic_binary_result` returning `Throw(TypeError)` so the bridge's existing Throw arm
   surfaces it faithfully).

## Then converge to faithful (post-milestone)

Emitted op_enter / prologue-SP-move (full JSStack B5); op_call (B5/B6 callee-frame push) +
property-access ICs (R4 direct cell pointers + inline/out-of-line storage split + IC machinery)
— each grows the S4 allowlist; async JITWorklist baseline compile. These are where R moves
MATERIALLY (Octane is property/call-heavy); the arith-only milestone is the proving scaffold.

## Prereqs / gating

Needs op_add (done) + the arith family (in flight) before Stage 1's emitter has its opcode set.
Stages 1-4 are otherwise un-gated (no R4/B5/B6). The opcodes that MOVE R materially (property
access, calls) stay gated on R4 / B5-B6 and enter via the S4 allowlist as those land.
