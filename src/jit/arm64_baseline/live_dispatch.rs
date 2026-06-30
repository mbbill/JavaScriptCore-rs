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
//! - HANDOFF (S2 / A1 native-stack entry): the driver `doVMEntry`-SEEDS the callee
//!   `CallFrame` on the NATIVE JS stack at the JSC `CallFrameSlot` offsets
//!   (`CallFrame.h:176-191`) and enters the image through the baseline-JIT entry
//!   trampoline with `sp = calleeFrame + sizeof(CallerFrameAndPC)`. The flipped
//!   `emitFunctionPrologue` (`pushPair(fp,lr); mov fp,sp`) makes `fp` the callee
//!   frame and `op_ret`'s epilogue returns the boxed value in `returnValueGPR` —
//!   the faithful Option-A model (`LowLevelInterpreter64.asm` `doVMEntry` /
//!   `makeJavaScriptCall`), retiring the pre-A1 hand-placed scratch arena. Each
//!   installed function owns a default-sized (5 MiB) native stack so a slow-path
//!   far-call (op_call's `operation_call` re-entering the interpreter,
//!   get/put_by_val) runs on it with headroom; a nested JIT entry switches `sp`
//!   to ITS function's stack via the trampoline (per-function stacks never
//!   overlap). Native JIT->JIT calls (`blr`) LANDED (A1.2 + broad engagement): a live
//!   op_call resolves the callee's installed native entry per call and `blr`s it when
//!   JIT'd (else the `operation_call` slow path) — call-heavy code runs native and
//!   beats the interpreter.
//!
//! Unsafe boundary: this module is SAFE (`jit/mod.rs` is `#![deny(unsafe_code)]`).
//! The native entry goes through the SAFE [`ExecutableMemoryHandle::call_baseline_jit_entry`]
//! wrapper (the entry trampoline lives in the `jit::unsafe_platform_boundary`
//! island); the `*mut Vm`/`*mut host`/`*const CodeBlock` parking (the D1/D5/S3
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
    _code_block_ptr: *const CodeBlock,
    _jit_pending_address: usize,
    _soft_stack_limit_address: usize,
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
    use crate::value::JsValue;
    use crate::vm::entry::round_vm_entry_argument_count_to_align_frame;
    use crate::vm::jsstack::{
        JsStack, Register, VmEntryFrameSeed, CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS,
        REGISTER_SIZE_IN_BYTES,
    };
    use crate::vm::Vm;

    use super::super::function_emitter::{count_property_ic_sites, emit_baseline_function};

    /// One installed Stage-1 baseline image (== `CodeBlock::m_jitCode`): the
    /// finalized, RX-sealed ARM64 code plus the NATIVE JS stack its CallFrames are
    /// built on (A1 / Option A). Owned by the `Vm` at a stable address so the baked
    /// `jit_pending` AbsoluteAddress and the parked `*mut Vm` stay valid for the
    /// image's lifetime (S7).
    pub(crate) struct InstalledBaselineFunction {
        handle: ExecutableMemoryHandle,
        /// The registered `CodeBlock*` for this image, written to CallFrame slot 2 on
        /// VM entry. JSC's `doVMEntry` / JS-call path seeds `CallFrameSlot::codeBlock`
        /// with the entered callee CodeBlock (`CallFrame.h:177`; linked calls patch the
        /// callee-frame CodeBlock before the call via `CallLinkInfo.cpp`). Store the raw
        /// address only; the VM's `CodeBlockRegistry` owns the shared `CodeBlock` and keeps
        /// it stable for the image's lifetime (or the caller supplies the direct borrowed
        /// pointer for standalone/tests where no registry entry exists).
        code_block: *const CodeBlock,
        /// The native JS stack this function's entry CallFrame is `doVMEntry`-seeded
        /// into and the flipped prologue runs on (A1, retiring the pre-A1 hand-placed
        /// scratch arena). Default-sized (`Options::maxPerThreadStackUsage`, 5 MiB)
        /// with a low-end PROT_NONE guard page, so a JIT->interpreter slow-path
        /// far-call (`op_call`'s `operation_call`, get/put_by_val) runs on it with
        /// headroom. A nested JIT entry switches `sp` to its OWN function's stack via
        /// the entry trampoline, so per-function stacks never overlap.
        stack: JsStack,
        /// `CodeBlock::m_numCalleeLocals` (the negative local slots `op_enter`
        /// zero-fills): the `callee_local_count` the entry seed reserves below `fp`.
        num_locals: u32,
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
        /// gc-r4 GAP C (A1.5): the `[region_low, entry_anchor]` bounds of THIS
        /// image's native JS stack, for the conservative GC scan of its live JIT
        /// CallFrame slots. `region_low` = the reservation's lowest allocatable
        /// address (the PROT_NONE guard page is BELOW it, so the scan never faults);
        /// `entry_anchor` = `highAddress()`, the top of the JIT-frame span where the
        /// entry frame is seeded. `run_installed_baseline_jit` pushes this onto the
        /// store's active-span stack around `run`.
        pub(crate) fn frame_scan_bounds(&self) -> (usize, usize) {
            (self.stack.low_address(), self.stack.high_address())
        }

        /// A1.x broad native-call engagement: the absolute entry address of THIS
        /// installed image (the finalized prologue). Faithful analog of
        /// `executable->generatedJITCodeFor(kind)->addressForCall(arity)`
        /// (ExecutableBase) — the callee entry a linked `op_call` jumps to. The
        /// install path records it in the `Vm`'s `baseline_native_targets` registry
        /// keyed by `CodeBlockId` so the per-call resolver can reach it even while the
        /// image is checked out of `baseline_jit_slots` during its own execution (a
        /// self-recursive callee). The RX memory the handle owns does not move when the
        /// `InstalledBaselineFunction` is moved by value, so this address stays valid
        /// for the image's lifetime.
        pub(crate) fn native_entry_address(&self) -> usize {
            self.handle.entry_address()
        }
        /// A1.4: the soft stack limit for THIS function's native stack — its
        /// `JsStack::stack_limit()` (low bound + soft-reserved zone). The driver
        /// writes this into `Vm::set_jit_soft_stack_limit` immediately before `run`
        /// so the emitted prologue's overflow check (which loads the VM's baked
        /// `addressOfSoftStackLimit`) compares against the stack the frame runs on.
        pub(crate) fn soft_stack_limit(&self) -> usize {
            self.stack.stack_limit()
        }

        /// `doVMEntry`-seed the callee `CallFrame` on the native JS stack (header +
        /// `this` + args + undefined-filled locals), then enter the image through
        /// the baseline-JIT entry trampoline with `sp = calleeFrame +
        /// sizeof(CallerFrameAndPC)` and the `*mut Vm` in x0. The flipped
        /// `emitFunctionPrologue` (`pushPair(fp,lr); mov fp,sp`) makes `fp` the
        /// callee frame, so `addressFor(operand)` reads the seeded slots. Returns
        /// the `op_ret` boxed value (`returnValueGPR`/x0); the driver reads the
        /// `jit_pending` mirror for the throw edge.
        ///
        /// `arguments_including_this[0]` is `this`; `[1..]` are the real arguments
        /// (`argumentCountIncludingThis - 1`).
        pub(crate) fn run(
            &mut self,
            vm_ptr_bits: u64,
            callee_bits: u64,
            arguments_including_this: &[u64],
        ) -> u64 {
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

            // Reset to an empty stack: this function's entry frame is the BOTTOM of
            // its own native-stack region (a nested JIT entry runs on its own
            // function's stack, switched in by the trampoline). Each entry seeds
            // afresh from the high end; `release_frame` bookkeeping is unnecessary.
            self.stack
                .set_current_stack_pointer(self.stack.high_address());

            let undefined_bits = JsValue::undefined().encoded().0;
            // doVMEntry inputs. `argumentCountIncludingThis` is the full slice len
            // (slot-0 == `this`); the real args are `[1..]`. Always >= 1 by the JS
            // calling convention (the driver always supplies `this`).
            let count_including_this = arguments_including_this.len().max(1) as u32;
            let this_bits = arguments_including_this
                .first()
                .copied()
                .unwrap_or(undefined_bits);
            let arg_regs: Vec<Register> = arguments_including_this
                .iter()
                .skip(1)
                .map(|&bits| Register::from_bits(bits))
                .collect();
            let padded = round_vm_entry_argument_count_to_align_frame(count_including_this);
            let seed = VmEntryFrameSeed {
                // EntryFrame sentinel + return-PC: the prologue's `pushPair(fp,lr)`
                // re-stamps slots 0/1 on entry; not stack-walked in this unit.
                caller_frame_or_entry: Register::from_bits(0),
                return_pc: Register::from_bits(0),
                code_block: Register::from_bits(self.code_block as u64),
                callee: Register::from_bits(callee_bits),
                argument_count_including_this: Register::from_bits(count_including_this as u64),
                this_value: Register::from_bits(this_bits),
                arguments: &arg_regs,
                padded_argument_count: padded,
                callee_local_count: self.num_locals,
            };

            let callee_frame = match self.stack.try_seed_entry_frame(&seed) {
                Ok(frame) => frame.registers().as_ptr() as usize,
                // A bounded allowlisted ENTRY frame in a 5 MiB stack cannot overflow
                // the single-frame seed. Deep NATIVE JIT->JIT recursion overflow is
                // now caught faithfully inside the emitted prologue (A1.4: the
                // softStackLimit check throws a RangeError); this entry-seed guard
                // remains a belt-and-suspenders bail to boxed `undefined` so a seed
                // failure never corrupts memory.
                Err(_) => {
                    debug_assert!(false, "entry frame seed must fit the native JS stack");
                    return undefined_bits;
                }
            };
            // `sp = calleeFrame + sizeof(CallerFrameAndPC)` (LowLevelInterpreter64
            // `makeJavaScriptCall`); the prologue lowers it to `calleeFrame`.
            let entry_sp = callee_frame
                + (CALLER_FRAME_AND_PC_SIZE_IN_REGISTERS as usize) * REGISTER_SIZE_IN_BYTES;
            self.handle.call_baseline_jit_entry(entry_sp, vm_ptr_bits)
        }
    }

    /// COMPILE (S8 synchronous) + INSTALL: lower the whole CodeBlock to one ARM64
    /// image via the Stage-1 emitter (its `Err` is the S4 allowlist), finalize it
    /// into RX executable memory, and reserve the native JS stack its CallFrames
    /// run on. Returns the installed image, or `Declined`/`Finalize` as control
    /// flow.
    pub(crate) fn install_baseline_function(
        code_block: &CodeBlock,
        code_block_ptr: *const CodeBlock,
        jit_pending_address: usize,
        soft_stack_limit_address: usize,
        install_vm: *const Vm,
    ) -> Result<InstalledBaselineFunction, BaselineInstallError> {
        // Allocate the baseline data-IC record store BEFORE emit (CodeBlock.cpp:802
        // `setupWithUnlinkedBaselineCode` allocates `BaselineJITData` with
        // `propertyCacheSize` in the same install step): the `get_by_id`/`put_by_id`
        // structure guard bakes this store's STABLE base + record_index*16, so it
        // must exist and be sized to the property-site count before the emitter walks
        // the bytecode and reads `baseline_jit_data_record_store_base()`. The `Box`
        // is never reallocated, so the baked address stays valid for the image's life.
        let property_site_count =
            count_property_ic_sites(code_block).map_err(BaselineInstallError::Declined)?;
        code_block.install_baseline_jit_data(property_site_count);

        let image =
            emit_baseline_function(code_block, jit_pending_address, soft_stack_limit_address)
                .map_err(BaselineInstallError::Declined)?;
        let mut records = image.link_records;
        let handle =
            finalize_arm64_link_buffer(&MapJitExecutableAllocator, &image.code, &mut records)
                .map_err(BaselineInstallError::Finalize)?;

        let (num_locals, _max_argument_slot) =
            scan_frame_extent(code_block).map_err(BaselineInstallError::Declined)?;
        // A1 / Option A: a default-sized (5 MiB) native JS stack with a low-end
        // PROT_NONE guard page. The image runs on the machine `sp` switched into
        // this region, so the slow-path far-calls (operation_call re-entering the
        // interpreter, get/put_by_val) descend into it and need real headroom —
        // unlike the pre-A1 scratch arena, which only held the register window
        // while the far-calls ran on the host C stack.
        let stack = JsStack::new_default().map_err(BaselineInstallError::Stack)?;
        Ok(InstalledBaselineFunction {
            handle,
            code_block: code_block_ptr,
            stack,
            num_locals,
            // INV-4: capture the install-time Vm base; `run` asserts the reuse-time
            // base matches so a Vm move (which would dangle the baked jit_pending
            // AbsoluteAddress) is caught in debug.
            install_vm,
        })
    }
}
