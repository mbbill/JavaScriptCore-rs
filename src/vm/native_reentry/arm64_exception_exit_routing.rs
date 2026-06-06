//! ARM64 public JSC stack-dispatch exception-exit routing proof.
//!
//! C++ JSC map: `genericUnwind` writes `VM::callFrameForCatch`,
//! `VM::targetMachinePCForThrow`, optional dispatch-and-catch target,
//! interpreter catch PC, metadata PC, and try depth. The LLInt/JIT throw
//! trampoline jumps through the staged target. Caught `op_catch` consumes and
//! clears `callFrameForCatch`, restores callee saves from the VM-entry record,
//! rebuilds `sp` from the CodeBlock frame extent, retrieves and clears the
//! exception, stores catch operands, profiles, and dispatches. The uncaught
//! handler clears `callFrameForCatch`, restores callee saves and the previous
//! VM top-frame pair from `VMEntryRecord`, returns `undefined`, and leaves the
//! VM-entry frame. Rust keeps this module descriptor-only: it proves the
//! routing shape and deliberately rejects ARM64E gate claims and real platform
//! implementation claims until the platform trampoline is JSC-shaped.

use super::super::entry::FrameAddress;
use super::arm64_exception_unwind::{
    P6Arm64VmEntryCalleeSaveBufferRestorationRecord, P6Arm64VmEntryExceptionHandlerTarget,
};
use super::arm64_public_dispatch::{
    validate_p6_arm64_verified_public_jsc_stack_dispatch_preconditions_proof,
    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof,
    P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofMismatch,
};
use super::rooting::P6Arm64BranchAwareCallableTopCallFramePublicationProof;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProof<
    'publication,
> {
    public_jsc_stack_dispatch_preconditions_proof:
        P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof<'publication>,
    routing_capabilities: P6Arm64PublicJscStackDispatchExceptionExitRoutingCapabilityRecord,
    caught_route: P6Arm64PublicJscStackDispatchCaughtExceptionRouteRecord,
    uncaught_route: P6Arm64PublicJscStackDispatchUncaughtExceptionRouteRecord,
}

impl<'publication> P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProof<'publication> {
    #[allow(dead_code)]
    pub(in crate::vm) fn from_exception_exit_routing_records(
        public_jsc_stack_dispatch_preconditions_proof:
            P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof<'publication>,
        routing_capabilities: P6Arm64PublicJscStackDispatchExceptionExitRoutingCapabilityRecord,
        caught_route: P6Arm64PublicJscStackDispatchCaughtExceptionRouteRecord,
        uncaught_route: P6Arm64PublicJscStackDispatchUncaughtExceptionRouteRecord,
    ) -> Result<Self, P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProofError> {
        validate_p6_arm64_public_jsc_stack_dispatch_exception_exit_routing_linkage(
            &public_jsc_stack_dispatch_preconditions_proof,
            &routing_capabilities,
            &caught_route,
            &uncaught_route,
        )
        .map_err(P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProofError::Linkage)?;

        Ok(Self {
            public_jsc_stack_dispatch_preconditions_proof,
            routing_capabilities,
            caught_route,
            uncaught_route,
        })
    }

    pub(in crate::vm) const fn public_jsc_stack_dispatch_preconditions_proof(
        &self,
    ) -> &P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof<'publication> {
        &self.public_jsc_stack_dispatch_preconditions_proof
    }

    pub(in crate::vm) const fn routing_capabilities(
        &self,
    ) -> &P6Arm64PublicJscStackDispatchExceptionExitRoutingCapabilityRecord {
        &self.routing_capabilities
    }

    pub(in crate::vm) const fn caught_route(
        &self,
    ) -> &P6Arm64PublicJscStackDispatchCaughtExceptionRouteRecord {
        &self.caught_route
    }

    pub(in crate::vm) const fn uncaught_route(
        &self,
    ) -> &P6Arm64PublicJscStackDispatchUncaughtExceptionRouteRecord {
        &self.uncaught_route
    }

    #[cfg(test)]
    pub(in crate::vm) fn with_caught_route_for_testing(
        mut self,
        caught_route: P6Arm64PublicJscStackDispatchCaughtExceptionRouteRecord,
    ) -> Self {
        self.caught_route = caught_route;
        self
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64PublicJscStackDispatchExceptionExitRoutingCapabilityRecord {
    pub(in crate::vm) metadata_supports_normal_return: bool,
    pub(in crate::vm) metadata_supports_caught_exception_exit: bool,
    pub(in crate::vm) metadata_supports_uncaught_exception_exit: bool,
    /// Rust has only descriptor coverage here. Claiming a real platform
    /// implementation would bypass the future JSC-shaped trampoline work.
    pub(in crate::vm) platform_implementation_available: bool,
    /// C++ ARM64E uses the exception-handler gate before jumping to the target.
    /// This batch models plain ARM64 routing metadata only and rejects gate
    /// claims until ARM64E is represented explicitly.
    pub(in crate::vm) arm64e_exception_handler_gate_claimed: bool,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64PublicJscStackDispatchCaughtExceptionRouteRecord {
    pub(in crate::vm) target_machine_pc_for_throw: P6Arm64VmEntryExceptionHandlerTarget,
    pub(in crate::vm) target_machine_pc_after_catch: Option<P6Arm64VmEntryExceptionHandlerTarget>,
    pub(in crate::vm) call_frame_for_catch: FrameAddress,
    pub(in crate::vm) call_frame_for_catch_consumed: bool,
    pub(in crate::vm) call_frame_for_catch_cleared: bool,
    pub(in crate::vm) callee_save_restore: P6Arm64VmEntryCalleeSaveBufferRestorationRecord,
    pub(in crate::vm) code_block_frame_extent_sp_restored: bool,
    pub(in crate::vm) restored_catch_sp: FrameAddress,
    pub(in crate::vm) catchable_exception_retrieved: bool,
    pub(in crate::vm) pending_exception_cleared: bool,
    pub(in crate::vm) exception_operand_store_frame: FrameAddress,
    pub(in crate::vm) thrown_value_operand_store_frame: FrameAddress,
    pub(in crate::vm) catch_profile_recorded: bool,
    pub(in crate::vm) dispatches_after_catch: bool,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64PublicJscStackDispatchUncaughtExceptionRouteRecord {
    pub(in crate::vm) target_machine_pc_for_throw: P6Arm64VmEntryExceptionHandlerTarget,
    pub(in crate::vm) call_frame_for_catch: FrameAddress,
    pub(in crate::vm) call_frame_for_catch_cleared: bool,
    pub(in crate::vm) callee_save_restore_from_vm_entry_record: bool,
    pub(in crate::vm) callee_save_restore: P6Arm64VmEntryCalleeSaveBufferRestorationRecord,
    pub(in crate::vm) top_entry_frame_loaded: FrameAddress,
    pub(in crate::vm) vm_entry_record: FrameAddress,
    pub(in crate::vm) vm_entry_record_previous_top_pair_restored: bool,
    pub(in crate::vm) restored_top_call_frame: Option<FrameAddress>,
    pub(in crate::vm) restored_top_entry_frame: Option<FrameAddress>,
    pub(in crate::vm) returned_undefined: bool,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProofError {
    Linkage(P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch),
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProofMismatch {
    Preconditions(P6Arm64VerifiedPublicJscStackDispatchPreconditionsProofMismatch),
    Linkage(P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch),
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch {
    MetadataNormalReturnCapabilityMissing,
    MetadataCaughtExceptionExitCapabilityMissing,
    MetadataUncaughtExceptionExitCapabilityMissing,
    PlatformImplementationClaimed,
    Arm64eExceptionHandlerGateClaimed,
    CaughtTargetMismatch {
        expected: P6Arm64VmEntryExceptionHandlerTarget,
        actual: P6Arm64VmEntryExceptionHandlerTarget,
    },
    CaughtDispatchAndCatchTargetMismatch {
        expected: Option<P6Arm64VmEntryExceptionHandlerTarget>,
        actual: Option<P6Arm64VmEntryExceptionHandlerTarget>,
    },
    CaughtCallFrameForCatchMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    CaughtCallFrameForCatchNotConsumed,
    CaughtCallFrameForCatchNotCleared,
    CaughtCalleeSaveBufferMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    CaughtCalleeSaveRegisterCountMismatch {
        expected: usize,
        actual: usize,
    },
    CaughtCalleeSaveBufferBytesMismatch {
        expected: usize,
        actual: usize,
    },
    CaughtStackPointerNotRestored,
    CaughtRestoredStackPointerMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    CaughtCatchableExceptionNotRetrieved,
    CaughtPendingExceptionNotCleared,
    CaughtExceptionOperandStoreFrameMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    CaughtThrownValueOperandStoreFrameMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    CaughtProfileNotRecorded,
    CaughtDispatchNotRecorded,
    UncaughtTargetMismatch {
        expected: P6Arm64VmEntryExceptionHandlerTarget,
        actual: P6Arm64VmEntryExceptionHandlerTarget,
    },
    UncaughtCallFrameForCatchMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    UncaughtCallFrameForCatchNotCleared,
    UncaughtCalleeSaveNotRestoredFromVmEntryRecord,
    UncaughtCalleeSaveBufferMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    UncaughtCalleeSaveRegisterCountMismatch {
        expected: usize,
        actual: usize,
    },
    UncaughtCalleeSaveBufferBytesMismatch {
        expected: usize,
        actual: usize,
    },
    UncaughtTopEntryFrameMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    UncaughtVmEntryRecordMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    UncaughtVmEntryPreviousTopPairNotRestored,
    UncaughtRestoredTopCallFrameMismatch {
        expected: Option<FrameAddress>,
        actual: Option<FrameAddress>,
    },
    UncaughtRestoredTopEntryFrameMismatch {
        expected: Option<FrameAddress>,
        actual: Option<FrameAddress>,
    },
    UncaughtUndefinedReturnMissing,
}

pub(in crate::vm) fn validate_p6_arm64_verified_public_jsc_stack_dispatch_exception_exit_routing_proof(
    top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
    routing_proof: &P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProof<'_>,
    expected_live_local_slots: usize,
) -> Result<(), P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProofMismatch> {
    validate_p6_arm64_verified_public_jsc_stack_dispatch_preconditions_proof(
        top_call_frame_publication,
        routing_proof.public_jsc_stack_dispatch_preconditions_proof(),
        expected_live_local_slots,
    )
    .map_err(
        P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProofMismatch::Preconditions,
    )?;
    validate_p6_arm64_public_jsc_stack_dispatch_exception_exit_routing_linkage(
        routing_proof.public_jsc_stack_dispatch_preconditions_proof(),
        routing_proof.routing_capabilities(),
        routing_proof.caught_route(),
        routing_proof.uncaught_route(),
    )
    .map_err(P6Arm64VerifiedPublicJscStackDispatchExceptionExitRoutingProofMismatch::Linkage)
}

fn validate_p6_arm64_public_jsc_stack_dispatch_exception_exit_routing_linkage(
    public_jsc_stack_dispatch_preconditions_proof:
        &P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof<'_>,
    routing_capabilities: &P6Arm64PublicJscStackDispatchExceptionExitRoutingCapabilityRecord,
    caught_route: &P6Arm64PublicJscStackDispatchCaughtExceptionRouteRecord,
    uncaught_route: &P6Arm64PublicJscStackDispatchUncaughtExceptionRouteRecord,
) -> Result<(), P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch> {
    validate_routing_capabilities(routing_capabilities)?;
    let exception_unwind =
        public_jsc_stack_dispatch_preconditions_proof.vm_entry_exception_unwind_restoration_proof();
    validate_caught_route(
        public_jsc_stack_dispatch_preconditions_proof,
        exception_unwind.caught_exception_dispatch_restore(),
        caught_route,
    )?;
    validate_uncaught_route(
        exception_unwind.uncaught_exception_entry_restore(),
        uncaught_route,
    )
}

fn validate_routing_capabilities(
    routing_capabilities: &P6Arm64PublicJscStackDispatchExceptionExitRoutingCapabilityRecord,
) -> Result<(), P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch> {
    if !routing_capabilities.metadata_supports_normal_return {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::MetadataNormalReturnCapabilityMissing,
        );
    }
    if !routing_capabilities.metadata_supports_caught_exception_exit {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::MetadataCaughtExceptionExitCapabilityMissing,
        );
    }
    if !routing_capabilities.metadata_supports_uncaught_exception_exit {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::MetadataUncaughtExceptionExitCapabilityMissing,
        );
    }
    if routing_capabilities.platform_implementation_available {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::PlatformImplementationClaimed,
        );
    }
    if routing_capabilities.arm64e_exception_handler_gate_claimed {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::Arm64eExceptionHandlerGateClaimed,
        );
    }
    Ok(())
}

fn validate_caught_route(
    public_jsc_stack_dispatch_preconditions_proof:
        &P6Arm64VerifiedPublicJscStackDispatchPreconditionsProof<'_>,
    expected: &super::arm64_exception_unwind::P6Arm64VmEntryCaughtExceptionDispatchRestorationRecord,
    actual: &P6Arm64PublicJscStackDispatchCaughtExceptionRouteRecord,
) -> Result<(), P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch> {
    if actual.target_machine_pc_for_throw != expected.staging.target_machine_pc_for_throw {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtTargetMismatch {
                expected: expected.staging.target_machine_pc_for_throw,
                actual: actual.target_machine_pc_for_throw,
            },
        );
    }
    if actual.target_machine_pc_after_catch != expected.staging.target_machine_pc_after_catch {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtDispatchAndCatchTargetMismatch {
                expected: expected.staging.target_machine_pc_after_catch,
                actual: actual.target_machine_pc_after_catch,
            },
        );
    }
    if actual.call_frame_for_catch != expected.staging.call_frame_for_catch {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtCallFrameForCatchMismatch {
                expected: expected.staging.call_frame_for_catch,
                actual: actual.call_frame_for_catch,
            },
        );
    }
    if !actual.call_frame_for_catch_consumed {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtCallFrameForCatchNotConsumed,
        );
    }
    if !actual.call_frame_for_catch_cleared {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtCallFrameForCatchNotCleared,
        );
    }
    validate_caught_callee_save_restore(expected.callee_save_restore, actual.callee_save_restore)?;
    if !actual.code_block_frame_extent_sp_restored {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtStackPointerNotRestored,
        );
    }
    let expected_sp = public_jsc_stack_dispatch_preconditions_proof
        .code_block_frame_extent()
        .restored_stack_pointer;
    if actual.restored_catch_sp != expected_sp {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtRestoredStackPointerMismatch {
                expected: expected_sp,
                actual: actual.restored_catch_sp,
            },
        );
    }
    if !actual.catchable_exception_retrieved {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtCatchableExceptionNotRetrieved,
        );
    }
    if !actual.pending_exception_cleared {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtPendingExceptionNotCleared,
        );
    }
    if actual.exception_operand_store_frame != expected.exception_operand_store_frame {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtExceptionOperandStoreFrameMismatch {
                expected: expected.exception_operand_store_frame,
                actual: actual.exception_operand_store_frame,
            },
        );
    }
    if actual.thrown_value_operand_store_frame != expected.thrown_value_operand_store_frame {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtThrownValueOperandStoreFrameMismatch {
                expected: expected.thrown_value_operand_store_frame,
                actual: actual.thrown_value_operand_store_frame,
            },
        );
    }
    if !actual.catch_profile_recorded {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtProfileNotRecorded,
        );
    }
    if !actual.dispatches_after_catch {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtDispatchNotRecorded,
        );
    }
    Ok(())
}

fn validate_caught_callee_save_restore(
    expected: P6Arm64VmEntryCalleeSaveBufferRestorationRecord,
    actual: P6Arm64VmEntryCalleeSaveBufferRestorationRecord,
) -> Result<(), P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch> {
    if actual.buffer != expected.buffer {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtCalleeSaveBufferMismatch {
                expected: expected.buffer,
                actual: actual.buffer,
            },
        );
    }
    if actual.register_count != expected.register_count {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtCalleeSaveRegisterCountMismatch {
                expected: expected.register_count,
                actual: actual.register_count,
            },
        );
    }
    if actual.buffer_bytes != expected.buffer_bytes {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::CaughtCalleeSaveBufferBytesMismatch {
                expected: expected.buffer_bytes,
                actual: actual.buffer_bytes,
            },
        );
    }
    Ok(())
}

fn validate_uncaught_route(
    expected: &super::arm64_exception_unwind::P6Arm64VmEntryUncaughtExceptionEntryRestorationRecord,
    actual: &P6Arm64PublicJscStackDispatchUncaughtExceptionRouteRecord,
) -> Result<(), P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch> {
    if actual.target_machine_pc_for_throw != expected.staging.target_machine_pc_for_throw {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::UncaughtTargetMismatch {
                expected: expected.staging.target_machine_pc_for_throw,
                actual: actual.target_machine_pc_for_throw,
            },
        );
    }
    if actual.call_frame_for_catch != expected.staging.call_frame_for_catch {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::UncaughtCallFrameForCatchMismatch {
                expected: expected.staging.call_frame_for_catch,
                actual: actual.call_frame_for_catch,
            },
        );
    }
    if !actual.call_frame_for_catch_cleared {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::UncaughtCallFrameForCatchNotCleared,
        );
    }
    if !actual.callee_save_restore_from_vm_entry_record {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::UncaughtCalleeSaveNotRestoredFromVmEntryRecord,
        );
    }
    validate_uncaught_callee_save_restore(
        expected.callee_save_restore,
        actual.callee_save_restore,
    )?;
    if actual.top_entry_frame_loaded != expected.top_entry_frame_loaded {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::UncaughtTopEntryFrameMismatch {
                expected: expected.top_entry_frame_loaded,
                actual: actual.top_entry_frame_loaded,
            },
        );
    }
    if actual.vm_entry_record != expected.vm_entry_record {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::UncaughtVmEntryRecordMismatch {
                expected: expected.vm_entry_record,
                actual: actual.vm_entry_record,
            },
        );
    }
    if !actual.vm_entry_record_previous_top_pair_restored {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::UncaughtVmEntryPreviousTopPairNotRestored,
        );
    }
    if actual.restored_top_call_frame != expected.restored_top_call_frame {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::UncaughtRestoredTopCallFrameMismatch {
                expected: expected.restored_top_call_frame,
                actual: actual.restored_top_call_frame,
            },
        );
    }
    if actual.restored_top_entry_frame != expected.restored_top_entry_frame {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::UncaughtRestoredTopEntryFrameMismatch {
                expected: expected.restored_top_entry_frame,
                actual: actual.restored_top_entry_frame,
            },
        );
    }
    if !actual.returned_undefined {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::UncaughtUndefinedReturnMissing,
        );
    }
    Ok(())
}

fn validate_uncaught_callee_save_restore(
    expected: P6Arm64VmEntryCalleeSaveBufferRestorationRecord,
    actual: P6Arm64VmEntryCalleeSaveBufferRestorationRecord,
) -> Result<(), P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch> {
    if actual.buffer != expected.buffer {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::UncaughtCalleeSaveBufferMismatch {
                expected: expected.buffer,
                actual: actual.buffer,
            },
        );
    }
    if actual.register_count != expected.register_count {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::UncaughtCalleeSaveRegisterCountMismatch {
                expected: expected.register_count,
                actual: actual.register_count,
            },
        );
    }
    if actual.buffer_bytes != expected.buffer_bytes {
        return Err(
            P6Arm64PublicJscStackDispatchExceptionExitRoutingLinkageMismatch::UncaughtCalleeSaveBufferBytesMismatch {
                expected: expected.buffer_bytes,
                actual: actual.buffer_bytes,
            },
        );
    }
    Ok(())
}
