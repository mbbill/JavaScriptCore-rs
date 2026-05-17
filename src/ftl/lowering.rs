//! FTL lowering and finalization boundaries.
//!
//! FTL consumes a DFG graph, lowers through B3, then lets Air own
//! machine-near register allocation and code generation. These types describe
//! those handoff points without creating B3 values, Air instructions, link
//! buffers, or executable memory.

use crate::b3::{
    AirBlockId, AirCodeId, AirInstructionId, B3BlockId, B3ProcedureId, B3ValueId, B3ValueKind,
};
use crate::dfg::{BasicBlockId, DfgGraphId, DfgNodeId, DfgOsrEntryDescriptor, DfgOsrExitId};
use crate::jit::{CallBoundaryId, CodeOrigin, JitCodeArtifact, JitCodeId, PatchpointDescriptor};
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
    pub dfg_block: Option<BasicBlockId>,
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
    pub target_block: Option<BasicBlockId>,
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

/// Artifact returned after FTL finalization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FtlArtifactDescriptor {
    pub compilation: FtlCompilationDescriptor,
    pub code: Option<JitCodeArtifact>,
    pub osr_entry_artifacts: Vec<JitCodeId>,
}
