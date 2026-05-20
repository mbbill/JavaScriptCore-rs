//! Machine-code ownership and patching descriptors.
//!
//! This module deliberately avoids executable allocation, pointer arithmetic,
//! cache flushing, and instruction encoding. It names the metadata needed by
//! future link buffers, patchpoints, and executable-memory handles.

use crate::jit::{CallBoundaryId, JitCodeId, PatchpointDescriptor};
use crate::runtime::{CodeBlockId, NativeCodeId};

/// Stable identity for an executable allocation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExecutableAllocationId(pub u64);

/// Ownership source for executable memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MachineCodeOwnership {
    CodeBlock(CodeBlockId),
    SharedStub,
    Thunk,
    WasmCallee,
    Host,
}

/// Protection state of an executable allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutableMemoryProtection {
    Unallocated,
    Writable,
    Executable,
    WritableForPatching,
    ReadOnlyData,
    Decommitted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutableAllocationLifecycle {
    Reserved,
    AllocatedWritable,
    LinkedExecutable,
    TemporarilyWritableForPatch,
    JettisonPending,
    Released,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutableMutationAuthority {
    LinkBuffer,
    RepatchBuffer,
    InlineCachePatcher,
    WatchpointInvalidator,
    AllocatorOnly,
}

/// Offset range inside an executable allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MachineCodeRange {
    pub allocation: ExecutableAllocationId,
    pub start_offset: u32,
    pub size_bytes: u32,
}

/// Opaque machine-code handle attached to a JIT artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MachineCodeHandle {
    pub allocation: ExecutableAllocationId,
    pub owner: MachineCodeOwnership,
    pub range: MachineCodeRange,
    pub symbol: Option<NativeCodeId>,
    pub protection: ExecutableMemoryProtection,
    pub lifecycle: ExecutableAllocationLifecycle,
    /// Machine-code bytes may only be mutated by the component named here while
    /// protection is writable; all other compiler layers record patch plans.
    pub mutation_authority: ExecutableMutationAuthority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MachineCodeValidationError {
    EmptyRange,
    RangeEndOverflow,
    RangeAllocationMismatch,
    ProtectionLifecycleMismatch,
    WritableAuthorityMismatch,
    PatchRangeAllocationMismatch,
    PatchBoundaryMismatch,
    PatchPlanEmpty,
    PatchPlanGenerationZero,
    PatchProtectionMismatch,
}

impl MachineCodeRange {
    pub fn validate(&self) -> Result<(), MachineCodeValidationError> {
        if self.size_bytes == 0 {
            return Err(MachineCodeValidationError::EmptyRange);
        }
        if self.end_offset().is_none() {
            return Err(MachineCodeValidationError::RangeEndOverflow);
        }
        Ok(())
    }

    pub fn end_offset(self) -> Option<u32> {
        self.start_offset.checked_add(self.size_bytes)
    }
}

impl MachineCodeHandle {
    pub fn validate(&self) -> Result<(), MachineCodeValidationError> {
        self.range.validate()?;
        if self.range.allocation != self.allocation {
            return Err(MachineCodeValidationError::RangeAllocationMismatch);
        }
        if !protection_matches_lifecycle(self.protection, self.lifecycle) {
            return Err(MachineCodeValidationError::ProtectionLifecycleMismatch);
        }
        if matches!(
            self.protection,
            ExecutableMemoryProtection::Writable | ExecutableMemoryProtection::WritableForPatching
        ) && self.mutation_authority == ExecutableMutationAuthority::AllocatorOnly
        {
            return Err(MachineCodeValidationError::WritableAuthorityMismatch);
        }

        Ok(())
    }
}

/// Relocation or patch family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelocationKind {
    Entrypoint,
    NearCall,
    FarCall,
    Jump,
    DataPointer,
    InlineCacheData,
    WatchpointJump,
    OsrExitJump,
    ExceptionHandler,
}

/// Patch lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodePatchState {
    Reserved,
    Linked,
    Armed,
    Applied,
    RolledBack,
    Invalidated,
}

/// Barrier required before a patch can be applied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatchWriteBarrier {
    MainThreadOnly,
    StopTheWorld,
    CodeBlockLock,
    ExecutableAllocatorLock,
    NoneRequired,
}

/// One patchable machine-code location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodePatchRecord {
    pub code: JitCodeId,
    pub location: PatchpointDescriptor,
    pub relocation: RelocationKind,
    pub range: Option<MachineCodeRange>,
    pub boundary: Option<CallBoundaryId>,
    pub state: CodePatchState,
}

/// Data-only patch plan. Applying the plan belongs to the future assembler and
/// executable-memory layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodePatchPlan {
    pub owner: CodeBlockId,
    pub records: Vec<CodePatchRecord>,
    pub write_barrier: PatchWriteBarrier,
    pub required_protection: ExecutableMemoryProtection,
    pub generation: u64,
}

impl CodePatchRecord {
    pub fn validate_for_plan(
        &self,
        plan: &CodePatchPlan,
    ) -> Result<(), MachineCodeValidationError> {
        if let Some(range) = self.range {
            range.validate()?;
            if self.location.owner_code != Some(self.code) {
                return Err(MachineCodeValidationError::PatchRangeAllocationMismatch);
            }
        }
        if self.boundary.is_some() != self.location.boundary.is_some() {
            return Err(MachineCodeValidationError::PatchBoundaryMismatch);
        }
        if plan.required_protection != ExecutableMemoryProtection::WritableForPatching
            && matches!(self.state, CodePatchState::Armed | CodePatchState::Applied)
        {
            return Err(MachineCodeValidationError::PatchProtectionMismatch);
        }

        Ok(())
    }
}

impl CodePatchPlan {
    pub fn validate(&self) -> Result<(), MachineCodeValidationError> {
        if self.records.is_empty() {
            return Err(MachineCodeValidationError::PatchPlanEmpty);
        }
        if self.generation == 0 {
            return Err(MachineCodeValidationError::PatchPlanGenerationZero);
        }
        for record in &self.records {
            record.validate_for_plan(self)?;
        }

        Ok(())
    }
}

pub const fn protection_matches_lifecycle(
    protection: ExecutableMemoryProtection,
    lifecycle: ExecutableAllocationLifecycle,
) -> bool {
    matches!(
        (protection, lifecycle),
        (
            ExecutableMemoryProtection::Unallocated,
            ExecutableAllocationLifecycle::Reserved
        ) | (
            ExecutableMemoryProtection::Writable,
            ExecutableAllocationLifecycle::AllocatedWritable
        ) | (
            ExecutableMemoryProtection::Executable,
            ExecutableAllocationLifecycle::LinkedExecutable
        ) | (
            ExecutableMemoryProtection::WritableForPatching,
            ExecutableAllocationLifecycle::TemporarilyWritableForPatch
        ) | (
            ExecutableMemoryProtection::Decommitted,
            ExecutableAllocationLifecycle::Released
        ) | (
            ExecutableMemoryProtection::Executable,
            ExecutableAllocationLifecycle::JettisonPending
        ) | (
            ExecutableMemoryProtection::ReadOnlyData,
            ExecutableAllocationLifecycle::LinkedExecutable
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::CellId;
    use crate::jit::{PatchpointDescriptor, PatchpointKind};
    use crate::runtime::CodeBlockId;

    #[test]
    fn machine_code_handle_rejects_mismatched_range_allocation() {
        let handle = MachineCodeHandle {
            allocation: ExecutableAllocationId(1),
            owner: MachineCodeOwnership::SharedStub,
            range: MachineCodeRange {
                allocation: ExecutableAllocationId(2),
                start_offset: 0,
                size_bytes: 8,
            },
            symbol: None,
            protection: ExecutableMemoryProtection::Executable,
            lifecycle: ExecutableAllocationLifecycle::LinkedExecutable,
            mutation_authority: ExecutableMutationAuthority::LinkBuffer,
        };

        assert_eq!(
            handle.validate(),
            Err(MachineCodeValidationError::RangeAllocationMismatch)
        );
    }

    #[test]
    fn patch_plan_rejects_armed_patch_without_writable_protection() {
        let plan = CodePatchPlan {
            owner: CodeBlockId(CellId(1)),
            records: vec![CodePatchRecord {
                code: JitCodeId(1),
                location: PatchpointDescriptor {
                    kind: PatchpointKind::Entrypoint,
                    owner_code: Some(JitCodeId(1)),
                    byte_offset: Some(0),
                    boundary: None,
                },
                relocation: RelocationKind::Entrypoint,
                range: None,
                boundary: None,
                state: CodePatchState::Armed,
            }],
            write_barrier: PatchWriteBarrier::MainThreadOnly,
            required_protection: ExecutableMemoryProtection::Executable,
            generation: 1,
        };

        assert_eq!(
            plan.validate(),
            Err(MachineCodeValidationError::PatchProtectionMismatch)
        );
    }
}
