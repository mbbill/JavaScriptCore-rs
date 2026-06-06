//! ARM64 public JSC stack-dispatch precondition proof.
//!
//! C++ JSC map: `Interpreter::execute*` prepares code under `DeferTraps`,
//! captures a retained `RefPtr<JITCode>` under `AssertNoGC`, initializes a
//! `ProtoCallFrame`, and then calls `vmEntryToJavaScript(jitCode->addressForCall(), ...)`.
//! `LowLevelInterpreter64.asm` `doVMEntry` publishes the stack CallFrame and
//! `EntryFrame`, `_llint_call_javascript` enters generated code with JSC's
//! stack shape, normal return restores the previous VM top-frame pair, and
//! throw routing is staged separately by `genericUnwind`. Rust keeps this
//! module descriptor-only: it proves the entry preconditions Rust can describe
//! today and records that the platform trampoline is normal-return-only. It
//! does not install a real `DeferTraps`/`AssertNoGC` execution region, expose a
//! public generated-code call, or claim caught/uncaught platform routing.

use crate::bytecode::{
    CodeBlockLifecycleState, ExecutableBaselineNativeEntrySelection, ExecutableEntryCacheRecord,
    JitCodeSlot,
};
use crate::jit::code::BaselineEntryArtifact;
use crate::jit::{
    BaselineNativeEntryToken, CodeLiveness, CodeRetentionPolicy, ExecutableAllocationLifecycle,
    ExecutableMemoryProtection, JitCodeValidationError, MachineCodeHandle,
    MachineCodeValidationError,
};
use crate::platform::executable_memory_compartment::{
    ExecutableMemoryArm64JscStackCallRequest,
    ExecutableMemoryArm64JscStackCallRequestValidationError,
};
use crate::runtime::CodeBlockId;

use super::super::entry::FrameAddress;
use super::arm64_exception_unwind::{
    validate_p6_arm64_verified_vm_entry_exception_unwind_restoration_proof,
    P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof,
    P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofMismatch,
};
use super::rooting::P6Arm64BranchAwareCallableTopCallFramePublicationProof;

const JSC_REGISTER_BYTES: usize = core::mem::size_of::<usize>();

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof<'publication> {
    vm_entry_exception_unwind_restoration_proof:
        P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<'publication>,
    executable_lifetime: P6Arm64PublicJscStackDispatchExecutableLifetimeRecord,
    vm_entry_window: P6Arm64PublicJscStackDispatchVmEntryWindowRecord,
    code_block_frame_extent: P6Arm64PublicJscStackDispatchCodeBlockFrameExtentRecord,
    platform_envelope: P6Arm64PublicJscStackDispatchPlatformEnvelopeRecord,
}

impl<'publication> P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof<'publication> {
    #[allow(dead_code)]
    pub(in crate::vm) fn from_public_jsc_stack_dispatch_preconditions(
        vm_entry_exception_unwind_restoration_proof:
            P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<'publication>,
        executable_lifetime: P6Arm64PublicJscStackDispatchExecutableLifetimeRecord,
        vm_entry_window: P6Arm64PublicJscStackDispatchVmEntryWindowRecord,
        code_block_frame_extent: P6Arm64PublicJscStackDispatchCodeBlockFrameExtentRecord,
        platform_envelope: P6Arm64PublicJscStackDispatchPlatformEnvelopeRecord,
    ) -> Result<Self, P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError> {
        validate_p6_arm64_public_jsc_stack_dispatch_preconditions_linkage(
            &vm_entry_exception_unwind_restoration_proof,
            &executable_lifetime,
            &vm_entry_window,
            &code_block_frame_extent,
            &platform_envelope,
        )
        .map_err(P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError::Linkage)?;

        Ok(Self {
            vm_entry_exception_unwind_restoration_proof,
            executable_lifetime,
            vm_entry_window,
            code_block_frame_extent,
            platform_envelope,
        })
    }

    pub(in crate::vm) fn vm_entry_exception_unwind_restoration_proof(
        &self,
    ) -> &P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<'publication> {
        &self.vm_entry_exception_unwind_restoration_proof
    }

    pub(in crate::vm) const fn executable_lifetime(
        &self,
    ) -> &P6Arm64PublicJscStackDispatchExecutableLifetimeRecord {
        &self.executable_lifetime
    }

    pub(in crate::vm) const fn vm_entry_window(
        &self,
    ) -> &P6Arm64PublicJscStackDispatchVmEntryWindowRecord {
        &self.vm_entry_window
    }

    pub(in crate::vm) const fn code_block_frame_extent(
        &self,
    ) -> &P6Arm64PublicJscStackDispatchCodeBlockFrameExtentRecord {
        &self.code_block_frame_extent
    }

    pub(in crate::vm) const fn platform_envelope(
        &self,
    ) -> &P6Arm64PublicJscStackDispatchPlatformEnvelopeRecord {
        &self.platform_envelope
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64PublicJscStackDispatchExecutableLifetimeRecord {
    pub(in crate::vm) baseline_entry_artifact: BaselineEntryArtifact,
    pub(in crate::vm) executable_entry_record: ExecutableEntryCacheRecord,
    pub(in crate::vm) selected_token: BaselineNativeEntryToken,
    pub(in crate::vm) current_baseline_jit_slot: JitCodeSlot,
    pub(in crate::vm) code_block_lifecycle: CodeBlockLifecycleState,
    pub(in crate::vm) code_liveness: CodeLiveness,
    pub(in crate::vm) retention_policy: CodeRetentionPolicy,
    pub(in crate::vm) machine_code: MachineCodeHandle,
    pub(in crate::vm) entry_offset: u32,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64PublicJscStackDispatchVmEntryWindowRecord {
    /// C++ uses `DeferTraps` so the about-to-run code cannot be jettisoned.
    /// Rust records this as metadata until the real VM execution region owns
    /// trap deferral.
    pub(in crate::vm) traps_deferred: bool,
    /// C++ uses `AssertNoGC` while reading generated JIT code and initializing
    /// the proto frame. Rust records this as metadata until the real execution
    /// call owns the no-GC region.
    pub(in crate::vm) no_gc: bool,
    pub(in crate::vm) retained_jit_code_ref: bool,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64PublicJscStackDispatchCodeBlockFrameExtentRecord {
    pub(in crate::vm) code_block: CodeBlockId,
    pub(in crate::vm) call_frame: FrameAddress,
    pub(in crate::vm) num_callee_locals: usize,
    pub(in crate::vm) max_frame_extent_for_slow_path_call_bytes: usize,
    pub(in crate::vm) restored_stack_pointer: FrameAddress,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64PublicJscStackDispatchPlatformEnvelopeRecord {
    pub(in crate::vm) platform_request: ExecutableMemoryArm64JscStackCallRequest,
    pub(in crate::vm) supports_normal_return: bool,
    pub(in crate::vm) supports_caught_exception_exit: bool,
    pub(in crate::vm) supports_uncaught_exception_exit: bool,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofError {
    Linkage(P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch),
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofMismatch {
    ExceptionUnwind(P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofMismatch),
    Linkage(P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch),
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64PublicJscStackDispatchPlatformRequestField {
    EntryOffset,
    EntryStackPointer,
    CallFrame,
    EntryFrame,
    VmEntryCalleeSaveBuffer,
    VmEntryCalleeSaveRegisterCount,
    VmEntryCalleeSaveBufferBytes,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch {
    SelectedTokenMismatch {
        expected: BaselineNativeEntryToken,
        actual: BaselineNativeEntryToken,
    },
    ArtifactOwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    ArtifactIdMismatch {
        expected: crate::jit::JitCodeId,
        actual: crate::jit::JitCodeId,
    },
    ArtifactNativeSymbolMismatch {
        expected: crate::runtime::NativeCodeId,
        actual: crate::runtime::NativeCodeId,
    },
    ArtifactMachineCodeMismatch {
        expected: MachineCodeHandle,
        actual: MachineCodeHandle,
    },
    ArtifactEntrypointMismatch,
    ArtifactLivenessMismatch {
        actual: CodeLiveness,
    },
    CodeLivenessMismatch {
        expected: CodeLiveness,
        actual: CodeLiveness,
    },
    RetentionPolicyMismatch {
        actual: CodeRetentionPolicy,
    },
    CodeBlockLifecycleMismatch {
        actual: CodeBlockLifecycleState,
    },
    BaselineArtifactValidation {
        error: JitCodeValidationError,
    },
    MachineCodeInvalid {
        error: MachineCodeValidationError,
    },
    MachineCodeNotExecutable {
        protection: ExecutableMemoryProtection,
        lifecycle: ExecutableAllocationLifecycle,
    },
    MachineCodeDrift {
        expected: MachineCodeHandle,
        actual: MachineCodeHandle,
    },
    EntryOffsetMismatch {
        expected: u32,
        actual: u32,
    },
    ExecutableCacheOwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    ExecutableCacheCodeBlockMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    ExecutableCacheBaselineJitSlotMismatch {
        expected: JitCodeSlot,
        actual: JitCodeSlot,
    },
    ExecutableCacheArtifactMismatch {
        expected: crate::jit::JitCodeId,
        actual: crate::jit::JitCodeId,
    },
    ExecutableCacheNativeSymbolMismatch {
        expected: crate::runtime::NativeCodeId,
        actual: crate::runtime::NativeCodeId,
    },
    ExecutableCacheMachineCodeMismatch {
        expected: MachineCodeHandle,
        actual: MachineCodeHandle,
    },
    ExecutableCacheEntrypointMismatch,
    ExecutableCacheMachineRangeMismatch,
    ExecutableCacheSelectionMismatch,
    TrapDeferralMissing,
    NoGcMissing,
    RetainedJitCodeRefMissing,
    FrameExtentCodeBlockMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    FrameExtentCallFrameMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    FrameExtentSizeOverflow,
    FrameExtentStackPointerUnderflow {
        call_frame: FrameAddress,
        frame_extent_bytes: usize,
    },
    FrameExtentRestoredStackPointerMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    CaughtRestoreStackPointerMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    PlatformRequestMismatch {
        field: P6Arm64PublicJscStackDispatchPlatformRequestField,
        expected: usize,
        actual: usize,
    },
    PlatformRequestInvalid {
        reason: ExecutableMemoryArm64JscStackCallRequestValidationError,
    },
    PlatformNormalReturnUnsupported,
    PlatformCaughtExceptionExitClaimed,
    PlatformUncaughtExceptionExitClaimed,
}

pub(in crate::vm) fn validate_p6_arm64_verified_public_jsc_stack_dispatch_preconditions_proof(
    top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
    preconditions_proof: &P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof<'_>,
    expected_live_local_slots: usize,
) -> Result<(), P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofMismatch> {
    validate_p6_arm64_verified_vm_entry_exception_unwind_restoration_proof(
        top_call_frame_publication,
        preconditions_proof.vm_entry_exception_unwind_restoration_proof(),
        expected_live_local_slots,
    )
    .map_err(P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofMismatch::ExceptionUnwind)?;
    validate_p6_arm64_public_jsc_stack_dispatch_preconditions_linkage(
        preconditions_proof.vm_entry_exception_unwind_restoration_proof(),
        preconditions_proof.executable_lifetime(),
        preconditions_proof.vm_entry_window(),
        preconditions_proof.code_block_frame_extent(),
        preconditions_proof.platform_envelope(),
    )
    .map_err(P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofMismatch::Linkage)
}

fn validate_p6_arm64_public_jsc_stack_dispatch_preconditions_linkage(
    vm_entry_exception_unwind_restoration_proof:
        &P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<'_>,
    executable_lifetime: &P6Arm64PublicJscStackDispatchExecutableLifetimeRecord,
    vm_entry_window: &P6Arm64PublicJscStackDispatchVmEntryWindowRecord,
    code_block_frame_extent: &P6Arm64PublicJscStackDispatchCodeBlockFrameExtentRecord,
    platform_envelope: &P6Arm64PublicJscStackDispatchPlatformEnvelopeRecord,
) -> Result<(), P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch> {
    validate_executable_lifetime(
        vm_entry_exception_unwind_restoration_proof,
        executable_lifetime,
    )?;
    validate_vm_entry_window(vm_entry_window)?;
    validate_code_block_frame_extent(
        vm_entry_exception_unwind_restoration_proof,
        executable_lifetime,
        code_block_frame_extent,
    )?;
    validate_platform_envelope(
        vm_entry_exception_unwind_restoration_proof,
        platform_envelope,
    )
}

fn validate_executable_lifetime(
    vm_entry_exception_unwind_restoration_proof:
        &P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<'_>,
    executable_lifetime: &P6Arm64PublicJscStackDispatchExecutableLifetimeRecord,
) -> Result<(), P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch> {
    let dispatch_proof = vm_entry_exception_unwind_restoration_proof
        .vm_entry_normal_return_restoration_proof()
        .jsc_stack_dispatch_request_proof();
    let executable_entry = dispatch_proof.executable_entry();
    let expected_token = executable_entry.selected_token;
    let artifact = executable_lifetime.baseline_entry_artifact;
    if executable_lifetime.selected_token != expected_token {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::SelectedTokenMismatch {
                expected: expected_token,
                actual: executable_lifetime.selected_token,
            },
        );
    }
    if artifact.owner != expected_token.owner {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ArtifactOwnerMismatch {
                expected: expected_token.owner,
                actual: artifact.owner,
            },
        );
    }
    if artifact.id != expected_token.artifact_id {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ArtifactIdMismatch {
                expected: expected_token.artifact_id,
                actual: artifact.id,
            },
        );
    }
    if artifact.native_code != expected_token.native_symbol {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ArtifactNativeSymbolMismatch {
                expected: expected_token.native_symbol,
                actual: artifact.native_code,
            },
        );
    }
    if artifact.machine_code != expected_token.machine_code {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ArtifactMachineCodeMismatch {
                expected: expected_token.machine_code,
                actual: artifact.machine_code,
            },
        );
    }
    if artifact.entrypoint != expected_token.entrypoint {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ArtifactEntrypointMismatch,
        );
    }
    if artifact.liveness != CodeLiveness::Live {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ArtifactLivenessMismatch {
                actual: artifact.liveness,
            },
        );
    }
    if executable_lifetime.code_liveness != artifact.liveness {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::CodeLivenessMismatch {
                expected: artifact.liveness,
                actual: executable_lifetime.code_liveness,
            },
        );
    }
    if executable_lifetime.code_liveness != CodeLiveness::Live {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::CodeLivenessMismatch {
                expected: CodeLiveness::Live,
                actual: executable_lifetime.code_liveness,
            },
        );
    }
    if executable_lifetime.retention_policy != CodeRetentionPolicy::CodeBlockKeepsAlive {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::RetentionPolicyMismatch {
                actual: executable_lifetime.retention_policy,
            },
        );
    }
    if executable_lifetime.code_block_lifecycle != CodeBlockLifecycleState::BaselineInstalled {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::CodeBlockLifecycleMismatch {
                actual: executable_lifetime.code_block_lifecycle,
            },
        );
    }
    executable_lifetime
        .machine_code
        .validate()
        .map_err(|error| {
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::MachineCodeInvalid { error }
        })?;
    if executable_lifetime.machine_code.protection != ExecutableMemoryProtection::Executable
        || executable_lifetime.machine_code.lifecycle
            != ExecutableAllocationLifecycle::LinkedExecutable
    {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::MachineCodeNotExecutable {
                protection: executable_lifetime.machine_code.protection,
                lifecycle: executable_lifetime.machine_code.lifecycle,
            },
        );
    }
    if executable_lifetime.machine_code != expected_token.machine_code {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::MachineCodeDrift {
                expected: expected_token.machine_code,
                actual: executable_lifetime.machine_code,
            },
        );
    }
    artifact
        .validate_native_entry_descriptor()
        .map_err(|error| {
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::BaselineArtifactValidation {
                error,
            }
        })?;
    if executable_lifetime.entry_offset != executable_entry.entry_offset {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::EntryOffsetMismatch {
                expected: executable_entry.entry_offset,
                actual: executable_lifetime.entry_offset,
            },
        );
    }
    let dispatch = dispatch_proof.jsc_stack_dispatch_request_proof();
    if executable_lifetime.entry_offset != dispatch.entry_offset {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::EntryOffsetMismatch {
                expected: dispatch.entry_offset,
                actual: executable_lifetime.entry_offset,
            },
        );
    }
    validate_executable_entry_cache(executable_lifetime, expected_token)
}

fn validate_executable_entry_cache(
    executable_lifetime: &P6Arm64PublicJscStackDispatchExecutableLifetimeRecord,
    expected_token: BaselineNativeEntryToken,
) -> Result<(), P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch> {
    let cache = executable_lifetime.executable_entry_record;
    if cache.owner != expected_token.owner {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ExecutableCacheOwnerMismatch {
                expected: expected_token.owner,
                actual: cache.owner,
            },
        );
    }
    if cache.code_block != expected_token.owner {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ExecutableCacheCodeBlockMismatch {
                expected: expected_token.owner,
                actual: cache.code_block,
            },
        );
    }
    if cache.baseline_jit_slot != executable_lifetime.current_baseline_jit_slot {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ExecutableCacheBaselineJitSlotMismatch {
                expected: executable_lifetime.current_baseline_jit_slot,
                actual: cache.baseline_jit_slot,
            },
        );
    }
    let baseline_entry = cache.baseline_native_entry;
    if baseline_entry.artifact_id != expected_token.artifact_id {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ExecutableCacheArtifactMismatch {
                expected: expected_token.artifact_id,
                actual: baseline_entry.artifact_id,
            },
        );
    }
    if baseline_entry.native_symbol != expected_token.native_symbol {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ExecutableCacheNativeSymbolMismatch {
                expected: expected_token.native_symbol,
                actual: baseline_entry.native_symbol,
            },
        );
    }
    if baseline_entry.machine_code != expected_token.machine_code {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ExecutableCacheMachineCodeMismatch {
                expected: expected_token.machine_code,
                actual: baseline_entry.machine_code,
            },
        );
    }
    if baseline_entry.machine_range != expected_token.machine_code.range {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ExecutableCacheMachineRangeMismatch,
        );
    }
    if baseline_entry.entrypoint != expected_token.entrypoint {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ExecutableCacheEntrypointMismatch,
        );
    }
    match baseline_entry.selection {
        ExecutableBaselineNativeEntrySelection::Normal(token) if token == expected_token => {}
        _ => {
            return Err(
                P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::ExecutableCacheSelectionMismatch,
            );
        }
    }
    Ok(())
}

fn validate_vm_entry_window(
    vm_entry_window: &P6Arm64PublicJscStackDispatchVmEntryWindowRecord,
) -> Result<(), P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch> {
    if !vm_entry_window.traps_deferred {
        return Err(P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::TrapDeferralMissing);
    }
    if !vm_entry_window.no_gc {
        return Err(P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::NoGcMissing);
    }
    if !vm_entry_window.retained_jit_code_ref {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::RetainedJitCodeRefMissing,
        );
    }
    Ok(())
}

fn validate_code_block_frame_extent(
    vm_entry_exception_unwind_restoration_proof:
        &P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<'_>,
    executable_lifetime: &P6Arm64PublicJscStackDispatchExecutableLifetimeRecord,
    code_block_frame_extent: &P6Arm64PublicJscStackDispatchCodeBlockFrameExtentRecord,
) -> Result<(), P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch> {
    if code_block_frame_extent.code_block != executable_lifetime.selected_token.owner {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::FrameExtentCodeBlockMismatch {
                expected: executable_lifetime.selected_token.owner,
                actual: code_block_frame_extent.code_block,
            },
        );
    }
    let dispatch = vm_entry_exception_unwind_restoration_proof
        .vm_entry_normal_return_restoration_proof()
        .jsc_stack_dispatch_request_proof()
        .jsc_stack_dispatch_request_proof();
    if code_block_frame_extent.call_frame != dispatch.call_frame {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::FrameExtentCallFrameMismatch {
                expected: dispatch.call_frame,
                actual: code_block_frame_extent.call_frame,
            },
        );
    }
    let callee_local_bytes = code_block_frame_extent
        .num_callee_locals
        .checked_mul(JSC_REGISTER_BYTES)
        .ok_or(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::FrameExtentSizeOverflow,
        )?;
    let frame_extent_bytes = callee_local_bytes
        .checked_add(code_block_frame_extent.max_frame_extent_for_slow_path_call_bytes)
        .ok_or(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::FrameExtentSizeOverflow,
        )?;
    let expected_sp = code_block_frame_extent
        .call_frame
        .0
        .checked_sub(frame_extent_bytes)
        .map(FrameAddress)
        .ok_or(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::FrameExtentStackPointerUnderflow {
                call_frame: code_block_frame_extent.call_frame,
                frame_extent_bytes,
            },
        )?;
    if code_block_frame_extent.restored_stack_pointer != expected_sp {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::FrameExtentRestoredStackPointerMismatch {
                expected: expected_sp,
                actual: code_block_frame_extent.restored_stack_pointer,
            },
        );
    }
    let caught_sp = vm_entry_exception_unwind_restoration_proof
        .caught_exception_dispatch_restore()
        .reconstructed_catch_sp;
    if caught_sp != code_block_frame_extent.restored_stack_pointer {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::CaughtRestoreStackPointerMismatch {
                expected: code_block_frame_extent.restored_stack_pointer,
                actual: caught_sp,
            },
        );
    }
    Ok(())
}

fn validate_platform_envelope(
    vm_entry_exception_unwind_restoration_proof:
        &P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<'_>,
    platform_envelope: &P6Arm64PublicJscStackDispatchPlatformEnvelopeRecord,
) -> Result<(), P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch> {
    let expected = vm_entry_exception_unwind_restoration_proof
        .vm_entry_normal_return_restoration_proof()
        .jsc_stack_dispatch_request_proof()
        .jsc_stack_dispatch_request_proof()
        .platform_request;
    let actual = platform_envelope.platform_request;
    compare_platform_request(expected, actual)?;
    actual.validate().map_err(|reason| {
        P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::PlatformRequestInvalid { reason }
    })?;
    if !platform_envelope.supports_normal_return {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::PlatformNormalReturnUnsupported,
        );
    }
    if platform_envelope.supports_caught_exception_exit {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::PlatformCaughtExceptionExitClaimed,
        );
    }
    if platform_envelope.supports_uncaught_exception_exit {
        return Err(
            P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::PlatformUncaughtExceptionExitClaimed,
        );
    }
    Ok(())
}

fn compare_platform_request(
    expected: ExecutableMemoryArm64JscStackCallRequest,
    actual: ExecutableMemoryArm64JscStackCallRequest,
) -> Result<(), P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch> {
    for (field, expected, actual) in [
        (
            P6Arm64PublicJscStackDispatchPlatformRequestField::EntryOffset,
            expected.entry_offset as usize,
            actual.entry_offset as usize,
        ),
        (
            P6Arm64PublicJscStackDispatchPlatformRequestField::EntryStackPointer,
            expected.entry_sp.as_ptr() as usize,
            actual.entry_sp.as_ptr() as usize,
        ),
        (
            P6Arm64PublicJscStackDispatchPlatformRequestField::CallFrame,
            expected.call_frame.as_ptr() as usize,
            actual.call_frame.as_ptr() as usize,
        ),
        (
            P6Arm64PublicJscStackDispatchPlatformRequestField::EntryFrame,
            expected.entry_frame.as_ptr() as usize,
            actual.entry_frame.as_ptr() as usize,
        ),
        (
            P6Arm64PublicJscStackDispatchPlatformRequestField::VmEntryCalleeSaveBuffer,
            expected.vm_entry_record_callee_save_buffer.as_ptr() as usize,
            actual.vm_entry_record_callee_save_buffer.as_ptr() as usize,
        ),
        (
            P6Arm64PublicJscStackDispatchPlatformRequestField::VmEntryCalleeSaveRegisterCount,
            expected.vm_entry_record_callee_save_register_count,
            actual.vm_entry_record_callee_save_register_count,
        ),
        (
            P6Arm64PublicJscStackDispatchPlatformRequestField::VmEntryCalleeSaveBufferBytes,
            expected.vm_entry_record_callee_save_buffer_bytes,
            actual.vm_entry_record_callee_save_buffer_bytes,
        ),
    ] {
        if expected != actual {
            return Err(
                P6Arm64PublicJscStackDispatchPreconditionsLinkageMismatch::PlatformRequestMismatch {
                    field,
                    expected,
                    actual,
                },
            );
        }
    }
    Ok(())
}
