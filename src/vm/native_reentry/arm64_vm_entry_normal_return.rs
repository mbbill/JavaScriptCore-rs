//! ARM64 VM-entry normal-return restoration proof.
//!
//! C++ JSC map: `LowLevelInterpreter64.asm` `doVMEntry` reloads the
//! `VMEntryRecord` from `cfr`, restores `VM::topCallFrame` /
//! `VM::topEntryFrame` from `VMEntryRecord::m_prevTopCallFrame` /
//! `m_prevTopEntryFrame`, resets `sp` from the entry frame, pops callee-saves,
//! and returns. `VMEntryRecord.h` defines the previous-top pair and callee-save
//! buffer. Rust keeps this as a proof-only extraction module so `rooting.rs`
//! stays below the oversized-file guardrail; this does not model exception,
//! catch, or uncaught unwind restoration and does not make ARM64 public
//! admission succeed.

use super::super::entry::{
    EntryKind, FrameAddress, VmEntryRootScope, VmStackEntryPublicationExitRecord,
};
use super::rooting::{
    validate_p6_arm64_verified_jsc_stack_dispatch_request_proof,
    P6Arm64BranchAwareCallableTopCallFramePublicationProof,
    P6Arm64VerifiedJscStackDispatchRequestProof,
    P6Arm64VerifiedJscStackDispatchRequestProofMismatch,
};

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64VerifiedVmEntryNormalReturnRestorationProof<'publication> {
    jsc_stack_dispatch_request_proof: P6Arm64VerifiedJscStackDispatchRequestProof<'publication>,
    normal_return_exit_record: VmStackEntryPublicationExitRecord,
}

impl<'publication> P6Arm64VerifiedVmEntryNormalReturnRestorationProof<'publication> {
    #[allow(dead_code)]
    pub(in crate::vm) fn from_vm_entry_normal_return_exit_record(
        top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<
            'publication,
        >,
        jsc_stack_dispatch_request_proof: P6Arm64VerifiedJscStackDispatchRequestProof<'publication>,
        normal_return_exit_record: VmStackEntryPublicationExitRecord,
    ) -> Result<Self, P6Arm64VerifiedVmEntryNormalReturnRestorationProofError> {
        validate_p6_arm64_vm_entry_normal_return_restoration_linkage(
            top_call_frame_publication,
            &jsc_stack_dispatch_request_proof,
            &normal_return_exit_record,
        )
        .map_err(P6Arm64VerifiedVmEntryNormalReturnRestorationProofError::Linkage)?;

        Ok(Self {
            jsc_stack_dispatch_request_proof,
            normal_return_exit_record,
        })
    }

    pub(in crate::vm) fn jsc_stack_dispatch_request_proof(
        &self,
    ) -> &P6Arm64VerifiedJscStackDispatchRequestProof<'publication> {
        &self.jsc_stack_dispatch_request_proof
    }

    pub(in crate::vm) const fn normal_return_exit_record(
        &self,
    ) -> &VmStackEntryPublicationExitRecord {
        &self.normal_return_exit_record
    }

    #[cfg(test)]
    pub(in crate::vm) fn with_normal_return_exit_record_for_testing(
        mut self,
        normal_return_exit_record: VmStackEntryPublicationExitRecord,
    ) -> Self {
        self.normal_return_exit_record = normal_return_exit_record;
        self
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VerifiedVmEntryNormalReturnRestorationProofError {
    Linkage(P6Arm64VmEntryNormalReturnRestorationLinkageMismatch),
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VerifiedVmEntryNormalReturnRestorationProofMismatch {
    Dispatch(P6Arm64VerifiedJscStackDispatchRequestProofMismatch),
    Linkage(P6Arm64VmEntryNormalReturnRestorationLinkageMismatch),
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VmEntryNormalReturnRestorationLinkageMismatch {
    DispatchCallFrameMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    DispatchEntryFrameMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    ClosedTopCallFrameMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    ClosedTopEntryFrameMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    ClosedVmEntryRecordMismatch {
        expected: FrameAddress,
        actual: FrameAddress,
    },
    ClosedPreviousTopCallFrameMismatch {
        expected: Option<FrameAddress>,
        actual: Option<FrameAddress>,
    },
    ClosedPreviousTopEntryFrameMismatch {
        expected: Option<FrameAddress>,
        actual: Option<FrameAddress>,
    },
    ExitOrdinalMismatch {
        expected: u64,
        actual: u64,
    },
    ExitDepthMismatch {
        expected: usize,
        actual: usize,
    },
    ExitRootScopeMismatch {
        expected: VmEntryRootScope,
        actual: VmEntryRootScope,
    },
    ClosedRootScopeOrdinalMismatch {
        expected: u64,
        actual: u64,
    },
    ClosedRootScopeKindMismatch {
        expected: EntryKind,
        actual: EntryKind,
    },
    RestoredTopCallFrameMismatch {
        expected: Option<FrameAddress>,
        actual: Option<FrameAddress>,
    },
    RestoredTopEntryFrameMismatch {
        expected: Option<FrameAddress>,
        actual: Option<FrameAddress>,
    },
}

pub(in crate::vm) fn validate_p6_arm64_verified_vm_entry_normal_return_restoration_proof(
    top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
    normal_return_restoration_proof: &P6Arm64VerifiedVmEntryNormalReturnRestorationProof<'_>,
    expected_live_local_slots: usize,
) -> Result<(), P6Arm64VerifiedVmEntryNormalReturnRestorationProofMismatch> {
    validate_p6_arm64_verified_jsc_stack_dispatch_request_proof(
        top_call_frame_publication,
        normal_return_restoration_proof.jsc_stack_dispatch_request_proof(),
        expected_live_local_slots,
    )
    .map_err(P6Arm64VerifiedVmEntryNormalReturnRestorationProofMismatch::Dispatch)?;
    validate_p6_arm64_vm_entry_normal_return_restoration_linkage(
        top_call_frame_publication,
        normal_return_restoration_proof.jsc_stack_dispatch_request_proof(),
        normal_return_restoration_proof.normal_return_exit_record(),
    )
    .map_err(P6Arm64VerifiedVmEntryNormalReturnRestorationProofMismatch::Linkage)
}

fn validate_p6_arm64_vm_entry_normal_return_restoration_linkage(
    top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
    jsc_stack_dispatch_request_proof: &P6Arm64VerifiedJscStackDispatchRequestProof<'_>,
    normal_return_exit_record: &VmStackEntryPublicationExitRecord,
) -> Result<(), P6Arm64VmEntryNormalReturnRestorationLinkageMismatch> {
    let publication = top_call_frame_publication.publication;
    let dispatch = jsc_stack_dispatch_request_proof.jsc_stack_dispatch_request_proof();
    if dispatch.call_frame != publication.published_top_frame {
        return Err(
            P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::DispatchCallFrameMismatch {
                expected: publication.published_top_frame,
                actual: dispatch.call_frame,
            },
        );
    }
    if dispatch.entry_frame != publication.current_entry_frame {
        return Err(
            P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::DispatchEntryFrameMismatch {
                expected: publication.current_entry_frame,
                actual: dispatch.entry_frame,
            },
        );
    }

    let closed_publication = normal_return_exit_record.closed_publication;
    if closed_publication.top_call_frame != publication.published_top_frame {
        return Err(
            P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::ClosedTopCallFrameMismatch {
                expected: publication.published_top_frame,
                actual: closed_publication.top_call_frame,
            },
        );
    }
    if closed_publication.top_entry_frame != publication.current_entry_frame {
        return Err(
            P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::ClosedTopEntryFrameMismatch {
                expected: publication.current_entry_frame,
                actual: closed_publication.top_entry_frame,
            },
        );
    }
    if closed_publication.vm_entry_record != publication.vm_entry_record {
        return Err(
            P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::ClosedVmEntryRecordMismatch {
                expected: publication.vm_entry_record,
                actual: closed_publication.vm_entry_record,
            },
        );
    }
    if closed_publication.previous_top_call_frame != publication.vm_entry_previous_top_call_frame {
        return Err(
            P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::ClosedPreviousTopCallFrameMismatch {
                expected: publication.vm_entry_previous_top_call_frame,
                actual: closed_publication.previous_top_call_frame,
            },
        );
    }
    if closed_publication.previous_top_entry_frame != publication.vm_entry_previous_top_entry_frame
    {
        return Err(
            P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::ClosedPreviousTopEntryFrameMismatch {
                expected: publication.vm_entry_previous_top_entry_frame,
                actual: closed_publication.previous_top_entry_frame,
            },
        );
    }

    if normal_return_exit_record.ordinal != closed_publication.ordinal {
        return Err(
            P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::ExitOrdinalMismatch {
                expected: closed_publication.ordinal,
                actual: normal_return_exit_record.ordinal,
            },
        );
    }
    if normal_return_exit_record.depth_before_exit != closed_publication.depth {
        return Err(
            P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::ExitDepthMismatch {
                expected: closed_publication.depth,
                actual: normal_return_exit_record.depth_before_exit,
            },
        );
    }
    if normal_return_exit_record.closed_root_scope != closed_publication.root_scope {
        return Err(
            P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::ExitRootScopeMismatch {
                expected: closed_publication.root_scope,
                actual: normal_return_exit_record.closed_root_scope,
            },
        );
    }
    if closed_publication.root_scope.ordinal != closed_publication.ordinal {
        return Err(
            P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::ClosedRootScopeOrdinalMismatch {
                expected: closed_publication.ordinal,
                actual: closed_publication.root_scope.ordinal,
            },
        );
    }
    if closed_publication.root_scope.kind != closed_publication.kind {
        return Err(
            P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::ClosedRootScopeKindMismatch {
                expected: closed_publication.kind,
                actual: closed_publication.root_scope.kind,
            },
        );
    }

    if normal_return_exit_record.restored_top_call_frame
        != closed_publication.previous_top_call_frame
    {
        return Err(
            P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::RestoredTopCallFrameMismatch {
                expected: closed_publication.previous_top_call_frame,
                actual: normal_return_exit_record.restored_top_call_frame,
            },
        );
    }
    if normal_return_exit_record.restored_top_entry_frame
        != closed_publication.previous_top_entry_frame
    {
        return Err(
            P6Arm64VmEntryNormalReturnRestorationLinkageMismatch::RestoredTopEntryFrameMismatch {
                expected: closed_publication.previous_top_entry_frame,
                actual: normal_return_exit_record.restored_top_entry_frame,
            },
        );
    }

    Ok(())
}
