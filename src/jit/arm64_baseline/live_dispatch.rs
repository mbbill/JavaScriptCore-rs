//! Live baseline-JIT tier-up dispatch (U3/U4 — where measured R lifts off the
//! interpreter floor). This module wires the Stage-1 full-function emitter
//! ([`super::function_emitter::emit_baseline_function`]) into LIVE execution: a
//! hot function's per-CodeBlock execution-count countdown crosses, the function
//! is SYNCHRONOUSLY compiled to one ARM64 image, installed, and thereafter
//! EXECUTED natively (the B5-lite frame handoff), returning its boxed result.
//!
//! C++ JSC map (source of truth):
//! - TRIGGER (S9): the per-CodeBlock `ExecutionCounter` countdown
//!   (`bytecode/ExecutionCounter.{h,cpp}`; the faithful port in
//!   `bytecode/profiling.rs` [`BytecodeExecutionCounter`]). JSC bumps it on every
//!   function entry AND every loop back-edge and, when the down-counter reaches
//!   zero, runs the tier-up check (`LLIntSlowPaths.cpp` `jitCompileAndSetHeuristics`
//!   at the prologue :388 + `loop_osr` :493). [`BaselineTierUpTrigger`] models that.
//! - COMPILE (S8): JSC's baseline JIT compiles ASYNCHRONOUSLY via the
//!   `JITWorklist`/`BaselineJITPlan`; this first cut compiles SYNCHRONOUSLY on the
//!   crossing (baseline is the cheap tier — DIVERGENCE documented at the call).
//! - ALLOWLIST (S4): `emit_baseline_function`'s `Err` IS the `can_baseline_compile`
//!   gate — a CodeBlock with any unsupported opcode/operand is DECLINED and stays
//!   in the interpreter (JSC declines unsupported bytecode similarly). There is no
//!   second allowlist; the emitter is the single source of truth.
//! - INSTALL: the finalized image == `CodeBlock::m_jitCode` (`CodeBlock.h:361`).
//!   The executable memory must outlive every native call, so the
//!   [`InstalledBaselineFunction`] (handle + reusable scratch frame) is owned at a
//!   stable address by the `Vm` (`vm/mod.rs` `baseline_jit_slots`).
//! - HANDOFF (S2, B5-lite): the interpreter SEEDS the callee frame at the JSC
//!   `CallFrameSlot` offsets (`CallFrame.h:176-191`) and jumps to the compiled
//!   entry; `op_ret`'s epilogue returns the boxed value in `returnValueGPR`. JSC's
//!   `addressForCall` enters the JITCode the same way (a seeded CallFrame + a jump
//!   to the code entry). This first cut seeds a DEDICATED scratch frame rather than
//!   the live arena window (the full JSStack B5 — emitted prologue/SP-move — is
//!   deferred): because the S4 allowlist gates out every property/call/global op,
//!   an allowlisted function is PURE (reads its arguments, returns one value, with
//!   no observable heap/frame side effects), so running it in a scratch frame
//!   seeded with the arguments is behavior-identical to running it in the live
//!   frame. Converging to full B5 revises only the entry/setup, not the lowerings.
//!
//! Unsafe boundary: this module is SAFE (`jit/mod.rs` is `#![deny(unsafe_code)]`).
//! The native call goes through the SAFE [`ExecutableMemoryHandle::call_finalized_binary_u64`]
//! wrapper; the `*mut Vm`/`*mut host`/`*const CodeBlock` parking (the D1/D5/S3
//! reborrow island) is performed by the `Vm` driver in `vm/mod.rs` (the crate's
//! audited unsafe), exactly as the standalone `op_add`/`function_emitter`
//! execution proofs do.

#![allow(dead_code)]

use crate::bytecode::profiling::{
    BytecodeExecutionCounter, CountingVariant, ExecutionCounterEnvironment,
};
use crate::bytecode::CodeBlock;

use super::function_emitter::EmitFunctionError;

/// `Options::thresholdForJITAfterWarmUp()` — JSC's default baseline tier-up
/// threshold (the number of executions before the LLInt requests a baseline
/// compile). The real per-CodeBlock value comes from `Options`/`CodeBlock`
/// (`ExecutionCounter` is UNWIRED from those — see `bytecode/profiling.rs`); the
/// live driver supplies it, defaulting here, until the `Options` table is wired.
pub(crate) const THRESHOLD_FOR_JIT_AFTER_WARM_UP: i32 = 500;

/// The `maximumExecutionCountsBetweenCheckpoints` the trigger uses while the
/// per-variant `Options` cap is unwired. Set high so the countdown is not clipped
/// for ordinary thresholds; `ExecutionCounter::setThreshold` clips against it
/// (`ExecutionCounter.cpp:163-197`).
const MAX_EXECUTION_COUNTS_BETWEEN_CHECKPOINTS: i32 = i32::MAX / 2;

/// S9 — the per-CodeBlock baseline tier-up trigger: the faithful
/// `ExecutionCounter` down-counter (`bytecode/profiling.rs`) bumped at function
/// entry and each loop back-edge, firing the compile check when it reaches zero.
///
/// JSC machine code does `add m_counter, count; branch >= 0 -> slow path`; the
/// slow path runs `checkIfThresholdCrossedAndSet` and, on a true result,
/// compiles. `crossed` latches the one-shot compile request so a tiered function
/// is not re-checked every entry.
pub(crate) struct BaselineTierUpTrigger {
    counter: BytecodeExecutionCounter,
    env: ExecutionCounterEnvironment,
    crossed: bool,
}

impl BaselineTierUpTrigger {
    /// Seed from `thresholdForJITAfterWarmUp` (S9): `setNewThreshold(threshold)`
    /// re-seeds `m_counter` so `count()` begins at 0 and counts up by `threshold`
    /// executions before the check fires (`ExecutionCounter.cpp:60-66`).
    pub(crate) fn new(threshold: i32) -> Self {
        let env = ExecutionCounterEnvironment {
            memory_usage_multiplier: 1.0,
            maximum_execution_counts_between_checkpoints: MAX_EXECUTION_COUNTS_BETWEEN_CHECKPOINTS,
        };
        let mut counter = BytecodeExecutionCounter::new(CountingVariant::Baseline);
        counter.set_new_threshold(threshold, env);
        Self {
            counter,
            env,
            crossed: false,
        }
    }

    /// Already-tiered latch: once the countdown crosses, the compile is requested
    /// once and every later entry short-circuits to the installed image.
    pub(crate) fn has_crossed(&self) -> bool {
        self.crossed
    }

    /// Bump on a function entry (JSC `jitCompileAndSetHeuristics` at the prologue,
    /// `LLIntSlowPaths.cpp:388`). Returns true on the FIRST entry that crosses.
    pub(crate) fn record_entry(&mut self) -> bool {
        self.bump_one()
    }

    /// Bump on a loop back-edge (JSC `loop_osr`, `LLIntSlowPaths.cpp:493`). Same
    /// counter; modeled identically.
    pub(crate) fn record_loop_backedge(&mut self) -> bool {
        self.bump_one()
    }

    fn bump_one(&mut self) -> bool {
        if self.crossed {
            return true;
        }
        // Machine-code increment of `m_counter` toward zero (ExecutionCounter.h:90).
        self.counter.counter = self.counter.counter.saturating_add(1);
        // JSC: when the down-counter reaches zero the LLInt takes the slow path and
        // runs the tier-up check; if it reports the threshold crossed, compile.
        if self.counter.counter >= 0 && self.counter.check_if_threshold_crossed_and_set(self.env) {
            self.crossed = true;
        }
        self.crossed
    }
}

/// Failure modes of [`install_baseline_function`]: either the S4 allowlist DECLINED
/// the CodeBlock (stays in the interpreter) or the executable-memory finalize
/// failed (an allocation/link failure — JSC bails the baseline plan and stays in
/// the LLInt). Both are control flow, never a panic.
#[derive(Clone, Debug)]
pub(crate) enum BaselineInstallError {
    /// `emit_baseline_function` rejected the CodeBlock (the `can_baseline_compile`
    /// gate). The function is DECLINED — the interpreter keeps running it.
    Declined(EmitFunctionError),
    /// The image could not be finalized into executable memory.
    Finalize(crate::jit::executable_allocator::ExecutableAllocationError),
    /// The immovable scratch frame (the B5-lite callee window) could not be
    /// reserved.
    Stack(crate::vm::jsstack::JsStackReservationError),
    /// The platform does not support live native execution (non-macOS/aarch64).
    UnsupportedPlatform,
}

/// Scan every register operand for the callee var count and the highest positive
/// argument slot the body addresses, so the scratch frame can be sized to host the
/// whole frame (negative locals below `fp`, positive args/header at/above `fp`,
/// the JSC `addressFor(vreg) = Address(cfr, vreg*8)` convention the emitter bakes).
fn scan_frame_extent(code_block: &CodeBlock) -> Result<(u32, i32), EmitFunctionError> {
    use crate::bytecode::BytecodeIndex;

    let stream = code_block.unlinked().instructions();
    let count = stream.instruction_count();
    let mut num_locals: i64 = 0;
    let mut max_argument_slot: i32 = 0;
    for bci in 0..count {
        let decoded = stream.decoded_at(BytecodeIndex::from_offset(bci as u32))?;
        for index in 0..decoded.operands.len() {
            if let Ok(register) = decoded.register_operand(index) {
                if let Some(local_index) = register.to_local_index() {
                    num_locals = num_locals.max(local_index as i64 + 1);
                } else if !register.is_constant() && register.raw() > max_argument_slot {
                    max_argument_slot = register.raw();
                }
            }
        }
    }
    Ok((num_locals as u32, max_argument_slot))
}

// The native install/execute path is inherently macOS/aarch64 (it relocates and
// executes ARM64 machine code under W^X), exactly like the `function_emitter`
// execution proofs. The platform-portable types above (trigger, errors, the
// scan) compile everywhere; only the executable handoff is gated.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) use platform::{install_baseline_function, InstalledBaselineFunction};

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub(crate) fn install_baseline_function(
    _code_block: &CodeBlock,
    _jit_pending_address: usize,
    _install_vm: *const crate::vm::Vm,
) -> Result<(), BaselineInstallError> {
    Err(BaselineInstallError::UnsupportedPlatform)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod platform {
    use super::{scan_frame_extent, BaselineInstallError};
    use crate::bytecode::CodeBlock;
    use crate::jit::executable_allocator::{
        finalize_arm64_link_buffer, ExecutableMemoryHandle, MapJitExecutableAllocator,
    };
    use crate::vm::jsstack::{
        argument_offset_including_this, JsStack, Register, REGISTER_SIZE_IN_BYTES,
    };
    use crate::vm::Vm;

    use super::super::function_emitter::emit_baseline_function;

    /// Slack registers added above the highest argument slot and below the lowest
    /// local so the seeded header/argument writes and the op_enter local zero-fill
    /// stay inside the immovable scratch window with margin.
    const FRAME_SLACK_REGISTERS: usize = 8;

    /// One installed Stage-1 baseline image (== `CodeBlock::m_jitCode`): the
    /// finalized, RX-sealed ARM64 code plus a REUSABLE immovable scratch frame the
    /// B5-lite handoff seeds and runs against. Owned by the `Vm` at a stable
    /// address so the baked `jit_pending` AbsoluteAddress and the parked `*mut Vm`
    /// stay valid for the image's lifetime (S7).
    pub(crate) struct InstalledBaselineFunction {
        handle: ExecutableMemoryHandle,
        /// Immovable backing for the seeded callee frame (B5-lite scratch).
        scratch: JsStack,
        /// `cfr` (x29): the frame base where VirtualRegister 0 lives; positive
        /// arg/header slots sit at/above it, negative locals below it.
        fp: usize,
        /// INV-4 (Vm pinned across install->reuse): the `*const Vm` of the owning
        /// `Vm` captured AT INSTALL. The image bakes `jit_pending` as an
        /// `AbsoluteAddress` that is an INTERIOR pointer of THIS `Vm`
        /// (`Vm::jit_pending_exception_address`, vm/mod.rs); it is reused unchanged on
        /// every later `RunInstalled` entry, so it dangles if the `Vm` ever moves
        /// between install and reuse. `run` compares this against the run-time `*mut
        /// Vm` so a move is caught immediately in debug instead of becoming a SILENT
        /// release use-after-free. Stored as a raw address; NEVER dereferenced (only
        /// compared), so it carries no aliasing/provenance obligation.
        install_vm: *const Vm,
    }

    impl InstalledBaselineFunction {
        /// Seed the callee frame's `this`+argument slots at the JSC `CallFrameSlot`
        /// offsets (`argument_offset_including_this`: `thisArgument` then
        /// `firstArgument..`) from `arguments_including_this` (boxed
        /// `EncodedJSValue` words; `arguments_including_this[0]` is `this`), then
        /// call the image as `extern "C" fn(*mut Vm, cfr) -> u64`. Returns the
        /// op_ret boxed value in `returnValueGPR` (x0); the driver reads the
        /// `jit_pending` mirror for the throw edge. The emitted `op_enter`
        /// zero-fills the locals, so only the arguments are seeded here.
        pub(crate) fn run(&mut self, vm_ptr_bits: u64, arguments_including_this: &[u64]) -> u64 {
            // INV-4 (Vm pinned across install->reuse): the `jit_pending`
            // AbsoluteAddress baked into this image is an interior pointer of the
            // install-time `Vm`; reusing it here is sound ONLY while that `Vm` has not
            // moved. `vm_ptr_bits` is the CURRENT `self as *mut Vm` the driver passes
            // in x0, so an install-time != run-time base means the `Vm` moved and the
            // baked address now dangles. Catch it here (debug) rather than letting the
            // native slow path stamp through a stale pointer (release UB). Address-only
            // compare: `install_vm` is never dereferenced.
            debug_assert_eq!(
                self.install_vm as usize, vm_ptr_bits as usize,
                "the Vm moved between baseline install and reuse: the baked jit_pending \
                 AbsoluteAddress now dangles (the live baseline path requires a \
                 pinned-address Vm, e.g. Box<Vm>)"
            );
            for (argument_index, &boxed) in arguments_including_this.iter().enumerate() {
                // argument_offset_including_this(0) == thisArgument; (1) == arg0.
                let vreg = argument_offset_including_this(argument_index as i32);
                let addr = (self.fp as isize + vreg as isize * 8) as usize;
                // The seeding write MUST happen in ALL profiles. `write_slot` is
                // itself the REAL bounds gate (`jsstack.rs` register_ptr ->
                // contains_address): on an out-of-range/misaligned `addr` it returns
                // `false` WITHOUT writing, so it can never corrupt memory. Capture the
                // in-range result and only `debug_assert!` on THAT — never put the
                // side-effecting call inside the assert, which release builds do not
                // evaluate (that would leave the args unseeded in release ->
                // wrong native results). Any argument the emitted body actually reads
                // is in range by construction: `scan_frame_extent` sized the scratch
                // to the max referenced argument slot + slack, so a `false` here can
                // only be a trailing argument the body never reads.
                let seeded_in_range = self.scratch.write_slot(addr, Register::from_bits(boxed));
                debug_assert!(
                    seeded_in_range,
                    "scratch frame argument slot must be in range",
                );
            }
            self.handle
                .call_finalized_binary_u64(vm_ptr_bits, self.fp as u64)
        }
    }

    /// COMPILE (S8 synchronous) + INSTALL: lower the whole CodeBlock to one ARM64
    /// image via the Stage-1 emitter (its `Err` is the S4 allowlist), finalize it
    /// into RX executable memory, and size+allocate the reusable scratch frame.
    /// Returns the installed image, or `Declined`/`Finalize` as control flow.
    pub(crate) fn install_baseline_function(
        code_block: &CodeBlock,
        jit_pending_address: usize,
        install_vm: *const Vm,
    ) -> Result<InstalledBaselineFunction, BaselineInstallError> {
        let image = emit_baseline_function(code_block, jit_pending_address)
            .map_err(BaselineInstallError::Declined)?;
        let mut records = image.link_records;
        let handle =
            finalize_arm64_link_buffer(&MapJitExecutableAllocator, &image.code, &mut records)
                .map_err(BaselineInstallError::Finalize)?;

        let (num_locals, max_argument_slot) =
            scan_frame_extent(code_block).map_err(BaselineInstallError::Declined)?;
        let below = num_locals as usize + FRAME_SLACK_REGISTERS;
        let above = (max_argument_slot.max(0) as usize + 1) + FRAME_SLACK_REGISTERS;
        let total = below + above;
        // B5-lite: a fresh immovable arena window for this function's callee frame
        // (CLoopStack-style mmap reservation, the same backing the live JSStack
        // uses). Reused for every native call; the seeded args + op_enter zero-fill
        // overwrite it each time.
        let scratch =
            JsStack::new(total * REGISTER_SIZE_IN_BYTES).map_err(BaselineInstallError::Stack)?;
        // Place `fp` so `below` registers sit beneath it (locals, negative offsets)
        // and `above` registers at/above it (header + arguments, positive offsets).
        // The reservation is page-rounded up (>= total*8), so `fp - below*8` stays
        // at/above `low_address()`.
        let fp = scratch.high_address() - above * REGISTER_SIZE_IN_BYTES;
        Ok(InstalledBaselineFunction {
            handle,
            scratch,
            fp,
            // INV-4: capture the install-time Vm base; `run` asserts the reuse-time
            // base matches so a Vm move (which would dangle the baked jit_pending
            // AbsoluteAddress) is caught in debug.
            install_vm,
        })
    }
}
