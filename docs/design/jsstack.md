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
- **B5:** frame push/pop = prologue/epilogue (move sp in-arena); overlapping outgoing/incoming
  arg region (copy-first acceptable until tail-calls/varargs need the CallFrameShuffler).
- **B6 (megafile, serial):** retire `CallFrameId(u32)` (~401 refs) — Stage A offset-backed
  bridge (all refs compile) → Stage B generation-tagged FrameAddress newtype → Stage C delete
  `Vec<InstalledCallFrame>`. Leaves first, megafiles last.
- **B7:** wire the proven encoder to emit `ldr/str [x29,#(vreg*8)]` per opcode against the arena.

## Open items / decisions to make when reached

- CallFrameId generation-tag scheme vs "id only compared while frame live" (audit
  ShadowChicken/inspector/dfg first).
- Arena size (CLoopStack uses maxPerThreadStackUsage 5MB + soft-reserved 128KB).
- **Duplicate-trampoline canonicalization (B4):** `platform/unix_arm64_jsc_stack_dispatch.rs`
  is the only real SP/FP-switching asm trampoline; `vm/arm64_native_entry/jsc_stack_dispatch.rs`
  is a complementary request-PROOF layer (NOT a duplicate). Decide whether the proof layer is
  the canonical request producer or is retired in favor of producing the request from the
  seeding path.
- B5 overlapping-arg timing (copy-first vs true overlap + CallFrameShuffler).
