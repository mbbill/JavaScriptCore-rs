//! Executable-memory residency proofs.
//!
//! This module is the platform-facing layer below VM executable
//! materialization. Most helpers here are data-only records for executable
//! memory allocation, write-to-execute protection transitions, and
//! instruction-cache flush markers. The platform-backed residency bridge at
//! the bottom of the module ties those records to the safe W^X compartment
//! lifecycle without exposing callable addresses or invoking generated code.
//!
//! The records here contain no machine-code bytes, expose no raw pointers or
//! executable addresses, flush no real caches, and make no native calls.
//! `NativeCodeId` remains an identity token.

use crate::assembler::{
    compute_assembler_byte_image_digest, AssemblerByteImageDigest, AssemblerByteImageId,
    JitPermissionTransition, LinkBufferProfile, LinkBufferState,
};
use crate::jit::executable::LinkBufferByteCopyEvidence;
use crate::jit::{
    ExecutableAllocationId, ExecutableAllocationLifecycle, ExecutableAllocationRecord,
    ExecutableLedgerValidationError, ExecutableMemoryProtection, ExecutableMutationAuthority,
    JitCodeId, LinkBufferFinalizationOutcome, LinkBufferFinalizationRecord, MachineCodeHandle,
    MachineCodeOwnership, MachineCodeRange, MachineCodeValidationError,
};
use crate::runtime::NativeCodeId;

use super::executable_memory_compartment::{
    ExecutableMemoryCompartment, ExecutableMemoryCompartmentError,
    ExecutableMemoryCompartmentRequest,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutableMemoryPageSize(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutableMemoryMappedRange {
    pub allocation: ExecutableAllocationId,
    pub start_offset: u32,
    pub byte_len: u32,
    pub page_size: ExecutableMemoryPageSize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExecutableMemoryOperationOrdinal(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutableMemoryByteCopyEvidence {
    pub source_image: AssemblerByteImageId,
    pub source_digest: AssemblerByteImageDigest,
    pub copied_digest: AssemblerByteImageDigest,
    pub copied_byte_len: u32,
    pub relocation_count: usize,
    pub profile: LinkBufferProfile,
    pub state: LinkBufferState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutableMemoryAllocationRequest {
    pub ordinal: ExecutableMemoryOperationOrdinal,
    pub allocation: ExecutableAllocationRecord,
    pub mapped_range: ExecutableMemoryMappedRange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutableMemoryAllocationRecord {
    pub ordinal: ExecutableMemoryOperationOrdinal,
    pub allocation: ExecutableAllocationId,
    pub owner: MachineCodeOwnership,
    pub allocation_range: MachineCodeRange,
    pub mapped_range: ExecutableMemoryMappedRange,
    pub protection: ExecutableMemoryProtection,
    pub lifecycle: ExecutableAllocationLifecycle,
    pub mutation_authority: ExecutableMutationAuthority,
    pub outcome: ExecutableMemoryResidencyOutcome,
    proof: ExecutableMemoryOperationProof,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutableMemoryByteCopyRequest<'a> {
    pub ordinal: ExecutableMemoryOperationOrdinal,
    pub allocation: &'a ExecutableMemoryAllocationRecord,
    pub link_finalization: &'a LinkBufferFinalizationRecord,
    pub copied_range: MachineCodeRange,
    pub copied_byte_len: u32,
    pub copied_digest: AssemblerByteImageDigest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutableMemoryByteCopyRecord {
    pub ordinal: ExecutableMemoryOperationOrdinal,
    pub allocation_ordinal: ExecutableMemoryOperationOrdinal,
    pub allocation: ExecutableAllocationId,
    pub owner: MachineCodeOwnership,
    pub code_id: JitCodeId,
    pub mapped_range: ExecutableMemoryMappedRange,
    pub copied_range: MachineCodeRange,
    pub copied_byte_len: u32,
    pub copied_digest: AssemblerByteImageDigest,
    pub byte_copy_evidence: Option<ExecutableMemoryByteCopyEvidence>,
    pub outcome: ExecutableMemoryResidencyOutcome,
    proof: ExecutableMemoryOperationProof,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutableMemoryProtectionTransitionRequest {
    pub ordinal: ExecutableMemoryOperationOrdinal,
    pub allocation: ExecutableMemoryAllocationRecord,
    pub transition: JitPermissionTransition,
    pub start_protection: ExecutableMemoryProtection,
    pub start_lifecycle: ExecutableAllocationLifecycle,
    pub end_protection: ExecutableMemoryProtection,
    pub end_lifecycle: ExecutableAllocationLifecycle,
    pub mutation_authority: ExecutableMutationAuthority,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutableMemoryProtectionTransitionRecord {
    pub ordinal: ExecutableMemoryOperationOrdinal,
    pub allocation_ordinal: ExecutableMemoryOperationOrdinal,
    pub allocation: ExecutableAllocationId,
    pub owner: MachineCodeOwnership,
    pub mapped_range: ExecutableMemoryMappedRange,
    pub transition: JitPermissionTransition,
    pub start_protection: ExecutableMemoryProtection,
    pub start_lifecycle: ExecutableAllocationLifecycle,
    pub end_protection: ExecutableMemoryProtection,
    pub end_lifecycle: ExecutableAllocationLifecycle,
    pub mutation_authority: ExecutableMutationAuthority,
    pub outcome: ExecutableMemoryResidencyOutcome,
    proof: ExecutableMemoryOperationProof,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstructionCacheFlushRequest {
    pub ordinal: ExecutableMemoryOperationOrdinal,
    pub transition: ExecutableMemoryProtectionTransitionRecord,
    pub code_id: JitCodeId,
    pub owner: MachineCodeOwnership,
    pub allocation: ExecutableAllocationId,
    pub range: MachineCodeRange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstructionCacheFlushRecord {
    pub ordinal: ExecutableMemoryOperationOrdinal,
    pub transition_ordinal: ExecutableMemoryOperationOrdinal,
    pub code_id: JitCodeId,
    pub owner: MachineCodeOwnership,
    pub allocation: ExecutableAllocationId,
    pub range: MachineCodeRange,
    pub outcome: ExecutableMemoryResidencyOutcome,
    proof: ExecutableMemoryOperationProof,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutableMemoryResidencyRequest<'a> {
    pub link_finalization: &'a LinkBufferFinalizationRecord,
    pub allocation: &'a ExecutableMemoryAllocationRecord,
    pub protection_transition: &'a ExecutableMemoryProtectionTransitionRecord,
    pub instruction_cache_flush: Option<&'a InstructionCacheFlushRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutableMemoryByteResidencyRequest<'a> {
    pub ordinal: ExecutableMemoryOperationOrdinal,
    pub link_finalization: &'a LinkBufferFinalizationRecord,
    pub allocation: &'a ExecutableMemoryAllocationRecord,
    pub byte_copy: &'a ExecutableMemoryByteCopyRecord,
    pub protection_transition: &'a ExecutableMemoryProtectionTransitionRecord,
    pub instruction_cache_flush: Option<&'a InstructionCacheFlushRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutableMemoryPlatformResidencyRequest<'a> {
    pub first_ordinal: ExecutableMemoryOperationOrdinal,
    pub link_finalization: &'a LinkBufferFinalizationRecord,
    pub linked_bytes: &'a [u8],
}

#[derive(Debug)]
pub struct ExecutableMemoryPlatformHandleEvidence {
    pub compartment: ExecutableMemoryCompartment,
    pub finalized_machine_code: MachineCodeHandle,
    pub platform_machine_code: MachineCodeHandle,
}

#[derive(Debug)]
pub struct ExecutableMemoryPlatformResidency {
    pub residency: ExecutableMemoryResidencyRecord,
    pub evidence: ExecutableMemoryPlatformHandleEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutableMemoryPlatformResidencyError {
    OperationOrdinalOverflow {
        first_ordinal: ExecutableMemoryOperationOrdinal,
    },
    Compartment {
        error: ExecutableMemoryCompartmentError,
    },
    ResidencyValidation {
        reason: ExecutableMemoryResidencyValidationError,
    },
}

impl<'a> ExecutableMemoryPlatformResidencyRequest<'a> {
    pub const fn new(
        first_ordinal: ExecutableMemoryOperationOrdinal,
        link_finalization: &'a LinkBufferFinalizationRecord,
        linked_bytes: &'a [u8],
    ) -> Self {
        Self {
            first_ordinal,
            link_finalization,
            linked_bytes,
        }
    }
}

impl From<ExecutableMemoryCompartmentError> for ExecutableMemoryPlatformResidencyError {
    fn from(error: ExecutableMemoryCompartmentError) -> Self {
        Self::Compartment { error }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutableMemoryResidencyRecord {
    pub ordinal: Option<ExecutableMemoryOperationOrdinal>,
    pub code_id: JitCodeId,
    pub owner: MachineCodeOwnership,
    pub symbol: Option<NativeCodeId>,
    pub allocation: ExecutableAllocationId,
    pub machine_range: MachineCodeRange,
    pub mapped_range: ExecutableMemoryMappedRange,
    pub final_protection: ExecutableMemoryProtection,
    pub final_lifecycle: ExecutableAllocationLifecycle,
    pub allocation_record: ExecutableMemoryAllocationRecord,
    pub byte_copy: Option<ExecutableMemoryByteCopyRecord>,
    pub protection_transition: ExecutableMemoryProtectionTransitionRecord,
    pub instruction_cache_flush: Option<InstructionCacheFlushRecord>,
    pub outcome: ExecutableMemoryResidencyOutcome,
    proof: ExecutableMemoryResidencyProof,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutableMemoryResidencyOutcome {
    Accepted,
    Rejected {
        reason: ExecutableMemoryResidencyValidationError,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutableMemoryResidencyValidationError {
    OperationOrdinalZero,
    OperationOrderingInvalid {
        before: ExecutableMemoryOperationOrdinal,
        after: ExecutableMemoryOperationOrdinal,
    },
    PageSizeZero,
    PageSizeNotPowerOfTwo {
        page_size: ExecutableMemoryPageSize,
    },
    MappedRangeEmpty,
    MappedRangeEndOverflow {
        range: ExecutableMemoryMappedRange,
    },
    MappedRangeStartNotPageAligned {
        start_offset: u32,
        page_size: ExecutableMemoryPageSize,
    },
    MappedRangeLengthNotPageMultiple {
        byte_len: u32,
        page_size: ExecutableMemoryPageSize,
    },
    MappedRangeAllocationMismatch {
        expected: ExecutableAllocationId,
        actual: ExecutableAllocationId,
    },
    MappedRangeDoesNotContainMachineRange {
        mapped_range: ExecutableMemoryMappedRange,
        machine_range: MachineCodeRange,
    },
    AllocationLedgerRejected {
        reason: Box<ExecutableLedgerValidationError>,
    },
    AllocationLedgerInvalid {
        reason: Box<ExecutableLedgerValidationError>,
    },
    AllocationRecordRejected {
        reason: Box<ExecutableMemoryResidencyValidationError>,
    },
    AllocationProofMissing,
    AllocationRangeInvalid {
        error: MachineCodeValidationError,
    },
    AllocationRangeMismatch {
        expected: MachineCodeRange,
        actual: MachineCodeRange,
    },
    AllocationOwnerMismatch {
        expected: MachineCodeOwnership,
        actual: MachineCodeOwnership,
    },
    AllocationIdMismatch {
        expected: ExecutableAllocationId,
        actual: ExecutableAllocationId,
    },
    AllocationStartStateMismatch {
        protection: ExecutableMemoryProtection,
        lifecycle: ExecutableAllocationLifecycle,
    },
    MutationAuthorityMismatch {
        expected: ExecutableMutationAuthority,
        actual: ExecutableMutationAuthority,
    },
    ByteCopyMissing,
    ByteCopyRecordRejected {
        reason: Box<ExecutableMemoryResidencyValidationError>,
    },
    ByteCopyProofMissing,
    ByteCopyEvidenceMissing,
    ByteCopyEvidenceMismatch {
        expected: Box<Option<ExecutableMemoryByteCopyEvidence>>,
        actual: Box<Option<ExecutableMemoryByteCopyEvidence>>,
    },
    ByteCopyCodeIdMismatch {
        expected: JitCodeId,
        actual: JitCodeId,
    },
    ByteCopyOwnerMismatch {
        expected: MachineCodeOwnership,
        actual: MachineCodeOwnership,
    },
    ByteCopyAllocationMismatch {
        expected: ExecutableAllocationId,
        actual: ExecutableAllocationId,
    },
    ByteCopyAllocationRangeMismatch {
        expected: MachineCodeRange,
        actual: MachineCodeRange,
    },
    ByteCopyRangeMismatch {
        expected: MachineCodeRange,
        actual: MachineCodeRange,
    },
    ByteCopyMappedRangeMismatch {
        expected: ExecutableMemoryMappedRange,
        actual: ExecutableMemoryMappedRange,
    },
    ByteCopyLengthMismatch {
        expected: u32,
        actual: u32,
    },
    ByteCopyDigestMismatch {
        expected: AssemblerByteImageDigest,
        actual: AssemblerByteImageDigest,
    },
    TransitionRecordRejected {
        reason: Box<ExecutableMemoryResidencyValidationError>,
    },
    TransitionProofMissing,
    TransitionRangeMismatch {
        expected: ExecutableMemoryMappedRange,
        actual: ExecutableMemoryMappedRange,
    },
    TransitionOwnerMismatch {
        expected: MachineCodeOwnership,
        actual: MachineCodeOwnership,
    },
    TransitionStartStateMismatch {
        protection: ExecutableMemoryProtection,
        lifecycle: ExecutableAllocationLifecycle,
    },
    TransitionEndStateMismatch {
        protection: ExecutableMemoryProtection,
        lifecycle: ExecutableAllocationLifecycle,
    },
    PermissionTransitionMismatch {
        expected: JitPermissionTransition,
        actual: JitPermissionTransition,
    },
    CacheFlushMissing,
    CacheFlushRecordRejected {
        reason: Box<ExecutableMemoryResidencyValidationError>,
    },
    CacheFlushProofMissing,
    CacheFlushCodeIdMismatch {
        expected: JitCodeId,
        actual: JitCodeId,
    },
    CacheFlushOwnerMismatch {
        expected: MachineCodeOwnership,
        actual: MachineCodeOwnership,
    },
    CacheFlushAllocationMismatch {
        expected: ExecutableAllocationId,
        actual: ExecutableAllocationId,
    },
    CacheFlushRangeMismatch {
        expected: MachineCodeRange,
        actual: MachineCodeRange,
    },
    LinkFinalizationRejected {
        reason: Box<ExecutableLedgerValidationError>,
    },
    LinkFinalizationInvalid {
        reason: Box<ExecutableLedgerValidationError>,
    },
    LinkFinalizationTransitionMismatch {
        actual: Option<JitPermissionTransition>,
    },
    ResidencyRecordRejected {
        reason: Box<ExecutableMemoryResidencyValidationError>,
    },
    ResidencyProofMissing,
    ResidencyOrdinalMissing,
    ResidencyCodeIdMismatch {
        expected: JitCodeId,
        actual: JitCodeId,
    },
    ResidencyOwnerMismatch {
        expected: MachineCodeOwnership,
        actual: MachineCodeOwnership,
    },
    ResidencyNativeSymbolMismatch {
        expected: Option<NativeCodeId>,
        actual: Option<NativeCodeId>,
    },
    ResidencyAllocationMismatch {
        expected: ExecutableAllocationId,
        actual: ExecutableAllocationId,
    },
    ResidencyMachineRangeMismatch {
        expected: MachineCodeRange,
        actual: MachineCodeRange,
    },
    ResidencyMachineCodeMismatch {
        expected: MachineCodeHandle,
        actual: MachineCodeHandle,
    },
    FinalProtectionMismatch {
        expected: ExecutableMemoryProtection,
        actual: ExecutableMemoryProtection,
    },
    FinalLifecycleMismatch {
        expected: ExecutableAllocationLifecycle,
        actual: ExecutableAllocationLifecycle,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ExecutableMemoryOperationProof {
    accepted: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ExecutableMemoryResidencyProof {
    link_finalization_verified: bool,
    allocation_verified: bool,
    byte_copy_verified: bool,
    protection_transition_verified: bool,
    cache_flush_verified: bool,
    residency_ordinal_verified: bool,
    machine_code_verified: bool,
}

impl ExecutableMemoryMappedRange {
    pub fn validate(&self) -> Result<(), ExecutableMemoryResidencyValidationError> {
        let page_size = self.page_size.0;
        if page_size == 0 {
            return Err(ExecutableMemoryResidencyValidationError::PageSizeZero);
        }
        if !page_size.is_power_of_two() {
            return Err(
                ExecutableMemoryResidencyValidationError::PageSizeNotPowerOfTwo {
                    page_size: self.page_size,
                },
            );
        }
        if self.byte_len == 0 {
            return Err(ExecutableMemoryResidencyValidationError::MappedRangeEmpty);
        }
        if self.end_offset().is_none() {
            return Err(
                ExecutableMemoryResidencyValidationError::MappedRangeEndOverflow { range: *self },
            );
        }
        if !self.start_offset.is_multiple_of(page_size) {
            return Err(
                ExecutableMemoryResidencyValidationError::MappedRangeStartNotPageAligned {
                    start_offset: self.start_offset,
                    page_size: self.page_size,
                },
            );
        }
        if !self.byte_len.is_multiple_of(page_size) {
            return Err(
                ExecutableMemoryResidencyValidationError::MappedRangeLengthNotPageMultiple {
                    byte_len: self.byte_len,
                    page_size: self.page_size,
                },
            );
        }
        Ok(())
    }

    pub fn end_offset(self) -> Option<u32> {
        self.start_offset.checked_add(self.byte_len)
    }

    pub fn contains_machine_range(
        self,
        range: MachineCodeRange,
    ) -> Result<(), ExecutableMemoryResidencyValidationError> {
        range.validate().map_err(|error| {
            ExecutableMemoryResidencyValidationError::AllocationRangeInvalid { error }
        })?;
        if range.allocation != self.allocation {
            return Err(
                ExecutableMemoryResidencyValidationError::MappedRangeAllocationMismatch {
                    expected: self.allocation,
                    actual: range.allocation,
                },
            );
        }
        let Some(mapped_end) = self.end_offset() else {
            return Err(
                ExecutableMemoryResidencyValidationError::MappedRangeEndOverflow { range: self },
            );
        };
        let Some(range_end) = range.end_offset() else {
            return Err(
                ExecutableMemoryResidencyValidationError::AllocationRangeInvalid {
                    error: MachineCodeValidationError::RangeEndOverflow,
                },
            );
        };
        if range.start_offset < self.start_offset || range_end > mapped_end {
            return Err(
                ExecutableMemoryResidencyValidationError::MappedRangeDoesNotContainMachineRange {
                    mapped_range: self,
                    machine_range: range,
                },
            );
        }
        Ok(())
    }
}

impl ExecutableMemoryResidencyRecord {
    pub fn validate_accepted_for_link_finalization(
        &self,
        link_finalization: &LinkBufferFinalizationRecord,
    ) -> Result<MachineCodeHandle, ExecutableMemoryResidencyValidationError> {
        match &self.outcome {
            ExecutableMemoryResidencyOutcome::Accepted => {}
            ExecutableMemoryResidencyOutcome::Rejected { reason } => {
                return Err(
                    ExecutableMemoryResidencyValidationError::ResidencyRecordRejected {
                        reason: Box::new(reason.clone()),
                    },
                );
            }
        }
        if self.proof
            != (ExecutableMemoryResidencyProof {
                link_finalization_verified: true,
                allocation_verified: true,
                byte_copy_verified: self.byte_copy.is_some(),
                protection_transition_verified: true,
                cache_flush_verified: true,
                residency_ordinal_verified: self.ordinal.is_some(),
                machine_code_verified: true,
            })
        {
            return Err(ExecutableMemoryResidencyValidationError::ResidencyProofMissing);
        }

        validate_residency_record_for_link_finalization(self, link_finalization, false)
    }

    pub fn validate_accepted_with_byte_evidence(
        &self,
        link_finalization: &LinkBufferFinalizationRecord,
    ) -> Result<MachineCodeHandle, ExecutableMemoryResidencyValidationError> {
        match &self.outcome {
            ExecutableMemoryResidencyOutcome::Accepted => {}
            ExecutableMemoryResidencyOutcome::Rejected { reason } => {
                return Err(
                    ExecutableMemoryResidencyValidationError::ResidencyRecordRejected {
                        reason: Box::new(reason.clone()),
                    },
                );
            }
        }
        if self.byte_copy.is_none() {
            return Err(ExecutableMemoryResidencyValidationError::ByteCopyMissing);
        }
        if self.ordinal.is_none() {
            return Err(ExecutableMemoryResidencyValidationError::ResidencyOrdinalMissing);
        }
        if self.proof
            != (ExecutableMemoryResidencyProof {
                link_finalization_verified: true,
                allocation_verified: true,
                byte_copy_verified: true,
                protection_transition_verified: true,
                cache_flush_verified: true,
                residency_ordinal_verified: true,
                machine_code_verified: true,
            })
        {
            return Err(ExecutableMemoryResidencyValidationError::ResidencyProofMissing);
        }

        validate_residency_record_for_link_finalization(self, link_finalization, true)
    }
}

pub fn record_executable_memory_allocation(
    request: ExecutableMemoryAllocationRequest,
) -> ExecutableMemoryAllocationRecord {
    let outcome = match validate_executable_memory_allocation_request(&request) {
        Ok(()) => ExecutableMemoryResidencyOutcome::Accepted,
        Err(reason) => ExecutableMemoryResidencyOutcome::Rejected { reason },
    };
    let accepted = outcome == ExecutableMemoryResidencyOutcome::Accepted;

    ExecutableMemoryAllocationRecord {
        ordinal: request.ordinal,
        allocation: request.allocation.allocation,
        owner: request.allocation.owner,
        allocation_range: request.allocation.range,
        mapped_range: request.mapped_range,
        protection: request.allocation.protection,
        lifecycle: request.allocation.lifecycle,
        mutation_authority: request.allocation.mutation_authority,
        outcome,
        proof: ExecutableMemoryOperationProof { accepted },
    }
}

pub fn record_executable_memory_protection_transition(
    request: ExecutableMemoryProtectionTransitionRequest,
) -> ExecutableMemoryProtectionTransitionRecord {
    let outcome = match validate_executable_memory_protection_transition_request(&request) {
        Ok(()) => ExecutableMemoryResidencyOutcome::Accepted,
        Err(reason) => ExecutableMemoryResidencyOutcome::Rejected { reason },
    };
    let accepted = outcome == ExecutableMemoryResidencyOutcome::Accepted;

    ExecutableMemoryProtectionTransitionRecord {
        ordinal: request.ordinal,
        allocation_ordinal: request.allocation.ordinal,
        allocation: request.allocation.allocation,
        owner: request.allocation.owner,
        mapped_range: request.allocation.mapped_range,
        transition: request.transition,
        start_protection: request.start_protection,
        start_lifecycle: request.start_lifecycle,
        end_protection: request.end_protection,
        end_lifecycle: request.end_lifecycle,
        mutation_authority: request.mutation_authority,
        outcome,
        proof: ExecutableMemoryOperationProof { accepted },
    }
}

pub fn record_executable_memory_byte_copy(
    request: ExecutableMemoryByteCopyRequest<'_>,
) -> ExecutableMemoryByteCopyRecord {
    let outcome = match validate_executable_memory_byte_copy_request(&request) {
        Ok(()) => ExecutableMemoryResidencyOutcome::Accepted,
        Err(reason) => ExecutableMemoryResidencyOutcome::Rejected { reason },
    };
    let accepted = outcome == ExecutableMemoryResidencyOutcome::Accepted;

    ExecutableMemoryByteCopyRecord {
        ordinal: request.ordinal,
        allocation_ordinal: request.allocation.ordinal,
        allocation: request.allocation.allocation,
        owner: request.allocation.owner,
        code_id: request.link_finalization.code_id,
        mapped_range: request.allocation.mapped_range,
        copied_range: request.copied_range,
        copied_byte_len: request.copied_byte_len,
        copied_digest: request.copied_digest,
        byte_copy_evidence: request
            .link_finalization
            .byte_copy_evidence
            .as_ref()
            .map(executable_memory_byte_copy_evidence),
        outcome,
        proof: ExecutableMemoryOperationProof { accepted },
    }
}

pub fn record_instruction_cache_flush(
    request: InstructionCacheFlushRequest,
) -> InstructionCacheFlushRecord {
    let outcome = match validate_instruction_cache_flush_request(&request) {
        Ok(()) => ExecutableMemoryResidencyOutcome::Accepted,
        Err(reason) => ExecutableMemoryResidencyOutcome::Rejected { reason },
    };
    let accepted = outcome == ExecutableMemoryResidencyOutcome::Accepted;

    InstructionCacheFlushRecord {
        ordinal: request.ordinal,
        transition_ordinal: request.transition.ordinal,
        code_id: request.code_id,
        owner: request.owner,
        allocation: request.allocation,
        range: request.range,
        outcome,
        proof: ExecutableMemoryOperationProof { accepted },
    }
}

pub fn record_executable_memory_residency(
    request: ExecutableMemoryResidencyRequest<'_>,
) -> ExecutableMemoryResidencyRecord {
    let outcome = match validate_residency_request(&request) {
        Ok(()) => ExecutableMemoryResidencyOutcome::Accepted,
        Err(reason) => ExecutableMemoryResidencyOutcome::Rejected { reason },
    };
    let accepted = outcome == ExecutableMemoryResidencyOutcome::Accepted;

    ExecutableMemoryResidencyRecord {
        ordinal: None,
        code_id: request.link_finalization.code_id,
        owner: request.link_finalization.owner,
        symbol: request.link_finalization.symbol,
        allocation: request.link_finalization.allocation,
        machine_range: request.link_finalization.allocation_range,
        mapped_range: request.allocation.mapped_range,
        final_protection: request.protection_transition.end_protection,
        final_lifecycle: request.protection_transition.end_lifecycle,
        allocation_record: request.allocation.clone(),
        byte_copy: None,
        protection_transition: request.protection_transition.clone(),
        instruction_cache_flush: request.instruction_cache_flush.cloned(),
        outcome,
        proof: if accepted {
            ExecutableMemoryResidencyProof {
                link_finalization_verified: true,
                allocation_verified: true,
                byte_copy_verified: false,
                protection_transition_verified: true,
                cache_flush_verified: true,
                residency_ordinal_verified: false,
                machine_code_verified: true,
            }
        } else {
            ExecutableMemoryResidencyProof::default()
        },
    }
}

pub fn record_executable_memory_residency_with_byte_copy(
    request: ExecutableMemoryByteResidencyRequest<'_>,
) -> ExecutableMemoryResidencyRecord {
    let outcome = match validate_byte_residency_request(&request) {
        Ok(()) => ExecutableMemoryResidencyOutcome::Accepted,
        Err(reason) => ExecutableMemoryResidencyOutcome::Rejected { reason },
    };
    let accepted = outcome == ExecutableMemoryResidencyOutcome::Accepted;

    ExecutableMemoryResidencyRecord {
        ordinal: Some(request.ordinal),
        code_id: request.link_finalization.code_id,
        owner: request.link_finalization.owner,
        symbol: request.link_finalization.symbol,
        allocation: request.link_finalization.allocation,
        machine_range: request.link_finalization.allocation_range,
        mapped_range: request.allocation.mapped_range,
        final_protection: request.protection_transition.end_protection,
        final_lifecycle: request.protection_transition.end_lifecycle,
        allocation_record: request.allocation.clone(),
        byte_copy: Some(request.byte_copy.clone()),
        protection_transition: request.protection_transition.clone(),
        instruction_cache_flush: request.instruction_cache_flush.cloned(),
        outcome,
        proof: if accepted {
            ExecutableMemoryResidencyProof {
                link_finalization_verified: true,
                allocation_verified: true,
                byte_copy_verified: true,
                protection_transition_verified: true,
                cache_flush_verified: true,
                residency_ordinal_verified: true,
                machine_code_verified: true,
            }
        } else {
            ExecutableMemoryResidencyProof::default()
        },
    }
}

/// Materialize linked bytes into a safe W^X platform compartment and then
/// produce the stricter byte-evidenced residency record.
///
/// This bridge is still below VM/JIT native execution: it does not expose a raw
/// executable address, function pointer, or callable entrypoint. The returned
/// compartment stays owned by the evidence object so the mapping is not
/// released before accepted residency has been established.
pub fn materialize_platform_executable_memory_residency(
    request: ExecutableMemoryPlatformResidencyRequest<'_>,
) -> Result<ExecutableMemoryPlatformResidency, ExecutableMemoryPlatformResidencyError> {
    let ordinals = platform_residency_ordinals(request.first_ordinal)?;
    let (machine_code, byte_evidence) = validate_link_finalization_with_byte_evidence(
        request.link_finalization,
        request.link_finalization.code_id,
    )
    .map_err(platform_residency_validation_error)?;

    let compartment_request = ExecutableMemoryCompartmentRequest::with_range(
        request.link_finalization.allocation,
        request.link_finalization.owner,
        machine_code.range,
        ExecutableMutationAuthority::LinkBuffer,
    );
    let mut compartment = ExecutableMemoryCompartment::allocate(compartment_request)?;

    let allocation = record_executable_memory_allocation(ExecutableMemoryAllocationRequest {
        ordinal: ordinals.allocation,
        allocation: compartment.allocation_record(),
        mapped_range: compartment.mapped_range(),
    });
    require_platform_record_accepted(&allocation.outcome)?;
    validate_allocation_for_machine_code(&allocation, request.link_finalization, machine_code)
        .map_err(platform_residency_validation_error)?;

    let copied_byte_len = u32::try_from(request.linked_bytes.len()).map_err(|_| {
        ExecutableMemoryPlatformResidencyError::Compartment {
            error: ExecutableMemoryCompartmentError::ByteLengthTooLarge {
                actual: request.linked_bytes.len(),
            },
        }
    })?;
    let copied_digest = compute_assembler_byte_image_digest(request.linked_bytes);
    validate_byte_copy_materialization(
        allocation.mapped_range,
        machine_code.range,
        copied_byte_len,
        copied_digest,
        request.link_finalization.allocation_range,
        machine_code,
        byte_evidence,
    )
    .map_err(platform_residency_validation_error)?;

    compartment.copy_from_slice(machine_code.range, request.linked_bytes)?;
    let byte_copy = record_executable_memory_byte_copy(ExecutableMemoryByteCopyRequest {
        ordinal: ordinals.byte_copy,
        allocation: &allocation,
        link_finalization: request.link_finalization,
        copied_range: machine_code.range,
        copied_byte_len,
        copied_digest,
    });
    require_platform_record_accepted(&byte_copy.outcome)?;

    compartment.protect_executable()?;
    let protection_transition = record_executable_memory_protection_transition(
        ExecutableMemoryProtectionTransitionRequest {
            ordinal: ordinals.protection_transition,
            allocation: allocation.clone(),
            transition: JitPermissionTransition::RwToRx,
            start_protection: allocation.protection,
            start_lifecycle: allocation.lifecycle,
            end_protection: compartment.protection(),
            end_lifecycle: compartment.lifecycle(),
            mutation_authority: compartment.mutation_authority(),
        },
    );
    require_platform_record_accepted(&protection_transition.outcome)?;

    // Marker only for now. Real instruction-cache flushing remains outside this
    // safe bridge until a platform API can do it without exposing code pointers.
    let instruction_cache_flush = record_instruction_cache_flush(InstructionCacheFlushRequest {
        ordinal: ordinals.instruction_cache_flush,
        transition: protection_transition.clone(),
        code_id: request.link_finalization.code_id,
        owner: request.link_finalization.owner,
        allocation: request.link_finalization.allocation,
        range: machine_code.range,
    });
    require_platform_record_accepted(&instruction_cache_flush.outcome)?;

    let residency =
        record_executable_memory_residency_with_byte_copy(ExecutableMemoryByteResidencyRequest {
            ordinal: ordinals.residency,
            link_finalization: request.link_finalization,
            allocation: &allocation,
            byte_copy: &byte_copy,
            protection_transition: &protection_transition,
            instruction_cache_flush: Some(&instruction_cache_flush),
        });
    require_platform_record_accepted(&residency.outcome)?;
    let finalized_machine_code = residency
        .validate_accepted_with_byte_evidence(request.link_finalization)
        .map_err(platform_residency_validation_error)?;
    let platform_machine_code = compartment.executable_machine_code_handle()?;
    let comparable_platform_machine_code = MachineCodeHandle {
        symbol: finalized_machine_code.symbol,
        ..platform_machine_code
    };
    if comparable_platform_machine_code != finalized_machine_code {
        return Err(platform_residency_validation_error(
            ExecutableMemoryResidencyValidationError::ResidencyMachineCodeMismatch {
                expected: finalized_machine_code,
                actual: comparable_platform_machine_code,
            },
        ));
    }

    Ok(ExecutableMemoryPlatformResidency {
        residency,
        evidence: ExecutableMemoryPlatformHandleEvidence {
            compartment,
            finalized_machine_code,
            platform_machine_code,
        },
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExecutableMemoryPlatformResidencyOrdinals {
    allocation: ExecutableMemoryOperationOrdinal,
    byte_copy: ExecutableMemoryOperationOrdinal,
    protection_transition: ExecutableMemoryOperationOrdinal,
    instruction_cache_flush: ExecutableMemoryOperationOrdinal,
    residency: ExecutableMemoryOperationOrdinal,
}

fn platform_residency_ordinals(
    first_ordinal: ExecutableMemoryOperationOrdinal,
) -> Result<ExecutableMemoryPlatformResidencyOrdinals, ExecutableMemoryPlatformResidencyError> {
    ensure_nonzero_ordinal(first_ordinal).map_err(platform_residency_validation_error)?;
    let byte_copy = next_platform_residency_ordinal(first_ordinal, first_ordinal)?;
    let protection_transition = next_platform_residency_ordinal(first_ordinal, byte_copy)?;
    let instruction_cache_flush =
        next_platform_residency_ordinal(first_ordinal, protection_transition)?;
    let residency = next_platform_residency_ordinal(first_ordinal, instruction_cache_flush)?;

    Ok(ExecutableMemoryPlatformResidencyOrdinals {
        allocation: first_ordinal,
        byte_copy,
        protection_transition,
        instruction_cache_flush,
        residency,
    })
}

fn next_platform_residency_ordinal(
    first_ordinal: ExecutableMemoryOperationOrdinal,
    ordinal: ExecutableMemoryOperationOrdinal,
) -> Result<ExecutableMemoryOperationOrdinal, ExecutableMemoryPlatformResidencyError> {
    ordinal
        .0
        .checked_add(1)
        .map(ExecutableMemoryOperationOrdinal)
        .ok_or(ExecutableMemoryPlatformResidencyError::OperationOrdinalOverflow { first_ordinal })
}

fn require_platform_record_accepted(
    outcome: &ExecutableMemoryResidencyOutcome,
) -> Result<(), ExecutableMemoryPlatformResidencyError> {
    match outcome {
        ExecutableMemoryResidencyOutcome::Accepted => Ok(()),
        ExecutableMemoryResidencyOutcome::Rejected { reason } => {
            Err(platform_residency_validation_error(reason.clone()))
        }
    }
}

fn platform_residency_validation_error(
    reason: ExecutableMemoryResidencyValidationError,
) -> ExecutableMemoryPlatformResidencyError {
    ExecutableMemoryPlatformResidencyError::ResidencyValidation { reason }
}

fn validate_executable_memory_allocation_request(
    request: &ExecutableMemoryAllocationRequest,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    ensure_nonzero_ordinal(request.ordinal)?;
    validate_executable_allocation_ledger_record(&request.allocation)?;
    request.mapped_range.validate()?;
    if request.mapped_range.allocation != request.allocation.allocation {
        return Err(
            ExecutableMemoryResidencyValidationError::MappedRangeAllocationMismatch {
                expected: request.allocation.allocation,
                actual: request.mapped_range.allocation,
            },
        );
    }
    request
        .mapped_range
        .contains_machine_range(request.allocation.range)?;
    validate_link_writable_state(request.allocation.protection, request.allocation.lifecycle)?;
    validate_link_buffer_authority(request.allocation.mutation_authority)?;

    Ok(())
}

fn validate_executable_memory_allocation_record(
    record: &ExecutableMemoryAllocationRecord,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    match &record.outcome {
        ExecutableMemoryResidencyOutcome::Accepted => {}
        ExecutableMemoryResidencyOutcome::Rejected { reason } => {
            return Err(
                ExecutableMemoryResidencyValidationError::AllocationRecordRejected {
                    reason: Box::new(reason.clone()),
                },
            );
        }
    }
    if !record.proof.accepted {
        return Err(ExecutableMemoryResidencyValidationError::AllocationProofMissing);
    }
    ensure_nonzero_ordinal(record.ordinal)?;
    record.mapped_range.validate()?;
    if record.mapped_range.allocation != record.allocation {
        return Err(
            ExecutableMemoryResidencyValidationError::MappedRangeAllocationMismatch {
                expected: record.allocation,
                actual: record.mapped_range.allocation,
            },
        );
    }
    if record.allocation_range.allocation != record.allocation {
        return Err(
            ExecutableMemoryResidencyValidationError::AllocationIdMismatch {
                expected: record.allocation,
                actual: record.allocation_range.allocation,
            },
        );
    }
    record.allocation_range.validate().map_err(|error| {
        ExecutableMemoryResidencyValidationError::AllocationRangeInvalid { error }
    })?;
    record
        .mapped_range
        .contains_machine_range(record.allocation_range)?;
    validate_link_writable_state(record.protection, record.lifecycle)?;
    validate_link_buffer_authority(record.mutation_authority)?;

    Ok(())
}

fn validate_executable_memory_byte_copy_request(
    request: &ExecutableMemoryByteCopyRequest<'_>,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    validate_executable_memory_allocation_record(request.allocation)?;
    validate_operation_after(request.allocation.ordinal, request.ordinal)?;
    let (machine_code, evidence) = validate_link_finalization_with_byte_evidence(
        request.link_finalization,
        request.link_finalization.code_id,
    )?;
    validate_allocation_for_byte_copy(request.allocation, request.link_finalization)?;
    validate_byte_copy_materialization(
        request.allocation.mapped_range,
        request.copied_range,
        request.copied_byte_len,
        request.copied_digest,
        request.link_finalization.allocation_range,
        machine_code,
        evidence,
    )
}

fn validate_executable_memory_byte_copy_record(
    record: &ExecutableMemoryByteCopyRecord,
    allocation: &ExecutableMemoryAllocationRecord,
    link_finalization: &LinkBufferFinalizationRecord,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    match &record.outcome {
        ExecutableMemoryResidencyOutcome::Accepted => {}
        ExecutableMemoryResidencyOutcome::Rejected { reason } => {
            return Err(
                ExecutableMemoryResidencyValidationError::ByteCopyRecordRejected {
                    reason: Box::new(reason.clone()),
                },
            );
        }
    }
    if !record.proof.accepted {
        return Err(ExecutableMemoryResidencyValidationError::ByteCopyProofMissing);
    }
    ensure_nonzero_ordinal(record.ordinal)?;
    validate_executable_memory_allocation_record(allocation)?;
    validate_operation_after(allocation.ordinal, record.ordinal)?;
    if record.allocation_ordinal != allocation.ordinal {
        return Err(
            ExecutableMemoryResidencyValidationError::OperationOrderingInvalid {
                before: allocation.ordinal,
                after: record.allocation_ordinal,
            },
        );
    }
    if record.allocation != allocation.allocation {
        return Err(
            ExecutableMemoryResidencyValidationError::ByteCopyAllocationMismatch {
                expected: allocation.allocation,
                actual: record.allocation,
            },
        );
    }
    if record.owner != allocation.owner {
        return Err(
            ExecutableMemoryResidencyValidationError::ByteCopyOwnerMismatch {
                expected: allocation.owner,
                actual: record.owner,
            },
        );
    }
    if record.mapped_range != allocation.mapped_range {
        return Err(
            ExecutableMemoryResidencyValidationError::ByteCopyMappedRangeMismatch {
                expected: allocation.mapped_range,
                actual: record.mapped_range,
            },
        );
    }
    if record.code_id != link_finalization.code_id {
        return Err(
            ExecutableMemoryResidencyValidationError::ByteCopyCodeIdMismatch {
                expected: link_finalization.code_id,
                actual: record.code_id,
            },
        );
    }

    let (machine_code, expected_evidence) =
        validate_link_finalization_with_byte_evidence(link_finalization, record.code_id)?;
    let actual_evidence = record
        .byte_copy_evidence
        .ok_or(ExecutableMemoryResidencyValidationError::ByteCopyEvidenceMissing)?;
    if actual_evidence != expected_evidence {
        return Err(
            ExecutableMemoryResidencyValidationError::ByteCopyEvidenceMismatch {
                expected: Box::new(Some(expected_evidence)),
                actual: Box::new(Some(actual_evidence)),
            },
        );
    }

    validate_allocation_for_byte_copy(allocation, link_finalization)?;
    validate_byte_copy_materialization(
        record.mapped_range,
        record.copied_range,
        record.copied_byte_len,
        record.copied_digest,
        link_finalization.allocation_range,
        machine_code,
        expected_evidence,
    )
}

fn validate_executable_memory_protection_transition_request(
    request: &ExecutableMemoryProtectionTransitionRequest,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    validate_executable_memory_allocation_record(&request.allocation)?;
    validate_operation_after(request.allocation.ordinal, request.ordinal)?;
    validate_transition_shape(
        request.transition,
        request.start_protection,
        request.start_lifecycle,
        request.end_protection,
        request.end_lifecycle,
        request.mutation_authority,
    )?;
    if request.start_protection != request.allocation.protection
        || request.start_lifecycle != request.allocation.lifecycle
    {
        return Err(
            ExecutableMemoryResidencyValidationError::TransitionStartStateMismatch {
                protection: request.start_protection,
                lifecycle: request.start_lifecycle,
            },
        );
    }

    Ok(())
}

fn validate_executable_memory_protection_transition_record(
    record: &ExecutableMemoryProtectionTransitionRecord,
    allocation: &ExecutableMemoryAllocationRecord,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    match &record.outcome {
        ExecutableMemoryResidencyOutcome::Accepted => {}
        ExecutableMemoryResidencyOutcome::Rejected { reason } => {
            return Err(
                ExecutableMemoryResidencyValidationError::TransitionRecordRejected {
                    reason: Box::new(reason.clone()),
                },
            );
        }
    }
    if !record.proof.accepted {
        return Err(ExecutableMemoryResidencyValidationError::TransitionProofMissing);
    }
    ensure_nonzero_ordinal(record.ordinal)?;
    validate_operation_after(allocation.ordinal, record.ordinal)?;
    if record.allocation_ordinal != allocation.ordinal {
        return Err(
            ExecutableMemoryResidencyValidationError::OperationOrderingInvalid {
                before: allocation.ordinal,
                after: record.allocation_ordinal,
            },
        );
    }
    if record.owner != allocation.owner {
        return Err(
            ExecutableMemoryResidencyValidationError::TransitionOwnerMismatch {
                expected: allocation.owner,
                actual: record.owner,
            },
        );
    }
    if record.allocation != allocation.allocation {
        return Err(
            ExecutableMemoryResidencyValidationError::AllocationIdMismatch {
                expected: allocation.allocation,
                actual: record.allocation,
            },
        );
    }
    if record.mapped_range != allocation.mapped_range {
        return Err(
            ExecutableMemoryResidencyValidationError::TransitionRangeMismatch {
                expected: allocation.mapped_range,
                actual: record.mapped_range,
            },
        );
    }
    validate_transition_shape(
        record.transition,
        record.start_protection,
        record.start_lifecycle,
        record.end_protection,
        record.end_lifecycle,
        record.mutation_authority,
    )?;
    if record.start_protection != allocation.protection
        || record.start_lifecycle != allocation.lifecycle
    {
        return Err(
            ExecutableMemoryResidencyValidationError::TransitionStartStateMismatch {
                protection: record.start_protection,
                lifecycle: record.start_lifecycle,
            },
        );
    }

    Ok(())
}

fn validate_transition_record_shape(
    record: &ExecutableMemoryProtectionTransitionRecord,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    match &record.outcome {
        ExecutableMemoryResidencyOutcome::Accepted => {}
        ExecutableMemoryResidencyOutcome::Rejected { reason } => {
            return Err(
                ExecutableMemoryResidencyValidationError::TransitionRecordRejected {
                    reason: Box::new(reason.clone()),
                },
            );
        }
    }
    if !record.proof.accepted {
        return Err(ExecutableMemoryResidencyValidationError::TransitionProofMissing);
    }
    ensure_nonzero_ordinal(record.ordinal)?;
    record.mapped_range.validate()?;
    validate_transition_shape(
        record.transition,
        record.start_protection,
        record.start_lifecycle,
        record.end_protection,
        record.end_lifecycle,
        record.mutation_authority,
    )
}

fn validate_instruction_cache_flush_request(
    request: &InstructionCacheFlushRequest,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    validate_transition_record_shape(&request.transition)?;
    validate_operation_after(request.transition.ordinal, request.ordinal)?;
    if request.owner != request.transition.owner {
        return Err(
            ExecutableMemoryResidencyValidationError::CacheFlushOwnerMismatch {
                expected: request.transition.owner,
                actual: request.owner,
            },
        );
    }
    if request.allocation != request.transition.allocation {
        return Err(
            ExecutableMemoryResidencyValidationError::CacheFlushAllocationMismatch {
                expected: request.transition.allocation,
                actual: request.allocation,
            },
        );
    }
    if request.code_id.0 == 0 {
        return Err(
            ExecutableMemoryResidencyValidationError::ResidencyCodeIdMismatch {
                expected: JitCodeId(0),
                actual: request.code_id,
            },
        );
    }
    request.range.validate().map_err(|error| {
        ExecutableMemoryResidencyValidationError::AllocationRangeInvalid { error }
    })?;
    if request.range.allocation != request.allocation {
        return Err(
            ExecutableMemoryResidencyValidationError::CacheFlushAllocationMismatch {
                expected: request.allocation,
                actual: request.range.allocation,
            },
        );
    }
    request
        .transition
        .mapped_range
        .contains_machine_range(request.range)?;

    Ok(())
}

fn validate_instruction_cache_flush_record(
    record: &InstructionCacheFlushRecord,
    transition: &ExecutableMemoryProtectionTransitionRecord,
    code_id: JitCodeId,
    owner: MachineCodeOwnership,
    allocation: ExecutableAllocationId,
    range: MachineCodeRange,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    match &record.outcome {
        ExecutableMemoryResidencyOutcome::Accepted => {}
        ExecutableMemoryResidencyOutcome::Rejected { reason } => {
            return Err(
                ExecutableMemoryResidencyValidationError::CacheFlushRecordRejected {
                    reason: Box::new(reason.clone()),
                },
            );
        }
    }
    if !record.proof.accepted {
        return Err(ExecutableMemoryResidencyValidationError::CacheFlushProofMissing);
    }
    ensure_nonzero_ordinal(record.ordinal)?;
    validate_operation_after(transition.ordinal, record.ordinal)?;
    if record.transition_ordinal != transition.ordinal {
        return Err(
            ExecutableMemoryResidencyValidationError::OperationOrderingInvalid {
                before: transition.ordinal,
                after: record.transition_ordinal,
            },
        );
    }
    if record.code_id != code_id {
        return Err(
            ExecutableMemoryResidencyValidationError::CacheFlushCodeIdMismatch {
                expected: code_id,
                actual: record.code_id,
            },
        );
    }
    if record.owner != owner {
        return Err(
            ExecutableMemoryResidencyValidationError::CacheFlushOwnerMismatch {
                expected: owner,
                actual: record.owner,
            },
        );
    }
    if record.allocation != allocation {
        return Err(
            ExecutableMemoryResidencyValidationError::CacheFlushAllocationMismatch {
                expected: allocation,
                actual: record.allocation,
            },
        );
    }
    if record.range != range {
        return Err(
            ExecutableMemoryResidencyValidationError::CacheFlushRangeMismatch {
                expected: range,
                actual: record.range,
            },
        );
    }

    Ok(())
}

fn validate_residency_request(
    request: &ExecutableMemoryResidencyRequest<'_>,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    let machine_code =
        validate_link_finalization(request.link_finalization, request.link_finalization.code_id)?;
    validate_allocation_for_machine_code(
        request.allocation,
        request.link_finalization,
        machine_code,
    )?;
    validate_executable_memory_protection_transition_record(
        request.protection_transition,
        request.allocation,
    )?;
    let flush = request
        .instruction_cache_flush
        .ok_or(ExecutableMemoryResidencyValidationError::CacheFlushMissing)?;
    validate_instruction_cache_flush_record(
        flush,
        request.protection_transition,
        request.link_finalization.code_id,
        request.link_finalization.owner,
        request.link_finalization.allocation,
        request.link_finalization.allocation_range,
    )?;
    validate_transition_final_state(request.protection_transition)?;

    Ok(())
}

fn validate_byte_residency_request(
    request: &ExecutableMemoryByteResidencyRequest<'_>,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    let (machine_code, _) = validate_link_finalization_with_byte_evidence(
        request.link_finalization,
        request.link_finalization.code_id,
    )?;
    validate_allocation_for_machine_code(
        request.allocation,
        request.link_finalization,
        machine_code,
    )?;
    validate_executable_memory_byte_copy_record(
        request.byte_copy,
        request.allocation,
        request.link_finalization,
    )?;
    validate_operation_after(
        request.byte_copy.ordinal,
        request.protection_transition.ordinal,
    )?;
    validate_executable_memory_protection_transition_record(
        request.protection_transition,
        request.allocation,
    )?;
    let flush = request
        .instruction_cache_flush
        .ok_or(ExecutableMemoryResidencyValidationError::CacheFlushMissing)?;
    validate_instruction_cache_flush_record(
        flush,
        request.protection_transition,
        request.link_finalization.code_id,
        request.link_finalization.owner,
        request.link_finalization.allocation,
        request.link_finalization.allocation_range,
    )?;
    validate_operation_after(flush.ordinal, request.ordinal)?;
    validate_transition_final_state(request.protection_transition)?;

    Ok(())
}

fn validate_residency_record_for_link_finalization(
    record: &ExecutableMemoryResidencyRecord,
    link_finalization: &LinkBufferFinalizationRecord,
    require_byte_copy: bool,
) -> Result<MachineCodeHandle, ExecutableMemoryResidencyValidationError> {
    if record.code_id != link_finalization.code_id {
        return Err(
            ExecutableMemoryResidencyValidationError::ResidencyCodeIdMismatch {
                expected: link_finalization.code_id,
                actual: record.code_id,
            },
        );
    }
    let machine_code = if require_byte_copy {
        let (machine_code, _) =
            validate_link_finalization_with_byte_evidence(link_finalization, record.code_id)?;
        machine_code
    } else {
        validate_link_finalization(link_finalization, record.code_id)?
    };
    if record.owner != link_finalization.owner {
        return Err(
            ExecutableMemoryResidencyValidationError::ResidencyOwnerMismatch {
                expected: link_finalization.owner,
                actual: record.owner,
            },
        );
    }
    if record.symbol != link_finalization.symbol {
        return Err(
            ExecutableMemoryResidencyValidationError::ResidencyNativeSymbolMismatch {
                expected: link_finalization.symbol,
                actual: record.symbol,
            },
        );
    }
    if record.allocation != link_finalization.allocation {
        return Err(
            ExecutableMemoryResidencyValidationError::ResidencyAllocationMismatch {
                expected: link_finalization.allocation,
                actual: record.allocation,
            },
        );
    }
    if record.machine_range != link_finalization.allocation_range {
        return Err(
            ExecutableMemoryResidencyValidationError::ResidencyMachineRangeMismatch {
                expected: link_finalization.allocation_range,
                actual: record.machine_range,
            },
        );
    }
    if record.allocation_record.mapped_range != record.mapped_range {
        return Err(
            ExecutableMemoryResidencyValidationError::TransitionRangeMismatch {
                expected: record.allocation_record.mapped_range,
                actual: record.mapped_range,
            },
        );
    }
    if record.final_protection != ExecutableMemoryProtection::Executable {
        return Err(
            ExecutableMemoryResidencyValidationError::FinalProtectionMismatch {
                expected: ExecutableMemoryProtection::Executable,
                actual: record.final_protection,
            },
        );
    }
    if record.final_lifecycle != ExecutableAllocationLifecycle::LinkedExecutable {
        return Err(
            ExecutableMemoryResidencyValidationError::FinalLifecycleMismatch {
                expected: ExecutableAllocationLifecycle::LinkedExecutable,
                actual: record.final_lifecycle,
            },
        );
    }
    if record.final_protection != machine_code.protection {
        return Err(
            ExecutableMemoryResidencyValidationError::FinalProtectionMismatch {
                expected: machine_code.protection,
                actual: record.final_protection,
            },
        );
    }
    if record.final_lifecycle != machine_code.lifecycle {
        return Err(
            ExecutableMemoryResidencyValidationError::FinalLifecycleMismatch {
                expected: machine_code.lifecycle,
                actual: record.final_lifecycle,
            },
        );
    }

    validate_allocation_for_machine_code(
        &record.allocation_record,
        link_finalization,
        machine_code,
    )?;
    if require_byte_copy || record.byte_copy.is_some() {
        let byte_copy = record
            .byte_copy
            .as_ref()
            .ok_or(ExecutableMemoryResidencyValidationError::ByteCopyMissing)?;
        validate_executable_memory_byte_copy_record(
            byte_copy,
            &record.allocation_record,
            link_finalization,
        )?;
        validate_operation_after(byte_copy.ordinal, record.protection_transition.ordinal)?;
    }
    validate_executable_memory_protection_transition_record(
        &record.protection_transition,
        &record.allocation_record,
    )?;
    validate_transition_final_state(&record.protection_transition)?;
    let flush = record
        .instruction_cache_flush
        .as_ref()
        .ok_or(ExecutableMemoryResidencyValidationError::CacheFlushMissing)?;
    validate_instruction_cache_flush_record(
        flush,
        &record.protection_transition,
        link_finalization.code_id,
        link_finalization.owner,
        link_finalization.allocation,
        link_finalization.allocation_range,
    )?;
    if require_byte_copy {
        let ordinal = record
            .ordinal
            .ok_or(ExecutableMemoryResidencyValidationError::ResidencyOrdinalMissing)?;
        validate_operation_after(flush.ordinal, ordinal)?;
    }

    if machine_code.range != record.machine_range {
        return Err(
            ExecutableMemoryResidencyValidationError::ResidencyMachineCodeMismatch {
                expected: machine_code,
                actual: MachineCodeHandle {
                    range: record.machine_range,
                    ..machine_code
                },
            },
        );
    }

    Ok(machine_code)
}

fn validate_link_finalization(
    link_finalization: &LinkBufferFinalizationRecord,
    expected_code_id: JitCodeId,
) -> Result<MachineCodeHandle, ExecutableMemoryResidencyValidationError> {
    match &link_finalization.outcome {
        LinkBufferFinalizationOutcome::Accepted => {}
        LinkBufferFinalizationOutcome::Rejected { reason } => {
            return Err(
                ExecutableMemoryResidencyValidationError::LinkFinalizationRejected {
                    reason: Box::new(reason.clone()),
                },
            );
        }
    }
    if link_finalization.permission_transition != Some(JitPermissionTransition::RwToRx) {
        return Err(
            ExecutableMemoryResidencyValidationError::LinkFinalizationTransitionMismatch {
                actual: link_finalization.permission_transition,
            },
        );
    }
    if link_finalization.code_id != expected_code_id {
        return Err(
            ExecutableMemoryResidencyValidationError::ResidencyCodeIdMismatch {
                expected: expected_code_id,
                actual: link_finalization.code_id,
            },
        );
    }
    link_finalization.validate_accepted().map_err(|reason| {
        ExecutableMemoryResidencyValidationError::LinkFinalizationInvalid {
            reason: Box::new(reason),
        }
    })
}

fn validate_link_finalization_with_byte_evidence(
    link_finalization: &LinkBufferFinalizationRecord,
    expected_code_id: JitCodeId,
) -> Result<
    (MachineCodeHandle, ExecutableMemoryByteCopyEvidence),
    ExecutableMemoryResidencyValidationError,
> {
    if link_finalization.byte_copy_evidence.is_none() {
        return Err(ExecutableMemoryResidencyValidationError::ByteCopyEvidenceMissing);
    }
    match &link_finalization.outcome {
        LinkBufferFinalizationOutcome::Accepted => {}
        LinkBufferFinalizationOutcome::Rejected { reason } => {
            return Err(
                ExecutableMemoryResidencyValidationError::LinkFinalizationRejected {
                    reason: Box::new(reason.clone()),
                },
            );
        }
    }
    if link_finalization.permission_transition != Some(JitPermissionTransition::RwToRx) {
        return Err(
            ExecutableMemoryResidencyValidationError::LinkFinalizationTransitionMismatch {
                actual: link_finalization.permission_transition,
            },
        );
    }
    if link_finalization.code_id != expected_code_id {
        return Err(
            ExecutableMemoryResidencyValidationError::ResidencyCodeIdMismatch {
                expected: expected_code_id,
                actual: link_finalization.code_id,
            },
        );
    }
    let machine_code = link_finalization
        .validate_accepted_with_byte_evidence()
        .map_err(map_byte_copy_finalization_error)?;
    let evidence = link_finalization
        .byte_copy_evidence
        .as_ref()
        .map(executable_memory_byte_copy_evidence)
        .ok_or(ExecutableMemoryResidencyValidationError::ByteCopyEvidenceMissing)?;

    Ok((machine_code, evidence))
}

fn executable_memory_byte_copy_evidence(
    evidence: &LinkBufferByteCopyEvidence,
) -> ExecutableMemoryByteCopyEvidence {
    ExecutableMemoryByteCopyEvidence {
        source_image: evidence.source_image,
        source_digest: evidence.source_digest,
        copied_digest: evidence.output_digest,
        copied_byte_len: evidence.output_byte_len,
        relocation_count: evidence.relocation_count,
        profile: evidence.profile,
        state: evidence.state,
    }
}

fn map_byte_copy_finalization_error(
    reason: ExecutableLedgerValidationError,
) -> ExecutableMemoryResidencyValidationError {
    match reason {
        ExecutableLedgerValidationError::FinalizationByteEvidenceMissing
        | ExecutableLedgerValidationError::CopyLinkByteEvidenceMissing => {
            ExecutableMemoryResidencyValidationError::ByteCopyEvidenceMissing
        }
        ExecutableLedgerValidationError::FinalizationByteEvidenceMismatch { expected, actual } => {
            ExecutableMemoryResidencyValidationError::ByteCopyEvidenceMismatch {
                expected: Box::new(
                    expected
                        .as_ref()
                        .as_ref()
                        .map(executable_memory_byte_copy_evidence),
                ),
                actual: Box::new(
                    actual
                        .as_ref()
                        .as_ref()
                        .map(executable_memory_byte_copy_evidence),
                ),
            }
        }
        reason => ExecutableMemoryResidencyValidationError::LinkFinalizationInvalid {
            reason: Box::new(reason),
        },
    }
}

fn validate_allocation_for_byte_copy(
    allocation: &ExecutableMemoryAllocationRecord,
    link_finalization: &LinkBufferFinalizationRecord,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    if allocation.owner != link_finalization.owner {
        return Err(
            ExecutableMemoryResidencyValidationError::ByteCopyOwnerMismatch {
                expected: link_finalization.owner,
                actual: allocation.owner,
            },
        );
    }
    if allocation.allocation != link_finalization.allocation {
        return Err(
            ExecutableMemoryResidencyValidationError::ByteCopyAllocationMismatch {
                expected: link_finalization.allocation,
                actual: allocation.allocation,
            },
        );
    }
    if allocation.allocation_range != link_finalization.allocation_range {
        return Err(
            ExecutableMemoryResidencyValidationError::ByteCopyAllocationRangeMismatch {
                expected: link_finalization.allocation_range,
                actual: allocation.allocation_range,
            },
        );
    }

    Ok(())
}

fn validate_byte_copy_materialization(
    mapped_range: ExecutableMemoryMappedRange,
    copied_range: MachineCodeRange,
    copied_byte_len: u32,
    copied_digest: AssemblerByteImageDigest,
    expected_range: MachineCodeRange,
    machine_code: MachineCodeHandle,
    evidence: ExecutableMemoryByteCopyEvidence,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    copied_range.validate().map_err(|error| {
        ExecutableMemoryResidencyValidationError::AllocationRangeInvalid { error }
    })?;
    if copied_range != expected_range {
        return Err(
            ExecutableMemoryResidencyValidationError::ByteCopyRangeMismatch {
                expected: expected_range,
                actual: copied_range,
            },
        );
    }
    if copied_range != machine_code.range {
        return Err(
            ExecutableMemoryResidencyValidationError::ByteCopyRangeMismatch {
                expected: machine_code.range,
                actual: copied_range,
            },
        );
    }
    mapped_range.contains_machine_range(copied_range)?;
    if copied_byte_len != evidence.copied_byte_len {
        return Err(
            ExecutableMemoryResidencyValidationError::ByteCopyLengthMismatch {
                expected: evidence.copied_byte_len,
                actual: copied_byte_len,
            },
        );
    }
    if copied_range.size_bytes != copied_byte_len {
        return Err(
            ExecutableMemoryResidencyValidationError::ByteCopyLengthMismatch {
                expected: copied_range.size_bytes,
                actual: copied_byte_len,
            },
        );
    }
    if copied_digest != evidence.copied_digest {
        return Err(
            ExecutableMemoryResidencyValidationError::ByteCopyDigestMismatch {
                expected: evidence.copied_digest,
                actual: copied_digest,
            },
        );
    }

    Ok(())
}

fn validate_allocation_for_machine_code(
    allocation: &ExecutableMemoryAllocationRecord,
    link_finalization: &LinkBufferFinalizationRecord,
    machine_code: MachineCodeHandle,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    validate_executable_memory_allocation_record(allocation)?;
    if allocation.owner != link_finalization.owner {
        return Err(
            ExecutableMemoryResidencyValidationError::AllocationOwnerMismatch {
                expected: link_finalization.owner,
                actual: allocation.owner,
            },
        );
    }
    if allocation.allocation != link_finalization.allocation {
        return Err(
            ExecutableMemoryResidencyValidationError::AllocationIdMismatch {
                expected: link_finalization.allocation,
                actual: allocation.allocation,
            },
        );
    }
    if allocation.allocation_range != link_finalization.allocation_range {
        return Err(
            ExecutableMemoryResidencyValidationError::AllocationRangeMismatch {
                expected: link_finalization.allocation_range,
                actual: allocation.allocation_range,
            },
        );
    }
    allocation
        .mapped_range
        .contains_machine_range(machine_code.range)?;

    Ok(())
}

fn validate_executable_allocation_ledger_record(
    allocation: &ExecutableAllocationRecord,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    allocation.validate().map_err(|reason| match reason {
        ExecutableLedgerValidationError::AllocationRecordRejected { reason } => {
            ExecutableMemoryResidencyValidationError::AllocationLedgerRejected { reason }
        }
        reason => ExecutableMemoryResidencyValidationError::AllocationLedgerInvalid {
            reason: Box::new(reason),
        },
    })
}

fn validate_link_writable_state(
    protection: ExecutableMemoryProtection,
    lifecycle: ExecutableAllocationLifecycle,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    if protection != ExecutableMemoryProtection::Writable
        || lifecycle != ExecutableAllocationLifecycle::AllocatedWritable
    {
        return Err(
            ExecutableMemoryResidencyValidationError::AllocationStartStateMismatch {
                protection,
                lifecycle,
            },
        );
    }
    Ok(())
}

fn validate_link_buffer_authority(
    actual: ExecutableMutationAuthority,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    if actual != ExecutableMutationAuthority::LinkBuffer {
        return Err(
            ExecutableMemoryResidencyValidationError::MutationAuthorityMismatch {
                expected: ExecutableMutationAuthority::LinkBuffer,
                actual,
            },
        );
    }
    Ok(())
}

fn validate_transition_shape(
    transition: JitPermissionTransition,
    start_protection: ExecutableMemoryProtection,
    start_lifecycle: ExecutableAllocationLifecycle,
    end_protection: ExecutableMemoryProtection,
    end_lifecycle: ExecutableAllocationLifecycle,
    mutation_authority: ExecutableMutationAuthority,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    if transition != JitPermissionTransition::RwToRx {
        return Err(
            ExecutableMemoryResidencyValidationError::PermissionTransitionMismatch {
                expected: JitPermissionTransition::RwToRx,
                actual: transition,
            },
        );
    }
    if start_protection != ExecutableMemoryProtection::Writable
        || start_lifecycle != ExecutableAllocationLifecycle::AllocatedWritable
    {
        return Err(
            ExecutableMemoryResidencyValidationError::TransitionStartStateMismatch {
                protection: start_protection,
                lifecycle: start_lifecycle,
            },
        );
    }
    if end_protection != ExecutableMemoryProtection::Executable
        || end_lifecycle != ExecutableAllocationLifecycle::LinkedExecutable
    {
        return Err(
            ExecutableMemoryResidencyValidationError::TransitionEndStateMismatch {
                protection: end_protection,
                lifecycle: end_lifecycle,
            },
        );
    }
    validate_link_buffer_authority(mutation_authority)
}

fn validate_transition_final_state(
    transition: &ExecutableMemoryProtectionTransitionRecord,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    if transition.end_protection != ExecutableMemoryProtection::Executable {
        return Err(
            ExecutableMemoryResidencyValidationError::FinalProtectionMismatch {
                expected: ExecutableMemoryProtection::Executable,
                actual: transition.end_protection,
            },
        );
    }
    if transition.end_lifecycle != ExecutableAllocationLifecycle::LinkedExecutable {
        return Err(
            ExecutableMemoryResidencyValidationError::FinalLifecycleMismatch {
                expected: ExecutableAllocationLifecycle::LinkedExecutable,
                actual: transition.end_lifecycle,
            },
        );
    }
    Ok(())
}

fn ensure_nonzero_ordinal(
    ordinal: ExecutableMemoryOperationOrdinal,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    if ordinal.0 == 0 {
        return Err(ExecutableMemoryResidencyValidationError::OperationOrdinalZero);
    }
    Ok(())
}

fn validate_operation_after(
    before: ExecutableMemoryOperationOrdinal,
    after: ExecutableMemoryOperationOrdinal,
) -> Result<(), ExecutableMemoryResidencyValidationError> {
    ensure_nonzero_ordinal(before)?;
    ensure_nonzero_ordinal(after)?;
    if after <= before {
        return Err(
            ExecutableMemoryResidencyValidationError::OperationOrderingInvalid { before, after },
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembler::{
        describe_assembler_byte_image, freeze_assembler_byte_image, link_assembler_byte_image,
        plan_link_buffer_layout, AssemblerArchitecture, AssemblerBufferDescriptor,
        AssemblerBufferId, AssemblerBufferLifecycle, AssemblerByteImageDigest,
        AssemblerByteImageId, LinkBufferProfile,
    };
    use crate::gc::CellId;
    use crate::jit::executable::{
        record_link_buffer_copy_link_with_linked_image, LinkBufferLinkedCopyLinkRequest,
    };
    use crate::jit::{
        finalize_link_buffer, record_link_buffer_copy_link, ExecutableAllocationRequest,
        JitCodeArtifact, JitType, LinkBufferCopyLinkOutcome, LinkBufferCopyLinkRequest,
        LinkBufferFinalizationRequest,
    };
    use crate::jit::{CodeFinalizationAuthority, CodeLiveness, CodeOrigin, CodeOriginKind};
    use crate::jit::{CodeOwnership, EntryAbi, Entrypoint, EntrypointKind, JitCodeId};
    use crate::runtime::{CodeBlockId, NativeCodeId};

    fn owner() -> CodeBlockId {
        CodeBlockId(CellId(1))
    }

    fn machine_code() -> MachineCodeHandle {
        let allocation = ExecutableAllocationId(7);
        MachineCodeHandle {
            allocation,
            owner: MachineCodeOwnership::CodeBlock(owner()),
            range: MachineCodeRange {
                allocation,
                start_offset: 0,
                size_bytes: 64,
            },
            symbol: Some(NativeCodeId(10)),
            protection: ExecutableMemoryProtection::Executable,
            lifecycle: ExecutableAllocationLifecycle::LinkedExecutable,
            mutation_authority: ExecutableMutationAuthority::LinkBuffer,
        }
    }

    fn baseline_artifact() -> JitCodeArtifact {
        let code = JitCodeId(9);
        JitCodeArtifact {
            id: code,
            tier: JitType::Baseline,
            origin: CodeOrigin {
                kind: CodeOriginKind::BaselineCodeBlock,
                owner: Some(owner()),
                executable: None,
                bytecode_index: Some(0),
            },
            ownership: CodeOwnership::CodeBlockOwned,
            native_code: Some(NativeCodeId(10)),
            machine_code: Some(machine_code()),
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

    fn accepted_finalization() -> LinkBufferFinalizationRecord {
        let artifact = baseline_artifact();
        let machine_code = artifact.machine_code.expect("machine code");
        let buffer = AssemblerBufferDescriptor::builder(AssemblerBufferId(artifact.id.0))
            .architecture(AssemblerArchitecture::X86_64)
            .lifecycle(AssemblerBufferLifecycle::FrozenForLink)
            .capacity(machine_code.range.size_bytes, machine_code.range.size_bytes)
            .build()
            .expect("buffer");
        let allocation = ExecutableAllocationRecord::from_request(
            ExecutableAllocationRequest::allocated_writable(
                machine_code.allocation,
                machine_code.owner,
                machine_code.range.size_bytes,
            ),
        );
        let layout = plan_link_buffer_layout(
            &buffer,
            LinkBufferProfile::Baseline,
            Some(machine_code.allocation),
        )
        .expect("layout");
        let image = describe_assembler_byte_image(
            &buffer,
            AssemblerByteImageId(artifact.id.0),
            AssemblerByteImageDigest(0xfeed),
        )
        .expect("assembler byte image");
        let copy_link = record_link_buffer_copy_link(LinkBufferCopyLinkRequest {
            source: &buffer,
            source_image: &image,
            layout: &layout,
            allocation: &allocation,
            code_id: artifact.id,
            owner: machine_code.owner,
        });
        assert_eq!(copy_link.outcome, LinkBufferCopyLinkOutcome::Accepted);
        let record = finalize_link_buffer(LinkBufferFinalizationRequest {
            copy_link: &copy_link,
            code_id: artifact.id,
            owner: machine_code.owner,
            symbol: machine_code.symbol,
        });
        assert_eq!(record.outcome, LinkBufferFinalizationOutcome::Accepted);
        record
    }

    fn accepted_byte_finalization() -> LinkBufferFinalizationRecord {
        accepted_byte_finalization_with_bytes().0
    }

    fn accepted_byte_finalization_with_bytes() -> (LinkBufferFinalizationRecord, Vec<u8>) {
        let artifact = baseline_artifact();
        let machine_code = artifact.machine_code.expect("machine code");
        let buffer = AssemblerBufferDescriptor::builder(AssemblerBufferId(artifact.id.0))
            .architecture(AssemblerArchitecture::X86_64)
            .lifecycle(AssemblerBufferLifecycle::FrozenForLink)
            .capacity(machine_code.range.size_bytes, machine_code.range.size_bytes)
            .build()
            .expect("buffer");
        let allocation = ExecutableAllocationRecord::from_request(
            ExecutableAllocationRequest::allocated_writable(
                machine_code.allocation,
                machine_code.owner,
                machine_code.range.size_bytes,
            ),
        );
        let layout = plan_link_buffer_layout(
            &buffer,
            LinkBufferProfile::Baseline,
            Some(machine_code.allocation),
        )
        .expect("layout");
        let bytes: Vec<u8> = (0..machine_code.range.size_bytes)
            .map(|index| (index as u8).wrapping_mul(5).wrapping_add(1))
            .collect();
        let image =
            freeze_assembler_byte_image(&buffer, AssemblerByteImageId(artifact.id.0), bytes)
                .expect("assembler byte image");
        let linked_image = link_assembler_byte_image(&image, &layout).expect("linked image");
        let copy_link =
            record_link_buffer_copy_link_with_linked_image(LinkBufferLinkedCopyLinkRequest {
                source: &buffer,
                source_image: &image,
                linked_image: &linked_image,
                layout: &layout,
                allocation: &allocation,
                code_id: artifact.id,
                owner: machine_code.owner,
            });
        assert_eq!(copy_link.outcome, LinkBufferCopyLinkOutcome::Accepted);
        let record = finalize_link_buffer(LinkBufferFinalizationRequest {
            copy_link: &copy_link,
            code_id: artifact.id,
            owner: machine_code.owner,
            symbol: machine_code.symbol,
        });
        assert_eq!(record.outcome, LinkBufferFinalizationOutcome::Accepted);
        assert!(record.byte_copy_evidence.is_some());
        (record, linked_image.bytes().to_vec())
    }

    fn allocation_record() -> ExecutableMemoryAllocationRecord {
        let finalization = accepted_finalization();
        allocation_record_for(&finalization)
    }

    fn allocation_record_for(
        finalization: &LinkBufferFinalizationRecord,
    ) -> ExecutableMemoryAllocationRecord {
        let allocation = ExecutableAllocationRecord::from_request(
            ExecutableAllocationRequest::allocated_writable(
                finalization.allocation,
                finalization.owner,
                finalization.allocation_range.size_bytes,
            ),
        );
        record_executable_memory_allocation(ExecutableMemoryAllocationRequest {
            ordinal: ExecutableMemoryOperationOrdinal(1),
            allocation,
            mapped_range: ExecutableMemoryMappedRange {
                allocation: finalization.allocation,
                start_offset: 0,
                byte_len: 4096,
                page_size: ExecutableMemoryPageSize(4096),
            },
        })
    }

    fn allocation_record_for_id_owner(
        finalization: &LinkBufferFinalizationRecord,
        allocation_id: ExecutableAllocationId,
        owner: MachineCodeOwnership,
    ) -> ExecutableMemoryAllocationRecord {
        let allocation = ExecutableAllocationRecord::from_request(
            ExecutableAllocationRequest::allocated_writable(
                allocation_id,
                owner,
                finalization.allocation_range.size_bytes,
            ),
        );
        record_executable_memory_allocation(ExecutableMemoryAllocationRequest {
            ordinal: ExecutableMemoryOperationOrdinal(1),
            allocation,
            mapped_range: ExecutableMemoryMappedRange {
                allocation: allocation_id,
                start_offset: 0,
                byte_len: 4096,
                page_size: ExecutableMemoryPageSize(4096),
            },
        })
    }

    fn transition_record(
        allocation: &ExecutableMemoryAllocationRecord,
    ) -> ExecutableMemoryProtectionTransitionRecord {
        transition_record_at(allocation, ExecutableMemoryOperationOrdinal(2))
    }

    fn transition_record_at(
        allocation: &ExecutableMemoryAllocationRecord,
        ordinal: ExecutableMemoryOperationOrdinal,
    ) -> ExecutableMemoryProtectionTransitionRecord {
        record_executable_memory_protection_transition(
            ExecutableMemoryProtectionTransitionRequest {
                ordinal,
                allocation: allocation.clone(),
                transition: JitPermissionTransition::RwToRx,
                start_protection: ExecutableMemoryProtection::Writable,
                start_lifecycle: ExecutableAllocationLifecycle::AllocatedWritable,
                end_protection: ExecutableMemoryProtection::Executable,
                end_lifecycle: ExecutableAllocationLifecycle::LinkedExecutable,
                mutation_authority: ExecutableMutationAuthority::LinkBuffer,
            },
        )
    }

    fn flush_record(
        finalization: &LinkBufferFinalizationRecord,
        transition: &ExecutableMemoryProtectionTransitionRecord,
    ) -> InstructionCacheFlushRecord {
        flush_record_at(
            finalization,
            transition,
            ExecutableMemoryOperationOrdinal(3),
        )
    }

    fn flush_record_at(
        finalization: &LinkBufferFinalizationRecord,
        transition: &ExecutableMemoryProtectionTransitionRecord,
        ordinal: ExecutableMemoryOperationOrdinal,
    ) -> InstructionCacheFlushRecord {
        record_instruction_cache_flush(InstructionCacheFlushRequest {
            ordinal,
            transition: transition.clone(),
            code_id: finalization.code_id,
            owner: finalization.owner,
            allocation: finalization.allocation,
            range: finalization.allocation_range,
        })
    }

    fn byte_copy_record(
        finalization: &LinkBufferFinalizationRecord,
        allocation: &ExecutableMemoryAllocationRecord,
    ) -> ExecutableMemoryByteCopyRecord {
        byte_copy_record_at(
            finalization,
            allocation,
            ExecutableMemoryOperationOrdinal(2),
        )
    }

    fn byte_copy_record_at(
        finalization: &LinkBufferFinalizationRecord,
        allocation: &ExecutableMemoryAllocationRecord,
        ordinal: ExecutableMemoryOperationOrdinal,
    ) -> ExecutableMemoryByteCopyRecord {
        let evidence = finalization
            .byte_copy_evidence
            .as_ref()
            .expect("byte copy evidence");
        record_executable_memory_byte_copy(ExecutableMemoryByteCopyRequest {
            ordinal,
            allocation,
            link_finalization: finalization,
            copied_range: finalization.allocation_range,
            copied_byte_len: evidence.output_byte_len,
            copied_digest: evidence.output_digest,
        })
    }

    fn accepted_residency_for(
        finalization: &LinkBufferFinalizationRecord,
    ) -> ExecutableMemoryResidencyRecord {
        let allocation = allocation_record_for(finalization);
        let transition = transition_record(&allocation);
        let flush = flush_record(finalization, &transition);
        let record = record_executable_memory_residency(ExecutableMemoryResidencyRequest {
            link_finalization: finalization,
            allocation: &allocation,
            protection_transition: &transition,
            instruction_cache_flush: Some(&flush),
        });
        assert_eq!(record.outcome, ExecutableMemoryResidencyOutcome::Accepted);
        record
    }

    #[test]
    fn executable_memory_residency_accepts_page_rounded_mapping_metadata() {
        let finalization = accepted_finalization();
        let record = accepted_residency_for(&finalization);
        let machine_code = finalization.validate_accepted().unwrap();

        assert_eq!(
            record.validate_accepted_for_link_finalization(&finalization),
            Ok(machine_code)
        );
        assert_eq!(record.mapped_range.byte_len, 4096);
        assert_eq!(record.machine_range.size_bytes, 64);
        assert_eq!(record.ordinal, None);
        assert_eq!(record.byte_copy, None);
        assert_eq!(
            record.validate_accepted_with_byte_evidence(&finalization),
            Err(ExecutableMemoryResidencyValidationError::ByteCopyMissing)
        );
    }

    #[test]
    fn executable_memory_byte_copy_accepts_linked_byte_evidence() {
        let finalization = accepted_byte_finalization();
        let allocation = allocation_record_for(&finalization);
        let copy = byte_copy_record(&finalization, &allocation);
        let evidence = finalization.byte_copy_evidence.as_ref().unwrap();

        assert_eq!(copy.outcome, ExecutableMemoryResidencyOutcome::Accepted);
        assert_eq!(copy.ordinal, ExecutableMemoryOperationOrdinal(2));
        assert_eq!(copy.allocation_ordinal, allocation.ordinal);
        assert_eq!(copy.allocation, finalization.allocation);
        assert_eq!(copy.owner, finalization.owner);
        assert_eq!(copy.copied_range, finalization.allocation_range);
        assert_eq!(copy.copied_byte_len, evidence.output_byte_len);
        assert_eq!(copy.copied_digest, evidence.output_digest);
        assert_eq!(
            copy.byte_copy_evidence.unwrap(),
            ExecutableMemoryByteCopyEvidence {
                source_image: evidence.source_image,
                source_digest: evidence.source_digest,
                copied_digest: evidence.output_digest,
                copied_byte_len: evidence.output_byte_len,
                relocation_count: evidence.relocation_count,
                profile: evidence.profile,
                state: evidence.state,
            }
        );
    }

    #[test]
    fn executable_memory_byte_copy_rejects_missing_or_mismatched_evidence() {
        let descriptor_finalization = accepted_finalization();
        let descriptor_allocation = allocation_record_for(&descriptor_finalization);
        let descriptor_copy = record_executable_memory_byte_copy(ExecutableMemoryByteCopyRequest {
            ordinal: ExecutableMemoryOperationOrdinal(2),
            allocation: &descriptor_allocation,
            link_finalization: &descriptor_finalization,
            copied_range: descriptor_finalization.allocation_range,
            copied_byte_len: descriptor_finalization.allocation_range.size_bytes,
            copied_digest: AssemblerByteImageDigest(0xfeed),
        });
        assert!(matches!(
            descriptor_copy.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason: ExecutableMemoryResidencyValidationError::ByteCopyEvidenceMissing,
            }
        ));

        let finalization = accepted_byte_finalization();
        let allocation = allocation_record_for(&finalization);
        let evidence = finalization.byte_copy_evidence.as_ref().unwrap();

        let digest_mismatch = record_executable_memory_byte_copy(ExecutableMemoryByteCopyRequest {
            ordinal: ExecutableMemoryOperationOrdinal(2),
            allocation: &allocation,
            link_finalization: &finalization,
            copied_range: finalization.allocation_range,
            copied_byte_len: evidence.output_byte_len,
            copied_digest: AssemblerByteImageDigest(evidence.output_digest.0.wrapping_add(1)),
        });
        assert!(matches!(
            digest_mismatch.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason: ExecutableMemoryResidencyValidationError::ByteCopyDigestMismatch { .. },
            }
        ));

        let length_mismatch = record_executable_memory_byte_copy(ExecutableMemoryByteCopyRequest {
            ordinal: ExecutableMemoryOperationOrdinal(2),
            allocation: &allocation,
            link_finalization: &finalization,
            copied_range: finalization.allocation_range,
            copied_byte_len: evidence.output_byte_len + 1,
            copied_digest: evidence.output_digest,
        });
        assert!(matches!(
            length_mismatch.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason: ExecutableMemoryResidencyValidationError::ByteCopyLengthMismatch { .. },
            }
        ));

        let range_mismatch = record_executable_memory_byte_copy(ExecutableMemoryByteCopyRequest {
            ordinal: ExecutableMemoryOperationOrdinal(2),
            allocation: &allocation,
            link_finalization: &finalization,
            copied_range: MachineCodeRange {
                start_offset: 1,
                ..finalization.allocation_range
            },
            copied_byte_len: evidence.output_byte_len,
            copied_digest: evidence.output_digest,
        });
        assert!(matches!(
            range_mismatch.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason: ExecutableMemoryResidencyValidationError::ByteCopyRangeMismatch { .. },
            }
        ));

        let allocation_mismatch = allocation_record_for_id_owner(
            &finalization,
            ExecutableAllocationId(999),
            finalization.owner,
        );
        let allocation_copy = record_executable_memory_byte_copy(ExecutableMemoryByteCopyRequest {
            ordinal: ExecutableMemoryOperationOrdinal(2),
            allocation: &allocation_mismatch,
            link_finalization: &finalization,
            copied_range: finalization.allocation_range,
            copied_byte_len: evidence.output_byte_len,
            copied_digest: evidence.output_digest,
        });
        assert!(matches!(
            allocation_copy.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason: ExecutableMemoryResidencyValidationError::ByteCopyAllocationMismatch {
                    actual: ExecutableAllocationId(999),
                    ..
                },
            }
        ));

        let owner_mismatch = allocation_record_for_id_owner(
            &finalization,
            finalization.allocation,
            MachineCodeOwnership::Host,
        );
        let owner_copy = record_executable_memory_byte_copy(ExecutableMemoryByteCopyRequest {
            ordinal: ExecutableMemoryOperationOrdinal(2),
            allocation: &owner_mismatch,
            link_finalization: &finalization,
            copied_range: finalization.allocation_range,
            copied_byte_len: evidence.output_byte_len,
            copied_digest: evidence.output_digest,
        });
        assert!(matches!(
            owner_copy.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason: ExecutableMemoryResidencyValidationError::ByteCopyOwnerMismatch {
                    actual: MachineCodeOwnership::Host,
                    ..
                },
            }
        ));

        let bad_order = record_executable_memory_byte_copy(ExecutableMemoryByteCopyRequest {
            ordinal: allocation.ordinal,
            allocation: &allocation,
            link_finalization: &finalization,
            copied_range: finalization.allocation_range,
            copied_byte_len: evidence.output_byte_len,
            copied_digest: evidence.output_digest,
        });
        assert!(matches!(
            bad_order.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason: ExecutableMemoryResidencyValidationError::OperationOrderingInvalid {
                    before: ExecutableMemoryOperationOrdinal(1),
                    after: ExecutableMemoryOperationOrdinal(1),
                },
            }
        ));
    }

    #[test]
    fn executable_memory_byte_residency_accepts_and_strictly_revalidates_copy() {
        let finalization = accepted_byte_finalization();
        let allocation = allocation_record_for(&finalization);
        let byte_copy = byte_copy_record(&finalization, &allocation);
        assert_eq!(
            byte_copy.outcome,
            ExecutableMemoryResidencyOutcome::Accepted
        );
        let transition = transition_record_at(&allocation, ExecutableMemoryOperationOrdinal(3));
        let flush = flush_record_at(
            &finalization,
            &transition,
            ExecutableMemoryOperationOrdinal(4),
        );
        let residency = record_executable_memory_residency_with_byte_copy(
            ExecutableMemoryByteResidencyRequest {
                ordinal: ExecutableMemoryOperationOrdinal(5),
                link_finalization: &finalization,
                allocation: &allocation,
                byte_copy: &byte_copy,
                protection_transition: &transition,
                instruction_cache_flush: Some(&flush),
            },
        );
        let machine_code = finalization.validate_accepted_with_byte_evidence().unwrap();

        assert_eq!(
            residency.outcome,
            ExecutableMemoryResidencyOutcome::Accepted
        );
        assert_eq!(residency.ordinal, Some(ExecutableMemoryOperationOrdinal(5)));
        assert_eq!(residency.byte_copy, Some(byte_copy.clone()));
        assert_eq!(
            residency.validate_accepted_with_byte_evidence(&finalization),
            Ok(machine_code)
        );
        assert_eq!(
            residency.validate_accepted_for_link_finalization(&finalization),
            Ok(machine_code)
        );

        let mut missing_copy = residency.clone();
        missing_copy.byte_copy = None;
        assert_eq!(
            missing_copy.validate_accepted_with_byte_evidence(&finalization),
            Err(ExecutableMemoryResidencyValidationError::ByteCopyMissing)
        );

        let mut tampered_copy = residency.clone();
        tampered_copy
            .byte_copy
            .as_mut()
            .expect("byte copy")
            .copied_digest = AssemblerByteImageDigest(0xdead);
        assert!(matches!(
            tampered_copy.validate_accepted_with_byte_evidence(&finalization),
            Err(ExecutableMemoryResidencyValidationError::ByteCopyDigestMismatch { .. })
        ));

        let mut mismatched_finalization = finalization.clone();
        let mut mismatched_evidence = mismatched_finalization
            .byte_copy_evidence
            .expect("byte copy evidence");
        mismatched_evidence.output_digest = AssemblerByteImageDigest(0xdead);
        mismatched_finalization.byte_copy_evidence = Some(mismatched_evidence);
        assert!(matches!(
            residency.validate_accepted_with_byte_evidence(&mismatched_finalization),
            Err(ExecutableMemoryResidencyValidationError::ByteCopyEvidenceMismatch { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn platform_backed_residency_materializes_compartment_and_byte_evidenced_records() {
        let (finalization, bytes) = accepted_byte_finalization_with_bytes();
        let platform_residency = materialize_platform_executable_memory_residency(
            ExecutableMemoryPlatformResidencyRequest::new(
                ExecutableMemoryOperationOrdinal(1),
                &finalization,
                &bytes,
            ),
        )
        .unwrap();
        let residency = &platform_residency.residency;
        let byte_copy = residency.byte_copy.as_ref().expect("byte copy");
        let flush = residency
            .instruction_cache_flush
            .as_ref()
            .expect("flush marker");
        let machine_code = finalization.validate_accepted_with_byte_evidence().unwrap();

        assert_eq!(
            residency.outcome,
            ExecutableMemoryResidencyOutcome::Accepted
        );
        assert_eq!(
            residency.allocation_record.ordinal,
            ExecutableMemoryOperationOrdinal(1)
        );
        assert_eq!(byte_copy.ordinal, ExecutableMemoryOperationOrdinal(2));
        assert_eq!(
            residency.protection_transition.ordinal,
            ExecutableMemoryOperationOrdinal(3)
        );
        assert_eq!(flush.ordinal, ExecutableMemoryOperationOrdinal(4));
        assert_eq!(residency.ordinal, Some(ExecutableMemoryOperationOrdinal(5)));
        assert_eq!(
            residency.validate_accepted_with_byte_evidence(&finalization),
            Ok(machine_code)
        );
        assert_eq!(
            platform_residency.evidence.compartment.protection(),
            ExecutableMemoryProtection::Executable
        );
        assert_eq!(
            platform_residency.evidence.compartment.lifecycle(),
            ExecutableAllocationLifecycle::LinkedExecutable
        );
        assert_eq!(
            platform_residency.evidence.finalized_machine_code,
            machine_code
        );
        assert_eq!(
            platform_residency.evidence.platform_machine_code.range,
            machine_code.range
        );
        assert_eq!(
            platform_residency.evidence.platform_machine_code.symbol,
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn platform_backed_residency_rejects_wrong_linked_bytes_digest() {
        let (finalization, mut bytes) = accepted_byte_finalization_with_bytes();
        let evidence = finalization.byte_copy_evidence.as_ref().unwrap();
        bytes[0] ^= 0xff;
        let actual_digest = crate::assembler::compute_assembler_byte_image_digest(&bytes);

        let error = materialize_platform_executable_memory_residency(
            ExecutableMemoryPlatformResidencyRequest::new(
                ExecutableMemoryOperationOrdinal(1),
                &finalization,
                &bytes,
            ),
        )
        .unwrap_err();

        assert_eq!(
            error,
            ExecutableMemoryPlatformResidencyError::ResidencyValidation {
                reason: ExecutableMemoryResidencyValidationError::ByteCopyDigestMismatch {
                    expected: evidence.output_digest,
                    actual: actual_digest,
                },
            }
        );
    }

    #[test]
    fn platform_backed_residency_rejects_descriptor_only_finalization() {
        let finalization = accepted_finalization();
        let bytes = vec![0x90; finalization.allocation_range.size_bytes as usize];

        let error = materialize_platform_executable_memory_residency(
            ExecutableMemoryPlatformResidencyRequest::new(
                ExecutableMemoryOperationOrdinal(1),
                &finalization,
                &bytes,
            ),
        )
        .unwrap_err();

        assert_eq!(
            error,
            ExecutableMemoryPlatformResidencyError::ResidencyValidation {
                reason: ExecutableMemoryResidencyValidationError::ByteCopyEvidenceMissing,
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn platform_backed_residency_rejects_mismatched_byte_length() {
        let (finalization, bytes) = accepted_byte_finalization_with_bytes();
        let evidence = finalization.byte_copy_evidence.as_ref().unwrap();
        let short_bytes = &bytes[..bytes.len() - 1];

        let error = materialize_platform_executable_memory_residency(
            ExecutableMemoryPlatformResidencyRequest::new(
                ExecutableMemoryOperationOrdinal(1),
                &finalization,
                short_bytes,
            ),
        )
        .unwrap_err();

        assert_eq!(
            error,
            ExecutableMemoryPlatformResidencyError::ResidencyValidation {
                reason: ExecutableMemoryResidencyValidationError::ByteCopyLengthMismatch {
                    expected: evidence.output_byte_len,
                    actual: short_bytes.len() as u32,
                },
            }
        );
    }

    #[test]
    fn platform_backed_residency_rejects_invalid_ordinal_range() {
        let finalization = accepted_finalization();
        let bytes = vec![0x90; finalization.allocation_range.size_bytes as usize];

        let zero = materialize_platform_executable_memory_residency(
            ExecutableMemoryPlatformResidencyRequest::new(
                ExecutableMemoryOperationOrdinal(0),
                &finalization,
                &bytes,
            ),
        )
        .unwrap_err();
        assert_eq!(
            zero,
            ExecutableMemoryPlatformResidencyError::ResidencyValidation {
                reason: ExecutableMemoryResidencyValidationError::OperationOrdinalZero,
            }
        );

        let overflow = materialize_platform_executable_memory_residency(
            ExecutableMemoryPlatformResidencyRequest::new(
                ExecutableMemoryOperationOrdinal(u64::MAX - 3),
                &finalization,
                &bytes,
            ),
        )
        .unwrap_err();
        assert_eq!(
            overflow,
            ExecutableMemoryPlatformResidencyError::OperationOrdinalOverflow {
                first_ordinal: ExecutableMemoryOperationOrdinal(u64::MAX - 3),
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn platform_backed_residency_keeps_compartment_rx_after_residency() {
        let (finalization, bytes) = accepted_byte_finalization_with_bytes();
        let mut platform_residency = materialize_platform_executable_memory_residency(
            ExecutableMemoryPlatformResidencyRequest::new(
                ExecutableMemoryOperationOrdinal(1),
                &finalization,
                &bytes,
            ),
        )
        .unwrap();

        assert_eq!(
            platform_residency
                .evidence
                .compartment
                .copy_from_slice(finalization.allocation_range, &bytes),
            Err(ExecutableMemoryCompartmentError::WrongProtection {
                expected: ExecutableMemoryProtection::Writable,
                actual: ExecutableMemoryProtection::Executable,
            })
        );
        assert_eq!(
            platform_residency
                .residency
                .validate_accepted_with_byte_evidence(&finalization),
            Ok(platform_residency.evidence.finalized_machine_code)
        );
    }

    #[test]
    fn executable_memory_byte_residency_rejects_bad_operation_ordering() {
        let finalization = accepted_byte_finalization();
        let allocation = allocation_record_for(&finalization);
        let byte_copy = byte_copy_record(&finalization, &allocation);
        let evidence = finalization.byte_copy_evidence.as_ref().unwrap();
        let rejected_byte_copy =
            record_executable_memory_byte_copy(ExecutableMemoryByteCopyRequest {
                ordinal: ExecutableMemoryOperationOrdinal(2),
                allocation: &allocation,
                link_finalization: &finalization,
                copied_range: finalization.allocation_range,
                copied_byte_len: evidence.output_byte_len,
                copied_digest: AssemblerByteImageDigest(evidence.output_digest.0.wrapping_add(1)),
            });
        let transition = transition_record_at(&allocation, byte_copy.ordinal);
        let flush = flush_record_at(
            &finalization,
            &transition,
            ExecutableMemoryOperationOrdinal(4),
        );
        let residency = record_executable_memory_residency_with_byte_copy(
            ExecutableMemoryByteResidencyRequest {
                ordinal: ExecutableMemoryOperationOrdinal(5),
                link_finalization: &finalization,
                allocation: &allocation,
                byte_copy: &byte_copy,
                protection_transition: &transition,
                instruction_cache_flush: Some(&flush),
            },
        );

        assert!(matches!(
            residency.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason: ExecutableMemoryResidencyValidationError::OperationOrderingInvalid {
                    before: ExecutableMemoryOperationOrdinal(2),
                    after: ExecutableMemoryOperationOrdinal(2),
                },
            }
        ));

        let transition = transition_record_at(&allocation, ExecutableMemoryOperationOrdinal(3));
        let flush = flush_record_at(
            &finalization,
            &transition,
            ExecutableMemoryOperationOrdinal(4),
        );
        let rejected_copy_residency = record_executable_memory_residency_with_byte_copy(
            ExecutableMemoryByteResidencyRequest {
                ordinal: ExecutableMemoryOperationOrdinal(5),
                link_finalization: &finalization,
                allocation: &allocation,
                byte_copy: &rejected_byte_copy,
                protection_transition: &transition,
                instruction_cache_flush: Some(&flush),
            },
        );
        assert!(matches!(
            &rejected_copy_residency.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason:
                    ExecutableMemoryResidencyValidationError::ByteCopyRecordRejected { reason },
            } if matches!(
                reason.as_ref(),
                ExecutableMemoryResidencyValidationError::ByteCopyDigestMismatch { .. }
            )
        ));

        let transition = transition_record_at(&allocation, ExecutableMemoryOperationOrdinal(3));
        let flush = flush_record_at(
            &finalization,
            &transition,
            ExecutableMemoryOperationOrdinal(4),
        );
        let stale_residency_ordinal = record_executable_memory_residency_with_byte_copy(
            ExecutableMemoryByteResidencyRequest {
                ordinal: flush.ordinal,
                link_finalization: &finalization,
                allocation: &allocation,
                byte_copy: &byte_copy,
                protection_transition: &transition,
                instruction_cache_flush: Some(&flush),
            },
        );
        assert!(matches!(
            stale_residency_ordinal.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason: ExecutableMemoryResidencyValidationError::OperationOrderingInvalid {
                    before: ExecutableMemoryOperationOrdinal(4),
                    after: ExecutableMemoryOperationOrdinal(4),
                },
            }
        ));
    }

    #[test]
    fn executable_memory_residency_rejects_bad_mapping_metadata() {
        let finalization = accepted_finalization();
        let ledger_allocation = ExecutableAllocationRecord::from_request(
            ExecutableAllocationRequest::allocated_writable(
                finalization.allocation,
                finalization.owner,
                finalization.allocation_range.size_bytes,
            ),
        );

        let undersized = record_executable_memory_allocation(ExecutableMemoryAllocationRequest {
            ordinal: ExecutableMemoryOperationOrdinal(1),
            allocation: ledger_allocation.clone(),
            mapped_range: ExecutableMemoryMappedRange {
                allocation: finalization.allocation,
                start_offset: 0,
                byte_len: 32,
                page_size: ExecutableMemoryPageSize(16),
            },
        });
        assert!(matches!(
            undersized.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason:
                    ExecutableMemoryResidencyValidationError::MappedRangeDoesNotContainMachineRange {
                        ..
                    },
            }
        ));

        let unaligned = record_executable_memory_allocation(ExecutableMemoryAllocationRequest {
            ordinal: ExecutableMemoryOperationOrdinal(1),
            allocation: ledger_allocation,
            mapped_range: ExecutableMemoryMappedRange {
                allocation: finalization.allocation,
                start_offset: 1,
                byte_len: 4096,
                page_size: ExecutableMemoryPageSize(4096),
            },
        });
        assert!(matches!(
            unaligned.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason: ExecutableMemoryResidencyValidationError::MappedRangeStartNotPageAligned { .. },
            }
        ));
    }

    #[test]
    fn executable_memory_residency_requires_rw_to_rx_link_buffer_transition() {
        let allocation = allocation_record();

        let rwx_transition = record_executable_memory_protection_transition(
            ExecutableMemoryProtectionTransitionRequest {
                ordinal: ExecutableMemoryOperationOrdinal(2),
                allocation: allocation.clone(),
                transition: JitPermissionTransition::RwxToRx,
                start_protection: ExecutableMemoryProtection::Writable,
                start_lifecycle: ExecutableAllocationLifecycle::AllocatedWritable,
                end_protection: ExecutableMemoryProtection::Executable,
                end_lifecycle: ExecutableAllocationLifecycle::LinkedExecutable,
                mutation_authority: ExecutableMutationAuthority::LinkBuffer,
            },
        );
        assert!(matches!(
            rwx_transition.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason: ExecutableMemoryResidencyValidationError::PermissionTransitionMismatch {
                    actual: JitPermissionTransition::RwxToRx,
                    ..
                },
            }
        ));

        let patch_transition = record_executable_memory_protection_transition(
            ExecutableMemoryProtectionTransitionRequest {
                ordinal: ExecutableMemoryOperationOrdinal(2),
                allocation,
                transition: JitPermissionTransition::RwToRx,
                start_protection: ExecutableMemoryProtection::WritableForPatching,
                start_lifecycle: ExecutableAllocationLifecycle::TemporarilyWritableForPatch,
                end_protection: ExecutableMemoryProtection::Executable,
                end_lifecycle: ExecutableAllocationLifecycle::LinkedExecutable,
                mutation_authority: ExecutableMutationAuthority::InlineCachePatcher,
            },
        );
        assert!(matches!(
            patch_transition.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason: ExecutableMemoryResidencyValidationError::TransitionStartStateMismatch {
                    protection: ExecutableMemoryProtection::WritableForPatching,
                    ..
                },
            }
        ));
    }

    #[test]
    fn executable_memory_residency_rejects_missing_or_stale_cache_flush() {
        let finalization = accepted_finalization();
        let allocation = allocation_record_for(&finalization);
        let transition = transition_record(&allocation);
        let missing_flush = record_executable_memory_residency(ExecutableMemoryResidencyRequest {
            link_finalization: &finalization,
            allocation: &allocation,
            protection_transition: &transition,
            instruction_cache_flush: None,
        });
        assert!(matches!(
            missing_flush.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason: ExecutableMemoryResidencyValidationError::CacheFlushMissing,
            }
        ));

        let stale_flush = record_instruction_cache_flush(InstructionCacheFlushRequest {
            ordinal: ExecutableMemoryOperationOrdinal(3),
            transition: transition.clone(),
            code_id: JitCodeId(999),
            owner: finalization.owner,
            allocation: finalization.allocation,
            range: finalization.allocation_range,
        });
        let stale_residency =
            record_executable_memory_residency(ExecutableMemoryResidencyRequest {
                link_finalization: &finalization,
                allocation: &allocation,
                protection_transition: &transition,
                instruction_cache_flush: Some(&stale_flush),
            });
        assert!(matches!(
            stale_residency.outcome,
            ExecutableMemoryResidencyOutcome::Rejected {
                reason: ExecutableMemoryResidencyValidationError::CacheFlushCodeIdMismatch {
                    actual: JitCodeId(999),
                    ..
                },
            }
        ));
    }

    #[test]
    fn executable_memory_residency_revalidates_against_tampered_finalization() {
        let finalization = accepted_finalization();
        let record = accepted_residency_for(&finalization);

        let mut stale_symbol = finalization.clone();
        stale_symbol.symbol = Some(NativeCodeId(999));
        assert!(matches!(
            record.validate_accepted_for_link_finalization(&stale_symbol),
            Err(ExecutableMemoryResidencyValidationError::LinkFinalizationInvalid { .. })
        ));

        let mut stale_range = finalization;
        stale_range.allocation_range.size_bytes = 32;
        assert!(matches!(
            record.validate_accepted_for_link_finalization(&stale_range),
            Err(ExecutableMemoryResidencyValidationError::LinkFinalizationInvalid { .. })
        ));

        let mut stale_copy_link = accepted_finalization();
        stale_copy_link.copy_link.digest = AssemblerByteImageDigest(0xdead);
        assert!(matches!(
            record.validate_accepted_for_link_finalization(&stale_copy_link),
            Err(
                ExecutableMemoryResidencyValidationError::LinkFinalizationInvalid {
                    reason,
                },
            ) if *reason == crate::jit::ExecutableLedgerValidationError::CopyLinkProofMissing
        ));
    }

    #[test]
    fn executable_memory_residency_rejects_tampered_accepted_record() {
        let finalization = accepted_finalization();
        let mut record = accepted_residency_for(&finalization);
        record.proof = ExecutableMemoryResidencyProof::default();
        assert_eq!(
            record.validate_accepted_for_link_finalization(&finalization),
            Err(ExecutableMemoryResidencyValidationError::ResidencyProofMissing)
        );

        let mut stale_allocation_proof = accepted_residency_for(&finalization);
        stale_allocation_proof.allocation_record.proof = ExecutableMemoryOperationProof::default();
        assert_eq!(
            stale_allocation_proof.validate_accepted_for_link_finalization(&finalization),
            Err(ExecutableMemoryResidencyValidationError::AllocationProofMissing)
        );

        let mut stale_transition_proof = accepted_residency_for(&finalization);
        stale_transition_proof.protection_transition.proof =
            ExecutableMemoryOperationProof::default();
        assert_eq!(
            stale_transition_proof.validate_accepted_for_link_finalization(&finalization),
            Err(ExecutableMemoryResidencyValidationError::TransitionProofMissing)
        );

        let mut stale_flush_proof = accepted_residency_for(&finalization);
        stale_flush_proof
            .instruction_cache_flush
            .as_mut()
            .expect("flush")
            .proof = ExecutableMemoryOperationProof::default();
        assert_eq!(
            stale_flush_proof.validate_accepted_for_link_finalization(&finalization),
            Err(ExecutableMemoryResidencyValidationError::CacheFlushProofMissing)
        );

        let mut stale_code_id = accepted_residency_for(&finalization);
        stale_code_id.code_id = JitCodeId(999);
        assert_eq!(
            stale_code_id.validate_accepted_for_link_finalization(&finalization),
            Err(
                ExecutableMemoryResidencyValidationError::ResidencyCodeIdMismatch {
                    expected: finalization.code_id,
                    actual: JitCodeId(999),
                }
            )
        );
    }
}
