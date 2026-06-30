# Design — JSStack execution substrate (the JIT's frame model)

Status: **ratified (native-thread-stack)**; B1+B2 landed, B3→B7 to go. The decision
is the orchestrator's, settled by evidence (below), not an external gate.

## Decision: native-thread-stack, reject `Vec<Register>`

A single **immovable, contiguous, descending `Register` reservation**, addressed
FP-relative at the JSC `CallFrameSlot` offsets, with the CallFrame header **in-frame**.
Reject `Vec<Register>`/`Vec<RuntimeValue>`. Three decisive, evidence-backed reasons:

1. **The JIT hardcodes FP-relative offsets into emitted instructions.**
   `AssemblyHelpers::addressFor(vreg) = Address(x29, vreg.offset()*8)` (AssemblyHelpers.h:
   1290-1298; cfr == x29, GPRInfo.h:582). Every local/arg/header access the baseline JIT
   emits is `ldr/str [x29, #(vreg*8)]` with the offset baked into the instruction word. A
   `Vec` **reallocates on push**, moving the backing buffer and invalidating every baked
   offset and every live x29. A reallocating Vec fundamentally cannot back FP-relative
   emitted machine code.
2. **`Vec<Register>` is the superseded model** (mcts call-frame-layout: move 2014-01-29
   a3ac51de replaced the private register stack with native-thread-stack frames; the
   Vec-like reserved stack survives only under `#if ENABLE(C_LOOP)`, the no-JIT fallback).
   Choosing it re-treads a rejected JSC dead-end.
3. **The native stack gives JIT-critical properties for free** — hardware call/ret manage
   callerFrame+returnPC, stack-limit checks derive from the thread origin, conservative GC
   roots are one contiguous span.

The Rust-provenance-forced realization is a dedicated contiguous reservation (16-aligned,
guard-paged, sp/fp at the high end, grows down) — byte-ABI-identical to the real thread
stack. **Post-parity (tracked):** swap the provider to the literal pthread stack (zero
emitted-offset change) so it doesn't calcify into the rejected private-register-stack.

## Spec (JSVALUE64/ARM64, sizeof(Register)=8) — `src/vm/jsstack.rs`

- `Register` = `#[repr(transparent)] struct Register(EncodedJSValue /*NaN-boxed u64*/)`,
  plain Copy POD, no Drop (Register.h:97-106,244) — **raw bits**, never the live
  `RuntimeValue` enum. Depends on the value-rep NaN-box keystone (satisfied).
- `CallFrame` = `#[repr(transparent)] struct CallFrame(NonNull<Register>)` pointing **at
  slot 0** (CallFrame : private Register, CallFrame.h:189). Retires the out-of-line
  InstalledCallFrame / CallFrameLayout.
- `CalleeBits` = `#[repr(transparent)] struct CalleeBits(usize)` with the JSVALUE64
  NativeCallee tag test (CalleeBits.h:152-176).
- **Exact FP-relative slot offsets** (CallFrame.h:176-191; CallerFrameAndPC::sizeInRegisters=2):
  callerFrame `[x29+0]`, returnPC `[x29+8]` (ARM64E PAC-signed), codeBlock `[x29+16]`,
  callee `[x29+24]`, argumentCountIncludingThis `[x29+32]` (payload=count, tag=CallSiteIndex),
  thisArgument `[x29+40]`, arg0 `[x29+48]` (argN `[x29+48+8N]`); **locals grow down**:
  local0 `[x29-8]` (localToOperand(n) = -1-n). headerSizeInRegisters = 5.
- **Unsafe boundary** = one audited module (`jsstack.rs`, `#![allow(unsafe_code)]`, rest of
  vm/ stays deny), structurally identical to the S4 GC arena: reserve via mmap(RW,
  MAP_PRIVATE|MAP_ANON — *not* MAP_JIT; this is the value stack, not code) + low-end
  PROT_NONE guard page; `expose_provenance()` **once**; all frame/slot pointers derived via
  `with_exposed_provenance::<Register>(addr)`. Emitted code receives x29/sp as raw addresses
  into the **same** exposed region → shared provenance through the expose-once gate.

## Migration B1–B7

- **B1 (done):** types + offset table + provenance gate (additive, dead_code). Unit-tested
  byte-exact vs CallFrame.h. Fixed the `jit/abi.rs` callee-slot defect (slot 3 was
  CalleeSaveArea, omitting callee).
- **B2 (done):** live mmap reservation + `doVMEntry`-shaped entry seeding (5 header slots +
  this + args, fill-undefined for padding) + the mandatory stack-limit guard (rejects an
  over-deep push as a Result before any write; PROT_NONE page is the hard backstop). The
  byte-identity cross-check (arena image == InstalledCallFrame for the same call) passes —
  this is the safety net for B3/B4.
- **B3:** dual-write bridge — drive entry seeding + `push_frame` from the live model, writing
  the arena header slots alongside the existing InstalledCallFrame with debug_assert they
  agree; reads still come from the Vec. Add the CallFrameId↔FrameAddress side table.
- **B4 (megafile, serial, dedicated refactor) — READ-FLIP LANDED:** the seed now
  reserves the FULL window (`callee_local_count` locals undefined-filled below
  `fp`, SP lowered past them, so nested callees never overlap the caller's
  locals). `RegisterFile::read` is served from the arena via the gate at
  `fp + vreg.raw()*8` (`frame_register_at`); `write` dual-writes arena + `Vec`.
  The `Vec` is RETAINED as a debug ORACLE: every read debug-asserts `arena == Vec`
  over the full window (locals + temporaries + args), proving the offset mapping.
  Reversible: the `Vec` is still dual-written; reads fall back to it when the
  shadow is inactive (overflow / mmap fail / non-unix / raw-native bypass, which
  disables the shadow). FOLLOW-UP B4b/B6: drop the `Vec` once green suite-wide.
- **B5/STACK MODEL (RATIFIED 2026-06-29 by judge panel — Option A: native machine stack = JS stack,
  phased A1→A2).** SUPERSEDES the earlier "Path-B sp-switch-INTO-the-arena" idea, which a second B5 pause
  proved would reroute every fat-Rust slow-path far-call onto the tiny arena → overflow. Panel verdict:
  2/3 vote A now, 3/3 name A the faithful end-state, 3/3 rank D (decoupled FP-arena/SP-native) **fatal**
  (splits `returnPC` from the CallFrame header → silently corrupts DFG-OSR/StackVisitor/GC/unwind). The
  DECISIVE axis: the CallFrame header (`callerFrame`@0 + `returnPC`@1 adjacent, FP/SP unified on ONE stack)
  is what DFG/FTL OSR, stack-walking, GC root-walk, and exception unwind all assume — only an option that
  keeps it faithful survives. **Option A IS that model on the native machine stack** (the JSC-with-JIT
  source of truth; `doVMEntry` builds CallFrames below native sp).
  - **A1 (the R-lever; lands WITHOUT the interpreter rewrite):** flip the JIT prologue to the already-modeled
    `mov fp,sp` + `pushPair(fp,lr)` (`entry_prologue.rs:29/106`) so JIT CallFrames are built on the NATIVE
    stack; emit native `bl` for JIT→JIT calls; **REUSE the B1-B4 register-window LAYOUT by RE-POINTING its
    base from arena-base to native fp** (`frame_register_at`/`try_seed_entry_frame` address native-stack
    slots — a re-point, NOT a rewrite; the slot-offset proofs transfer); bridge the still-fat interpreter at
    JSC's own faithful ENTRY/EXIT boundary (`doVMEntry`/`vmEntryToJavaScript`). The interpreter keeps fat-Rust
    frames on the SAME native stack as pure headroom (NOT a divergence DFG builds on). Retire the per-function
    scratch arena as the JIT stack.
  - **A2 (convergence = Option C):** migrate the interpreter to small native-stack CallFrames (LLInt-faithful
    dispatch loop), retiring the entry/exit bridge → one shared native JS stack.
  - **REJECTED:** D (fatal header split — Principle-#2 trap, fast-local-win that poisons every optimizing
    tier); B (arena + sp-switch + native-stack bounce — faithful *layout* but parks on the CLoop/no-JIT arena
    model and bakes a two-stack bounce whose only exit is completing C, so it calcifies if C slips).
  - **⚠ KEY RISK TO VALIDATE EARLY:** that the first JIT→JIT native `bl` genuinely lands via the entry/exit
    bridge WITHOUT the interpreter rewrite (J2's lone dissent assumed it cannot; J1+J3 refute with the
    modeled prologue + the doVMEntry boundary). If A1 stalls there, fall back to B-as-bridge (reuse the
    tested `_jsc_rs_arm64_jsc_stack_trampoline`) **but treat completing C as a HARD scheduled commitment**,
    not optional, so the bounce cannot calcify. Interim mixed-recursion overflow exposure = SAME as today's
    working Path A far-calls, guarded by softStackLimit, fixed permanently by A2. First cut = no-arity entry.
  - **A1 PREMISE VALIDATED + PLAN (spike 2026-06-29 — verdict HOLDS-WITH-CAVEATS).** The first JIT→JIT native
    call lands WITHOUT the interpreter rewrite: the doVMEntry bridge is ~90% built (`try_seed_entry_frame`
    jsstack.rs:995-1064 does doVMEntry steps 1-5 incl. the softStackLimit guard; the sp-switch+`blr` trampoline
    `_jsc_rs_arm64_jsc_stack_trampoline` is tested); the prologue flip is ONE already-byte-validated instruction
    (`mov fp,x1`→`mov fp,sp`, entry_prologue.rs); `address_for(x29,op*8)` is UNCHANGED (the B1-B4 layout transfers
    by re-point); JIT→interpreter far-calls stay on ONE stack (no bounce, confirmed both sides); JSC roots GC
    CONSERVATIVELY over the whole native-stack span (MachineStackMarker.cpp:44), so the minimal arith proof needs
    ZERO new GC code (the arith-only allowlist guarantees JIT frames hold only numbers). Today's `mov fp,x1`
    (fp=arena, sp=native) is a LATENT Option-D split; A1 corrects it to unified Option A (a divergence-correction).
  - **UNIFIED FRAME CONTRACT (ratified):** prologue = `push_pair(fp,lr); mov fp,sp` for BOTH entry and JIT→JIT;
    `sp = calleeFrame + 16` (CallerFrameAndPC) on every call edge; the per-function scratch arena is RETIRED as
    the JIT stack; JIT-frame rooting = a SCOPED conservative scan of the native-stack JIT-frame span (entry-fp…sp)
    — a bounded slice of GAP C, faithful (what JSC does), DEFERRED until cells flow through native calls.
  - **A1 SUB-UNITS (order):** A1.0 prologue flip + A1.1 native-stack entry seed (ONE serial commit; all existing
    JIT-exec tests stay green, entered via the bridge) → A1.2 op_call native fast path + A1.3 CallLinkInfo→entry
    resolution = the MINIMAL NATIVE-CALL PROOF (`callee(a,b){return a+b}`, `caller(x){return callee(x,1)}`; assert
    header adjacency `callerFrame@0==caller_fp` / `returnPC@1==bl+4`, contiguous callee frame, sp restored,
    jit_pending==0; NO GC dependency) → A1.4 prologue softStackLimit check + A1.5 scoped JIT-frame GC scan
    (REQUIRED before op_call args may carry CELLS). First cut uses `far_call`/`blr ip0` (absolute — no ±128MB `bl`
    range limit) to the resolved entry; repatchable relative `bl` is the follow-up.
  - **B-FALLBACK TRIGGER:** if the entry frame + the JIT→JIT callee frame cannot share ONE contiguous descending
    span the emitted code addresses through a single provenance gate (observed as failed `callerFrame@0==caller_fp`
    / non-contiguous callee frame / Miri provenance trap in the minimal test) → fall back to B-as-bridge (reuse the
    tested trampoline as the interim two-stack bridge) with Option C (A2 interpreter migration) as a HARD scheduled
    commitment so the bounce cannot calcify.
- **B6 (megafile, serial):** retire `CallFrameId(u32)` (~401 refs) — Stage A offset-backed
  bridge (all refs compile) → Stage B generation-tagged FrameAddress newtype → Stage C delete
  `Vec<InstalledCallFrame>`. Leaves first, megafiles last.
- **B7:** wire the proven encoder to emit `ldr/str [x29,#(vreg*8)]` per opcode against the arena.

## Open items / decisions to make when reached

- CallFrameId generation-tag scheme vs "id only compared while frame live" (audit
  ShadowChicken/inspector/dfg first).
- Arena size (CLoopStack uses maxPerThreadStackUsage 5MB + soft-reserved 128KB).
- **Duplicate-trampoline canonicalization (B4) — RESOLVED 2026-06-29 (part of the B5 Path-B cutover):**
  the real SP/FP-switching trampoline (`platform/unix_arm64_jsc_stack_dispatch.rs`) is CANONICAL; the
  entry request is produced from the SEEDING path (`try_seed_entry_frame` positions the arena `CallFrame`)
  routed through it. The proof layer (`vm/arm64_native_entry/jsc_stack_dispatch.rs`) is subordinated to a test.
- B5 overlapping-arg timing (copy-first vs true overlap + CallFrameShuffler).
- **STACK-OWNERSHIP FORK — RESOLVED 2026-06-29 by judge panel → Option A (native stack = JS stack), phased
  A1→A2. See the B5/STACK MODEL entry above for the full decision, rejected options (B/D), and the early-
  validation risk.** (The fork arose because B5 Path-B's sp-switch-into-the-arena would reroute fat-Rust
  slow-path far-calls onto the tiny arena → overflow; root divergence = Rust interpreter on the native stack
  vs the JIT register window in a separate mmap arena, two stacks where JSC has one.)
