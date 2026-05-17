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
    BasicBlockId, BranchTarget, DfgBasicBlock, DfgEdge, DfgEdgeId, DfgGraph, DfgGraphId, DfgNode,
    DfgNodeId, DfgNodeKind, DfgPhase, DfgValueRep, EdgeKillStatus, EdgeProofStatus, EdgeUseKind,
    GraphForm, NodeEffects, NodeOrigin, NodeOriginKind,
};
pub use osr::{
    DfgOsrEntryDescriptor, DfgOsrExitDescriptor, DfgOsrExitId, ExitProfileUpdate, FlushFormat,
    MaterializationKind, OsrEntryAvailability, OsrEntryKind, OsrExitKind, OsrExitRecovery,
    RecoverySource,
};
pub use speculation::{
    AbstractValueSource, PredictionSource, SpeculatedType, SpeculationCheck, SpeculationCheckId,
    SpeculationCheckKind, SpeculationDirection, SpeculationRecovery, SpeculationRecoveryKind,
    SpeculationSite,
};
pub use watchpoint::{
    DfgDesiredWatchpoints, DfgInvalidationPoint, DfgInvalidationPointId,
    DfgWatchpointCollectionPhase, InvalidationPointKind, WatchpointCollectionMode,
};
