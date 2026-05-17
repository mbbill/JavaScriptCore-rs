//! Heap, cell, rooting, tracing, and barrier contracts for the Rust JSC design.
//!
//! This module intentionally has no local dependencies. It provides the lowest
//! layer names used by values, objects, and the VM without choosing a collector
//! algorithm or a C++-compatible layout.

#![deny(unsafe_op_in_unsafe_fn)]

mod barrier;
mod cell;
mod heap;
mod phase;
mod refs;
mod snapshot;
mod space;
mod trace;
mod visitor;
mod weak;

pub use barrier::{
    BarrierEdge, BarrierKind, BarrierThreshold, RememberedSetEntry, ValueBarrier, WriteBarrier,
    WriteBarrierPlan,
};
pub use cell::{
    CellHeaderFlags, CellLock, CellMetadata, CellMetadataKey, CellState, CellType, CellVTable,
    DestructionMode, HeapCellKind, JsCell, JsCellHeader, StructureId, TraceCell, TypeInfo,
};
pub use heap::{
    AllocationPlan, CellArena, CellInit, ConservativeRootSpan, FinalizationRegistry, FinalizerId,
    Heap, HeapEpoch, HeapId, RootId, RootKind, RootRecord, RootSet, RootVisitor, Subspace, WeakId,
    WeakProcessingPhase, WeakRegistry,
};
pub use phase::{
    CollectionKind, CollectionRequest, CollectionScope, CollectionTriggerKind,
    GcActivityCallbackState, GcActivityKind, GcConductor, GcPhase, GcScheduleDecision,
    MutatorSchedulerPolicy, MutatorState, NoGcScope, Synchronousness,
};
pub use refs::{GcRef, Handle, HandleScope, HandleScopeId, Root, Weak};
pub use snapshot::{
    HeapSnapshot, HeapSnapshotBuilder, HeapSnapshotEdge, HeapSnapshotEdgeName,
    HeapSnapshotEdgeType, HeapSnapshotId, HeapSnapshotKind, HeapSnapshotNode, HeapSnapshotNodeId,
    HeapSpaceStatistics, HeapStatistics,
};
pub use space::{
    AllocationFailureMode, AllocationMode, AllocationProfile, AllocationProfileEntry, Allocator,
    AllocatorId, BlockDirectoryDescriptor, BlockDirectoryId, BlockState, FreeListDescriptor,
    LocalAllocatorDescriptor, MarkedBlockDescriptor, MarkedBlockId, MarkedSpaceDescriptor,
    PreciseAllocationDescriptor, PreciseAllocationId, SizeClass, SizeClassIndex,
    SubspaceDescriptor, SubspaceKind, TypedSubspace, MARKED_BLOCK_ATOM_SIZE, MARKED_BLOCK_SIZE,
    MARKED_SPACE_PRECISE_CUTOFF, WEAK_BLOCK_SIZE,
};
pub use trace::{
    ConstraintExecutionPhase, ConstraintMode, MarkReason, MarkingConstraint, MarkingConstraintSet,
    Trace, Tracer,
};
pub use visitor::{
    ConservativeRootSource, ConservativeRoots, DrainMode, DrainResult, MarkDependency,
    MarkWorkItem, MarkWorklistDescriptor, MarkWorklistId, MarkWorklistKind, MarkWorklistStats,
    RootMarkingPlan, SlotVisitorDescriptor,
};
pub use weak::{
    FinalizerKind, FinalizerRecord, HeapFinalizerCallback, HeapFinalizerCallbackId,
    WeakBlockDescriptor, WeakBlockId, WeakEdgeKind, WeakHandleOwnerId, WeakSetDescriptor,
    WeakSetId, WeakSlotRecord, WeakSlotState, WeakSweepResult,
};
