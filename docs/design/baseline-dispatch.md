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

## Stage 1 implementer spec (2026-06-29) — the full-function emitter

- **S5 (RATIFIED) — one control-flow model.** Standardize on the assembler-level
  `Jump`/`Label`/`to_link_record` + `finalize_arm64_link_buffer` path (op_add-proven end-to-end),
  NOT a second abstract model. Retire/demote `Arm64BaselineControlFlowBuilder`
  (arm64_baseline.rs ~:1532/:1607) — lift its bci-bookkeeping as a thin struct OVER the assembler
  path. Decide before the branch unit so the family doesn't fork.
- **S6 (RATIFIED) — 3-pass slow-case deferral.** Collect each op's slow `Jump`s into a
  `SlowCaseRecord` during MAIN; emit the slow cases AFTER the epilogue (JSC privateCompileSlowCases,
  contiguous fast paths) — the family pattern, vs op_add's single-op inline slow path.

**`emit_baseline_function(code_block) -> OpAddImage`** mirrors JIT.cpp's 3 passes (:813-815):
state `labels: Vec<Label>` (one per bytecode index == m_labels, :200), `jumps: Vec<(Jump,
target_bci)>` (== m_jmpTable), `slow: Vec<SlowCaseRecord{fast_jumps, resume_bci, kind}>` (==
m_slowCases). PROLOGUE = op_add's verbatim (push_pair fp/lr, x19=vm, x27/x28 tags). MAIN pass:
per-bytecode, `labels[bci]=masm.label()` then dispatch the per-op fast path (arith = op_add's
body but COLLECTING its slow jumps; branches push `(branch32(cond,x1,x2), target_bci)`; op_ret
loads x0 + jumps to the shared `done`). Inline epilogue at `done` (pop_pair×3 + ret). SLOW pass:
per record, link its fast jumps to a slow label, emit the bridge slow-call, jump to
`labels[resume_bci]`. LINK pass: `jump.to_link_record(labels[target_bci])` — forward + backward
(loop) uniformly (every bci has a Label). The branch-to-BYTECODE-INDEX target is the genuinely
new piece: `target_bci = bci + instr.branch_offset`, resolved in LINK.

New per-op lowerings: **op_enter** (zero-fill locals with undefined bits at addressFor(local)),
**op_mov** (load64 src → store64 dst), **op_ret** (load64 ret-vreg → x0 → jump `done`), **int32
branches** jless/jgreater/jlesseq (emit_compareAndJump: guards → slow; branch32(cond,x1,x2) →
target; slow = operationCompareLess via the bridge, then branchTest32(NonZero,x0) → target),
**jtrue/jfalse** (branch_test64(cond, x1, mask=1) / branch_test32 → target — needs the Imm(1)
mask in a reg, the box/tag-flagged overload), **op_jmp** (unconditional → target).

**Unit ordering:** U1 (3-pass skeleton + op_enter/mov/ret; needs the arith family) ∥ U2 (the int32
branch lowerings + the label/jump-table bci resolution + the operationCompareLess/jtrue bridge
shims — the new control-flow piece) → U3 (the `can_baseline_compile` allowlist + the S1
ExecutionCounter trigger in select_interpreter_entry_plan + JitCode install) → U4 (the S2 B5-lite
handoff execute path + **the milestone test**: an int-sum loop function tiers up, executes
natively, returns correctly, moves its r_i). U5 (parallel: S3 set_jit_code_block + Fail→type_error).
First smoke target `(a,b)=>a+b` (op_enter/add/ret); milestone target the int-sum loop (proves
backward-branch LINK + native loop).

## U3/U4 live-wiring spec (2026-06-29) — where R lifts off

Stage 1 (the emitter) is DONE + verified; U3/U4 wire it into LIVE tier-up. Ratified decisions:
- **S7 (pin-stable Vm) — own the Vm as `Box<Vm>`** at the dispatch site (shell/octane.rs:1827/2115 today
  a move-fragile stack local). The baked `jit_pending` AbsoluteAddress + the parked `*mut Vm` are reused
  by every execution of a compiled function, so the Vm must stay at one heap address for the JIT code's
  lifetime (JSC's VM is heap-allocated). Bake the address only after the Vm is in its boxed home.
- **S8 — SYNCHRONOUS baseline compile** on the tier-up crossing (async JITWorklist deferred).
- **S9 — per-CodeBlock ExecutionCounter** (bytecode/profiling.rs:1241, the faithful countdown) bumped at
  function entry + LoopHint back-edge (JSC loop_osr); fire on crossing ≥0, seed from thresholdForJITAfterWarmUp.

Unit ordering (critical path V0→V3→V4→V5; V1,V2 parallel): **V0** the emitter deadness-guard (HARD
prereq, in flight — else a `let b=a<c;…;return b` CodeBlock mis-compiles live); **V1** `set_jit_code_block`
parking (S3 — the real CodeBlock for the slow path's object-operand ToNumber) + route engine `Fail` →
the existing `type_error_outcome_with_heap` (interpreter/mod.rs:18262, real TypeError, replacing the 0x8
sentinel) [bridge-local]; **V2** S7 Box<Vm>; **V3** U3 — the ExecutionCounter trigger + `can_baseline_compile`
S4 allowlist (use the emitter's `Err` as the single source of truth) + emit→finalize→JitCode-slot install
in select_interpreter_entry_plan (vm/mod.rs:4926, replacing the size heuristic); **V4** U4 — the B5-lite
execute path (park ×3, seed the callee frame header+args at the JSC CallFrameSlot offsets via the B2
seeding, `call_finalized_binary_u64(vm,cfr)`, post-call jit_pending→throw / decode x0→dst→resume) + the
milestone test; **V5** the r_i A/B measurement. LIVE-FEED RISK to verify: the emitter treats BytecodeIndex
as an instruction ORDINAL — confirm the live bytecompiler emits branch targets as ordinals (else add an
offset→ordinal map).

**HONEST OCTANE CAVEAT (strategic):** NO Octane bench is int32-arith-ONLY — all 15 are property/call-heavy,
and those ops STAY interpreted (S4) until R4 (property ICs) + B5/B6 (calls) land. So arith-only tier-up
moves measured Octane R only MARGINALLY (Crypto — int32 cipher rounds — at best, and it still does
interpreted array access in-loop). The honest U3/U4 milestone is a **synthetic hot-arith function's r_i
lift** (proves the tier-up mechanism + R lifts off for arith) + a small Crypto probe; the MATERIAL Octane
R move requires the gated property/call opcodes. Do NOT claim a big Octane R from arith-only tier-up.
