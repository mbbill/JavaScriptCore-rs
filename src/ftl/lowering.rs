//! FTL lowering and finalization boundaries.
//!
//! FTL consumes a DFG graph, lowers through B3, then lets Air own
//! machine-near register allocation and code generation. These types describe
//! those handoff points without creating B3 values, Air instructions, link
//! buffers, or executable memory.

use crate::b3::{
    AirBlockId, AirCodeId, AirInstructionId, B3BlockId, B3ProcedureId, B3ValueId, B3ValueKind,
};
use crate::dfg::{DfgBasicBlockId, DfgGraphId, DfgNodeId, DfgOsrEntryDescriptor, DfgOsrExitId};
use crate::jit::{
    CallBoundaryId, CodeOrigin, JitCodeArtifact, JitCodeId, JitType, PatchpointDescriptor,
};
use crate::runtime::CodeBlockId;

/// Coarse FTL pipeline stage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FtlCompilationStage {
    Queued,
    CapturingDfg,
    LoweringDfgToB3,
    OptimizingB3,
    LoweringB3ToAir,
    AllocatingRegisters,
    Linking,
    Finalizing,
    ReadyToInstall,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FtlStateAuthority {
    DfgGraphOwner,
    FtlLoweringPhase,
    B3ProcedureOwner,
    LinkBufferOwner,
    FinalizerOwner,
}

/// Lowering phase name used by diagnostics and plan metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FtlLoweringPhase {
    DfgNodeSelection,
    StackmapCreation,
    PatchpointCreation,
    ExceptionTargetCreation,
    B3ProcedureValidation,
    AirLowering,
    AirRegisterAllocation,
    LinkBufferFinalization,
}

/// Non-crashing failure reason for a deferred FTL product.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoweringFailureReason {
    UnsupportedDfgNode,
    UnsupportedSpeculation,
    B3AllocationFailed,
    AirAllocationFailed,
    LinkFailed,
    WatchpointInvalidated,
    OwnerInvalidated,
    PolicyDisabled,
}

/// Mapping from a DFG source node to a B3 value or block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DfgToB3LoweringBoundary {
    pub graph: DfgGraphId,
    pub dfg_node: Option<DfgNodeId>,
    pub dfg_block: Option<DfgBasicBlockId>,
    pub b3_procedure: B3ProcedureId,
    pub b3_value: Option<B3ValueId>,
    pub b3_block: Option<B3BlockId>,
    pub value_kind: Option<B3ValueKind>,
    pub phase: FtlLoweringPhase,
}

/// Patchpoint descriptor with both DFG and B3 identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FtlPatchpointDescriptor {
    pub dfg_node: Option<DfgNodeId>,
    pub b3_value: Option<B3ValueId>,
    pub patchpoint: PatchpointDescriptor,
    pub boundary: Option<CallBoundaryId>,
    pub osr_exit: Option<DfgOsrExitId>,
}

/// Slow path call emitted as metadata only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FtlSlowPathDescriptor {
    pub origin: CodeOrigin,
    pub call_boundary: CallBoundaryId,
    pub may_throw: bool,
    pub may_reenter_vm: bool,
    pub patchpoint: Option<FtlPatchpointDescriptor>,
}

/// Exception target used by lowered code and stackmaps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FtlExceptionTarget {
    pub owner: CodeBlockId,
    pub bytecode_index: Option<u32>,
    pub target_block: Option<DfgBasicBlockId>,
    pub boundary: Option<CallBoundaryId>,
    pub patchpoint: Option<PatchpointDescriptor>,
}

/// Air generation metadata. Air remains responsible for concrete registers,
/// stack slots, and instruction emission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AirGenerationDescriptor {
    pub code: AirCodeId,
    pub procedure: B3ProcedureId,
    pub entry_block: Option<AirBlockId>,
    pub emitted_blocks: Vec<AirBlockId>,
    pub terminal_instructions: Vec<AirInstructionId>,
    pub frame_size_bytes: Option<u32>,
    pub call_arg_area_size_bytes: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FtlStateDescriptor {
    pub owner: CodeBlockId,
    pub graph: DfgGraphId,
    pub procedure: Option<B3ProcedureId>,
    pub air: Option<AirCodeId>,
    pub allocation_failed: bool,
    pub default_exception_handle: Option<CallBoundaryId>,
    pub exception_handler: Option<PatchpointDescriptor>,
    pub jump_replacements: Vec<FtlPatchpointDescriptor>,
    /// FTL State borrows DFG graph ownership in C++; Rust should preserve that
    /// separation and avoid making FTL the owner of graph lifetime.
    pub authority: FtlStateAuthority,
}

/// FTL OSR entry plan that may produce a dedicated code artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FtlOsrEntryPlan {
    pub owner: CodeBlockId,
    pub graph: DfgGraphId,
    pub entry: DfgOsrEntryDescriptor,
    pub procedure: Option<B3ProcedureId>,
    pub code: Option<JitCodeId>,
}

/// Whole FTL compilation descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FtlCompilationDescriptor {
    pub owner: CodeBlockId,
    pub graph: DfgGraphId,
    pub stage: FtlCompilationStage,
    pub procedure: Option<B3ProcedureId>,
    pub air: Option<AirGenerationDescriptor>,
    pub boundaries: Vec<DfgToB3LoweringBoundary>,
    pub patchpoints: Vec<FtlPatchpointDescriptor>,
    pub slow_paths: Vec<FtlSlowPathDescriptor>,
    pub exception_targets: Vec<FtlExceptionTarget>,
    pub failure: Option<LoweringFailureReason>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FtlValidationError {
    EmptyName,
    EmptyProvenance(&'static str),
    DuplicatePlanName(&'static str),
    EmptyStages(&'static str),
    EmptyLoweringPhases(&'static str),
    EmptyStateAuthorities(&'static str),
    StageFailureMismatch,
    MissingProcedureForB3Stage,
    MissingAirForAirStage,
    BoundaryProcedureMismatch,
    BoundaryGraphMismatch,
    PatchpointWithoutOrigin,
    SlowPathPatchpointMismatch,
    ArtifactCodeTierMismatch,
}

impl FtlCompilationDescriptor {
    pub fn validate(&self) -> Result<(), FtlValidationError> {
        if self.failure.is_some() != (self.stage == FtlCompilationStage::Failed) {
            return Err(FtlValidationError::StageFailureMismatch);
        }

        if matches!(
            self.stage,
            FtlCompilationStage::OptimizingB3
                | FtlCompilationStage::LoweringB3ToAir
                | FtlCompilationStage::AllocatingRegisters
                | FtlCompilationStage::Linking
                | FtlCompilationStage::Finalizing
                | FtlCompilationStage::ReadyToInstall
        ) && self.procedure.is_none()
        {
            return Err(FtlValidationError::MissingProcedureForB3Stage);
        }

        if matches!(
            self.stage,
            FtlCompilationStage::AllocatingRegisters
                | FtlCompilationStage::Linking
                | FtlCompilationStage::Finalizing
                | FtlCompilationStage::ReadyToInstall
        ) && self.air.is_none()
        {
            return Err(FtlValidationError::MissingAirForAirStage);
        }

        for boundary in &self.boundaries {
            if boundary.graph != self.graph {
                return Err(FtlValidationError::BoundaryGraphMismatch);
            }
            if self
                .procedure
                .is_some_and(|procedure| procedure != boundary.b3_procedure)
            {
                return Err(FtlValidationError::BoundaryProcedureMismatch);
            }
            if boundary.dfg_node.is_none()
                && boundary.dfg_block.is_none()
                && boundary.b3_value.is_none()
                && boundary.b3_block.is_none()
            {
                return Err(FtlValidationError::PatchpointWithoutOrigin);
            }
        }

        for patchpoint in &self.patchpoints {
            if patchpoint.dfg_node.is_none() && patchpoint.b3_value.is_none() {
                return Err(FtlValidationError::PatchpointWithoutOrigin);
            }
        }

        for slow_path in &self.slow_paths {
            if slow_path.may_reenter_vm && slow_path.patchpoint.is_none() {
                return Err(FtlValidationError::SlowPathPatchpointMismatch);
            }
        }

        Ok(())
    }
}

impl FtlArtifactDescriptor {
    pub fn validate(&self) -> Result<(), FtlValidationError> {
        self.compilation.validate()?;
        if let Some(code) = &self.code {
            if code.tier != JitType::Ftl {
                return Err(FtlValidationError::ArtifactCodeTierMismatch);
            }
        }

        Ok(())
    }
}

/// Artifact returned after FTL finalization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FtlArtifactDescriptor {
    pub compilation: FtlCompilationDescriptor,
    pub code: Option<JitCodeArtifact>,
    pub osr_entry_artifacts: Vec<JitCodeId>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum FtlPlanSchemaOwner {
    #[default]
    FtlPlanRegistry,
    DfgToB3Lowering,
    AirGeneration,
    LinkFinalization,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum FtlPlanRegistryMutationAuthority {
    #[default]
    GeneratedStaticDataRefresh,
    CrateInitialization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticFtlPlanDescriptor {
    pub name: &'static str,
    pub target_tier: JitType,
    pub stages: &'static [FtlCompilationStage],
    pub lowering_phases: &'static [FtlLoweringPhase],
    pub state_authorities: &'static [FtlStateAuthority],
    pub owner: FtlPlanSchemaOwner,
    pub mutation_authority: FtlPlanRegistryMutationAuthority,
    pub provenance: &'static str,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FtlPlanDescriptorRegistry {
    pub descriptors: &'static [StaticFtlPlanDescriptor],
}

impl FtlPlanDescriptorRegistry {
    pub const fn new(descriptors: &'static [StaticFtlPlanDescriptor]) -> Self {
        Self { descriptors }
    }

    pub const fn descriptors(self) -> &'static [StaticFtlPlanDescriptor] {
        self.descriptors
    }

    pub fn descriptor_for_name(self, name: &str) -> Option<&'static StaticFtlPlanDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn validate(self) -> Result<(), FtlValidationError> {
        for (index, descriptor) in self.descriptors.iter().enumerate() {
            descriptor.validate()?;
            if self.descriptors[index + 1..]
                .iter()
                .any(|other| other.name == descriptor.name)
            {
                return Err(FtlValidationError::DuplicatePlanName(descriptor.name));
            }
        }

        Ok(())
    }
}

impl StaticFtlPlanDescriptor {
    pub fn validate(&self) -> Result<(), FtlValidationError> {
        if self.name.is_empty() {
            return Err(FtlValidationError::EmptyName);
        }
        if self.provenance.is_empty() {
            return Err(FtlValidationError::EmptyProvenance(self.name));
        }
        if self.stages.is_empty() {
            return Err(FtlValidationError::EmptyStages(self.name));
        }
        if self.lowering_phases.is_empty() {
            return Err(FtlValidationError::EmptyLoweringPhases(self.name));
        }
        if self.state_authorities.is_empty() {
            return Err(FtlValidationError::EmptyStateAuthorities(self.name));
        }

        Ok(())
    }
}

const FTL_PIPELINE_STAGES: &[FtlCompilationStage] = &[
    FtlCompilationStage::CapturingDfg,
    FtlCompilationStage::LoweringDfgToB3,
    FtlCompilationStage::OptimizingB3,
    FtlCompilationStage::LoweringB3ToAir,
    FtlCompilationStage::AllocatingRegisters,
    FtlCompilationStage::Linking,
    FtlCompilationStage::Finalizing,
];
const FTL_LOWERING_PHASES: &[FtlLoweringPhase] = &[
    FtlLoweringPhase::DfgNodeSelection,
    FtlLoweringPhase::StackmapCreation,
    FtlLoweringPhase::PatchpointCreation,
    FtlLoweringPhase::ExceptionTargetCreation,
    FtlLoweringPhase::B3ProcedureValidation,
    FtlLoweringPhase::AirLowering,
    FtlLoweringPhase::AirRegisterAllocation,
    FtlLoweringPhase::LinkBufferFinalization,
];
const FTL_STATE_AUTHORITIES: &[FtlStateAuthority] = &[
    FtlStateAuthority::DfgGraphOwner,
    FtlStateAuthority::FtlLoweringPhase,
    FtlStateAuthority::B3ProcedureOwner,
    FtlStateAuthority::LinkBufferOwner,
    FtlStateAuthority::FinalizerOwner,
];

pub const STATIC_FTL_PLAN_DESCRIPTORS: &[StaticFtlPlanDescriptor] = &[StaticFtlPlanDescriptor {
    name: "ftl-dfg-b3-air",
    target_tier: JitType::Ftl,
    stages: FTL_PIPELINE_STAGES,
    lowering_phases: FTL_LOWERING_PHASES,
    state_authorities: FTL_STATE_AUTHORITIES,
    owner: FtlPlanSchemaOwner::FtlPlanRegistry,
    mutation_authority: FtlPlanRegistryMutationAuthority::GeneratedStaticDataRefresh,
    provenance: "static Rust FTL plan schema",
}];

pub const FTL_PLAN_DESCRIPTOR_REGISTRY: FtlPlanDescriptorRegistry =
    FtlPlanDescriptorRegistry::new(STATIC_FTL_PLAN_DESCRIPTORS);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::CellId;

    #[test]
    fn static_ftl_registry_validates() {
        assert_eq!(FTL_PLAN_DESCRIPTOR_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn ftl_descriptor_rejects_ready_stage_without_air() {
        let compilation = FtlCompilationDescriptor {
            owner: CodeBlockId(CellId(1)),
            graph: DfgGraphId(1),
            stage: FtlCompilationStage::ReadyToInstall,
            procedure: Some(B3ProcedureId(1)),
            air: None,
            boundaries: Vec::new(),
            patchpoints: Vec::new(),
            slow_paths: Vec::new(),
            exception_targets: Vec::new(),
            failure: None,
        };

        assert_eq!(
            compilation.validate(),
            Err(FtlValidationError::MissingAirForAirStage)
        );
    }
}
