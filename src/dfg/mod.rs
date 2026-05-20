//! Data Flow Graph tier design contracts.
//!
//! The DFG is planned as an optional optimized tier. This module reserves the
//! ownership shape for IR graphs, speculation, OSR, and invalidation without
//! compiling or executing optimized code.

#![forbid(unsafe_code)]

pub(crate) mod graph;
pub(crate) mod osr;
pub(crate) mod speculation;
pub(crate) mod watchpoint;

pub use graph::{
    BranchTarget, DfgBasicBlock, DfgBasicBlockId, DfgCommonDataDescriptor, DfgDominanceSet,
    DfgEdge, DfgEdgeId, DfgGraph, DfgGraphBuilder, DfgGraphId, DfgGraphMutationAuthority,
    DfgInlineCallFrameId, DfgNode, DfgNodeId, DfgNodeKind, DfgPhase, DfgPlanDescriptorRegistry,
    DfgPlanRegistryMutationAuthority, DfgPlanSchemaOwner, DfgValidationError, DfgValueRep,
    DfgVariableAccessDataId, EdgeKillStatus, EdgeProofStatus, EdgeUseKind, GraphForm,
    InlineVariableData, NodeEffects, NodeOrigin, NodeOriginKind, StaticDfgPlanDescriptor,
    DFG_PLAN_DESCRIPTOR_REGISTRY, STATIC_DFG_PLAN_DESCRIPTORS,
};
pub use osr::{
    DfgOsrEntryDescriptor, DfgOsrExitDescriptor, DfgOsrExitId, ExitProfileUpdate, FlushFormat,
    MaterializationKind, OsrEntryAvailability, OsrEntryKind, OsrExitKind, OsrExitOutcomeKind,
    OsrExitOutcomeRecord, OsrExitRecovery, RecoverySource,
};
pub use speculation::{
    AbstractValueSource, PredictionSource, SpeculatedType, SpeculationCheck, SpeculationCheckId,
    SpeculationCheckKind, SpeculationDirection, SpeculationFailureSemantics,
    SpeculationPreconditionKind, SpeculationRecovery, SpeculationRecoveryKind,
    SpeculationSemanticValidationError, SpeculationSite,
};
pub use watchpoint::{
    DesiredWatchpointCounts, DfgDesiredWatchpoints, DfgInvalidationPoint, DfgInvalidationPointId,
    DfgWatchpointCollectionPhase, InvalidationPointKind, WatchpointCollectionMode,
    WatchpointRegistrationConcurrency,
};
