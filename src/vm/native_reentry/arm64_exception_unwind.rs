//! ARM64 VM-entry exception/unwind restoration proof.
//!
//! C++ JSC map: `genericUnwind` stages `VM::callFrameForCatch`,
//! `VM::targetMachinePCForThrow`, optional dispatch-and-catch PC,
//! interpreter catch PC, metadata PC, and try depth. The throw trampoline jumps
//! through the staged machine handler. The caught path restores callee-saves,
//! consumes `callFrameForCatch`, rebuilds `sp` from the catch CallFrame, clears
//! and stores the catchable exception, profiles, and dispatches. The uncaught
//! path restores callee-saves, clears `callFrameForCatch`, restores the
//! previous VM top-frame pair from `VMEntryRecord`, returns `undefined`, and
//! leaves the VM-entry frame. Rust keeps this as descriptor-only proof metadata;
//! it does not install VM-wide throw fields, jump to generated code, or make
//! public ARM64 admission succeed.

use crate::bytecode::BytecodeIndex;
use crate::runtime::NativeCodeId;

use super::super::entry::FrameAddress;
use super::arm64_vm_entry_normal_return::{
    validate_p6_arm64_verified_vm_entry_normal_return_restoration_proof,
    P6Arm64VerifiedVmEntryNormalReturnRestorationProof,
    P6Arm64VerifiedVmEntryNormalReturnRestorationProofMismatch,
};
use super::rooting::P6Arm64BranchAwareCallableTopCallFramePublicationProof;

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<'publication> {
    vm_entry_normal_return_restoration_proof:
        P6Arm64VerifiedVmEntryNormalReturnRestorationProof<'publication>,
    caught_exception_dispatch_restore: P6Arm64VmEntryCaughtExceptionDispatchRestorationRecord,
    uncaught_exception_entry_restore: P6Arm64VmEntryUncaughtExceptionEntryRestorationRecord,
}

impl<'publication> P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<'publication> {
    #[allow(dead_code)]
    pub(in crate::vm) fn from_exception_unwind_restoration_records(
        vm_entry_normal_return_restoration_proof:
            P6Arm64VerifiedVmEntryNormalReturnRestorationProof<'publication>,
        caught_exception_dispatch_restore: P6Arm64VmEntryCaughtExceptionDispatchRestorationRecord,
        uncaught_exception_entry_restore: P6Arm64VmEntryUncaughtExceptionEntryRestorationRecord,
    ) -> Result<Self, P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofError> {
        validate_p6_arm64_vm_entry_exception_unwind_restoration_linkage(
            &vm_entry_normal_return_restoration_proof,
            &caught_exception_dispatch_restore,
            &uncaught_exception_entry_restore,
        )
        .map_err(P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofError::Linkage)?;

        Ok(Self {
            vm_entry_normal_return_restoration_proof,
            caught_exception_dispatch_restore,
            uncaught_exception_entry_restore,
        })
    }

    pub(in crate::vm) fn vm_entry_normal_return_restoration_proof(
        &self,
    ) -> &P6Arm64VerifiedVmEntryNormalReturnRestorationProof<'publication> {
        &self.vm_entry_normal_return_restoration_proof
    }

    pub(in crate::vm) const fn caught_exception_dispatch_restore(
        &self,
    ) -> &P6Arm64VmEntryCaughtExceptionDispatchRestorationRecord {
        &self.caught_exception_dispatch_restore
    }

    pub(in crate::vm) const fn uncaught_exception_entry_restore(
        &self,
    ) -> &P6Arm64VmEntryUncaughtExceptionEntryRestorationRecord {
        &self.uncaught_exception_entry_restore
    }

    #[cfg(test)]
    pub(in crate::vm) fn with_caught_exception_dispatch_restore_for_testing(
        mut self,
        caught_exception_dispatch_restore: P6Arm64VmEntryCaughtExceptionDispatchRestorationRecord,
    ) -> Self {
        self.caught_exception_dispatch_restore = caught_exception_dispatch_restore;
        self
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64VmEntryExceptionHandlerTarget {
    pub(in crate::vm) kind: P6Arm64VmEntryExceptionHandlerTargetKind,
    pub(in crate::vm) address: NativeCodeId,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VmEntryExceptionHandlerTargetKind {
    LlIntOpCatch,
    BaselineOpCatch,
    DispatchAndCatch,
    LlIntHandleUncaughtException,
    JitHandleUncaughtException,
}

impl P6Arm64VmEntryExceptionHandlerTargetKind {
    const fn is_caught_handler(self) -> bool {
        matches!(self, Self::LlIntOpCatch | Self::BaselineOpCatch)
    }

    const fn is_uncaught_handler(self) -> bool {
        matches!(
            self,
            Self::LlIntHandleUncaughtException | Self::JitHandleUncaughtException
        )
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64VmEntryExceptionUnwindStagingRecord {
    pub(in crate::vm) call_frame_for_catch: FrameAddress,
    pub(in crate::vm) target_machine_pc_for_throw: P6Arm64VmEntryExceptionHandlerTarget,
    pub(in crate::vm) target_machine_pc_after_catch: Option<P6Arm64VmEntryExceptionHandlerTarget>,
    pub(in crate::vm) target_interpreter_pc_for_throw: Option<BytecodeIndex>,
    pub(in crate::vm) target_interpreter_metadata_pc_for_throw: Option<usize>,
    pub(in crate::vm) target_try_depth_for_throw: u32,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64VmEntryCalleeSaveBufferRestorationRecord {
    pub(in crate::vm) buffer: FrameAddress,
    pub(in crate::vm) register_count: usize,
    pub(in crate::vm) buffer_bytes: usize,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64VmEntryCaughtExceptionDispatchRestorationRecord {
    pub(in crate::vm) staging: P6Arm64VmEntryExceptionUnwindStagingRecord,
    pub(in crate::vm) callee_save_restore: P6Arm64VmEntryCalleeSaveBufferRestorationRecord,
    pub(in crate::vm) reconstructed_catch_sp: FrameAddress,
    pub(in crate::vm) pending_exception_cleared: bool,
    pub(in crate::vm) catchable_exception_retrieved: bool,
    pub(in crate::vm) exception_operand_store_frame: FrameAddress,
    pub(in crate::vm) thrown_value_operand_store_frame: FrameAddress,
    pub(in crate::vm) catch_profile_recorded: bool,
    pub(in crate::vm) dispatches_after_catch: bool,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64VmEntryUncaughtExceptionEntryRestorationRecord {
    pub(in crate::vm) staging: P6Arm64VmEntryExceptionUnwindStagingRecord,
    pub(in crate::vm) callee_save_restore: P6Arm64VmEntryCalleeSaveBufferRestorationRecord,
    pub(in crate::vm) top_entry_frame_loaded: FrameAddress,
    pub(in crate::vm) vm_entry_record: FrameAddress,
    pub(in crate::vm) restored_top_call_frame: Option<FrameAddress>,
    pub(in crate::vm) restored_top_entry_frame: Option<FrameAddress>,
    pub(in crate::vm) call_frame_for_catch_cleared: bool,
    pub(in crate::vm) returned_undefined: bool,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofError {
    Linkage(P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch),
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofMismatch {
    NormalReturn(P6Arm64VerifiedVmEntryNormalReturnRestorationProofMismatch),
    Linkage(P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch),
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch {
    CaughtCallFrameForCatchMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    CaughtTargetAddressZero,
    CaughtTargetKindMismatch {
        actual: P6Arm64VmEntryExceptionHandlerTargetKind,
    },
    CaughtDispatchAndCatchTargetAddressZero,
    CaughtDispatchAndCatchTargetKindMismatch {
        actual: P6Arm64VmEntryExceptionHandlerTargetKind,
    },
    CaughtMissingInterpreterPc,
    CaughtMissingMetadataPc,
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
    CaughtReconstructedStackPointerMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    CaughtPendingExceptionNotCleared,
    CaughtCatchableExceptionNotRetrieved,
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
    UncaughtCallFrameForCatchMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    UncaughtTargetAddressZero,
    UncaughtTargetKindMismatch {
        actual: P6Arm64VmEntryExceptionHandlerTargetKind,
    },
    UncaughtUnexpectedDispatchAndCatchTarget,
    UncaughtUnexpectedInterpreterPc {
        actual: BytecodeIndex,
    },
    UncaughtUnexpectedMetadataPc {
        actual: usize,
    },
    UncaughtTryDepthMismatch {
        expected: u32,
        actual: u32,
    },
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
    UncaughtRestoredTopCallFrameMismatch {
        expected: Option<FrameAddress>,
        actual: Option<FrameAddress>,
    },
    UncaughtRestoredTopEntryFrameMismatch {
        expected: Option<FrameAddress>,
        actual: Option<FrameAddress>,
    },
    UncaughtCallFrameForCatchNotCleared,
    UncaughtUndefinedReturnMissing,
}

pub(in crate::vm) fn validate_p6_arm64_verified_vm_entry_exception_unwind_restoration_proof(
    top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
    exception_unwind_restoration_proof: &P6Arm64VerifiedVmEntryExceptionUnwindRestorationProof<'_>,
    expected_live_local_slots: usize,
) -> Result<(), P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofMismatch> {
    validate_p6_arm64_verified_vm_entry_normal_return_restoration_proof(
        top_call_frame_publication,
        exception_unwind_restoration_proof.vm_entry_normal_return_restoration_proof(),
        expected_live_local_slots,
    )
    .map_err(P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofMismatch::NormalReturn)?;
    validate_p6_arm64_vm_entry_exception_unwind_restoration_linkage(
        exception_unwind_restoration_proof.vm_entry_normal_return_restoration_proof(),
        exception_unwind_restoration_proof.caught_exception_dispatch_restore(),
        exception_unwind_restoration_proof.uncaught_exception_entry_restore(),
    )
    .map_err(P6Arm64VerifiedVmEntryExceptionUnwindRestorationProofMismatch::Linkage)
}

fn validate_p6_arm64_vm_entry_exception_unwind_restoration_linkage(
    normal_return_restoration_proof: &P6Arm64VerifiedVmEntryNormalReturnRestorationProof<'_>,
    caught_exception_dispatch_restore: &P6Arm64VmEntryCaughtExceptionDispatchRestorationRecord,
    uncaught_exception_entry_restore: &P6Arm64VmEntryUncaughtExceptionEntryRestorationRecord,
) -> Result<(), P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch> {
    let dispatch = normal_return_restoration_proof
        .jsc_stack_dispatch_request_proof()
        .jsc_stack_dispatch_request_proof();
    let materialization = normal_return_restoration_proof
        .jsc_stack_dispatch_request_proof()
        .generated_native_frame_materialization_proof()
        .materialization_descriptor();
    let normal_return_exit = normal_return_restoration_proof.normal_return_exit_record();

    validate_caught_exception_dispatch_restore(
        dispatch.call_frame,
        dispatch.vm_entry_record_callee_save_buffer,
        dispatch.vm_entry_record_callee_save_register_count,
        dispatch.vm_entry_record_callee_save_buffer_bytes,
        FrameAddress(materialization.post_frame_allocation.post_allocation_sp),
        caught_exception_dispatch_restore,
    )?;
    validate_uncaught_exception_entry_restore(
        dispatch.call_frame,
        dispatch.vm_entry_record_callee_save_buffer,
        dispatch.vm_entry_record_callee_save_register_count,
        dispatch.vm_entry_record_callee_save_buffer_bytes,
        normal_return_exit.closed_publication.top_entry_frame,
        normal_return_exit.closed_publication.vm_entry_record,
        normal_return_exit.restored_top_call_frame,
        normal_return_exit.restored_top_entry_frame,
        uncaught_exception_entry_restore,
    )
}

fn validate_caught_exception_dispatch_restore(
    expected_call_frame: FrameAddress,
    expected_callee_save_buffer: FrameAddress,
    expected_callee_save_register_count: usize,
    expected_callee_save_buffer_bytes: usize,
    expected_reconstructed_sp: FrameAddress,
    restore: &P6Arm64VmEntryCaughtExceptionDispatchRestorationRecord,
) -> Result<(), P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch> {
    if restore.staging.call_frame_for_catch != expected_call_frame {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtCallFrameForCatchMismatch {
                expected: expected_call_frame,
                actual: restore.staging.call_frame_for_catch,
            },
        );
    }
    if restore.staging.target_machine_pc_for_throw.address == NativeCodeId::default() {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtTargetAddressZero,
        );
    }
    if !restore
        .staging
        .target_machine_pc_for_throw
        .kind
        .is_caught_handler()
    {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtTargetKindMismatch {
                actual: restore.staging.target_machine_pc_for_throw.kind,
            },
        );
    }
    if let Some(dispatch_and_catch) = restore.staging.target_machine_pc_after_catch {
        if dispatch_and_catch.address == NativeCodeId::default() {
            return Err(
                P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtDispatchAndCatchTargetAddressZero,
            );
        }
        if dispatch_and_catch.kind != P6Arm64VmEntryExceptionHandlerTargetKind::DispatchAndCatch {
            return Err(
                P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtDispatchAndCatchTargetKindMismatch {
                    actual: dispatch_and_catch.kind,
                },
            );
        }
    }
    if restore.staging.target_interpreter_pc_for_throw.is_none() {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtMissingInterpreterPc,
        );
    }
    if restore
        .staging
        .target_interpreter_metadata_pc_for_throw
        .is_none()
    {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtMissingMetadataPc,
        );
    }
    validate_caught_callee_save_restore(
        restore.callee_save_restore,
        expected_callee_save_buffer,
        expected_callee_save_register_count,
        expected_callee_save_buffer_bytes,
    )?;
    if restore.reconstructed_catch_sp != expected_reconstructed_sp {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtReconstructedStackPointerMismatch {
                expected: expected_reconstructed_sp,
                actual: restore.reconstructed_catch_sp,
            },
        );
    }
    if !restore.pending_exception_cleared {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtPendingExceptionNotCleared,
        );
    }
    if !restore.catchable_exception_retrieved {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtCatchableExceptionNotRetrieved,
        );
    }
    if restore.exception_operand_store_frame != expected_call_frame {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtExceptionOperandStoreFrameMismatch {
                expected: expected_call_frame,
                actual: restore.exception_operand_store_frame,
            },
        );
    }
    if restore.thrown_value_operand_store_frame != expected_call_frame {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtThrownValueOperandStoreFrameMismatch {
                expected: expected_call_frame,
                actual: restore.thrown_value_operand_store_frame,
            },
        );
    }
    if !restore.catch_profile_recorded {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtProfileNotRecorded,
        );
    }
    if !restore.dispatches_after_catch {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtDispatchNotRecorded,
        );
    }
    Ok(())
}

fn validate_caught_callee_save_restore(
    actual: P6Arm64VmEntryCalleeSaveBufferRestorationRecord,
    expected_buffer: FrameAddress,
    expected_register_count: usize,
    expected_buffer_bytes: usize,
) -> Result<(), P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch> {
    if actual.buffer != expected_buffer {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtCalleeSaveBufferMismatch {
                expected: expected_buffer,
                actual: actual.buffer,
            },
        );
    }
    if actual.register_count != expected_register_count {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtCalleeSaveRegisterCountMismatch {
                expected: expected_register_count,
                actual: actual.register_count,
            },
        );
    }
    if actual.buffer_bytes != expected_buffer_bytes {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::CaughtCalleeSaveBufferBytesMismatch {
                expected: expected_buffer_bytes,
                actual: actual.buffer_bytes,
            },
        );
    }
    Ok(())
}

fn validate_uncaught_exception_entry_restore(
    expected_call_frame: FrameAddress,
    expected_callee_save_buffer: FrameAddress,
    expected_callee_save_register_count: usize,
    expected_callee_save_buffer_bytes: usize,
    expected_top_entry_frame: FrameAddress,
    expected_vm_entry_record: FrameAddress,
    expected_restored_top_call_frame: Option<FrameAddress>,
    expected_restored_top_entry_frame: Option<FrameAddress>,
    restore: &P6Arm64VmEntryUncaughtExceptionEntryRestorationRecord,
) -> Result<(), P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch> {
    if restore.staging.call_frame_for_catch != expected_call_frame {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtCallFrameForCatchMismatch {
                expected: expected_call_frame,
                actual: restore.staging.call_frame_for_catch,
            },
        );
    }
    if restore.staging.target_machine_pc_for_throw.address == NativeCodeId::default() {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtTargetAddressZero,
        );
    }
    if !restore
        .staging
        .target_machine_pc_for_throw
        .kind
        .is_uncaught_handler()
    {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtTargetKindMismatch {
                actual: restore.staging.target_machine_pc_for_throw.kind,
            },
        );
    }
    if restore.staging.target_machine_pc_after_catch.is_some() {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtUnexpectedDispatchAndCatchTarget,
        );
    }
    if let Some(actual) = restore.staging.target_interpreter_pc_for_throw {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtUnexpectedInterpreterPc {
                actual,
            },
        );
    }
    if let Some(actual) = restore.staging.target_interpreter_metadata_pc_for_throw {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtUnexpectedMetadataPc {
                actual,
            },
        );
    }
    if restore.staging.target_try_depth_for_throw != 0 {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtTryDepthMismatch {
                expected: 0,
                actual: restore.staging.target_try_depth_for_throw,
            },
        );
    }
    validate_uncaught_callee_save_restore(
        restore.callee_save_restore,
        expected_callee_save_buffer,
        expected_callee_save_register_count,
        expected_callee_save_buffer_bytes,
    )?;
    if restore.top_entry_frame_loaded != expected_top_entry_frame {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtTopEntryFrameMismatch {
                expected: expected_top_entry_frame,
                actual: restore.top_entry_frame_loaded,
            },
        );
    }
    if restore.vm_entry_record != expected_vm_entry_record {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtVmEntryRecordMismatch {
                expected: expected_vm_entry_record,
                actual: restore.vm_entry_record,
            },
        );
    }
    if restore.restored_top_call_frame != expected_restored_top_call_frame {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtRestoredTopCallFrameMismatch {
                expected: expected_restored_top_call_frame,
                actual: restore.restored_top_call_frame,
            },
        );
    }
    if restore.restored_top_entry_frame != expected_restored_top_entry_frame {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtRestoredTopEntryFrameMismatch {
                expected: expected_restored_top_entry_frame,
                actual: restore.restored_top_entry_frame,
            },
        );
    }
    if !restore.call_frame_for_catch_cleared {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtCallFrameForCatchNotCleared,
        );
    }
    if !restore.returned_undefined {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtUndefinedReturnMissing,
        );
    }
    Ok(())
}

fn validate_uncaught_callee_save_restore(
    actual: P6Arm64VmEntryCalleeSaveBufferRestorationRecord,
    expected_buffer: FrameAddress,
    expected_register_count: usize,
    expected_buffer_bytes: usize,
) -> Result<(), P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch> {
    if actual.buffer != expected_buffer {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtCalleeSaveBufferMismatch {
                expected: expected_buffer,
                actual: actual.buffer,
            },
        );
    }
    if actual.register_count != expected_register_count {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtCalleeSaveRegisterCountMismatch {
                expected: expected_register_count,
                actual: actual.register_count,
            },
        );
    }
    if actual.buffer_bytes != expected_buffer_bytes {
        return Err(
            P6Arm64VmEntryExceptionUnwindRestorationLinkageMismatch::UncaughtCalleeSaveBufferBytesMismatch {
                expected: expected_buffer_bytes,
                actual: actual.buffer_bytes,
            },
        );
    }
    Ok(())
}
