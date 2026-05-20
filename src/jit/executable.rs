//! Data-only executable allocation and link-finalization ledger.
//!
//! This module records the boundary between assembler buffers, executable
//! allocation ownership, and VM materialization. It never allocates executable
//! memory, stores code bytes, exposes function pointers, calls native entries,
//! or applies inline-cache patches.

use crate::assembler::{
    compute_assembler_byte_image_digest, AssemblerBufferDescriptor, AssemblerBufferId,
    AssemblerBufferLifecycle, AssemblerByteImage, AssemblerByteImageDescriptor,
    AssemblerByteImageDigest, AssemblerByteImageId, AssemblerValidationError, CodeRefOwnership,
    JitPermissionTransition, LinkBufferLayoutPlan, LinkBufferProfile, LinkBufferState,
    LinkedAssemblerByteImage, MacroAssemblerCodeRefDescriptor, ASSEMBLER_SCHEMA_REGISTRY,
};
use crate::runtime::{CodeBlockId, NativeCodeId};

use super::code::JitCodeId;
use super::machine::{
    CodePatchState, ExecutableAllocationId, ExecutableAllocationLifecycle,
    ExecutableMemoryProtection, ExecutableMutationAuthority, MachineCodeHandle,
    MachineCodeOwnership, MachineCodeRange, MachineCodeValidationError,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutableLedgerValidationError {
    AllocationIdZero,
    AllocationByteLengthZero,
    AllocationRangeInvalid {
        error: MachineCodeValidationError,
    },
    AllocationRangeMismatch {
        expected: ExecutableAllocationId,
        actual: ExecutableAllocationId,
    },
    AllocationRangeLengthMismatch {
        expected: u32,
        actual: u32,
    },
    AllocationProtectionLifecycleMismatch {
        protection: ExecutableMemoryProtection,
        lifecycle: ExecutableAllocationLifecycle,
    },
    AllocationWritableAuthorityMismatch {
        authority: ExecutableMutationAuthority,
    },
    AllocationReleasedAuthorityMismatch {
        authority: ExecutableMutationAuthority,
    },
    AllocationRecordRejected {
        reason: Box<ExecutableLedgerValidationError>,
    },
    AllocationRecordNotLinkWritable {
        protection: ExecutableMemoryProtection,
        lifecycle: ExecutableAllocationLifecycle,
    },
    AllocationLinkAuthorityMismatch {
        authority: ExecutableMutationAuthority,
    },
    AllocationOwnerMismatch {
        expected: MachineCodeOwnership,
        actual: MachineCodeOwnership,
    },
    AllocationCodeSizeMismatch {
        expected: u32,
        actual: u32,
    },
    SourceBufferInvalid {
        error: AssemblerValidationError,
    },
    SourceBufferNotFrozen {
        actual: AssemblerBufferLifecycle,
    },
    SourceBufferEmpty,
    ByteImageInvalid {
        error: AssemblerValidationError,
    },
    ByteImageDigestMissing,
    LinkedByteImageInvalid {
        error: AssemblerValidationError,
    },
    LinkedImageSourceImageMismatch {
        expected: AssemblerByteImageId,
        actual: AssemblerByteImageId,
    },
    LinkedImageSourceDigestMismatch {
        expected: AssemblerByteImageDigest,
        actual: AssemblerByteImageDigest,
    },
    LinkedImageOutputDigestMismatch {
        expected: AssemblerByteImageDigest,
        actual: AssemblerByteImageDigest,
    },
    LinkedImageOutputByteLengthMismatch {
        expected: u32,
        actual: u32,
    },
    LinkedImageRelocationCountMismatch {
        expected: usize,
        actual: usize,
    },
    LinkedImageProfileMismatch {
        expected: LinkBufferProfile,
        actual: LinkBufferProfile,
    },
    LinkedImageStateMismatch {
        actual: LinkBufferState,
    },
    ByteImageSourceMismatch {
        expected: AssemblerBufferId,
        actual: AssemblerBufferId,
    },
    LayoutSourceMismatch {
        expected: AssemblerBufferId,
        actual: AssemblerBufferId,
    },
    LayoutProfileMismatch {
        expected: LinkBufferProfile,
        actual: Option<LinkBufferProfile>,
    },
    LayoutStateMismatch {
        expected: LinkBufferState,
        actual: Option<LinkBufferState>,
    },
    LayoutAllocationMismatch {
        expected: ExecutableAllocationId,
        actual: Option<ExecutableAllocationId>,
    },
    LayoutCodeSizeMismatch {
        expected: u32,
        actual: u32,
    },
    LinkBufferSchemaMissing {
        profile: LinkBufferProfile,
    },
    LinkBufferTransitionMismatch {
        expected: Option<JitPermissionTransition>,
        actual: Option<JitPermissionTransition>,
    },
    CodeIdZero,
    NativeSymbolMissing,
    PatchPlanInvalid {
        error: MachineCodeValidationError,
    },
    PatchPlanOwnerMismatch {
        expected: CodeBlockId,
        actual: CodeBlockId,
    },
    PatchPlanRangeAllocationMismatch {
        expected: ExecutableAllocationId,
        actual: ExecutableAllocationId,
    },
    PatchPlanStateNotFinalizationSafe {
        state: CodePatchState,
    },
    PatchPlanCodeIdMismatch {
        expected: JitCodeId,
        actual: JitCodeId,
    },
    CopyLinkMissing,
    CopyLinkRecordRejected {
        reason: Box<ExecutableLedgerValidationError>,
    },
    CopyLinkProofMissing,
    CopyLinkSourceMismatch {
        expected: AssemblerBufferId,
        actual: AssemblerBufferId,
    },
    CopyLinkImageMismatch {
        expected: AssemblerByteImageId,
        actual: AssemblerByteImageId,
    },
    CopyLinkDigestMismatch {
        expected: AssemblerByteImageDigest,
        actual: AssemblerByteImageDigest,
    },
    CopyLinkByteLengthMismatch {
        expected: u32,
        actual: u32,
    },
    CopyLinkAllocationMismatch {
        expected: ExecutableAllocationId,
        actual: ExecutableAllocationId,
    },
    CopyLinkAllocationRangeMismatch {
        expected: MachineCodeRange,
        actual: MachineCodeRange,
    },
    CopyLinkCodeIdMismatch {
        expected: JitCodeId,
        actual: JitCodeId,
    },
    CopyLinkOwnerMismatch {
        expected: MachineCodeOwnership,
        actual: MachineCodeOwnership,
    },
    CopyLinkStateMismatch {
        actual: LinkBufferState,
    },
    CopyLinkByteEvidenceMissing,
    CopyLinkByteEvidenceUnexpected,
    CopyLinkByteEvidenceProofMismatch,
    FinalizationProofMissing,
    FinalizationRecordRejected,
    FinalizationStateMismatch {
        actual: LinkBufferState,
    },
    FinalizationByteEvidenceMissing,
    FinalizationByteEvidenceMismatch {
        expected: Box<Option<LinkBufferByteCopyEvidence>>,
        actual: Box<Option<LinkBufferByteCopyEvidence>>,
    },
    FinalizedCodeRefMissing,
    FinalizedCodeRefAllocationMismatch {
        expected: Option<ExecutableAllocationId>,
        actual: Option<ExecutableAllocationId>,
    },
    FinalizedCodeRefOwnershipMismatch {
        actual: CodeRefOwnership,
    },
    FinalizedCodeRefRangeMismatch {
        expected_offset: u32,
        actual_offset: u32,
        expected_size: u32,
        actual_size: u32,
    },
    FinalizedHandleMissing,
    FinalizedHandleMismatch {
        expected: MachineCodeHandle,
        actual: MachineCodeHandle,
    },
    FinalizedHandleInvalid {
        error: MachineCodeValidationError,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutableAllocationRequest {
    pub allocation: ExecutableAllocationId,
    pub owner: MachineCodeOwnership,
    pub byte_len: u32,
    pub range: MachineCodeRange,
    pub protection: ExecutableMemoryProtection,
    pub lifecycle: ExecutableAllocationLifecycle,
    pub mutation_authority: ExecutableMutationAuthority,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutableAllocationOutcome {
    Accepted,
    Rejected {
        reason: ExecutableLedgerValidationError,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutableAllocationRecord {
    pub allocation: ExecutableAllocationId,
    pub owner: MachineCodeOwnership,
    pub byte_len: u32,
    pub range: MachineCodeRange,
    pub protection: ExecutableMemoryProtection,
    pub lifecycle: ExecutableAllocationLifecycle,
    pub mutation_authority: ExecutableMutationAuthority,
    pub outcome: ExecutableAllocationOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkBufferCopyLinkRequest<'a> {
    pub source: &'a AssemblerBufferDescriptor,
    pub source_image: &'a AssemblerByteImageDescriptor,
    pub layout: &'a LinkBufferLayoutPlan,
    pub allocation: &'a ExecutableAllocationRecord,
    pub code_id: JitCodeId,
    pub owner: MachineCodeOwnership,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkBufferLinkedCopyLinkRequest<'a> {
    pub source: &'a AssemblerBufferDescriptor,
    pub source_image: &'a AssemblerByteImage,
    pub linked_image: &'a LinkedAssemblerByteImage,
    pub layout: &'a LinkBufferLayoutPlan,
    pub allocation: &'a ExecutableAllocationRecord,
    pub code_id: JitCodeId,
    pub owner: MachineCodeOwnership,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkBufferCopyLinkOutcome {
    Accepted,
    Rejected {
        reason: ExecutableLedgerValidationError,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkBufferByteCopyEvidence {
    pub source_image: AssemblerByteImageId,
    pub source_digest: AssemblerByteImageDigest,
    pub output_digest: AssemblerByteImageDigest,
    pub output_byte_len: u32,
    pub relocation_count: usize,
    pub profile: LinkBufferProfile,
    pub state: LinkBufferState,
    proof: LinkBufferByteCopyEvidenceProof,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LinkBufferByteCopyEvidenceProof {
    source_image: AssemblerByteImageId,
    source_digest: AssemblerByteImageDigest,
    output_digest: AssemblerByteImageDigest,
    output_byte_len: u32,
    relocation_count: usize,
    profile: LinkBufferProfile,
    state: LinkBufferState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkBufferCopyLinkRecord {
    pub source: AssemblerBufferId,
    pub source_image: AssemblerByteImageId,
    pub digest: AssemblerByteImageDigest,
    pub source_byte_len: u32,
    pub profile: Option<LinkBufferProfile>,
    pub state: LinkBufferState,
    pub allocation: ExecutableAllocationId,
    pub allocation_range: MachineCodeRange,
    pub code_id: JitCodeId,
    pub owner: MachineCodeOwnership,
    pub linked_relocation_count: usize,
    pub patch_plan_count: usize,
    pub byte_copy_evidence: Option<LinkBufferByteCopyEvidence>,
    pub outcome: LinkBufferCopyLinkOutcome,
    proof: LinkBufferCopyLinkProof,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LinkBufferCopyLinkProof {
    source_image_verified: bool,
    layout_verified: bool,
    allocation_verified: bool,
    patch_plans_validated: bool,
    byte_evidence_verified: bool,
    source: AssemblerBufferId,
    source_image: AssemblerByteImageId,
    digest: AssemblerByteImageDigest,
    source_byte_len: u32,
    profile: Option<LinkBufferProfile>,
    state: LinkBufferState,
    allocation: ExecutableAllocationId,
    allocation_range: MachineCodeRange,
    code_id: JitCodeId,
    owner: MachineCodeOwnership,
    linked_relocation_count: usize,
    patch_plan_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkBufferFinalizationRequest<'a> {
    pub copy_link: &'a LinkBufferCopyLinkRecord,
    pub code_id: JitCodeId,
    pub owner: MachineCodeOwnership,
    pub symbol: Option<NativeCodeId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkBufferFinalizationOutcome {
    Accepted,
    Rejected {
        reason: ExecutableLedgerValidationError,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkBufferFinalizationRecord {
    pub source: AssemblerBufferId,
    pub source_lifecycle: AssemblerBufferLifecycle,
    pub source_byte_len: u32,
    pub profile: Option<LinkBufferProfile>,
    pub state: LinkBufferState,
    pub permission_transition: Option<JitPermissionTransition>,
    pub allocation: ExecutableAllocationId,
    pub allocation_range: MachineCodeRange,
    pub code_id: JitCodeId,
    pub owner: MachineCodeOwnership,
    pub symbol: Option<NativeCodeId>,
    pub code_ref: Option<MacroAssemblerCodeRefDescriptor>,
    pub machine_code: Option<MachineCodeHandle>,
    pub patch_plan_count: usize,
    pub byte_copy_evidence: Option<LinkBufferByteCopyEvidence>,
    pub copy_link: LinkBufferCopyLinkRecord,
    pub outcome: LinkBufferFinalizationOutcome,
    proof: LinkBufferFinalizationProof,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct LinkBufferFinalizationProof {
    copy_link_verified: bool,
    source_buffer_verified: bool,
    allocation_verified: bool,
    code_ref_verified: bool,
    machine_handle_verified: bool,
    patch_plans_validated: bool,
    byte_evidence_verified: bool,
}

impl Default for LinkBufferCopyLinkProof {
    fn default() -> Self {
        Self {
            source_image_verified: false,
            layout_verified: false,
            allocation_verified: false,
            patch_plans_validated: false,
            byte_evidence_verified: false,
            source: AssemblerBufferId(0),
            source_image: AssemblerByteImageId(0),
            digest: AssemblerByteImageDigest(0),
            source_byte_len: 0,
            profile: None,
            state: LinkBufferState::Unlinked,
            allocation: ExecutableAllocationId(0),
            allocation_range: MachineCodeRange {
                allocation: ExecutableAllocationId(0),
                start_offset: 0,
                size_bytes: 0,
            },
            code_id: JitCodeId(0),
            owner: MachineCodeOwnership::Host,
            linked_relocation_count: 0,
            patch_plan_count: 0,
        }
    }
}

impl LinkBufferByteCopyEvidence {
    #[allow(dead_code)]
    fn from_linked_image(linked_image: &LinkedAssemblerByteImage) -> Self {
        let evidence = Self {
            source_image: linked_image.source_image_id,
            source_digest: linked_image.source_image_digest,
            output_digest: linked_image.output_digest,
            output_byte_len: linked_image.output_size_bytes,
            relocation_count: linked_image.relocation_count,
            profile: linked_image.profile,
            state: linked_image.state,
            proof: LinkBufferByteCopyEvidenceProof {
                source_image: linked_image.source_image_id,
                source_digest: linked_image.source_image_digest,
                output_digest: linked_image.output_digest,
                output_byte_len: linked_image.output_size_bytes,
                relocation_count: linked_image.relocation_count,
                profile: linked_image.profile,
                state: linked_image.state,
            },
        };
        debug_assert_eq!(evidence.proof, evidence.expected_proof());
        evidence
    }

    fn expected_proof(&self) -> LinkBufferByteCopyEvidenceProof {
        LinkBufferByteCopyEvidenceProof {
            source_image: self.source_image,
            source_digest: self.source_digest,
            output_digest: self.output_digest,
            output_byte_len: self.output_byte_len,
            relocation_count: self.relocation_count,
            profile: self.profile,
            state: self.state,
        }
    }

    fn validate_for_copy_link(
        &self,
        copy_link: &LinkBufferCopyLinkRecord,
    ) -> Result<(), ExecutableLedgerValidationError> {
        if self.proof != self.expected_proof() {
            return Err(ExecutableLedgerValidationError::CopyLinkByteEvidenceProofMismatch);
        }
        if self.source_image != copy_link.source_image {
            return Err(
                ExecutableLedgerValidationError::LinkedImageSourceImageMismatch {
                    expected: copy_link.source_image,
                    actual: self.source_image,
                },
            );
        }
        if self.source_digest != copy_link.digest {
            return Err(
                ExecutableLedgerValidationError::LinkedImageSourceDigestMismatch {
                    expected: copy_link.digest,
                    actual: self.source_digest,
                },
            );
        }
        if self.output_byte_len != copy_link.source_byte_len {
            return Err(
                ExecutableLedgerValidationError::LinkedImageOutputByteLengthMismatch {
                    expected: copy_link.source_byte_len,
                    actual: self.output_byte_len,
                },
            );
        }
        if self.relocation_count != copy_link.linked_relocation_count {
            return Err(
                ExecutableLedgerValidationError::LinkedImageRelocationCountMismatch {
                    expected: copy_link.linked_relocation_count,
                    actual: self.relocation_count,
                },
            );
        }
        if Some(self.profile) != copy_link.profile {
            let expected = copy_link.profile.unwrap_or(LinkBufferProfile::Baseline);
            return Err(
                ExecutableLedgerValidationError::LinkedImageProfileMismatch {
                    expected,
                    actual: self.profile,
                },
            );
        }
        if self.state != LinkBufferState::Linked {
            return Err(ExecutableLedgerValidationError::LinkedImageStateMismatch {
                actual: self.state,
            });
        }
        if self.state != copy_link.state {
            return Err(ExecutableLedgerValidationError::CopyLinkStateMismatch {
                actual: copy_link.state,
            });
        }
        if self.output_digest.0 == 0 {
            return Err(ExecutableLedgerValidationError::ByteImageDigestMissing);
        }

        Ok(())
    }
}

impl ExecutableAllocationRequest {
    pub const fn with_lifecycle(
        allocation: ExecutableAllocationId,
        owner: MachineCodeOwnership,
        start_offset: u32,
        byte_len: u32,
        protection: ExecutableMemoryProtection,
        lifecycle: ExecutableAllocationLifecycle,
        mutation_authority: ExecutableMutationAuthority,
    ) -> Self {
        Self {
            allocation,
            owner,
            byte_len,
            range: MachineCodeRange {
                allocation,
                start_offset,
                size_bytes: byte_len,
            },
            protection,
            lifecycle,
            mutation_authority,
        }
    }

    pub const fn allocated_writable(
        allocation: ExecutableAllocationId,
        owner: MachineCodeOwnership,
        byte_len: u32,
    ) -> Self {
        Self::with_lifecycle(
            allocation,
            owner,
            0,
            byte_len,
            ExecutableMemoryProtection::Writable,
            ExecutableAllocationLifecycle::AllocatedWritable,
            ExecutableMutationAuthority::LinkBuffer,
        )
    }

    pub const fn jettison_pending(
        allocation: ExecutableAllocationId,
        owner: MachineCodeOwnership,
        byte_len: u32,
    ) -> Self {
        Self::with_lifecycle(
            allocation,
            owner,
            0,
            byte_len,
            ExecutableMemoryProtection::Executable,
            ExecutableAllocationLifecycle::JettisonPending,
            ExecutableMutationAuthority::AllocatorOnly,
        )
    }

    pub const fn released(
        allocation: ExecutableAllocationId,
        owner: MachineCodeOwnership,
        byte_len: u32,
    ) -> Self {
        Self::with_lifecycle(
            allocation,
            owner,
            0,
            byte_len,
            ExecutableMemoryProtection::Decommitted,
            ExecutableAllocationLifecycle::Released,
            ExecutableMutationAuthority::AllocatorOnly,
        )
    }

    pub fn validate(&self) -> Result<(), ExecutableLedgerValidationError> {
        if self.allocation.0 == 0 {
            return Err(ExecutableLedgerValidationError::AllocationIdZero);
        }
        if self.byte_len == 0 {
            return Err(ExecutableLedgerValidationError::AllocationByteLengthZero);
        }
        self.range
            .validate()
            .map_err(|error| ExecutableLedgerValidationError::AllocationRangeInvalid { error })?;
        if self.range.allocation != self.allocation {
            return Err(ExecutableLedgerValidationError::AllocationRangeMismatch {
                expected: self.allocation,
                actual: self.range.allocation,
            });
        }
        if self.range.size_bytes != self.byte_len {
            return Err(
                ExecutableLedgerValidationError::AllocationRangeLengthMismatch {
                    expected: self.byte_len,
                    actual: self.range.size_bytes,
                },
            );
        }
        if !super::machine::protection_matches_lifecycle(self.protection, self.lifecycle) {
            return Err(
                ExecutableLedgerValidationError::AllocationProtectionLifecycleMismatch {
                    protection: self.protection,
                    lifecycle: self.lifecycle,
                },
            );
        }
        if matches!(
            self.protection,
            ExecutableMemoryProtection::Writable | ExecutableMemoryProtection::WritableForPatching
        ) && self.mutation_authority == ExecutableMutationAuthority::AllocatorOnly
        {
            return Err(
                ExecutableLedgerValidationError::AllocationWritableAuthorityMismatch {
                    authority: self.mutation_authority,
                },
            );
        }
        if self.lifecycle == ExecutableAllocationLifecycle::Released
            && self.mutation_authority != ExecutableMutationAuthority::AllocatorOnly
        {
            return Err(
                ExecutableLedgerValidationError::AllocationReleasedAuthorityMismatch {
                    authority: self.mutation_authority,
                },
            );
        }

        Ok(())
    }
}

impl ExecutableAllocationRecord {
    pub fn from_request(request: ExecutableAllocationRequest) -> Self {
        let outcome = match request.validate() {
            Ok(()) => ExecutableAllocationOutcome::Accepted,
            Err(reason) => ExecutableAllocationOutcome::Rejected { reason },
        };
        Self {
            allocation: request.allocation,
            owner: request.owner,
            byte_len: request.byte_len,
            range: request.range,
            protection: request.protection,
            lifecycle: request.lifecycle,
            mutation_authority: request.mutation_authority,
            outcome,
        }
    }

    pub fn validate(&self) -> Result<(), ExecutableLedgerValidationError> {
        let request = ExecutableAllocationRequest {
            allocation: self.allocation,
            owner: self.owner,
            byte_len: self.byte_len,
            range: self.range,
            protection: self.protection,
            lifecycle: self.lifecycle,
            mutation_authority: self.mutation_authority,
        };
        request.validate()?;
        match &self.outcome {
            ExecutableAllocationOutcome::Accepted => Ok(()),
            ExecutableAllocationOutcome::Rejected { reason } => {
                Err(ExecutableLedgerValidationError::AllocationRecordRejected {
                    reason: Box::new(reason.clone()),
                })
            }
        }
    }

    pub fn validate_for_link_buffer(&self) -> Result<(), ExecutableLedgerValidationError> {
        self.validate()?;
        if self.protection != ExecutableMemoryProtection::Writable
            || self.lifecycle != ExecutableAllocationLifecycle::AllocatedWritable
        {
            return Err(
                ExecutableLedgerValidationError::AllocationRecordNotLinkWritable {
                    protection: self.protection,
                    lifecycle: self.lifecycle,
                },
            );
        }
        if self.mutation_authority != ExecutableMutationAuthority::LinkBuffer {
            return Err(
                ExecutableLedgerValidationError::AllocationLinkAuthorityMismatch {
                    authority: self.mutation_authority,
                },
            );
        }

        Ok(())
    }
}

impl LinkBufferCopyLinkRecord {
    pub fn validate_accepted(&self) -> Result<(), ExecutableLedgerValidationError> {
        match &self.outcome {
            LinkBufferCopyLinkOutcome::Accepted => {}
            LinkBufferCopyLinkOutcome::Rejected { reason } => {
                return Err(ExecutableLedgerValidationError::CopyLinkRecordRejected {
                    reason: Box::new(reason.clone()),
                });
            }
        }
        if self.digest.0 == 0 {
            return Err(ExecutableLedgerValidationError::ByteImageDigestMissing);
        }
        if self.source_byte_len == 0 {
            return Err(ExecutableLedgerValidationError::SourceBufferEmpty);
        }
        if self.profile != Some(LinkBufferProfile::Baseline) {
            return Err(ExecutableLedgerValidationError::LayoutProfileMismatch {
                expected: LinkBufferProfile::Baseline,
                actual: self.profile,
            });
        }
        if self.state != LinkBufferState::Linked {
            return Err(ExecutableLedgerValidationError::CopyLinkStateMismatch {
                actual: self.state,
            });
        }
        if self.allocation_range.allocation != self.allocation {
            return Err(
                ExecutableLedgerValidationError::CopyLinkAllocationMismatch {
                    expected: self.allocation,
                    actual: self.allocation_range.allocation,
                },
            );
        }
        self.allocation_range
            .validate()
            .map_err(|error| ExecutableLedgerValidationError::AllocationRangeInvalid { error })?;
        if self.allocation_range.size_bytes != self.source_byte_len {
            return Err(
                ExecutableLedgerValidationError::AllocationCodeSizeMismatch {
                    expected: self.source_byte_len,
                    actual: self.allocation_range.size_bytes,
                },
            );
        }
        if self.code_id.0 == 0 {
            return Err(ExecutableLedgerValidationError::CodeIdZero);
        }
        if self.proof.byte_evidence_verified && self.byte_copy_evidence.is_none() {
            return Err(ExecutableLedgerValidationError::CopyLinkByteEvidenceMissing);
        }
        if !self.proof.byte_evidence_verified && self.byte_copy_evidence.is_some() {
            return Err(ExecutableLedgerValidationError::CopyLinkByteEvidenceUnexpected);
        }
        if let Some(evidence) = &self.byte_copy_evidence {
            evidence.validate_for_copy_link(self)?;
        }
        if self.proof != self.expected_proof() {
            return Err(ExecutableLedgerValidationError::CopyLinkProofMissing);
        }

        Ok(())
    }

    pub fn validate_accepted_with_byte_evidence(
        &self,
    ) -> Result<&LinkBufferByteCopyEvidence, ExecutableLedgerValidationError> {
        if self.byte_copy_evidence.is_none() {
            return Err(ExecutableLedgerValidationError::CopyLinkByteEvidenceMissing);
        }
        self.validate_accepted()?;
        Ok(self
            .byte_copy_evidence
            .as_ref()
            .expect("byte evidence checked above"))
    }

    fn expected_proof(&self) -> LinkBufferCopyLinkProof {
        LinkBufferCopyLinkProof {
            source_image_verified: true,
            layout_verified: true,
            allocation_verified: true,
            patch_plans_validated: true,
            byte_evidence_verified: self.byte_copy_evidence.is_some(),
            source: self.source,
            source_image: self.source_image,
            digest: self.digest,
            source_byte_len: self.source_byte_len,
            profile: self.profile,
            state: self.state,
            allocation: self.allocation,
            allocation_range: self.allocation_range,
            code_id: self.code_id,
            owner: self.owner,
            linked_relocation_count: self.linked_relocation_count,
            patch_plan_count: self.patch_plan_count,
        }
    }
}

impl LinkBufferFinalizationRecord {
    pub fn accepted_machine_code(&self) -> Option<MachineCodeHandle> {
        self.validate_accepted().ok()
    }

    pub fn accepted_code_ref(&self) -> Option<&MacroAssemblerCodeRefDescriptor> {
        if self.validate_accepted().is_ok() {
            self.code_ref.as_ref()
        } else {
            None
        }
    }

    pub fn validate_accepted(&self) -> Result<MachineCodeHandle, ExecutableLedgerValidationError> {
        if self.outcome != LinkBufferFinalizationOutcome::Accepted {
            return Err(ExecutableLedgerValidationError::FinalizationRecordRejected);
        }
        if self.proof.byte_evidence_verified && self.byte_copy_evidence.is_none() {
            return Err(ExecutableLedgerValidationError::FinalizationByteEvidenceMissing);
        }
        if self.byte_copy_evidence != self.copy_link.byte_copy_evidence {
            return Err(
                ExecutableLedgerValidationError::FinalizationByteEvidenceMismatch {
                    expected: Box::new(self.copy_link.byte_copy_evidence),
                    actual: Box::new(self.byte_copy_evidence),
                },
            );
        }
        if self.proof
            != (LinkBufferFinalizationProof {
                copy_link_verified: true,
                source_buffer_verified: true,
                allocation_verified: true,
                code_ref_verified: true,
                machine_handle_verified: true,
                patch_plans_validated: true,
                byte_evidence_verified: self.byte_copy_evidence.is_some(),
            })
        {
            return Err(ExecutableLedgerValidationError::FinalizationProofMissing);
        }
        self.copy_link.validate_accepted()?;
        if let Some(evidence) = &self.byte_copy_evidence {
            evidence.validate_for_copy_link(&self.copy_link)?;
        }
        if self.copy_link.source != self.source {
            return Err(ExecutableLedgerValidationError::CopyLinkSourceMismatch {
                expected: self.source,
                actual: self.copy_link.source,
            });
        }
        if self.copy_link.source_byte_len != self.source_byte_len {
            return Err(
                ExecutableLedgerValidationError::CopyLinkByteLengthMismatch {
                    expected: self.source_byte_len,
                    actual: self.copy_link.source_byte_len,
                },
            );
        }
        if self.copy_link.profile != self.profile {
            return Err(ExecutableLedgerValidationError::LayoutProfileMismatch {
                expected: LinkBufferProfile::Baseline,
                actual: self.copy_link.profile,
            });
        }
        if self.copy_link.allocation != self.allocation {
            return Err(
                ExecutableLedgerValidationError::CopyLinkAllocationMismatch {
                    expected: self.allocation,
                    actual: self.copy_link.allocation,
                },
            );
        }
        if self.copy_link.allocation_range != self.allocation_range {
            return Err(
                ExecutableLedgerValidationError::CopyLinkAllocationRangeMismatch {
                    expected: self.allocation_range,
                    actual: self.copy_link.allocation_range,
                },
            );
        }
        if self.copy_link.code_id != self.code_id {
            return Err(ExecutableLedgerValidationError::CopyLinkCodeIdMismatch {
                expected: self.code_id,
                actual: self.copy_link.code_id,
            });
        }
        if self.copy_link.owner != self.owner {
            return Err(ExecutableLedgerValidationError::CopyLinkOwnerMismatch {
                expected: self.owner,
                actual: self.copy_link.owner,
            });
        }
        if self.copy_link.patch_plan_count != self.patch_plan_count {
            return Err(ExecutableLedgerValidationError::CopyLinkProofMissing);
        }
        if self.state != LinkBufferState::Finalized {
            return Err(ExecutableLedgerValidationError::FinalizationStateMismatch {
                actual: self.state,
            });
        }
        if self.source_lifecycle != AssemblerBufferLifecycle::FrozenForLink {
            return Err(ExecutableLedgerValidationError::SourceBufferNotFrozen {
                actual: self.source_lifecycle,
            });
        }
        if self.source_byte_len == 0 {
            return Err(ExecutableLedgerValidationError::SourceBufferEmpty);
        }
        if self.profile != Some(LinkBufferProfile::Baseline) {
            return Err(ExecutableLedgerValidationError::LayoutProfileMismatch {
                expected: LinkBufferProfile::Baseline,
                actual: self.profile,
            });
        }
        if self.permission_transition != Some(JitPermissionTransition::RwToRx) {
            return Err(
                ExecutableLedgerValidationError::LinkBufferTransitionMismatch {
                    expected: Some(JitPermissionTransition::RwToRx),
                    actual: self.permission_transition,
                },
            );
        }
        if self.code_id.0 == 0 {
            return Err(ExecutableLedgerValidationError::CodeIdZero);
        }
        let symbol = self
            .symbol
            .ok_or(ExecutableLedgerValidationError::NativeSymbolMissing)?;
        let code_ref = self
            .code_ref
            .as_ref()
            .ok_or(ExecutableLedgerValidationError::FinalizedCodeRefMissing)?;
        if code_ref.allocation != Some(self.allocation) {
            return Err(
                ExecutableLedgerValidationError::FinalizedCodeRefAllocationMismatch {
                    expected: Some(self.allocation),
                    actual: code_ref.allocation,
                },
            );
        }
        if code_ref.ownership != CodeRefOwnership::ExecutableMemoryHandle {
            return Err(
                ExecutableLedgerValidationError::FinalizedCodeRefOwnershipMismatch {
                    actual: code_ref.ownership,
                },
            );
        }
        if code_ref.code_offset != self.allocation_range.start_offset
            || code_ref.size_bytes != self.allocation_range.size_bytes
        {
            return Err(
                ExecutableLedgerValidationError::FinalizedCodeRefRangeMismatch {
                    expected_offset: self.allocation_range.start_offset,
                    actual_offset: code_ref.code_offset,
                    expected_size: self.allocation_range.size_bytes,
                    actual_size: code_ref.size_bytes,
                },
            );
        }

        let machine_code = self
            .machine_code
            .ok_or(ExecutableLedgerValidationError::FinalizedHandleMissing)?;
        let expected = MachineCodeHandle {
            allocation: self.allocation,
            owner: self.owner,
            range: self.allocation_range,
            symbol: Some(symbol),
            protection: ExecutableMemoryProtection::Executable,
            lifecycle: ExecutableAllocationLifecycle::LinkedExecutable,
            mutation_authority: ExecutableMutationAuthority::LinkBuffer,
        };
        if machine_code != expected {
            return Err(ExecutableLedgerValidationError::FinalizedHandleMismatch {
                expected,
                actual: machine_code,
            });
        }
        machine_code
            .validate()
            .map_err(|error| ExecutableLedgerValidationError::FinalizedHandleInvalid { error })?;

        Ok(machine_code)
    }

    pub fn validate_accepted_with_byte_evidence(
        &self,
    ) -> Result<MachineCodeHandle, ExecutableLedgerValidationError> {
        if self.byte_copy_evidence.is_none() {
            return Err(ExecutableLedgerValidationError::FinalizationByteEvidenceMissing);
        }
        self.copy_link.validate_accepted_with_byte_evidence()?;
        self.validate_accepted()
    }
}

pub fn record_link_buffer_copy_link(
    request: LinkBufferCopyLinkRequest<'_>,
) -> LinkBufferCopyLinkRecord {
    let result = validate_link_buffer_copy_link(&request);
    let accepted = result.is_ok();
    let state = if accepted {
        LinkBufferState::Linked
    } else {
        request
            .layout
            .plan
            .state
            .unwrap_or(LinkBufferState::Unlinked)
    };
    let outcome = match result {
        Ok(()) => LinkBufferCopyLinkOutcome::Accepted,
        Err(reason) => LinkBufferCopyLinkOutcome::Rejected { reason },
    };
    let proof = if accepted {
        LinkBufferCopyLinkProof {
            source_image_verified: true,
            layout_verified: true,
            allocation_verified: true,
            patch_plans_validated: true,
            byte_evidence_verified: false,
            source: request.source.id,
            source_image: request.source_image.id,
            digest: request.source_image.digest,
            source_byte_len: request.source_image.byte_len,
            profile: request.layout.plan.profile,
            state,
            allocation: request.allocation.allocation,
            allocation_range: request.allocation.range,
            code_id: request.code_id,
            owner: request.owner,
            linked_relocation_count: request.layout.ordered_relocations.len(),
            patch_plan_count: request.layout.plan.patches.len(),
        }
    } else {
        LinkBufferCopyLinkProof::default()
    };

    LinkBufferCopyLinkRecord {
        source: request.source.id,
        source_image: request.source_image.id,
        digest: request.source_image.digest,
        source_byte_len: request.source_image.byte_len,
        profile: request.layout.plan.profile,
        state,
        allocation: request.allocation.allocation,
        allocation_range: request.allocation.range,
        code_id: request.code_id,
        owner: request.owner,
        linked_relocation_count: request.layout.ordered_relocations.len(),
        patch_plan_count: request.layout.plan.patches.len(),
        byte_copy_evidence: None,
        outcome,
        proof,
    }
}

#[allow(dead_code)]
pub fn record_link_buffer_copy_link_with_linked_image(
    request: LinkBufferLinkedCopyLinkRequest<'_>,
) -> LinkBufferCopyLinkRecord {
    let result = validate_link_buffer_linked_copy_link(&request);
    let accepted = result.is_ok();
    let state = if accepted {
        LinkBufferState::Linked
    } else {
        request
            .layout
            .plan
            .state
            .unwrap_or(LinkBufferState::Unlinked)
    };
    let (outcome, byte_copy_evidence) = match result {
        Ok(evidence) => (LinkBufferCopyLinkOutcome::Accepted, Some(evidence)),
        Err(reason) => (LinkBufferCopyLinkOutcome::Rejected { reason }, None),
    };
    let proof = if accepted {
        LinkBufferCopyLinkProof {
            source_image_verified: true,
            layout_verified: true,
            allocation_verified: true,
            patch_plans_validated: true,
            byte_evidence_verified: true,
            source: request.source.id,
            source_image: request.source_image.id(),
            digest: request.source_image.digest(),
            source_byte_len: request.source_image.byte_len(),
            profile: request.layout.plan.profile,
            state,
            allocation: request.allocation.allocation,
            allocation_range: request.allocation.range,
            code_id: request.code_id,
            owner: request.owner,
            linked_relocation_count: request.layout.ordered_relocations.len(),
            patch_plan_count: request.layout.plan.patches.len(),
        }
    } else {
        LinkBufferCopyLinkProof::default()
    };

    LinkBufferCopyLinkRecord {
        source: request.source.id,
        source_image: request.source_image.id(),
        digest: request.source_image.digest(),
        source_byte_len: request.source_image.byte_len(),
        profile: request.layout.plan.profile,
        state,
        allocation: request.allocation.allocation,
        allocation_range: request.allocation.range,
        code_id: request.code_id,
        owner: request.owner,
        linked_relocation_count: request.layout.ordered_relocations.len(),
        patch_plan_count: request.layout.plan.patches.len(),
        byte_copy_evidence,
        outcome,
        proof,
    }
}

pub fn finalize_link_buffer(
    request: LinkBufferFinalizationRequest<'_>,
) -> LinkBufferFinalizationRecord {
    let result = validate_link_buffer_finalization(&request);
    let permission_transition = permission_transition_for_profile(request.copy_link.profile);
    match result {
        Ok((code_ref, machine_code)) => LinkBufferFinalizationRecord {
            source: request.copy_link.source,
            source_lifecycle: AssemblerBufferLifecycle::FrozenForLink,
            source_byte_len: request.copy_link.source_byte_len,
            profile: request.copy_link.profile,
            state: LinkBufferState::Finalized,
            permission_transition,
            allocation: request.copy_link.allocation,
            allocation_range: request.copy_link.allocation_range,
            code_id: request.code_id,
            owner: request.owner,
            symbol: request.symbol,
            code_ref: Some(code_ref),
            machine_code: Some(machine_code),
            patch_plan_count: request.copy_link.patch_plan_count,
            byte_copy_evidence: request.copy_link.byte_copy_evidence,
            copy_link: request.copy_link.clone(),
            outcome: LinkBufferFinalizationOutcome::Accepted,
            proof: LinkBufferFinalizationProof {
                copy_link_verified: true,
                source_buffer_verified: true,
                allocation_verified: true,
                code_ref_verified: true,
                machine_handle_verified: true,
                patch_plans_validated: true,
                byte_evidence_verified: request.copy_link.byte_copy_evidence.is_some(),
            },
        },
        Err(reason) => LinkBufferFinalizationRecord {
            source: request.copy_link.source,
            source_lifecycle: AssemblerBufferLifecycle::FrozenForLink,
            source_byte_len: request.copy_link.source_byte_len,
            profile: request.copy_link.profile,
            state: request.copy_link.state,
            permission_transition,
            allocation: request.copy_link.allocation,
            allocation_range: request.copy_link.allocation_range,
            code_id: request.code_id,
            owner: request.owner,
            symbol: request.symbol,
            code_ref: None,
            machine_code: None,
            patch_plan_count: request.copy_link.patch_plan_count,
            byte_copy_evidence: request.copy_link.byte_copy_evidence,
            copy_link: request.copy_link.clone(),
            outcome: LinkBufferFinalizationOutcome::Rejected { reason },
            proof: LinkBufferFinalizationProof::default(),
        },
    }
}

fn validate_link_buffer_copy_link(
    request: &LinkBufferCopyLinkRequest<'_>,
) -> Result<(), ExecutableLedgerValidationError> {
    request
        .source
        .validate()
        .map_err(|error| ExecutableLedgerValidationError::SourceBufferInvalid { error })?;
    if request.source.lifecycle != AssemblerBufferLifecycle::FrozenForLink {
        return Err(ExecutableLedgerValidationError::SourceBufferNotFrozen {
            actual: request.source.lifecycle,
        });
    }
    if request.source.byte_len == 0 {
        return Err(ExecutableLedgerValidationError::SourceBufferEmpty);
    }
    if request.source_image.digest.0 == 0 {
        return Err(ExecutableLedgerValidationError::ByteImageDigestMissing);
    }
    request
        .source_image
        .validate_against_source(request.source)
        .map_err(|error| ExecutableLedgerValidationError::ByteImageInvalid { error })?;
    if request.source_image.source != request.source.id {
        return Err(ExecutableLedgerValidationError::ByteImageSourceMismatch {
            expected: request.source.id,
            actual: request.source_image.source,
        });
    }
    if request.layout.plan.source != request.source.id {
        return Err(ExecutableLedgerValidationError::LayoutSourceMismatch {
            expected: request.source.id,
            actual: request.layout.plan.source,
        });
    }
    if request.layout.plan.profile != Some(LinkBufferProfile::Baseline) {
        return Err(ExecutableLedgerValidationError::LayoutProfileMismatch {
            expected: LinkBufferProfile::Baseline,
            actual: request.layout.plan.profile,
        });
    }
    if request.layout.plan.state != Some(LinkBufferState::Linking) {
        return Err(ExecutableLedgerValidationError::LayoutStateMismatch {
            expected: LinkBufferState::Linking,
            actual: request.layout.plan.state,
        });
    }
    if request.layout.plan.allocation != Some(request.allocation.allocation) {
        return Err(ExecutableLedgerValidationError::LayoutAllocationMismatch {
            expected: request.allocation.allocation,
            actual: request.layout.plan.allocation,
        });
    }
    if request.layout.code_size_bytes != request.source.byte_len {
        return Err(ExecutableLedgerValidationError::LayoutCodeSizeMismatch {
            expected: request.source.byte_len,
            actual: request.layout.code_size_bytes,
        });
    }
    if request.layout.code_size_bytes != request.source_image.byte_len {
        return Err(
            ExecutableLedgerValidationError::CopyLinkByteLengthMismatch {
                expected: request.source_image.byte_len,
                actual: request.layout.code_size_bytes,
            },
        );
    }

    let schema = ASSEMBLER_SCHEMA_REGISTRY
        .link_buffer_for_profile(LinkBufferProfile::Baseline)
        .ok_or(ExecutableLedgerValidationError::LinkBufferSchemaMissing {
            profile: LinkBufferProfile::Baseline,
        })?;
    if request.layout.plan.required_permission_transition != schema.required_transition {
        return Err(
            ExecutableLedgerValidationError::LinkBufferTransitionMismatch {
                expected: schema.required_transition,
                actual: request.layout.plan.required_permission_transition,
            },
        );
    }
    if schema.required_transition != Some(JitPermissionTransition::RwToRx) {
        return Err(
            ExecutableLedgerValidationError::LinkBufferTransitionMismatch {
                expected: Some(JitPermissionTransition::RwToRx),
                actual: schema.required_transition,
            },
        );
    }

    request.allocation.validate_for_link_buffer()?;
    if request.allocation.owner != request.owner {
        return Err(ExecutableLedgerValidationError::AllocationOwnerMismatch {
            expected: request.owner,
            actual: request.allocation.owner,
        });
    }
    if request.allocation.range.size_bytes != request.layout.code_size_bytes {
        return Err(
            ExecutableLedgerValidationError::AllocationCodeSizeMismatch {
                expected: request.layout.code_size_bytes,
                actual: request.allocation.range.size_bytes,
            },
        );
    }
    if request.code_id.0 == 0 {
        return Err(ExecutableLedgerValidationError::CodeIdZero);
    }

    validate_patch_plans(
        request.layout,
        request.allocation.allocation,
        request.owner,
        request.code_id,
    )
}

#[allow(dead_code)]
fn validate_link_buffer_linked_copy_link(
    request: &LinkBufferLinkedCopyLinkRequest<'_>,
) -> Result<LinkBufferByteCopyEvidence, ExecutableLedgerValidationError> {
    request
        .source_image
        .validate_against_source(request.source)
        .map_err(|error| ExecutableLedgerValidationError::ByteImageInvalid { error })?;

    let descriptor_request = LinkBufferCopyLinkRequest {
        source: request.source,
        source_image: request.source_image.descriptor(),
        layout: request.layout,
        allocation: request.allocation,
        code_id: request.code_id,
        owner: request.owner,
    };
    validate_link_buffer_copy_link(&descriptor_request)?;

    validate_linked_image_against_request(request)
}

#[allow(dead_code)]
fn validate_linked_image_against_request(
    request: &LinkBufferLinkedCopyLinkRequest<'_>,
) -> Result<LinkBufferByteCopyEvidence, ExecutableLedgerValidationError> {
    let linked_image = request.linked_image;
    let source_image = request.source_image;

    if linked_image.source_image_id != source_image.id() {
        return Err(
            ExecutableLedgerValidationError::LinkedImageSourceImageMismatch {
                expected: source_image.id(),
                actual: linked_image.source_image_id,
            },
        );
    }
    if linked_image.source_image_digest != source_image.digest() {
        return Err(
            ExecutableLedgerValidationError::LinkedImageSourceDigestMismatch {
                expected: source_image.digest(),
                actual: linked_image.source_image_digest,
            },
        );
    }

    let expected_profile = request.layout.plan.profile.ok_or(
        ExecutableLedgerValidationError::LayoutProfileMismatch {
            expected: LinkBufferProfile::Baseline,
            actual: None,
        },
    )?;
    if linked_image.profile != expected_profile {
        return Err(
            ExecutableLedgerValidationError::LinkedImageProfileMismatch {
                expected: expected_profile,
                actual: linked_image.profile,
            },
        );
    }
    if linked_image.state != LinkBufferState::Linked {
        return Err(ExecutableLedgerValidationError::LinkedImageStateMismatch {
            actual: linked_image.state,
        });
    }
    if linked_image.output_size_bytes != request.layout.code_size_bytes {
        return Err(
            ExecutableLedgerValidationError::LinkedImageOutputByteLengthMismatch {
                expected: request.layout.code_size_bytes,
                actual: linked_image.output_size_bytes,
            },
        );
    }
    let actual_output_len = linked_image.bytes().len();
    if actual_output_len != linked_image.output_size_bytes as usize {
        return Err(
            ExecutableLedgerValidationError::LinkedImageOutputByteLengthMismatch {
                expected: linked_image.output_size_bytes,
                actual: actual_output_len as u32,
            },
        );
    }
    if linked_image.relocation_count != request.layout.ordered_relocations.len() {
        return Err(
            ExecutableLedgerValidationError::LinkedImageRelocationCountMismatch {
                expected: request.layout.ordered_relocations.len(),
                actual: linked_image.relocation_count,
            },
        );
    }

    let actual_output_digest = compute_assembler_byte_image_digest(linked_image.bytes());
    if linked_image.output_digest != actual_output_digest {
        return Err(
            ExecutableLedgerValidationError::LinkedImageOutputDigestMismatch {
                expected: actual_output_digest,
                actual: linked_image.output_digest,
            },
        );
    }

    linked_image
        .validate()
        .map_err(|error| ExecutableLedgerValidationError::LinkedByteImageInvalid { error })?;

    Ok(LinkBufferByteCopyEvidence::from_linked_image(linked_image))
}

fn validate_link_buffer_finalization(
    request: &LinkBufferFinalizationRequest<'_>,
) -> Result<(MacroAssemblerCodeRefDescriptor, MachineCodeHandle), ExecutableLedgerValidationError> {
    request.copy_link.validate_accepted()?;
    if request.copy_link.code_id != request.code_id {
        return Err(ExecutableLedgerValidationError::CopyLinkCodeIdMismatch {
            expected: request.code_id,
            actual: request.copy_link.code_id,
        });
    }
    if request.copy_link.owner != request.owner {
        return Err(ExecutableLedgerValidationError::CopyLinkOwnerMismatch {
            expected: request.owner,
            actual: request.copy_link.owner,
        });
    }
    let symbol = request
        .symbol
        .ok_or(ExecutableLedgerValidationError::NativeSymbolMissing)?;

    let code_ref = MacroAssemblerCodeRefDescriptor {
        allocation: Some(request.copy_link.allocation),
        ownership: CodeRefOwnership::ExecutableMemoryHandle,
        code_offset: request.copy_link.allocation_range.start_offset,
        size_bytes: request.copy_link.allocation_range.size_bytes,
        may_disassemble: true,
    };
    let machine_code = MachineCodeHandle {
        allocation: request.copy_link.allocation,
        owner: request.owner,
        range: request.copy_link.allocation_range,
        symbol: Some(symbol),
        protection: ExecutableMemoryProtection::Executable,
        lifecycle: ExecutableAllocationLifecycle::LinkedExecutable,
        mutation_authority: ExecutableMutationAuthority::LinkBuffer,
    };
    machine_code
        .validate()
        .map_err(|error| ExecutableLedgerValidationError::FinalizedHandleInvalid { error })?;

    Ok((code_ref, machine_code))
}

fn validate_patch_plans(
    layout: &LinkBufferLayoutPlan,
    allocation: ExecutableAllocationId,
    owner: MachineCodeOwnership,
    code_id: JitCodeId,
) -> Result<(), ExecutableLedgerValidationError> {
    for plan in &layout.plan.patches {
        plan.validate()
            .map_err(|error| ExecutableLedgerValidationError::PatchPlanInvalid { error })?;
        if let MachineCodeOwnership::CodeBlock(expected_owner) = owner {
            if plan.owner != expected_owner {
                return Err(ExecutableLedgerValidationError::PatchPlanOwnerMismatch {
                    expected: expected_owner,
                    actual: plan.owner,
                });
            }
        }
        for record in &plan.records {
            if record.code != code_id {
                return Err(ExecutableLedgerValidationError::PatchPlanCodeIdMismatch {
                    expected: code_id,
                    actual: record.code,
                });
            }
            if let Some(range) = record.range {
                if range.allocation != allocation {
                    return Err(
                        ExecutableLedgerValidationError::PatchPlanRangeAllocationMismatch {
                            expected: allocation,
                            actual: range.allocation,
                        },
                    );
                }
            }
            if matches!(
                record.state,
                CodePatchState::Armed | CodePatchState::Applied
            ) {
                return Err(
                    ExecutableLedgerValidationError::PatchPlanStateNotFinalizationSafe {
                        state: record.state,
                    },
                );
            }
        }
    }

    Ok(())
}

fn permission_transition_for_profile(
    profile: Option<LinkBufferProfile>,
) -> Option<JitPermissionTransition> {
    profile.and_then(|profile| {
        ASSEMBLER_SCHEMA_REGISTRY
            .link_buffer_for_profile(profile)
            .and_then(|schema| schema.required_transition)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembler::{
        describe_assembler_byte_image, freeze_assembler_byte_image, link_assembler_byte_image,
        plan_link_buffer_layout, AssemblerArchitecture, AssemblerBufferDescriptor,
        AssemblerBufferLifecycle, AssemblerByteImage, AssemblerByteImageDescriptor,
        AssemblerByteImageDigest, AssemblerByteImageId, LinkBufferLayoutPlan,
        LinkedAssemblerByteImage,
    };
    use crate::gc::CellId;
    use crate::jit::{
        CodePatchPlan, CodePatchRecord, PatchWriteBarrier, PatchpointDescriptor, PatchpointKind,
        RelocationKind,
    };

    fn owner() -> CodeBlockId {
        CodeBlockId(CellId(1))
    }

    fn frozen_buffer(byte_len: u32) -> AssemblerBufferDescriptor {
        AssemblerBufferDescriptor::builder(AssemblerBufferId(1))
            .architecture(AssemblerArchitecture::X86_64)
            .lifecycle(AssemblerBufferLifecycle::FrozenForLink)
            .capacity(byte_len, byte_len)
            .build()
            .unwrap()
    }

    fn allocation(byte_len: u32) -> ExecutableAllocationRecord {
        ExecutableAllocationRecord::from_request(ExecutableAllocationRequest::allocated_writable(
            ExecutableAllocationId(7),
            MachineCodeOwnership::CodeBlock(owner()),
            byte_len,
        ))
    }

    fn source_image(buffer: &AssemblerBufferDescriptor) -> AssemblerByteImageDescriptor {
        describe_assembler_byte_image(
            buffer,
            AssemblerByteImageId(17),
            AssemblerByteImageDigest(0xfeed),
        )
        .unwrap()
    }

    fn source_byte_image(buffer: &AssemblerBufferDescriptor) -> AssemblerByteImage {
        let bytes: Vec<u8> = (0..buffer.byte_len)
            .map(|index| (index as u8).wrapping_mul(3).wrapping_add(1))
            .collect();
        freeze_assembler_byte_image(buffer, AssemblerByteImageId(17), bytes).unwrap()
    }

    fn copy_link_record(
        buffer: &AssemblerBufferDescriptor,
        allocation: &ExecutableAllocationRecord,
        layout: &LinkBufferLayoutPlan,
        code_id: JitCodeId,
    ) -> LinkBufferCopyLinkRecord {
        let image = source_image(buffer);
        record_link_buffer_copy_link(LinkBufferCopyLinkRequest {
            source: buffer,
            source_image: &image,
            layout,
            allocation,
            code_id,
            owner: MachineCodeOwnership::CodeBlock(owner()),
        })
    }

    fn linked_copy_link_record(
        buffer: &AssemblerBufferDescriptor,
        allocation: &ExecutableAllocationRecord,
        layout: &LinkBufferLayoutPlan,
        code_id: JitCodeId,
    ) -> LinkBufferCopyLinkRecord {
        let image = source_byte_image(buffer);
        let linked_image = link_assembler_byte_image(&image, layout).unwrap();
        linked_copy_link_record_for(buffer, allocation, layout, code_id, &image, &linked_image)
    }

    fn linked_copy_link_record_for(
        buffer: &AssemblerBufferDescriptor,
        allocation: &ExecutableAllocationRecord,
        layout: &LinkBufferLayoutPlan,
        code_id: JitCodeId,
        image: &AssemblerByteImage,
        linked_image: &LinkedAssemblerByteImage,
    ) -> LinkBufferCopyLinkRecord {
        record_link_buffer_copy_link_with_linked_image(LinkBufferLinkedCopyLinkRequest {
            source: buffer,
            source_image: image,
            linked_image,
            layout,
            allocation,
            code_id,
            owner: MachineCodeOwnership::CodeBlock(owner()),
        })
    }

    fn accepted_copy_link(byte_len: u32) -> LinkBufferCopyLinkRecord {
        let buffer = frozen_buffer(byte_len);
        let allocation = allocation(byte_len);
        let layout = plan_link_buffer_layout(
            &buffer,
            LinkBufferProfile::Baseline,
            Some(allocation.allocation),
        )
        .unwrap();
        let record = copy_link_record(&buffer, &allocation, &layout, JitCodeId(9));
        assert_eq!(record.outcome, LinkBufferCopyLinkOutcome::Accepted);
        record
    }

    fn accepted_linked_copy_link(byte_len: u32) -> LinkBufferCopyLinkRecord {
        let buffer = frozen_buffer(byte_len);
        let allocation = allocation(byte_len);
        let layout = plan_link_buffer_layout(
            &buffer,
            LinkBufferProfile::Baseline,
            Some(allocation.allocation),
        )
        .unwrap();
        let record = linked_copy_link_record(&buffer, &allocation, &layout, JitCodeId(9));
        assert_eq!(record.outcome, LinkBufferCopyLinkOutcome::Accepted);
        record
    }

    fn accepted_finalization(byte_len: u32) -> LinkBufferFinalizationRecord {
        let copy_link = accepted_copy_link(byte_len);

        finalize_link_buffer(LinkBufferFinalizationRequest {
            copy_link: &copy_link,
            code_id: JitCodeId(9),
            owner: MachineCodeOwnership::CodeBlock(owner()),
            symbol: Some(NativeCodeId(10)),
        })
    }

    fn accepted_linked_finalization(byte_len: u32) -> LinkBufferFinalizationRecord {
        let copy_link = accepted_linked_copy_link(byte_len);

        finalize_link_buffer(LinkBufferFinalizationRequest {
            copy_link: &copy_link,
            code_id: JitCodeId(9),
            owner: MachineCodeOwnership::CodeBlock(owner()),
            symbol: Some(NativeCodeId(10)),
        })
    }

    fn linked_copy_link_parts(
        byte_len: u32,
    ) -> (
        AssemblerBufferDescriptor,
        ExecutableAllocationRecord,
        LinkBufferLayoutPlan,
        AssemblerByteImage,
        LinkedAssemblerByteImage,
    ) {
        let buffer = frozen_buffer(byte_len);
        let allocation = allocation(byte_len);
        let layout = plan_link_buffer_layout(
            &buffer,
            LinkBufferProfile::Baseline,
            Some(allocation.allocation),
        )
        .unwrap();
        let image = source_byte_image(&buffer);
        let linked_image = link_assembler_byte_image(&image, &layout).unwrap();

        (buffer, allocation, layout, image, linked_image)
    }

    #[test]
    fn executable_allocation_record_accepts_link_writable_metadata_only_request() {
        let record = allocation(64);

        assert_eq!(record.outcome, ExecutableAllocationOutcome::Accepted);
        assert_eq!(record.validate_for_link_buffer(), Ok(()));
        assert_eq!(record.range.end_offset(), Some(64));
    }

    #[test]
    fn executable_allocation_release_and_jettison_states_are_not_linkable() {
        let released =
            ExecutableAllocationRecord::from_request(ExecutableAllocationRequest::released(
                ExecutableAllocationId(10),
                MachineCodeOwnership::CodeBlock(owner()),
                32,
            ));
        let jettison = ExecutableAllocationRecord::from_request(
            ExecutableAllocationRequest::jettison_pending(
                ExecutableAllocationId(11),
                MachineCodeOwnership::CodeBlock(owner()),
                32,
            ),
        );

        assert_eq!(released.validate(), Ok(()));
        assert_eq!(jettison.validate(), Ok(()));
        assert_eq!(
            released.validate_for_link_buffer(),
            Err(
                ExecutableLedgerValidationError::AllocationRecordNotLinkWritable {
                    protection: ExecutableMemoryProtection::Decommitted,
                    lifecycle: ExecutableAllocationLifecycle::Released,
                },
            )
        );
        assert_eq!(
            jettison.validate_for_link_buffer(),
            Err(
                ExecutableLedgerValidationError::AllocationRecordNotLinkWritable {
                    protection: ExecutableMemoryProtection::Executable,
                    lifecycle: ExecutableAllocationLifecycle::JettisonPending,
                },
            )
        );
    }

    #[test]
    fn link_buffer_copy_link_accepts_frozen_byte_image_without_bytes() {
        let record = accepted_copy_link(64);

        assert_eq!(record.outcome, LinkBufferCopyLinkOutcome::Accepted);
        assert_eq!(record.state, LinkBufferState::Linked);
        assert_eq!(record.source_byte_len, 64);
        assert_eq!(record.profile, Some(LinkBufferProfile::Baseline));
        assert_eq!(record.byte_copy_evidence, None);
        assert_eq!(record.validate_accepted(), Ok(()));
    }

    #[test]
    fn link_buffer_copy_link_accepts_linked_byte_image_with_identity_evidence() {
        let buffer = frozen_buffer(64);
        let allocation = allocation(64);
        let layout = plan_link_buffer_layout(
            &buffer,
            LinkBufferProfile::Baseline,
            Some(allocation.allocation),
        )
        .unwrap();
        let image = source_byte_image(&buffer);
        let linked_image = link_assembler_byte_image(&image, &layout).unwrap();

        let record = linked_copy_link_record_for(
            &buffer,
            &allocation,
            &layout,
            JitCodeId(9),
            &image,
            &linked_image,
        );
        let evidence = record.byte_copy_evidence.expect("byte-copy evidence");

        assert_eq!(record.outcome, LinkBufferCopyLinkOutcome::Accepted);
        assert_eq!(record.state, LinkBufferState::Linked);
        assert_eq!(record.source_image, image.id());
        assert_eq!(record.digest, image.digest());
        assert_eq!(evidence.source_image, image.id());
        assert_eq!(evidence.source_digest, image.digest());
        assert_eq!(evidence.output_digest, linked_image.output_digest);
        assert_eq!(evidence.output_byte_len, 64);
        assert_eq!(evidence.relocation_count, 0);
        assert_eq!(evidence.profile, LinkBufferProfile::Baseline);
        assert_eq!(evidence.state, LinkBufferState::Linked);
        assert_eq!(record.validate_accepted(), Ok(()));
        assert_eq!(
            record.validate_accepted_with_byte_evidence().copied(),
            Ok(evidence)
        );
    }

    #[test]
    fn link_buffer_copy_link_rejects_linked_image_identity_mismatches() {
        {
            let (buffer, allocation, layout, image, mut linked_image) = linked_copy_link_parts(16);
            linked_image.source_image_id = AssemblerByteImageId(999);
            let record = linked_copy_link_record_for(
                &buffer,
                &allocation,
                &layout,
                JitCodeId(9),
                &image,
                &linked_image,
            );

            assert_eq!(
                record.outcome,
                LinkBufferCopyLinkOutcome::Rejected {
                    reason: ExecutableLedgerValidationError::LinkedImageSourceImageMismatch {
                        expected: image.id(),
                        actual: AssemblerByteImageId(999),
                    },
                }
            );
            assert_eq!(record.byte_copy_evidence, None);
        }

        {
            let (buffer, allocation, layout, image, mut linked_image) = linked_copy_link_parts(16);
            linked_image.source_image_digest = AssemblerByteImageDigest(0xdead);
            let record = linked_copy_link_record_for(
                &buffer,
                &allocation,
                &layout,
                JitCodeId(9),
                &image,
                &linked_image,
            );

            assert_eq!(
                record.outcome,
                LinkBufferCopyLinkOutcome::Rejected {
                    reason: ExecutableLedgerValidationError::LinkedImageSourceDigestMismatch {
                        expected: image.digest(),
                        actual: AssemblerByteImageDigest(0xdead),
                    },
                }
            );
            assert_eq!(record.byte_copy_evidence, None);
        }

        {
            let (buffer, allocation, layout, image, mut linked_image) = linked_copy_link_parts(16);
            let actual_digest = AssemblerByteImageDigest(linked_image.output_digest.0 ^ 0xfeed);
            linked_image.output_digest = actual_digest;
            let record = linked_copy_link_record_for(
                &buffer,
                &allocation,
                &layout,
                JitCodeId(9),
                &image,
                &linked_image,
            );

            assert_eq!(
                record.outcome,
                LinkBufferCopyLinkOutcome::Rejected {
                    reason: ExecutableLedgerValidationError::LinkedImageOutputDigestMismatch {
                        expected: compute_assembler_byte_image_digest(linked_image.bytes()),
                        actual: actual_digest,
                    },
                }
            );
            assert_eq!(record.byte_copy_evidence, None);
        }

        {
            let (buffer, allocation, layout, image, mut linked_image) = linked_copy_link_parts(16);
            linked_image.output_size_bytes = 15;
            let record = linked_copy_link_record_for(
                &buffer,
                &allocation,
                &layout,
                JitCodeId(9),
                &image,
                &linked_image,
            );

            assert_eq!(
                record.outcome,
                LinkBufferCopyLinkOutcome::Rejected {
                    reason: ExecutableLedgerValidationError::LinkedImageOutputByteLengthMismatch {
                        expected: 16,
                        actual: 15,
                    },
                }
            );
            assert_eq!(record.byte_copy_evidence, None);
        }

        {
            let (buffer, allocation, layout, image, mut linked_image) = linked_copy_link_parts(16);
            linked_image.relocation_count = 1;
            let record = linked_copy_link_record_for(
                &buffer,
                &allocation,
                &layout,
                JitCodeId(9),
                &image,
                &linked_image,
            );

            assert_eq!(
                record.outcome,
                LinkBufferCopyLinkOutcome::Rejected {
                    reason: ExecutableLedgerValidationError::LinkedImageRelocationCountMismatch {
                        expected: 0,
                        actual: 1,
                    },
                }
            );
            assert_eq!(record.byte_copy_evidence, None);
        }

        {
            let (buffer, allocation, layout, image, mut linked_image) = linked_copy_link_parts(16);
            linked_image.profile = LinkBufferProfile::Dfg;
            let record = linked_copy_link_record_for(
                &buffer,
                &allocation,
                &layout,
                JitCodeId(9),
                &image,
                &linked_image,
            );

            assert_eq!(
                record.outcome,
                LinkBufferCopyLinkOutcome::Rejected {
                    reason: ExecutableLedgerValidationError::LinkedImageProfileMismatch {
                        expected: LinkBufferProfile::Baseline,
                        actual: LinkBufferProfile::Dfg,
                    },
                }
            );
            assert_eq!(record.byte_copy_evidence, None);
        }

        {
            let (buffer, allocation, layout, image, mut linked_image) = linked_copy_link_parts(16);
            linked_image.state = LinkBufferState::Finalized;
            let record = linked_copy_link_record_for(
                &buffer,
                &allocation,
                &layout,
                JitCodeId(9),
                &image,
                &linked_image,
            );

            assert_eq!(
                record.outcome,
                LinkBufferCopyLinkOutcome::Rejected {
                    reason: ExecutableLedgerValidationError::LinkedImageStateMismatch {
                        actual: LinkBufferState::Finalized,
                    },
                }
            );
            assert_eq!(record.byte_copy_evidence, None);
        }
    }

    #[test]
    fn link_buffer_copy_link_rejects_image_layout_source_mismatch() {
        let buffer = frozen_buffer(16);
        let image = source_image(&buffer);
        let other_buffer = AssemblerBufferDescriptor::builder(AssemblerBufferId(99))
            .architecture(AssemblerArchitecture::X86_64)
            .lifecycle(AssemblerBufferLifecycle::FrozenForLink)
            .capacity(16, 16)
            .build()
            .unwrap();
        let allocation = allocation(16);
        let layout = plan_link_buffer_layout(
            &other_buffer,
            LinkBufferProfile::Baseline,
            Some(allocation.allocation),
        )
        .unwrap();

        let record = record_link_buffer_copy_link(LinkBufferCopyLinkRequest {
            source: &buffer,
            source_image: &image,
            layout: &layout,
            allocation: &allocation,
            code_id: JitCodeId(9),
            owner: MachineCodeOwnership::CodeBlock(owner()),
        });

        assert_eq!(
            record.outcome,
            LinkBufferCopyLinkOutcome::Rejected {
                reason: ExecutableLedgerValidationError::LayoutSourceMismatch {
                    expected: AssemblerBufferId(1),
                    actual: AssemblerBufferId(99),
                },
            }
        );
    }

    #[test]
    fn link_buffer_copy_link_rejects_allocation_size_mismatch() {
        let buffer = frozen_buffer(32);
        let allocation = allocation(16);
        let layout = plan_link_buffer_layout(
            &buffer,
            LinkBufferProfile::Baseline,
            Some(allocation.allocation),
        )
        .unwrap();
        let record = copy_link_record(&buffer, &allocation, &layout, JitCodeId(9));

        assert_eq!(
            record.outcome,
            LinkBufferCopyLinkOutcome::Rejected {
                reason: ExecutableLedgerValidationError::AllocationCodeSizeMismatch {
                    expected: 32,
                    actual: 16,
                },
            }
        );
    }

    #[test]
    fn link_buffer_finalization_produces_executable_linked_handle_without_bytes() {
        let record = accepted_finalization(64);
        let machine = record.machine_code.expect("machine-code descriptor");

        assert_eq!(record.outcome, LinkBufferFinalizationOutcome::Accepted);
        assert_eq!(record.state, LinkBufferState::Finalized);
        assert_eq!(record.profile, Some(LinkBufferProfile::Baseline));
        assert_eq!(
            record.permission_transition,
            Some(JitPermissionTransition::RwToRx)
        );
        assert_eq!(record.accepted_machine_code(), Some(machine));
        assert_eq!(record.validate_accepted(), Ok(machine));
        assert_eq!(machine.protection, ExecutableMemoryProtection::Executable);
        assert_eq!(
            machine.lifecycle,
            ExecutableAllocationLifecycle::LinkedExecutable
        );
        assert_eq!(
            machine.mutation_authority,
            ExecutableMutationAuthority::LinkBuffer
        );
        assert_eq!(record.accepted_code_ref().unwrap().size_bytes, 64);
    }

    #[test]
    fn link_buffer_finalization_carries_and_strictly_validates_byte_evidence() {
        let record = accepted_linked_finalization(64);
        let machine = record.machine_code.expect("machine-code descriptor");
        let evidence = record.byte_copy_evidence.expect("finalization evidence");

        assert_eq!(record.outcome, LinkBufferFinalizationOutcome::Accepted);
        assert_eq!(record.copy_link.byte_copy_evidence, Some(evidence));
        assert_eq!(record.validate_accepted(), Ok(machine));
        assert_eq!(record.validate_accepted_with_byte_evidence(), Ok(machine));

        let mut tampered_copy_link = accepted_linked_copy_link(16);
        tampered_copy_link
            .byte_copy_evidence
            .as_mut()
            .expect("copy-link evidence")
            .output_byte_len = 15;
        assert_eq!(
            tampered_copy_link.validate_accepted_with_byte_evidence(),
            Err(ExecutableLedgerValidationError::CopyLinkByteEvidenceProofMismatch)
        );

        let mut missing_copy_evidence = accepted_linked_copy_link(16);
        missing_copy_evidence.byte_copy_evidence = None;
        assert_eq!(
            missing_copy_evidence.validate_accepted_with_byte_evidence(),
            Err(ExecutableLedgerValidationError::CopyLinkByteEvidenceMissing)
        );

        let mut tampered_finalization = record.clone();
        tampered_finalization
            .byte_copy_evidence
            .as_mut()
            .expect("finalization evidence")
            .output_byte_len = 63;
        assert_eq!(
            tampered_finalization.validate_accepted_with_byte_evidence(),
            Err(
                ExecutableLedgerValidationError::FinalizationByteEvidenceMismatch {
                    expected: Box::new(record.copy_link.byte_copy_evidence),
                    actual: Box::new(tampered_finalization.byte_copy_evidence),
                }
            )
        );

        let mut missing_finalization_evidence = record.clone();
        missing_finalization_evidence.byte_copy_evidence = None;
        assert_eq!(
            missing_finalization_evidence.validate_accepted_with_byte_evidence(),
            Err(ExecutableLedgerValidationError::FinalizationByteEvidenceMissing)
        );

        let mut missing_carried_copy_evidence = record;
        missing_carried_copy_evidence.copy_link.byte_copy_evidence = None;
        assert_eq!(
            missing_carried_copy_evidence.validate_accepted_with_byte_evidence(),
            Err(ExecutableLedgerValidationError::CopyLinkByteEvidenceMissing)
        );
    }

    #[test]
    fn link_buffer_finalization_rejects_unfrozen_source_and_missing_symbol() {
        let frozen = AssemblerBufferDescriptor::builder(AssemblerBufferId(2))
            .architecture(AssemblerArchitecture::X86_64)
            .lifecycle(AssemblerBufferLifecycle::FrozenForLink)
            .capacity(16, 16)
            .build()
            .unwrap();
        let image = source_image(&frozen);
        let building = AssemblerBufferDescriptor::builder(AssemblerBufferId(2))
            .architecture(AssemblerArchitecture::X86_64)
            .capacity(16, 16)
            .build()
            .unwrap();
        let allocation = ExecutableAllocationRecord::from_request(
            ExecutableAllocationRequest::allocated_writable(
                ExecutableAllocationId(12),
                MachineCodeOwnership::CodeBlock(owner()),
                16,
            ),
        );
        let layout = plan_link_buffer_layout(
            &building,
            LinkBufferProfile::Baseline,
            Some(allocation.allocation),
        )
        .unwrap();
        let unfrozen = record_link_buffer_copy_link(LinkBufferCopyLinkRequest {
            source: &building,
            source_image: &image,
            layout: &layout,
            allocation: &allocation,
            code_id: JitCodeId(12),
            owner: MachineCodeOwnership::CodeBlock(owner()),
        });
        assert_eq!(
            unfrozen.outcome,
            LinkBufferCopyLinkOutcome::Rejected {
                reason: ExecutableLedgerValidationError::SourceBufferNotFrozen {
                    actual: AssemblerBufferLifecycle::Building,
                },
            }
        );

        let copy_link = accepted_copy_link(16);
        let missing_symbol = finalize_link_buffer(LinkBufferFinalizationRequest {
            copy_link: &copy_link,
            code_id: JitCodeId(9),
            owner: MachineCodeOwnership::CodeBlock(owner()),
            symbol: None,
        });
        assert_eq!(
            missing_symbol.outcome,
            LinkBufferFinalizationOutcome::Rejected {
                reason: ExecutableLedgerValidationError::NativeSymbolMissing,
            }
        );
    }

    #[test]
    fn link_buffer_copy_link_rejects_patch_application_states() {
        let buffer = frozen_buffer(16);
        let allocation = allocation(16);
        let mut layout = plan_link_buffer_layout(
            &buffer,
            LinkBufferProfile::Baseline,
            Some(allocation.allocation),
        )
        .unwrap();
        layout.plan.patches.push(CodePatchPlan {
            owner: owner(),
            records: vec![CodePatchRecord {
                code: JitCodeId(9),
                location: PatchpointDescriptor {
                    kind: PatchpointKind::Entrypoint,
                    owner_code: Some(JitCodeId(9)),
                    byte_offset: Some(0),
                    boundary: None,
                },
                relocation: RelocationKind::Entrypoint,
                range: Some(MachineCodeRange {
                    allocation: allocation.allocation,
                    start_offset: 0,
                    size_bytes: 4,
                }),
                boundary: None,
                state: CodePatchState::Applied,
            }],
            write_barrier: PatchWriteBarrier::ExecutableAllocatorLock,
            required_protection: ExecutableMemoryProtection::WritableForPatching,
            generation: 1,
        });

        let record = copy_link_record(&buffer, &allocation, &layout, JitCodeId(9));

        assert_eq!(
            record.outcome,
            LinkBufferCopyLinkOutcome::Rejected {
                reason: ExecutableLedgerValidationError::PatchPlanStateNotFinalizationSafe {
                    state: CodePatchState::Applied,
                },
            }
        );
    }

    #[test]
    fn link_buffer_finalization_requires_accepted_copy_link_record() {
        let buffer = frozen_buffer(16);
        let allocation = allocation(32);
        let layout = plan_link_buffer_layout(
            &buffer,
            LinkBufferProfile::Baseline,
            Some(allocation.allocation),
        )
        .unwrap();
        let copy_link = copy_link_record(&buffer, &allocation, &layout, JitCodeId(9));
        assert!(matches!(
            copy_link.outcome,
            LinkBufferCopyLinkOutcome::Rejected { .. }
        ));

        let record = finalize_link_buffer(LinkBufferFinalizationRequest {
            copy_link: &copy_link,
            code_id: JitCodeId(9),
            owner: MachineCodeOwnership::CodeBlock(owner()),
            symbol: Some(NativeCodeId(10)),
        });

        assert!(matches!(
            record.outcome,
            LinkBufferFinalizationOutcome::Rejected {
                reason: ExecutableLedgerValidationError::CopyLinkRecordRejected { .. },
            }
        ));
    }
}
