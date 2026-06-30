//! Heap, cell, rooting, tracing, and barrier contracts for the Rust JSC design.
//!
//! This module intentionally has no local dependencies. It provides the lowest
//! layer names used by values, objects, and the VM without choosing a collector
//! algorithm or a C++-compatible layout.
//!
//! Ownership boundary:
//! - `CellId` is the canonical raw identity for heap cells.
//! - `Heap` and its allocation containers own cell storage and lifecycle state.
//! - `GcRef`, handles, roots, weak records, worklists, snapshots, and allocator
//!   IDs are borrower or registry identities. They must not be treated as raw
//!   heap-cell authority.
//! - Mutation of topology, mark state, weak state, and barriers is reserved to
//!   the explicitly named heap/collector/sweeper/mutator authority type.

#![deny(unsafe_op_in_unsafe_fn)]

mod barrier;
mod cell;
mod conservative_roots;
mod fast_hash;
mod heap;
mod machine_stack_marker;
mod phase;
mod refs;
mod snapshot;
mod space;
mod trace;
mod visitor;
mod weak;

pub use barrier::{
    butterfly_reallocation_barrier, static_barrier_schema_registry, static_barrier_schemas,
    BarrierAction, BarrierDecision, BarrierDecisionError, BarrierEdge, BarrierFieldKind,
    BarrierKind, BarrierMutationAuthority, BarrierNotRequiredReason, BarrierRegistryAuthority,
    BarrierRequirementOutcome, BarrierRequirementRequest, BarrierSchemaDescriptor,
    BarrierSchemaDescriptorBuilder, BarrierSchemaOwner, BarrierSchemaRegistry,
    BarrierSchemaValidationError, BarrierThreshold, BarrierWriteContext, RememberedSetEntry,
    ValueBarrier, WriteBarrier, WriteBarrierCounterSet, WriteBarrierPlan, WriteBarrierUseKind,
    STATIC_BARRIER_SCHEMAS, STATIC_BARRIER_SCHEMA_REGISTRY,
};
pub use cell::{
    static_cell_metadata_descriptors, static_cell_metadata_registry, static_type_info_descriptors,
    CellAttributes, CellDestructionState, CellHeaderFlags, CellId, CellLifecycleRecord, CellLock,
    CellMetadata, CellMetadataDescriptor, CellMetadataDescriptorBuilder, CellMetadataKey,
    CellMetadataRegistry, CellMetadataRegistryAuthority, CellMetadataValidationError,
    CellSchemaOwner, CellSchemaProvenance, CellState, CellType, CellVTable, CellZapReason,
    DestructionMode, HeapCellKind, JsCell, JsCellHeader, StructureId, TraceCell, TypeInfo,
    TypeInfoDescriptor, STATIC_CELL_METADATA_DESCRIPTORS, STATIC_CELL_METADATA_REGISTRY,
    STATIC_TYPE_INFO_DESCRIPTORS,
};
pub use conservative_roots::{ConservativeRootCell, ConservativeRoots};
pub use fast_hash::{FxIntBuildHasher, FxIntHasher};
pub use heap::{
    static_heap_schema_descriptor, AllocationPlan, CellArena, CellInit, ConservativeRootSpan,
    FinalizationRegistry, FinalizerId, FinalizerQueueRecord, FinalizerQueueTransitionRecord, Heap,
    HeapAllocationRecord, HeapAllocationRequest, HeapAllocationResponse,
    HeapCellInvalidationRequest, HeapEpoch, HeapId, HeapIntegrationError, HeapSchemaDescriptor,
    HeapSchemaDescriptorBuilder, HeapSchemaOwner, HeapSchemaRegistryAuthority,
    HeapSchemaValidationError, NoGcScopeDepth, RootId, RootIdSet, RootKind,
    RootReachabilitySemantics, RootRecord, RootSet, RootSetMutation, RootSetMutationAuthority,
    RootSetMutationKind, RootSetMutationOutcome, RootSetSemanticError, RootVisitor, Subspace,
    TargetedRootRecord, TargetedRootSet, WeakId, WeakProcessingPhase,
    WeakProcessingTransitionRecord, WeakRegistrationRecord, WeakRegistry,
    WriteBarrierApplicationRecord, WriteBarrierApplicationRequest, STATIC_HEAP_SCHEMA_DESCRIPTOR,
};
#[cfg(feature = "arm64_native_entry_proof")]
pub(crate) use heap::{HeapConservativeScanAppendReceipt, HeapMarkingError, HeapMarkingRecord};
// gc-r4 R3 (reversible shadow oracle): the S4 arena + carried cell address, surfaced to
// interpreter::object_store: the RELEASE object-cell arena (gc-r4 R4a — the arena is THE
// cell store and its address is identity).
pub(crate) use heap::MarkedSpace;
// gc-r4 R4b-mark: the collector marking core + its method-table boundary trait + the
// carried cell-address type, surfaced so interpreter::object_store can drive the live
// mark (`CoreObjectStore::mark_live_set`) over its `CoreObjectCell` graph.
pub(crate) use heap::{CellPtr, SlotVisitor, VisitChildren};
pub(crate) use machine_stack_marker::{
    JscMachineStackConservativeRootingProof, JscMachineStackMarker,
};
// JscMachineStackRootSpanKind is consumed only by the gated ARM64
// admission-proof cluster (native_reentry/rooting.rs); the always-compiled
// machine_stack_marker module and its other two re-exports stay live.
#[cfg(feature = "arm64_native_entry_proof")]
pub(crate) use machine_stack_marker::JscMachineStackRootSpanKind;
pub use phase::{
    evaluate_heap_semantics, CollectionCompletionCallbackId, CollectionKind, CollectionRequest,
    CollectionScope, CollectionTriggerKind, GcActivityCallbackState, GcActivityKind, GcConductor,
    GcPhase, GcScheduleDecision, HeapMutationAuthority, HeapSemanticError, HeapSemanticGrant,
    HeapSemanticOperation, HeapStateDescriptor, MutatorSchedulerPolicy, MutatorState, NoGcScope,
    NoGcScopeContract, Synchronousness,
};
pub use refs::{
    GcRef, Handle, HandleScope, HandleScopeId, HandleSetDescriptor, HandleSlotId, HandleSlotState,
    Root, StrongHandle, Weak,
};
pub use snapshot::{
    HeapSnapshot, HeapSnapshotBuilder, HeapSnapshotCellRecord, HeapSnapshotEdge,
    HeapSnapshotEdgeName, HeapSnapshotEdgeType, HeapSnapshotId, HeapSnapshotKind, HeapSnapshotNode,
    HeapSnapshotNodeId, HeapSnapshotRecord, HeapSnapshotValidationError, HeapSpaceStatistics,
    HeapStatistics,
};
pub use space::{
    static_allocation_schema_registry, static_allocator_descriptors, static_subspace_descriptors,
    AlignedMemoryAllocatorDescriptor, AlignedMemoryAllocatorDescriptorBuilder,
    AlignedMemoryAllocatorId, AllocationDescriptorValidationError, AllocationFailureMode,
    AllocationMode, AllocationProfile, AllocationProfileEntry, AllocationRegistryAuthority,
    AllocationSchemaOwner, AllocationSchemaProvenance, AllocationSchemaRegistry,
    AllocationSchemaValidationError, AllocationSelectionError, AllocationSelectionKind,
    AllocationSizeClassSelection, Allocator, AllocatorId, BlockDirectoryBit,
    BlockDirectoryDescriptor, BlockDirectoryId, BlockState, FreeListDescriptor,
    LocalAllocatorDescriptor, LocalAllocatorDescriptorBuilder, MarkedBlockDescriptor,
    MarkedBlockDescriptorBuilder, MarkedBlockId, MarkedSpaceDescriptor,
    PreciseAllocationDescriptor, PreciseAllocationId, PreciseAllocationTier, SizeClass,
    SizeClassIndex, SpaceIterationState, StaticAllocatorDescriptor, StaticMarkedSpaceDescriptor,
    StaticSubspaceDescriptor, SubspaceDescriptor, SubspaceDescriptorBuilder, SubspaceKind,
    SubspaceMutationAuthority, SweepMode, TypedSubspace, MARKED_BLOCK_ATOM_SIZE, MARKED_BLOCK_SIZE,
    MARKED_SPACE_PRECISE_CUTOFF, STATIC_ALLOCATION_SCHEMA_REGISTRY, STATIC_ALLOCATOR_DESCRIPTORS,
    STATIC_MARKED_ALLOCATOR_DESCRIPTOR, STATIC_MARKED_SIZE_CLASSES, STATIC_MARKED_SPACE_DESCRIPTOR,
    STATIC_PRECISE_ALLOCATOR_DESCRIPTOR, STATIC_SUBSPACE_DESCRIPTORS, WEAK_BLOCK_SIZE,
};
pub use trace::{
    ConstraintConcurrency, ConstraintExecutionPhase, ConstraintMode, ConstraintParallelism,
    ConstraintVolatility, MarkReason, MarkingConstraint, MarkingConstraintSet, MarkingGraphEdge,
    MarkingGraphNode, MarkingPlan, MarkingPlanGraph, MarkingPlanGraphBuilder,
    MarkingPlanGraphError, MarkingPlanStep, Trace, Tracer,
};
pub use visitor::{
    root_mark_reason_for_kind, ConservativeRootSource, DrainMode, DrainResult, MarkDependency,
    MarkStackTransfer, MarkStackTransferKind, MarkWorkItem, MarkWorklistDescriptor, MarkWorklistId,
    MarkWorklistKind, MarkWorklistStats, OpaqueRootId, OpaqueRootRecord, ReferrerToken,
    ReferrerTokenKind, RootMarkReason, RootMarkingPlan, RootPlanStep, RootPlanningError,
    SlotVisitorConservativeRootAppendError, SlotVisitorConservativeRootAppendPlan,
    SlotVisitorConservativeRootAppendRecord, SlotVisitorDescriptor,
};
// Salvage: SlotVisitor collector-effects / conservative-root-marking and
// VerifierSlotVisitor append, consumed only by the gated ARM64 admission-proof
// cluster + their own tests. Gated off by default. (Map: heap/SlotVisitor.cpp /
// VerifierSlotVisitor.cpp — gated, never deleted.)
#[cfg(feature = "arm64_native_entry_proof")]
#[allow(unused_imports)]
pub(crate) use visitor::{
    SlotVisitorAppendToMarkStackRecord, SlotVisitorCollectorEffectAction,
    SlotVisitorCollectorEffectRecord, SlotVisitorCollectorEffectsError,
    SlotVisitorCollectorEffectsPlan, SlotVisitorConservativeRootMarkingAction,
    SlotVisitorConservativeRootMarkingError, SlotVisitorConservativeRootMarkingPlan,
    SlotVisitorConservativeRootMarkingRecord, SlotVisitorContainerNoteMarkedRecord,
    SlotVisitorNoteLiveAuxiliaryCellRecord, VerifierSlotVisitorCollectorStackAppendRecord,
    VerifierSlotVisitorConservativeRootAppendAction,
    VerifierSlotVisitorConservativeRootAppendError, VerifierSlotVisitorConservativeRootAppendPlan,
    VerifierSlotVisitorConservativeRootAppendProof,
    VerifierSlotVisitorConservativeRootAppendRecord, VerifierSlotVisitorDescriptor,
    VerifierSlotVisitorTestAndSetMarkRecord,
};
pub use weak::{
    FinalizerKind, FinalizerPlan, FinalizerPlanEntry, FinalizerPlanningError,
    FinalizerPlanningRecord, FinalizerRecord, FinalizerState, FinalizerStateTransitionError,
    FinalizerTransitionOutcome, FinalizerTransitionRequest, HeapFinalizerCallback,
    HeapFinalizerCallbackId, WeakBlockDescriptor, WeakBlockId, WeakBlockPlanAction,
    WeakBlockPlanEntry, WeakContextTag, WeakEdgeKind, WeakHandleOwnerContract, WeakHandleOwnerId,
    WeakOwnerAuthority, WeakPlanningError, WeakProcessingPlan, WeakRootPolicyAction,
    WeakRootPolicyDescriptor, WeakRootPolicyError, WeakRootPolicyPlan, WeakRootPolicyPlanEntry,
    WeakRootPolicyReason, WeakSetDescriptor, WeakSetId, WeakSlotRecord, WeakSlotState,
    WeakSlotTransitionOutcome, WeakSlotTransitionRequest, WeakStateTransitionError,
    WeakSweepResult,
};
