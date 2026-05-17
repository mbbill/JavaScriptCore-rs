//! DFG watchpoint collection and invalidation points.
//!
//! The C++ DFG first collects desired watchpoints, then materializes them on
//! the main thread. This module keeps that two-phase contract explicit without
//! registering watchpoints or observing runtime objects.

use crate::dfg::DfgNodeId;
use crate::jit::{WatchpointDependency, WatchpointDependencyId, WatchpointSetId};
use crate::runtime::CodeBlockId;

/// Phase of DFG watchpoint handling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DfgWatchpointCollectionPhase {
    Collect,
    Materialize,
    Finalize,
}

/// Mode used by a future collector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchpointCollectionMode {
    CountOnly,
    AddToCodeBlock,
    RecheckBeforeInstall,
}

/// Watchpoints requested by a graph before installation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DfgDesiredWatchpoints {
    pub owner: CodeBlockId,
    pub phase: DfgWatchpointCollectionPhase,
    pub mode: WatchpointCollectionMode,
    pub dependencies: Vec<WatchpointDependency>,
    pub watchpoint_sets: Vec<WatchpointSetId>,
    pub adaptive_dependencies: Vec<WatchpointDependencyId>,
}

/// Stable identity for an explicit invalidation point in generated metadata.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DfgInvalidationPointId(pub u32);

/// Invalidation point family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidationPointKind {
    WatchpointCheck,
    StructureEpochCheck,
    CodeBlockJettisonCheck,
    InlineCacheReset,
    TierReplacement,
    WeakReferenceBarrier,
}

/// Descriptor for a point that can invalidate optimized code before or after
/// install.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DfgInvalidationPoint {
    pub id: DfgInvalidationPointId,
    pub owner: CodeBlockId,
    pub node: Option<DfgNodeId>,
    pub kind: InvalidationPointKind,
    pub dependencies: Vec<WatchpointDependencyId>,
    pub bytecode_index: Option<u32>,
}
