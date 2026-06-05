//! ARM64 JSC-shaped native-entry stack skeleton.
//!
//! C++ JSC map: `LowLevelInterpreter64.asm` `doVMEntry` creates an
//! `EntryFrame`/`VMEntryRecord`, copies a callee `CallFrame` to the machine
//! stack, publishes `VM::topCallFrame`/`VM::topEntryFrame`, calls generated
//! code, and restores the previous pair. `AssemblyHelpers::emitFunctionPrologue`
//! then makes ARM64 `fp/x29` the baseline `CallFrame*`.
//!
//! This Rust module is still proof-only. It deliberately does not call the
//! current ARM64 C-ABI seed and does not make public native admission succeed.
//! The seed takes a Rust register-file pointer in x1; JSC-shaped admission needs
//! a stack `CallFrame*`.

use core::marker::PhantomData;

use crate::jit::{
    BaselineNativeEntryCallableAuthority, BaselineNativeEntryCallableKind,
    BaselineNativeEntryToken, BaselineNativeEntryTokenKind,
};
use crate::runtime::{CallFrameId, CodeBlockId, EntryFrameId};

use super::entry::{
    vm_entry_argument_count_is_frame_aligned, BaselineNativeDispatchTokenSelection, FrameAddress,
    VmEntryDispatchSelection, VmEntryLaunchDescriptor,
};

const JSC_REGISTER_BYTES: usize = 8;
const JSC_STACK_ALIGNMENT_BYTES: usize = 16;
const JSC_CALLER_FRAME_AND_PC_WORDS: usize = 2;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64NativeEntryFrameAddressSource {
    StackLocalRustEntryGuard,
    BoxedVmStorage,
    RegisterFileWindow,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Arm64NativeEntryStackFrameError {
    LocalAreaDoesNotCoverLiveLocals {
        live_local_count: usize,
        local_area_words: usize,
    },
    LocalAreaDoesNotPreserveStackAlignment {
        local_area_words: usize,
    },
    CallFrameAddressUnaligned {
        call_frame: usize,
    },
    PostAllocationStackPointerUnaligned {
        post_allocation_sp: usize,
    },
    PostAllocationStackPointerNotBelowCallFrame {
        post_allocation_sp: usize,
        call_frame: usize,
    },
    EntryFrameNotAboveCallFrame {
        entry_frame: usize,
        call_frame: usize,
    },
    VmEntryRecordNotBetweenCallFrameAndEntryFrame {
        vm_entry_record: usize,
        call_frame: usize,
        entry_frame: usize,
    },
    NonStackLocalAddressSource {
        source: Arm64NativeEntryFrameAddressSource,
    },
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm64NativeEntryLaunchProofError {
    UnsupportedCallableKind {
        actual: BaselineNativeEntryCallableKind,
    },
    DispatchSelectionNotNormalEntry,
    CallableTokenMismatch {
        expected: BaselineNativeEntryToken,
        actual: BaselineNativeEntryToken,
    },
    NativeDescriptorTokenMismatch {
        expected: BaselineNativeEntryToken,
        actual: BaselineNativeEntryToken,
    },
    SelectedTokenKindMismatch {
        actual: BaselineNativeEntryTokenKind,
    },
    MissingActiveEntryFrame,
    MissingActiveTopCallFrame,
    ActiveTopFrameMismatch {
        expected: CallFrameId,
        actual: CallFrameId,
    },
    EntryCodeBlockMismatch {
        expected: CodeBlockId,
        actual: Option<CodeBlockId>,
    },
    TopFrameCodeBlockMismatch {
        expected: CodeBlockId,
        actual: Option<CodeBlockId>,
    },
    TopFrameEntryMismatch {
        expected: EntryFrameId,
        actual: Option<EntryFrameId>,
    },
    ArgumentCountDoesNotIncludeThis,
    PaddedArgumentCountTooSmall {
        argument_count_including_this: u32,
        padded_argument_count: u32,
    },
    PaddedArgumentCountNotFrameAligned {
        padded_argument_count: u32,
    },
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Arm64NativeEntryLaunchProofRequest<'descriptor> {
    pub(crate) launch_descriptor: &'descriptor VmEntryLaunchDescriptor,
    pub(crate) callable_kind: BaselineNativeEntryCallableKind,
    pub(crate) callable_token: BaselineNativeEntryToken,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Arm64NativeEntryLaunchProof {
    owner: CodeBlockId,
    code_block: CodeBlockId,
    active_entry_frame: EntryFrameId,
    active_top_call_frame: CallFrameId,
    selected_token: BaselineNativeEntryToken,
    argument_count_excluding_this: u32,
    padded_argument_count: u32,
    required_frame_source: Arm64NativeEntryFrameAddressSource,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Arm64NativeEntryStackFrameRequest<const ARGUMENTS_EXCLUDING_THIS: usize> {
    pub(crate) vm: usize,
    pub(crate) context: usize,
    pub(crate) previous_top_call_frame: Option<FrameAddress>,
    pub(crate) previous_top_entry_frame: Option<FrameAddress>,
    pub(crate) code_block: usize,
    pub(crate) callee: u64,
    pub(crate) this_value: u64,
    pub(crate) arguments: [u64; ARGUMENTS_EXCLUDING_THIS],
    pub(crate) live_local_count: usize,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
#[repr(C, align(16))]
pub(crate) struct Arm64VmEntryRecord {
    vm: usize,
    context: usize,
    previous_top_call_frame: usize,
    previous_top_entry_frame: usize,
    // C++ stores a platform-dependent callee-save buffer after the top-frame
    // pair. This placeholder preserves the boundary without claiming real
    // register save/restore support.
    callee_save_registers_buffer_placeholder: usize,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
#[repr(C, align(16))]
pub(crate) struct Arm64VmEntryFrame {
    // C++ `EntryFrame` is an ABI anchor, not the storage for
    // `VMEntryRecord`; `vmEntryRecord(entryFrame)` points below it.
    anchor: usize,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
#[repr(C, align(16))]
pub(crate) struct Arm64StackCallFrame<
    const LOCAL_AREA_WORDS: usize,
    const ARGUMENTS_EXCLUDING_THIS: usize,
> {
    local_area: [u64; LOCAL_AREA_WORDS],
    caller_frame_and_pc: [usize; JSC_CALLER_FRAME_AND_PC_WORDS],
    code_block: usize,
    callee: u64,
    argument_count_including_this: u64,
    this_value: u64,
    arguments: [u64; ARGUMENTS_EXCLUDING_THIS],
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
#[repr(C, align(16))]
pub(crate) struct Arm64NativeEntryStackFrame<
    const LOCAL_AREA_WORDS: usize,
    const ARGUMENTS_EXCLUDING_THIS: usize,
> {
    call_frame: Arm64StackCallFrame<LOCAL_AREA_WORDS, ARGUMENTS_EXCLUDING_THIS>,
    vm_entry_record: Arm64VmEntryRecord,
    entry_frame: Arm64VmEntryFrame,
    live_local_count: usize,
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Arm64NativeEntryStackFrameProof<'frame> {
    source: Arm64NativeEntryFrameAddressSource,
    entry_frame: FrameAddress,
    vm_entry_record: FrameAddress,
    vm_entry_record_previous_top_call_frame: Option<FrameAddress>,
    vm_entry_record_previous_top_entry_frame: Option<FrameAddress>,
    call_frame: FrameAddress,
    post_allocation_sp: FrameAddress,
    local_area_words: usize,
    live_local_count: usize,
    argument_count_excluding_this: usize,
    _stack_frame: PhantomData<&'frame ()>,
}

#[allow(dead_code)]
pub(crate) fn prove_arm64_native_entry_launch_descriptor_for_callable(
    launch_descriptor: &VmEntryLaunchDescriptor,
    callable: BaselineNativeEntryCallableAuthority,
) -> Result<Arm64NativeEntryLaunchProof, Arm64NativeEntryLaunchProofError> {
    prove_arm64_native_entry_launch_descriptor(Arm64NativeEntryLaunchProofRequest {
        launch_descriptor,
        callable_kind: callable.kind(),
        callable_token: callable.token(),
    })
}

#[allow(dead_code)]
pub(crate) fn prove_arm64_native_entry_launch_descriptor(
    request: Arm64NativeEntryLaunchProofRequest<'_>,
) -> Result<Arm64NativeEntryLaunchProof, Arm64NativeEntryLaunchProofError> {
    if request.callable_kind != BaselineNativeEntryCallableKind::P6Arm64EmittedSemanticCAbiEntry {
        return Err(Arm64NativeEntryLaunchProofError::UnsupportedCallableKind {
            actual: request.callable_kind,
        });
    }

    let descriptor = request.launch_descriptor;
    let selected_token = match descriptor.dispatch {
        VmEntryDispatchSelection::BaselineNative(
            BaselineNativeDispatchTokenSelection::NormalEntry { token },
        ) => token,
        VmEntryDispatchSelection::BaselineNative(_) => {
            return Err(Arm64NativeEntryLaunchProofError::DispatchSelectionNotNormalEntry);
        }
    };
    if selected_token != request.callable_token {
        return Err(Arm64NativeEntryLaunchProofError::CallableTokenMismatch {
            expected: selected_token,
            actual: request.callable_token,
        });
    }
    if selected_token != descriptor.native_entry.normal_entry {
        return Err(
            Arm64NativeEntryLaunchProofError::NativeDescriptorTokenMismatch {
                expected: descriptor.native_entry.normal_entry,
                actual: selected_token,
            },
        );
    }
    if selected_token.kind != BaselineNativeEntryTokenKind::Normal {
        return Err(
            Arm64NativeEntryLaunchProofError::SelectedTokenKindMismatch {
                actual: selected_token.kind,
            },
        );
    }

    let active_entry_frame = descriptor
        .scope
        .active_entry_frame
        .ok_or(Arm64NativeEntryLaunchProofError::MissingActiveEntryFrame)?;
    let active_top_call_frame = descriptor
        .scope
        .active_top_call_frame
        .ok_or(Arm64NativeEntryLaunchProofError::MissingActiveTopCallFrame)?;
    if descriptor.call_frame.frame != active_top_call_frame {
        return Err(Arm64NativeEntryLaunchProofError::ActiveTopFrameMismatch {
            expected: active_top_call_frame,
            actual: descriptor.call_frame.frame,
        });
    }
    if descriptor.scope.entry_code_block != Some(descriptor.owner) {
        return Err(Arm64NativeEntryLaunchProofError::EntryCodeBlockMismatch {
            expected: descriptor.owner,
            actual: descriptor.scope.entry_code_block,
        });
    }
    if descriptor.call_frame.code_block != Some(descriptor.code_block) {
        return Err(
            Arm64NativeEntryLaunchProofError::TopFrameCodeBlockMismatch {
                expected: descriptor.code_block,
                actual: descriptor.call_frame.code_block,
            },
        );
    }
    if descriptor.call_frame.entry_frame != Some(active_entry_frame) {
        return Err(Arm64NativeEntryLaunchProofError::TopFrameEntryMismatch {
            expected: active_entry_frame,
            actual: descriptor.call_frame.entry_frame,
        });
    }
    if descriptor.call_frame.argument_count_including_this == 0 {
        return Err(Arm64NativeEntryLaunchProofError::ArgumentCountDoesNotIncludeThis);
    }
    if descriptor.call_frame.padded_argument_count
        < descriptor.call_frame.argument_count_including_this
    {
        return Err(
            Arm64NativeEntryLaunchProofError::PaddedArgumentCountTooSmall {
                argument_count_including_this: descriptor.call_frame.argument_count_including_this,
                padded_argument_count: descriptor.call_frame.padded_argument_count,
            },
        );
    }
    if !vm_entry_argument_count_is_frame_aligned(descriptor.call_frame.padded_argument_count) {
        return Err(
            Arm64NativeEntryLaunchProofError::PaddedArgumentCountNotFrameAligned {
                padded_argument_count: descriptor.call_frame.padded_argument_count,
            },
        );
    }

    Ok(Arm64NativeEntryLaunchProof {
        owner: descriptor.owner,
        code_block: descriptor.code_block,
        active_entry_frame,
        active_top_call_frame,
        selected_token,
        argument_count_excluding_this: descriptor
            .call_frame
            .argument_count_including_this
            .saturating_sub(1),
        padded_argument_count: descriptor.call_frame.padded_argument_count,
        // C++ `doVMEntry` materializes a machine-stack CallFrame and publishes
        // that address as VM::topCallFrame. Rust still calls the temporary ARM64
        // raw-register C ABI in production; this proof records the required
        // future stack source without treating that register window as valid
        // JSC frame residency.
        required_frame_source: Arm64NativeEntryFrameAddressSource::StackLocalRustEntryGuard,
    })
}

#[allow(dead_code)]
pub(crate) fn with_arm64_native_entry_stack_frame<
    const LOCAL_AREA_WORDS: usize,
    const ARGUMENTS_EXCLUDING_THIS: usize,
>(
    request: Arm64NativeEntryStackFrameRequest<ARGUMENTS_EXCLUDING_THIS>,
    body: impl for<'frame> FnOnce(Arm64NativeEntryStackFrameProof<'frame>),
) -> Result<(), Arm64NativeEntryStackFrameError> {
    validate_local_area::<LOCAL_AREA_WORDS>(request.live_local_count)?;
    let frame =
        Arm64NativeEntryStackFrame::<LOCAL_AREA_WORDS, ARGUMENTS_EXCLUDING_THIS>::new(request);
    let proof = frame.proof(Arm64NativeEntryFrameAddressSource::StackLocalRustEntryGuard)?;
    body(proof);
    Ok(())
}

fn validate_arm64_native_entry_stack_frame_candidate(
    source: Arm64NativeEntryFrameAddressSource,
    entry_frame: FrameAddress,
    vm_entry_record: FrameAddress,
    previous_top_call_frame: Option<FrameAddress>,
    previous_top_entry_frame: Option<FrameAddress>,
    call_frame: FrameAddress,
    post_allocation_sp: FrameAddress,
    local_area_words: usize,
    live_local_count: usize,
    argument_count_excluding_this: usize,
) -> Result<Arm64NativeEntryStackFrameProof<'static>, Arm64NativeEntryStackFrameError> {
    if source != Arm64NativeEntryFrameAddressSource::StackLocalRustEntryGuard {
        return Err(Arm64NativeEntryStackFrameError::NonStackLocalAddressSource { source });
    }
    validate_local_area_runtime(local_area_words, live_local_count)?;
    validate_addresses(entry_frame, call_frame, post_allocation_sp, vm_entry_record)?;
    Ok(Arm64NativeEntryStackFrameProof {
        source,
        entry_frame,
        vm_entry_record,
        vm_entry_record_previous_top_call_frame: previous_top_call_frame,
        vm_entry_record_previous_top_entry_frame: previous_top_entry_frame,
        call_frame,
        post_allocation_sp,
        local_area_words,
        live_local_count,
        argument_count_excluding_this,
        _stack_frame: PhantomData,
    })
}

impl<const LOCAL_AREA_WORDS: usize, const ARGUMENTS_EXCLUDING_THIS: usize>
    Arm64NativeEntryStackFrame<LOCAL_AREA_WORDS, ARGUMENTS_EXCLUDING_THIS>
{
    fn new(request: Arm64NativeEntryStackFrameRequest<ARGUMENTS_EXCLUDING_THIS>) -> Self {
        Self {
            call_frame: Arm64StackCallFrame {
                local_area: [0; LOCAL_AREA_WORDS],
                caller_frame_and_pc: [0; JSC_CALLER_FRAME_AND_PC_WORDS],
                code_block: request.code_block,
                callee: request.callee,
                argument_count_including_this: (ARGUMENTS_EXCLUDING_THIS as u64).saturating_add(1),
                this_value: request.this_value,
                arguments: request.arguments,
            },
            vm_entry_record: Arm64VmEntryRecord {
                vm: request.vm,
                context: request.context,
                previous_top_call_frame: option_frame_address_to_raw(
                    request.previous_top_call_frame,
                ),
                previous_top_entry_frame: option_frame_address_to_raw(
                    request.previous_top_entry_frame,
                ),
                callee_save_registers_buffer_placeholder: 0,
            },
            entry_frame: Arm64VmEntryFrame { anchor: 0 },
            live_local_count: request.live_local_count,
        }
    }

    fn proof(
        &self,
        source: Arm64NativeEntryFrameAddressSource,
    ) -> Result<Arm64NativeEntryStackFrameProof<'_>, Arm64NativeEntryStackFrameError> {
        validate_arm64_native_entry_stack_frame_candidate(
            source,
            self.entry_frame_address(),
            self.vm_entry_record_address(),
            raw_frame_address_to_option(self.vm_entry_record.previous_top_call_frame),
            raw_frame_address_to_option(self.vm_entry_record.previous_top_entry_frame),
            self.call_frame_address(),
            self.post_allocation_sp(),
            LOCAL_AREA_WORDS,
            self.live_local_count,
            ARGUMENTS_EXCLUDING_THIS,
        )
        .map(|proof| Arm64NativeEntryStackFrameProof {
            _stack_frame: PhantomData,
            ..proof
        })
    }

    fn call_frame_address(&self) -> FrameAddress {
        FrameAddress((&self.call_frame.caller_frame_and_pc as *const [usize; 2]) as usize)
    }

    fn post_allocation_sp(&self) -> FrameAddress {
        FrameAddress((&self.call_frame.local_area as *const [u64; LOCAL_AREA_WORDS]) as usize)
    }

    fn entry_frame_address(&self) -> FrameAddress {
        FrameAddress((&self.entry_frame as *const Arm64VmEntryFrame) as usize)
    }

    fn vm_entry_record_address(&self) -> FrameAddress {
        FrameAddress((&self.vm_entry_record as *const Arm64VmEntryRecord) as usize)
    }
}

const fn option_frame_address_to_raw(address: Option<FrameAddress>) -> usize {
    match address {
        Some(address) => address.0,
        None => 0,
    }
}

const fn raw_frame_address_to_option(address: usize) -> Option<FrameAddress> {
    if address == 0 {
        None
    } else {
        Some(FrameAddress(address))
    }
}

fn validate_local_area<const LOCAL_AREA_WORDS: usize>(
    live_local_count: usize,
) -> Result<(), Arm64NativeEntryStackFrameError> {
    validate_local_area_runtime(LOCAL_AREA_WORDS, live_local_count)
}

fn validate_local_area_runtime(
    local_area_words: usize,
    live_local_count: usize,
) -> Result<(), Arm64NativeEntryStackFrameError> {
    if local_area_words < live_local_count {
        return Err(
            Arm64NativeEntryStackFrameError::LocalAreaDoesNotCoverLiveLocals {
                live_local_count,
                local_area_words,
            },
        );
    }
    if local_area_words.saturating_mul(JSC_REGISTER_BYTES) % JSC_STACK_ALIGNMENT_BYTES != 0 {
        return Err(
            Arm64NativeEntryStackFrameError::LocalAreaDoesNotPreserveStackAlignment {
                local_area_words,
            },
        );
    }
    Ok(())
}

fn validate_addresses(
    entry_frame: FrameAddress,
    call_frame: FrameAddress,
    post_allocation_sp: FrameAddress,
    vm_entry_record: FrameAddress,
) -> Result<(), Arm64NativeEntryStackFrameError> {
    if call_frame.0 % JSC_STACK_ALIGNMENT_BYTES != 0 {
        return Err(Arm64NativeEntryStackFrameError::CallFrameAddressUnaligned {
            call_frame: call_frame.0,
        });
    }
    if post_allocation_sp.0 % JSC_STACK_ALIGNMENT_BYTES != 0 {
        return Err(
            Arm64NativeEntryStackFrameError::PostAllocationStackPointerUnaligned {
                post_allocation_sp: post_allocation_sp.0,
            },
        );
    }
    if post_allocation_sp.0 >= call_frame.0 {
        return Err(
            Arm64NativeEntryStackFrameError::PostAllocationStackPointerNotBelowCallFrame {
                post_allocation_sp: post_allocation_sp.0,
                call_frame: call_frame.0,
            },
        );
    }
    if entry_frame.0 <= call_frame.0 {
        return Err(
            Arm64NativeEntryStackFrameError::EntryFrameNotAboveCallFrame {
                entry_frame: entry_frame.0,
                call_frame: call_frame.0,
            },
        );
    }
    if vm_entry_record.0 <= call_frame.0 || vm_entry_record.0 >= entry_frame.0 {
        return Err(
            Arm64NativeEntryStackFrameError::VmEntryRecordNotBetweenCallFrameAndEntryFrame {
                vm_entry_record: vm_entry_record.0,
                call_frame: call_frame.0,
                entry_frame: entry_frame.0,
            },
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::entry::{
        VmEntryCallFrameMetadata, VmEntryLaunchArgumentValue, VmEntryLaunchScope,
    };
    use super::super::tiering::{
        BaselineEntryGateOutcome, BaselineEntryGateRecord, BaselineNativeEntryExecutionPolicy,
        BaselineNativeEntryReadinessOutcome, BaselineNativeEntryReadinessRecord,
    };
    use super::*;
    use crate::gc::CellId;
    use crate::jit::code::BaselineEntryArtifact;
    use crate::jit::{
        CodeFinalizationAuthority, CodeLiveness, CodeOrigin, CodeOriginKind, CodeOwnership,
        EntryAbi, Entrypoint, EntrypointKind, ExecutableAllocationId,
        ExecutableAllocationLifecycle, ExecutableMemoryProtection, ExecutableMutationAuthority,
        JitCodeArtifact, JitCodeId, JitType, MachineCodeHandle, MachineCodeOwnership,
        MachineCodeRange,
    };
    use crate::runtime::{ArityCheckMode, CodeSpecializationKind, NativeCodeId, RuntimeValue};

    fn request() -> Arm64NativeEntryStackFrameRequest<2> {
        Arm64NativeEntryStackFrameRequest {
            vm: 0x1000,
            context: 0x2000,
            previous_top_call_frame: Some(FrameAddress(0x3000)),
            previous_top_entry_frame: Some(FrameAddress(0x4000)),
            code_block: 0x5000,
            callee: 0x6000,
            this_value: 0x7000,
            arguments: [0x8000, 0x9000],
            live_local_count: 1,
        }
    }

    fn launch_descriptor() -> VmEntryLaunchDescriptor {
        let owner = CodeBlockId(CellId(20));
        let artifact = baseline_entry_artifact(owner, 20);
        let descriptor = artifact
            .validate_native_entry_descriptor()
            .expect("native entry descriptor");
        let readiness = BaselineNativeEntryReadinessRecord {
            ordinal: 7,
            owner,
            materialization_ordinal: 1,
            install_ordinal: 2,
            artifact_id: Some(artifact.id),
            native_code: Some(artifact.native_code),
            machine_code: Some(artifact.machine_code),
            machine_range: Some(artifact.machine_code.range),
            bytecode_snapshot: None,
            body_capability: None,
            execution_policy: BaselineNativeEntryExecutionPolicy::Enabled,
            descriptor: Some(descriptor),
            callable: None,
            outcome: BaselineNativeEntryReadinessOutcome::Ready,
        };
        let gate = BaselineEntryGateRecord {
            owner,
            requested_tier: JitType::Baseline,
            native_artifact: Some(artifact),
            native_entry_readiness_ordinal: Some(readiness.ordinal),
            generated_artifact: None,
            outcome: BaselineEntryGateOutcome::NativeEntryReady,
        };
        VmEntryLaunchDescriptor::baseline_native_entry(
            launch_scope(owner),
            launch_call_frame(owner),
            gate,
            &readiness,
        )
        .expect("valid ARM64 launch descriptor")
    }

    fn launch_scope(owner: CodeBlockId) -> VmEntryLaunchScope {
        VmEntryLaunchScope {
            owner,
            entry_code_block: Some(owner),
            active_entry_frame: Some(EntryFrameId(1)),
            previous_entry_frame: None,
            saved_top_call_frame: None,
            active_top_call_frame: Some(CallFrameId(2)),
        }
    }

    fn launch_call_frame(owner: CodeBlockId) -> VmEntryCallFrameMetadata {
        VmEntryCallFrameMetadata {
            frame: CallFrameId(2),
            entry_frame: Some(EntryFrameId(1)),
            caller_frame: Some(CallFrameId(1)),
            code_block: Some(owner),
            callee: None,
            callee_value: None,
            context: None,
            global_object: None,
            entry_value: VmEntryLaunchArgumentValue::This(RuntimeValue::from_i32(41)),
            argument_count_including_this: 3,
            provided_argument_count: 2,
            padded_argument_count: 5,
            specialization: CodeSpecializationKind::Call,
            arity_mode: ArityCheckMode::AlreadyChecked,
        }
    }

    fn baseline_entry_artifact(owner: CodeBlockId, id: u64) -> BaselineEntryArtifact {
        baseline_artifact(owner, id)
            .validate_baseline_entry_artifact(owner)
            .expect("baseline entry artifact")
    }

    fn baseline_artifact(owner: CodeBlockId, id: u64) -> JitCodeArtifact {
        let code = JitCodeId(id);
        let native_code = NativeCodeId(id as u32 + 100);
        let allocation = ExecutableAllocationId(id + 200);
        JitCodeArtifact {
            id: code,
            tier: JitType::Baseline,
            origin: CodeOrigin {
                kind: CodeOriginKind::BaselineCodeBlock,
                owner: Some(owner),
                executable: None,
                bytecode_index: Some(0),
            },
            ownership: CodeOwnership::CodeBlockOwned,
            native_code: Some(native_code),
            machine_code: Some(MachineCodeHandle {
                allocation,
                owner: MachineCodeOwnership::CodeBlock(owner),
                range: MachineCodeRange {
                    allocation,
                    start_offset: 0,
                    size_bytes: 64,
                },
                symbol: Some(native_code),
                protection: ExecutableMemoryProtection::Executable,
                lifecycle: ExecutableAllocationLifecycle::LinkedExecutable,
                mutation_authority: ExecutableMutationAuthority::LinkBuffer,
            }),
            entrypoint: Entrypoint {
                kind: EntrypointKind::GeneratedCode,
                abi: EntryAbi::GeneratedCode,
                code: Some(code),
                boundary: None,
            },
            patchpoints: Vec::new(),
            dependencies: Vec::new(),
            byproducts: Vec::new(),
            disassembly: None,
            liveness: CodeLiveness::Live,
            finalization_authority: CodeFinalizationAuthority::MainThread,
        }
    }

    #[test]
    fn arm64_native_entry_stack_guard_builds_jsc_ordered_frame_proof() {
        with_arm64_native_entry_stack_frame::<2, 2>(request(), |proof| {
            assert_eq!(
                proof.source,
                Arm64NativeEntryFrameAddressSource::StackLocalRustEntryGuard
            );
            assert_eq!(
                proof.vm_entry_record_previous_top_call_frame,
                Some(FrameAddress(0x3000))
            );
            assert_eq!(
                proof.vm_entry_record_previous_top_entry_frame,
                Some(FrameAddress(0x4000))
            );
            assert_eq!(proof.argument_count_excluding_this, 2);
            assert_eq!(proof.local_area_words, 2);
            assert_eq!(proof.live_local_count, 1);
            assert!(proof.post_allocation_sp.0 < proof.call_frame.0);
            assert!(proof.call_frame.0 < proof.vm_entry_record.0);
            assert!(proof.vm_entry_record.0 < proof.entry_frame.0);
            assert_eq!(proof.call_frame.0 % JSC_STACK_ALIGNMENT_BYTES, 0);
            assert_eq!(proof.post_allocation_sp.0 % JSC_STACK_ALIGNMENT_BYTES, 0);
        })
        .expect("stack-local ARM64 entry proof");
    }

    #[test]
    fn arm64_native_entry_stack_guard_rejects_too_small_local_area() {
        assert_eq!(
            with_arm64_native_entry_stack_frame::<0, 2>(request(), |_| ()),
            Err(
                Arm64NativeEntryStackFrameError::LocalAreaDoesNotCoverLiveLocals {
                    live_local_count: 1,
                    local_area_words: 0,
                },
            )
        );
    }

    #[test]
    fn arm64_native_entry_stack_guard_rejects_unaligned_local_area() {
        assert_eq!(
            with_arm64_native_entry_stack_frame::<1, 2>(request(), |_| ()),
            Err(
                Arm64NativeEntryStackFrameError::LocalAreaDoesNotPreserveStackAlignment {
                    local_area_words: 1,
                },
            )
        );
    }

    #[test]
    fn arm64_native_entry_candidate_rejects_boxed_storage_source() {
        assert_eq!(
            validate_arm64_native_entry_stack_frame_candidate(
                Arm64NativeEntryFrameAddressSource::BoxedVmStorage,
                FrameAddress(0x3000),
                FrameAddress(0x3000),
                None,
                None,
                FrameAddress(0x2000),
                FrameAddress(0x1ff0),
                2,
                1,
                0,
            ),
            Err(
                Arm64NativeEntryStackFrameError::NonStackLocalAddressSource {
                    source: Arm64NativeEntryFrameAddressSource::BoxedVmStorage,
                }
            )
        );
    }

    #[test]
    fn arm64_native_entry_candidate_rejects_register_file_window_source() {
        assert_eq!(
            validate_arm64_native_entry_stack_frame_candidate(
                Arm64NativeEntryFrameAddressSource::RegisterFileWindow,
                FrameAddress(0x3000),
                FrameAddress(0x2800),
                None,
                None,
                FrameAddress(0x2000),
                FrameAddress(0x1ff0),
                2,
                1,
                0,
            ),
            Err(
                Arm64NativeEntryStackFrameError::NonStackLocalAddressSource {
                    source: Arm64NativeEntryFrameAddressSource::RegisterFileWindow,
                }
            )
        );
    }

    #[test]
    fn arm64_native_entry_candidate_rejects_vm_entry_record_outside_jsc_order() {
        assert_eq!(
            validate_arm64_native_entry_stack_frame_candidate(
                Arm64NativeEntryFrameAddressSource::StackLocalRustEntryGuard,
                FrameAddress(0x3000),
                FrameAddress(0x3000),
                None,
                None,
                FrameAddress(0x2000),
                FrameAddress(0x1ff0),
                2,
                1,
                0,
            ),
            Err(
                Arm64NativeEntryStackFrameError::VmEntryRecordNotBetweenCallFrameAndEntryFrame {
                    vm_entry_record: 0x3000,
                    call_frame: 0x2000,
                    entry_frame: 0x3000,
                }
            )
        );
    }

    #[test]
    fn arm64_native_entry_launch_proof_accepts_normal_arm64_descriptor() {
        let descriptor = launch_descriptor();
        let proof =
            prove_arm64_native_entry_launch_descriptor(Arm64NativeEntryLaunchProofRequest {
                launch_descriptor: &descriptor,
                callable_kind: BaselineNativeEntryCallableKind::P6Arm64EmittedSemanticCAbiEntry,
                callable_token: descriptor.native_entry.normal_entry,
            })
            .expect("ARM64 launch proof");

        assert_eq!(proof.owner, descriptor.owner);
        assert_eq!(proof.code_block, descriptor.code_block);
        assert_eq!(proof.active_entry_frame, EntryFrameId(1));
        assert_eq!(proof.active_top_call_frame, CallFrameId(2));
        assert_eq!(proof.argument_count_excluding_this, 2);
        assert_eq!(proof.padded_argument_count, 5);
        assert_eq!(
            proof.required_frame_source,
            Arm64NativeEntryFrameAddressSource::StackLocalRustEntryGuard
        );
    }

    #[test]
    fn arm64_native_entry_launch_proof_rejects_non_arm64_callable_kind() {
        let descriptor = launch_descriptor();
        assert_eq!(
            prove_arm64_native_entry_launch_descriptor(Arm64NativeEntryLaunchProofRequest {
                launch_descriptor: &descriptor,
                callable_kind: BaselineNativeEntryCallableKind::P6X86_64EmittedSemanticCAbiEntry,
                callable_token: descriptor.native_entry.normal_entry,
            }),
            Err(Arm64NativeEntryLaunchProofError::UnsupportedCallableKind {
                actual: BaselineNativeEntryCallableKind::P6X86_64EmittedSemanticCAbiEntry,
            })
        );
    }

    #[test]
    fn arm64_native_entry_launch_proof_rejects_arity_dispatch() {
        let mut descriptor = launch_descriptor();
        descriptor.dispatch = VmEntryDispatchSelection::BaselineNative(
            BaselineNativeDispatchTokenSelection::ArityCheckUnavailable {
                reason: crate::jit::BaselineArityCheckUnavailableReason::NotEmitted,
            },
        );

        assert_eq!(
            prove_arm64_native_entry_launch_descriptor(Arm64NativeEntryLaunchProofRequest {
                launch_descriptor: &descriptor,
                callable_kind: BaselineNativeEntryCallableKind::P6Arm64EmittedSemanticCAbiEntry,
                callable_token: descriptor.native_entry.normal_entry,
            }),
            Err(Arm64NativeEntryLaunchProofError::DispatchSelectionNotNormalEntry)
        );
    }

    #[test]
    fn arm64_native_entry_launch_proof_rejects_top_frame_mismatch() {
        let mut descriptor = launch_descriptor();
        descriptor.call_frame.frame = CallFrameId(99);

        assert_eq!(
            prove_arm64_native_entry_launch_descriptor(Arm64NativeEntryLaunchProofRequest {
                launch_descriptor: &descriptor,
                callable_kind: BaselineNativeEntryCallableKind::P6Arm64EmittedSemanticCAbiEntry,
                callable_token: descriptor.native_entry.normal_entry,
            }),
            Err(Arm64NativeEntryLaunchProofError::ActiveTopFrameMismatch {
                expected: CallFrameId(2),
                actual: CallFrameId(99),
            })
        );
    }

    #[test]
    fn arm64_native_entry_launch_proof_rejects_unaligned_padded_argument_count() {
        let mut descriptor = launch_descriptor();
        descriptor.call_frame.padded_argument_count = 4;

        assert_eq!(
            prove_arm64_native_entry_launch_descriptor(Arm64NativeEntryLaunchProofRequest {
                launch_descriptor: &descriptor,
                callable_kind: BaselineNativeEntryCallableKind::P6Arm64EmittedSemanticCAbiEntry,
                callable_token: descriptor.native_entry.normal_entry,
            }),
            Err(
                Arm64NativeEntryLaunchProofError::PaddedArgumentCountNotFrameAligned {
                    padded_argument_count: 4,
                }
            )
        );
    }
}
