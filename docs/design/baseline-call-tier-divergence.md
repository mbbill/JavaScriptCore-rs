# Design — the baseline call/tier divergence (CONFIRMED) and its faithful correction

Status: **CONFIRMED load-bearing divergence** (strategic assessment 2026-06-29, 4 auditors +
synthesis + adversarial anti-anchor; C++ JSC grep corroborated). This file is the durable
record so the divergence is **corrected when unblocked, never built around** (Prime-Focus #2).
It is NOT a green-light to start now — the R-bearing core is gated (see Sequencing).

## The divergence (no JSC counterpart)

The Rust engine grew an invented baseline call/tier substrate with **zero** JSC analog
(C++ grep over all of `Source/JavaScriptCore`: `DirectCallTransaction`, `RouteOpportunity`,
`GeneratedDirectCall`, `NativeEntryKind`, `BaselineGeneratedExecution`, `HostBlocked` = **0 hits**):

1. **`generated_executor` — a bytecode RE-INTERPRETER used as the baseline "JITCode".**
   `src/vm/generated_executor.rs` (+ `src/jit/baseline.rs:2312`). Its own header admits it is a
   shim "while the real register-allocated baseline … is missing." ~105 `execute_baseline_generated_code`
   sites. This is an interpreter-class engine that asymptotes far below C++. JSC HAS NO SUCH CONCEPT —
   the baseline JIT lowers **every** opcode to machine code; unhandled fast cases tail into a C++
   `operation*` slow path (`jit/JIT.cpp:171` `privateCompileMainPass`: `DEFINE_OP` fast path /
   `DEFINE_SLOW_OP` native call / `default: RELEASE_ASSERT_NOT_REACHED`). No re-interpreter fallback.

2. **Per-call route arbiter with transaction accounting.**
   `src/vm/call_link.rs:138` `execute_generated_js_direct_call_transaction` picks a
   `VmGeneratedDirectCallTransactionRoute` (`src/vm/tiering.rs:7467`:
   Generated/Native/NativeEntryInterpreterFallback/NestedInterpreterFallback/FrameSetupFailed/…)
   **per call** and books it via `record_generated_direct_call_transaction` (`tiering.rs:2576`).
   122 route refs / 4 files; 568 `GeneratedDirectCall` refs. JSC HAS NO SUCH CONCEPT — a call site is
   **one** `CallLinkInfo` Mode (`bytecode/CallLinkInfo.h:69` Init/Monomorphic/Polymorphic/Virtual) with
   **one** baked `m_monomorphicCallDestination`; switching happens only by relinking
   (`RepatchInlines.h:130` `linkFor`), never by per-call multi-route arbitration or bookkeeping.

3. **Callee native entry emitted as x86_64 bytes that a non-x86_64 host rejects.**
   `BaselineNativeEntryCallableKind::P6X86_64EmittedSemanticCAbiEntry` (`src/jit/code.rs:276`);
   gate `can_execute_p6_x86_64_emitted_native_entry` (`src/vm/mod.rs:2553`, `cfg x86_64` only) → on
   this **arm64** host every entry misses as `HostBlockedX86_64` (`tiering.rs:7512`) and drops to
   `execute_nested_callee_code_block` (`mod.rs:12415`), a nested bytecode interpreter. JSC HAS NO SUCH
   CONCEPT — JSC emits the callee entry in the **host** ISA only (`AssemblyHelpers::emitFunctionPrologue`).

**Measured symptom (richards `--tiering-summary`, arm64):** `generated_direct_call_transactions ≈ 3.62M`,
**100% `NestedInterpreterFallback`**, `native_entries = 0`. The opt-in baseline JIT is therefore the
interpreter PLUS per-call route/accounting overhead → a **net regression** (geomean ~0.64× vs interp on
the completing benches; richards/delta-blue ~3× slower; raytrace/earley DNF). Zero correctness failures.

## The faithful target (JSC; seeds already in the Rust tree)

1. **One in-site `CallLinkInfo` per `op_call`/`op_construct`/`op_tail_call`** — already modeled at
   `src/bytecode/ic.rs:961` (`CallLinkMode` Init/Monomorphic/Polymorphic/Virtual). Holds one destination +
   callee identity; mutated O(1) by a **link-once** slow path (`linkFor`: setSeen → setMonomorphicCallee /
   linkPolymorphicCall / setVirtualCall) and by GC `visitWeak` (`CallLinkInfo.cpp:171`). Fast path =
   `emitFastPathImpl` (`CallLinkInfo.cpp:323`): load destination, compare live callee, **one** indirect
   machine call (farJump for tail); miss vectors the SAME call to the virtual thunk (`virtualThunkFor`,
   `ThunkGenerators.cpp:217`) which relinks in place.
2. **A real ARM64 baseline callee entrypoint emitted for the running CPU only** —
   `emitFunctionPrologue` shape: x29/fp = stack `CallFrame*`, x0 = JSValue return (replacing the
   `src/vm/arm64_native_entry.rs` seed that passes a Rust register-file ptr in x1). Exactly one pair per
   executable: no-arity (`addressForCall`) and arity-check (`addressForCall(MustCheckArity)`).
3. **Total baseline lowering** — every opcode → ARM64 machine code (`privateCompileMainPass`-equivalent);
   unhandled fast cases tail into a native call to a Rust `operation*`; an unhandled opcode is a hard
   assert, **never** a per-opcode fallback-to-interpreter edge.
4. **Tier-up via a `BaselineExecutionCounter` emitted into the machine code** (`ExecutionCounter.h:56`);
   threshold-cross enqueues a baseline plan whose completion `installCode`-installs native code; loop OSR
   maps bytecodeIndex→machine label. The Rust bytecode interpreter is the faithful **LLInt-tier analog**:
   an unlinked/not-yet-compiled callee legitimately runs through it as the single fallback state — never
   via a route/transaction arbiter.

## Correction path — incremental cutover (recommended), with anti-anchor corrections applied

Keeps the 13/15 completing benches green throughout; converges to JSC, not around it.

- **STEP 1 — collapse onto `CallLinkInfo`, delete the route/transaction/rootless-proof accounting**
  (`call_link.rs:138`, `tiering.rs:7467`). Linked target initially the callee **interpreter** entry
  (no codegen). **Honest label: this is divergence-correction (delete a no-JSC-counterpart layer while
  dependents are few), NOT a parity win — it moves measured R by ZERO** (the substrate is opt-in; the
  default is `InterpreterOnly` with the flip held). **Blocked on two SERIAL decisions first** (Open Qs #1,#3).
- **STEP 2 — real ARM64 `emitFunctionPrologue` callee entry** (host-ISA only). Additive/unwired.
  **GATED on JSStack B5–B7** (a real stack `CallFrame`, prologue, arity, tail-call transfer).
- **STEP 3 — total native opcode lowering** (per-opcode-group, commit-sized; slow cases → native
  `operation*`). Install via `installCode`-equivalent so the STEP-1 linked target becomes the STEP-2
  native entry. **This is the real >1.0× lever.** GATED on STEP 2.
- **STEP 4 — faithful `BaselineExecutionCounter` tier-up** (the substrate DFG/FTL require).
- **STEP 5 — delete the now-dead divergence cluster** (route/miss-reason enums, transaction/rootless-proof
  caches, the x86_64-on-arm64 entry, `generated_executor`, policy toggles). **Off-gate hygiene.**
- **STEP 6 — de-megafile the 35,423-line `src/vm/tiering.rs`** by JSC subsystem boundary (tier-up →
  `src/jit/tiering.rs`; call-link/IC → `src/bytecode/ic.rs`). **Off-gate hygiene; never preempt R4/calls.**

## Sequencing & gates (the load-bearing correction, NOT a parity win in itself)

- **R does not move here until the very end** — native breadth (STEP 3) **AND** the default flip. Until
  then measured R is the interpreter (~0.001), and R is **undefined** until mandreel + octane-zlib complete.
- **STEP 2/3 are downstream of JSStack B5–B7 and GC/R4** — the roadmap's already-sequenced critical path.
  Do not build STEP 2 on the non-faithful arm64 seed (x1 = Rust register-file ptr) before they land.
- **Serial decisions — RESOLVED 2026-06-29 by the CallLinkInfo-seed + GC/R4 audits:**
  - **#1 Linked-target representation — RESOLVED: ADMITTED.** The seed's destination (`src/bytecode/ic.rs:1426`
    `CallTarget`) is typed to IDs/slots, NOT a machine-code pointer, and `EntrypointKind::InterpreterThunk`
    already exists (`jit/abi.rs:19`). STEP 1 adds ONE explicit `CallTarget` interpreter-entry variant
    ("linked → callee `CodeBlockId`, entered via the bytecode interpreter"), code-commented as a transient
    language-mapping, so `linked` never silently means "no code pointer." No machine-code dependency.
  - **#3 `visitWeak`/R4 rooting — RESOLVED: STEP 1 does NOT cache callee identity.** `visitWeak` does not
    exist in the live collector (`src/gc/weak.rs` is vocabulary-only, unreferenced by mark/sweep); the seed's
    cached callee is a raw `ObjectId/CellId`, untraced, no weak-clear — so caching it would dangle. STEP 1's
    linked target = callee `CodeBlockId` **re-resolved each call** (GC-safe; identical to today's live
    `function_call_target` path). The monomorphic `m_callee`-weak fast path (the real call speedup) is
    DEFERRED to synchronize with the **R4 `visitWeak` weak-processing phase** (now in design) — it is a
    prerequisite of STEP 2's caching, not of STEP 1's de-divergence. Missing `CallLinkMode` transitions to
    add later: setSeen / →Polymorphic / →Virtual / setLastSeenCallee / visitWeak.
  - **#4 tail-call/construct/varargs + arity** in the 2-state model (farJump tail, distinct arity entries) —
    decide JSStack support before native callee entries; STEP 1 may defer op_tail_call/op_construct to the
    interpreter fallback. (B5 first cut = no-arity entry; arity-check copy-first shuffle deferred — jsstack.md:94.)
  - **STEP 1 net scope (now unblocked, GC-safe, R-neutral):** collapse `op_call` dispatch onto the in-site
    `CallLinkInfo` (new interpreter-entry `CallTarget`, callee re-resolved each call), DELETE the
    `VmGeneratedDirectCallTransactionRoute` arbiter + transaction/route-opportunity/rootless-proof accounting
    (`call_link.rs:138`, `tiering.rs:7467`). Correctness-identical (callee still runs interpreted); moves R by 0.
- **Evidence question that outranks this whole campaign for *defining* R:** are mandreel/octane-zlib
  **GC/memory-gated** (→ finishing GC/R4 makes the suite 15/15 → R becomes DEFINED) or **native-speed-gated**
  (→ the JIT)? The tracker says "asm.js, too-slow, JIT-gated, NOT OOM" — verify with evidence before
  committing; if GC-gated, GC/R4 outranks this campaign for the correctness gate.

Authority: mcts_mem `baseline-jit.md`, `baseline-jit/{call-linking-stubs,platform-calling-convention,
osr-tier-boundary,unlinked-code-sharing}.md`, `dfg/{call-dispatch,tier-up}.md`, `interpreter/call-frame-layout.md`.
