//! ARM64 public JSC stack-dispatch platform implementation proof.
//!
//! C++ source of truth:
//! - `LowLevelInterpreter64.asm` `doVMEntry(makeJavaScriptCall)` builds the
//!   VMEntryRecord-backed frame, publishes/restores the VM top-frame pair, and
//!   calls generated code with the JSC ARM64 stack shape.
//! - `_llint_throw_from_slow_path_trampoline`, `llint_op_catch`, and
//!   `llint_handle_uncaught_exception` use machine jumps through
//!   `VM::targetMachinePCForThrow` and VM throw fields; they are not Rust return
//!   variants.
//! - `VMEntryRecord.h` defines the previous-top pair plus callee-save buffer
//!   storage consumed by those paths.
//!
//! Rust currently has only a private normal-return trampoline that sets the
//! generated-code `sp`/`fp` and restores Rust's C ABI state. That platform
//! descriptor is intentionally rejected here until the platform owns the full
//! C++ `doVMEntry(makeJavaScriptCall)` and exception-exit protocol.

use crate::platform::executable_memory_compartment::{
    ExecutableMemoryArm64JscStackCallRequest,
    ExecutableMemoryArm64JscStackDispatchArm64eGatePolicy,
    ExecutableMemoryArm64JscStackDispatchImplementationDescriptor,
    ExecutableMemoryArm64JscStackDispatchImplementationKind,
};

use super::arm64_exception_exit_routing::{
    validate_p6_arm64_verified_public_jsc_stack_dispatch_exception_exit_routing_proof,
    P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProof,
    P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProofMismatch,
};
use super::arm64_public_dispatch::P6Arm64PublicJscStackDispatchPlatformRequestField;
use super::rooting::P6Arm64BranchAwareCallableTopCallFramePublicationProof;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProof<
    'publication,
> {
    public_jsc_stack_dispatch_exception_exit_routing_proof:
        P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProof<'publication>,
    platform_descriptor: ExecutableMemoryArm64JscStackDispatchImplementationDescriptor,
}

#[allow(dead_code)]
impl<'publication> P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProof<'publication> {
    pub(in crate::vm) fn from_platform_implementation_descriptor(
        public_jsc_stack_dispatch_exception_exit_routing_proof:
            P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProof<'publication>,
        platform_descriptor: ExecutableMemoryArm64JscStackDispatchImplementationDescriptor,
    ) -> Result<Self, P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProofError> {
        validate_p6_arm64_public_jsc_stack_dispatch_platform_implementation_linkage(
            &public_jsc_stack_dispatch_exception_exit_routing_proof,
            platform_descriptor,
        )
        .map_err(P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProofError::Linkage)?;
        Ok(Self {
            public_jsc_stack_dispatch_exception_exit_routing_proof,
            platform_descriptor,
        })
    }

    pub(in crate::vm) const fn public_jsc_stack_dispatch_exception_exit_routing_proof(
        &self,
    ) -> &P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProof<'publication> {
        &self.public_jsc_stack_dispatch_exception_exit_routing_proof
    }

    pub(in crate::vm) const fn platform_descriptor(
        &self,
    ) -> ExecutableMemoryArm64JscStackDispatchImplementationDescriptor {
        self.platform_descriptor
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProofError {
    Linkage(P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch),
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProofMismatch {
    ExceptionExitRouting(P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProofMismatch),
    Linkage(P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch),
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch {
    PlatformRequestMismatch {
        field: P6Arm64PublicJscStackDispatchPlatformRequestField,
        expected: usize,
        actual: usize,
    },
    ImplementationKindUnsupported {
        actual: ExecutableMemoryArm64JscStackDispatchImplementationKind,
    },
    NormalReturnUnsupported,
    CaughtExceptionExitUnsupported,
    UncaughtExceptionExitUnsupported,
    VmEntryRecordFrameNotConstructed,
    VmTopFramePairNotPublished,
    VmTopFramePairNormalReturnNotRestored,
    VmEntryCalleeSavesNotCopiedForException,
    VmEntryCalleeSavesNotRestoredForException,
    CaughtExceptionExitDoesNotUseTargetMachinePcForThrow,
    UncaughtExceptionExitDoesNotUseTargetMachinePcForThrow,
    Arm64eGateClaimed {
        policy: ExecutableMemoryArm64JscStackDispatchArm64eGatePolicy,
    },
}

#[allow(dead_code)]
pub(in crate::vm) fn validate_p6_arm64_verified_public_jsc_stack_dispatch_platform_implementation_proof(
    top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
    platform_implementation_proof:
        &P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProof<'_>,
    expected_live_local_slots: usize,
) -> Result<(), P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProofMismatch> {
    validate_p6_arm64_public_jsc_stack_dispatch_platform_implementation_descriptor(
        top_call_frame_publication,
        platform_implementation_proof.public_jsc_stack_dispatch_exception_exit_routing_proof(),
        platform_implementation_proof.platform_descriptor(),
        expected_live_local_slots,
    )
}

pub(in crate::vm) fn validate_p6_arm64_public_jsc_stack_dispatch_platform_implementation_descriptor(
    top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
    public_jsc_stack_dispatch_exception_exit_routing_proof:
        &P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProof<'_>,
    platform_descriptor: ExecutableMemoryArm64JscStackDispatchImplementationDescriptor,
    expected_live_local_slots: usize,
) -> Result<(), P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProofMismatch> {
    validate_p6_arm64_verified_public_jsc_stack_dispatch_exception_exit_routing_proof(
        top_call_frame_publication,
        public_jsc_stack_dispatch_exception_exit_routing_proof,
        expected_live_local_slots,
    )
    .map_err(
        P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProofMismatch::ExceptionExitRouting,
    )?;
    validate_p6_arm64_public_jsc_stack_dispatch_platform_implementation_linkage(
        public_jsc_stack_dispatch_exception_exit_routing_proof,
        platform_descriptor,
    )
    .map_err(P6Arm64VerifiedPublicJscStackDispatchPlatformImplementationProofMismatch::Linkage)
}

fn validate_p6_arm64_public_jsc_stack_dispatch_platform_implementation_linkage(
    public_jsc_stack_dispatch_exception_exit_routing_proof:
        &P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProof<'_>,
    platform_descriptor: ExecutableMemoryArm64JscStackDispatchImplementationDescriptor,
) -> Result<(), P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch> {
    let expected_request = public_jsc_stack_dispatch_exception_exit_routing_proof
        .public_jsc_stack_dispatch_preconditions_proof()
        .platform_envelope()
        .platform_request;
    validate_platform_request_identity(expected_request, platform_descriptor.platform_request())?;

    if platform_descriptor.implementation_kind()
        != ExecutableMemoryArm64JscStackDispatchImplementationKind::FullDoVmEntryMakeJavaScriptCall
    {
        return Err(
            P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::ImplementationKindUnsupported {
                actual: platform_descriptor.implementation_kind(),
            },
        );
    }
    if !platform_descriptor.supports_normal_return() {
        return Err(
            P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::NormalReturnUnsupported,
        );
    }
    if !platform_descriptor.supports_caught_exception_exit() {
        return Err(
            P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::CaughtExceptionExitUnsupported,
        );
    }
    if !platform_descriptor.supports_uncaught_exception_exit() {
        return Err(
            P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::UncaughtExceptionExitUnsupported,
        );
    }
    if !platform_descriptor.constructs_vm_entry_record_frame() {
        return Err(
            P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::VmEntryRecordFrameNotConstructed,
        );
    }
    if !platform_descriptor.publishes_vm_top_frame_pair() {
        return Err(
            P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::VmTopFramePairNotPublished,
        );
    }
    if !platform_descriptor.restores_vm_top_frame_pair_on_normal_return() {
        return Err(
            P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::VmTopFramePairNormalReturnNotRestored,
        );
    }
    if !platform_descriptor.copies_callee_saves_to_vm_entry_record_on_exception() {
        return Err(
            P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::VmEntryCalleeSavesNotCopiedForException,
        );
    }
    if !platform_descriptor.restores_callee_saves_from_vm_entry_record_on_exception() {
        return Err(
            P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::VmEntryCalleeSavesNotRestoredForException,
        );
    }
    if !platform_descriptor.routes_caught_exception_via_target_machine_pc_for_throw() {
        return Err(
            P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::CaughtExceptionExitDoesNotUseTargetMachinePcForThrow,
        );
    }
    if !platform_descriptor.routes_uncaught_exception_via_target_machine_pc_for_throw() {
        return Err(
            P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::UncaughtExceptionExitDoesNotUseTargetMachinePcForThrow,
        );
    }
    if platform_descriptor.arm64e_gate_policy()
        == ExecutableMemoryArm64JscStackDispatchArm64eGatePolicy::Arm64eGateModeled
    {
        return Err(
            P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::Arm64eGateClaimed {
                policy: platform_descriptor.arm64e_gate_policy(),
            },
        );
    }
    Ok(())
}

fn validate_platform_request_identity(
    expected: ExecutableMemoryArm64JscStackCallRequest,
    actual: ExecutableMemoryArm64JscStackCallRequest,
) -> Result<(), P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch> {
    validate_platform_request_usize(
        P6Arm64PublicJscStackDispatchPlatformRequestField::EntryOffset,
        expected.entry_offset as usize,
        actual.entry_offset as usize,
    )?;
    validate_platform_request_usize(
        P6Arm64PublicJscStackDispatchPlatformRequestField::EntryStackPointer,
        pointer_value(expected.entry_sp),
        pointer_value(actual.entry_sp),
    )?;
    validate_platform_request_usize(
        P6Arm64PublicJscStackDispatchPlatformRequestField::CallFrame,
        pointer_value(expected.call_frame),
        pointer_value(actual.call_frame),
    )?;
    validate_platform_request_usize(
        P6Arm64PublicJscStackDispatchPlatformRequestField::EntryFrame,
        pointer_value(expected.entry_frame),
        pointer_value(actual.entry_frame),
    )?;
    validate_platform_request_usize(
        P6Arm64PublicJscStackDispatchPlatformRequestField::VmEntryCalleeSaveBuffer,
        pointer_value(expected.vm_entry_record_callee_save_buffer),
        pointer_value(actual.vm_entry_record_callee_save_buffer),
    )?;
    validate_platform_request_usize(
        P6Arm64PublicJscStackDispatchPlatformRequestField::VmEntryCalleeSaveRegisterCount,
        expected.vm_entry_record_callee_save_register_count,
        actual.vm_entry_record_callee_save_register_count,
    )?;
    validate_platform_request_usize(
        P6Arm64PublicJscStackDispatchPlatformRequestField::VmEntryCalleeSaveBufferBytes,
        expected.vm_entry_record_callee_save_buffer_bytes,
        actual.vm_entry_record_callee_save_buffer_bytes,
    )
}

fn validate_platform_request_usize(
    field: P6Arm64PublicJscStackDispatchPlatformRequestField,
    expected: usize,
    actual: usize,
) -> Result<(), P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch> {
    if expected != actual {
        return Err(
            P6Arm64PublicJscStackDispatchPlatformImplementationLinkageMismatch::PlatformRequestMismatch {
                field,
                expected,
                actual,
            },
        );
    }
    Ok(())
}

fn pointer_value(pointer: core::ptr::NonNull<core::ffi::c_void>) -> usize {
    pointer.as_ptr() as usize
}
