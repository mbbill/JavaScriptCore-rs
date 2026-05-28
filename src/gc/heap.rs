//! Heap ownership and allocation contracts.

use core::marker::PhantomData;
use std::collections::{HashMap, HashSet};

use crate::gc::{
    evaluate_heap_semantics, static_allocation_schema_registry, static_barrier_schema_registry,
    AllocationMode, AllocationProfile, AllocationSchemaRegistry, BarrierDecisionError,
    BarrierMutationAuthority, BarrierRequirementOutcome, BarrierRequirementRequest,
    BarrierWriteContext, CellDestructionState, CellId, CellLifecycleRecord, CellMetadata,
    CellZapReason, CollectionRequest, ConservativeRoots, FinalizerKind, FinalizerPlan,
    FinalizerPlanningError, FinalizerPlanningRecord, FinalizerRecord, FinalizerState,
    FinalizerStateTransitionError, FinalizerTransitionRequest, GcActivityCallbackState,
    GcConductor, GcPhase, GcRef, HeapFinalizerCallback, HeapFinalizerCallbackId,
    HeapMutationAuthority, HeapSemanticError, HeapSemanticOperation, HeapSnapshotBuilder,
    HeapSnapshotCellRecord, HeapSnapshotEdge, HeapSnapshotId, HeapSnapshotKind, HeapSnapshotNodeId,
    HeapSnapshotRecord, HeapSnapshotValidationError, HeapStateDescriptor, HeapStatistics,
    MarkedSpaceDescriptor, MutatorState, RootMarkingPlan, RootPlanningError, SlotVisitorDescriptor,
    SubspaceDescriptor, SubspaceKind, TraceCell, WeakEdgeKind, WeakHandleOwnerContract,
    WeakHandleOwnerId, WeakSetDescriptor, WeakSetId, WeakSlotState, WeakSlotTransitionOutcome,
    WeakSlotTransitionRequest, WeakStateTransitionError,
};

/// Opaque identity for one VM-owned heap.
///
/// This identifies the heap container and its registries. It is not a cell
/// pointer, cell table entry, or substitute for `CellId`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct HeapId(pub u64);

/// Monotonic marker used to separate allocation and marking epochs.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct HeapEpoch(pub u64);

/// Static owner for heap-level schema rows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeapSchemaOwner {
    /// `gc::heap` owns the root heap schema and registry wiring.
    #[default]
    GcHeapSchema,
    /// A future generated source owns VM-specific heap layout rows.
    GeneratedVmHeapSchema,
}

/// Registry mutation authority for heap schema data.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeapSchemaRegistryAuthority {
    /// The heap schema is immutable compiled data.
    #[default]
    StaticReadOnly,
    /// A generated source refresh may replace the compiled schema.
    GeneratedSourceRefresh,
}

/// Immutable descriptor for a heap family.
///
/// This records registry wiring and ownership boundaries only. It does not
/// allocate a heap, enqueue collection requests, trace roots, or publish cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeapSchemaDescriptor {
    pub name: &'static str,
    pub owner: HeapSchemaOwner,
    pub authority: HeapSchemaRegistryAuthority,
    pub allocation_registry: &'static AllocationSchemaRegistry,
    pub root_registry_owner: &'static str,
    pub weak_registry_owner: &'static str,
    pub finalizer_registry_owner: &'static str,
}

impl HeapSchemaDescriptor {
    pub const fn allocation_registry(&self) -> &'static AllocationSchemaRegistry {
        self.allocation_registry
    }

    pub fn validate(&self) -> Result<(), HeapSchemaValidationError> {
        if self.name.is_empty()
            || self.root_registry_owner.is_empty()
            || self.weak_registry_owner.is_empty()
            || self.finalizer_registry_owner.is_empty()
        {
            return Err(HeapSchemaValidationError::EmptyName);
        }
        self.allocation_registry
            .validate()
            .map_err(HeapSchemaValidationError::AllocationSchema)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeapSchemaValidationError {
    EmptyName,
    AllocationSchema(crate::gc::AllocationSchemaValidationError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeapSchemaDescriptorBuilder {
    descriptor: HeapSchemaDescriptor,
}

impl HeapSchemaDescriptorBuilder {
    pub const fn new(
        name: &'static str,
        allocation_registry: &'static AllocationSchemaRegistry,
    ) -> Self {
        Self {
            descriptor: HeapSchemaDescriptor {
                name,
                owner: HeapSchemaOwner::GcHeapSchema,
                authority: HeapSchemaRegistryAuthority::StaticReadOnly,
                allocation_registry,
                root_registry_owner: "",
                weak_registry_owner: "",
                finalizer_registry_owner: "",
            },
        }
    }

    pub const fn owner(mut self, owner: HeapSchemaOwner) -> Self {
        self.descriptor.owner = owner;
        self
    }

    pub const fn authority(mut self, authority: HeapSchemaRegistryAuthority) -> Self {
        self.descriptor.authority = authority;
        self
    }

    pub const fn root_registry_owner(mut self, owner: &'static str) -> Self {
        self.descriptor.root_registry_owner = owner;
        self
    }

    pub const fn weak_registry_owner(mut self, owner: &'static str) -> Self {
        self.descriptor.weak_registry_owner = owner;
        self
    }

    pub const fn finalizer_registry_owner(mut self, owner: &'static str) -> Self {
        self.descriptor.finalizer_registry_owner = owner;
        self
    }

    pub fn build(self) -> Result<HeapSchemaDescriptor, HeapSchemaValidationError> {
        self.descriptor.validate()?;
        Ok(self.descriptor)
    }
}

pub const STATIC_HEAP_SCHEMA_DESCRIPTOR: HeapSchemaDescriptor = HeapSchemaDescriptor {
    name: "javascriptcore-heap",
    owner: HeapSchemaOwner::GcHeapSchema,
    authority: HeapSchemaRegistryAuthority::StaticReadOnly,
    allocation_registry: static_allocation_schema_registry(),
    root_registry_owner: "gc::heap::RootSet",
    weak_registry_owner: "gc::heap::WeakRegistry",
    finalizer_registry_owner: "gc::heap::FinalizationRegistry",
};

pub const fn static_heap_schema_descriptor() -> &'static HeapSchemaDescriptor {
    &STATIC_HEAP_SCHEMA_DESCRIPTOR
}

/// Heap allocation request consumed by the GC integration layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeapAllocationRequest {
    pub heap: HeapId,
    pub subspace: &'static str,
    pub metadata: CellMetadata,
    pub byte_size: usize,
    pub mode: AllocationMode,
    pub may_trigger_collection: bool,
}

/// Result of assigning canonical cell identity to an allocation request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeapAllocationResponse {
    pub heap: HeapId,
    pub cell: CellId,
    pub epoch: HeapEpoch,
    pub metadata: CellMetadata,
    pub byte_size: usize,
}

/// Heap-owned allocation lifecycle record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeapAllocationRecord {
    pub request: HeapAllocationRequest,
    pub response: HeapAllocationResponse,
    pub lifecycle: CellLifecycleRecord,
    pub published: bool,
}

/// Request to invalidate a heap cell during sweep or destruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeapCellInvalidationRequest {
    pub cell: CellId,
    pub reason: CellZapReason,
}

/// Lexical no-GC depth token returned by heap entry APIs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoGcScopeDepth {
    pub heap: HeapId,
    pub depth: u32,
}

/// Recorded owner-aware write barrier application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteBarrierApplicationRequest {
    pub owner: CellId,
    pub target: Option<CellId>,
    pub context: BarrierWriteContext,
    pub authority: BarrierMutationAuthority,
    pub owner_is_published: bool,
}

/// Heap log entry for a barriered mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteBarrierApplicationRecord {
    pub request: WriteBarrierApplicationRequest,
    pub outcome: BarrierRequirementOutcome,
}

/// ID-only weak slot registration owned by the heap weak registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeakRegistrationRecord {
    pub id: WeakId,
    pub set: WeakSetId,
    pub owner: Option<WeakHandleOwnerId>,
    pub target: Option<CellId>,
    pub kind: WeakEdgeKind,
    pub state: WeakSlotState,
}

/// Weak-processing transition recorded after liveness has been decided.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeakProcessingTransitionRecord {
    pub weak: WeakId,
    pub phase: WeakProcessingPhase,
    pub target_was_live: bool,
    pub outcome: WeakSlotTransitionOutcome,
}

/// Finalizer registration tracked without borrowing payload storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FinalizerQueueRecord {
    pub id: FinalizerId,
    pub callback: HeapFinalizerCallbackId,
    pub target: CellId,
    pub owner: Option<WeakHandleOwnerId>,
    pub kind: FinalizerKind,
    pub state: FinalizerState,
}

/// Finalization transition selected by end-phase processing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FinalizerQueueTransitionRecord {
    pub finalizer: FinalizerId,
    pub target_was_live: bool,
    pub from: FinalizerState,
    pub to: FinalizerState,
    pub invokes_callback: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeapIntegrationError {
    HeapSemantic(HeapSemanticError),
    Barrier(BarrierDecisionError),
    WeakState(WeakStateTransitionError),
    FinalizerState(FinalizerStateTransitionError),
    FinalizerPlanning(FinalizerPlanningError),
    Snapshot(HeapSnapshotValidationError),
    HeapMismatch {
        expected: HeapId,
        actual: HeapId,
    },
    InvalidAllocationSize,
    UnknownCell(CellId),
    ZeroCellPayload,
    DuplicatePayloadBinding {
        payload: usize,
        existing: CellId,
        requested: CellId,
    },
    DuplicateCellPayloadBinding {
        cell: CellId,
        existing: usize,
        requested: usize,
    },
    DuplicateWeak(WeakId),
    UnknownWeak(WeakId),
    DuplicateWeakSet(WeakSetId),
    UnknownWeakSet(WeakSetId),
    DuplicateFinalizerCallback(HeapFinalizerCallbackId),
    UnknownFinalizerCallback(HeapFinalizerCallbackId),
    DuplicateFinalizer(FinalizerId),
    UnknownFinalizer(FinalizerId),
    InvalidSnapshotId(HeapSnapshotId),
    SnapshotAlreadyFinalized(HeapSnapshotId),
    Root(RootSetSemanticError),
    ConservativeRoot(RootPlanningError),
}

impl From<HeapSemanticError> for HeapIntegrationError {
    fn from(error: HeapSemanticError) -> Self {
        Self::HeapSemantic(error)
    }
}

impl From<BarrierDecisionError> for HeapIntegrationError {
    fn from(error: BarrierDecisionError) -> Self {
        Self::Barrier(error)
    }
}

impl From<WeakStateTransitionError> for HeapIntegrationError {
    fn from(error: WeakStateTransitionError) -> Self {
        Self::WeakState(error)
    }
}

impl From<FinalizerStateTransitionError> for HeapIntegrationError {
    fn from(error: FinalizerStateTransitionError) -> Self {
        Self::FinalizerState(error)
    }
}

impl From<FinalizerPlanningError> for HeapIntegrationError {
    fn from(error: FinalizerPlanningError) -> Self {
        Self::FinalizerPlanning(error)
    }
}

impl From<HeapSnapshotValidationError> for HeapIntegrationError {
    fn from(error: HeapSnapshotValidationError) -> Self {
        Self::Snapshot(error)
    }
}

impl From<RootSetSemanticError> for HeapIntegrationError {
    fn from(error: RootSetSemanticError) -> Self {
        Self::Root(error)
    }
}

impl From<RootPlanningError> for HeapIntegrationError {
    fn from(error: RootPlanningError) -> Self {
        Self::ConservativeRoot(error)
    }
}

/// Owner of JavaScript-managed memory.
///
/// The heap owns allocation containers, root/weak/finalizer registries, and
/// lifecycle mutation authority. Runtime layers may hold typed IDs or `JsValue`
/// bits, but raw cell identity and storage interpretation remain here.
#[derive(Debug)]
pub struct Heap {
    id: HeapId,
    phase: GcPhase,
    epoch: HeapEpoch,
    mutator_state: MutatorState,
    conductor: GcConductor,
    mutator_has_heap_access: bool,
    mutator_should_be_fenced: bool,
    no_gc_scope_depth: u32,
    next_cell: u32,
    next_snapshot: u64,
    roots: RootSet,
    targeted_roots: TargetedRootSet,
    weak: WeakRegistry,
    finalizers: FinalizationRegistry,
    object_space: MarkedSpaceDescriptor,
    requests: Vec<CollectionRequest>,
    allocations: Vec<HeapAllocationRecord>,
    payload_to_cell: HashMap<usize, CellId>,
    cell_to_payload: HashMap<CellId, usize>,
    barriers: Vec<WriteBarrierApplicationRecord>,
    conservative_roots: ConservativeRoots,
    snapshots: Vec<HeapSnapshotRecord>,
    full_activity: GcActivityCallbackState,
    eden_activity: GcActivityCallbackState,
}

impl Heap {
    pub fn new() -> Self {
        let id = HeapId::default();
        Self {
            id,
            phase: GcPhase::NotRunning,
            epoch: HeapEpoch::default(),
            mutator_state: MutatorState::Running,
            conductor: GcConductor::Mutator,
            mutator_has_heap_access: true,
            mutator_should_be_fenced: false,
            no_gc_scope_depth: 0,
            next_cell: 1,
            next_snapshot: 1,
            roots: RootSet::default(),
            targeted_roots: TargetedRootSet::default(),
            weak: WeakRegistry::default(),
            finalizers: FinalizationRegistry::default(),
            object_space: MarkedSpaceDescriptor::new(id),
            requests: Vec::new(),
            allocations: Vec::new(),
            payload_to_cell: HashMap::new(),
            cell_to_payload: HashMap::new(),
            barriers: Vec::new(),
            conservative_roots: ConservativeRoots::new(),
            snapshots: Vec::new(),
            full_activity: GcActivityCallbackState::default(),
            eden_activity: GcActivityCallbackState {
                kind: crate::gc::GcActivityKind::Eden,
                ..GcActivityCallbackState::default()
            },
        }
    }

    pub fn id(&self) -> HeapId {
        self.id
    }

    pub fn phase(&self) -> GcPhase {
        self.phase
    }

    pub fn epoch(&self) -> HeapEpoch {
        self.epoch
    }

    pub fn roots(&self) -> &RootSet {
        &self.roots
    }

    pub fn targeted_roots(&self) -> &TargetedRootSet {
        &self.targeted_roots
    }

    pub fn weak_registry(&self) -> &WeakRegistry {
        &self.weak
    }

    pub fn finalization_registry(&self) -> &FinalizationRegistry {
        &self.finalizers
    }

    pub fn object_space(&self) -> &MarkedSpaceDescriptor {
        &self.object_space
    }

    pub fn pending_collection_requests(&self) -> &[CollectionRequest] {
        &self.requests
    }

    pub fn allocation_records(&self) -> &[HeapAllocationRecord] {
        &self.allocations
    }

    pub fn bind_cell_payload(
        &mut self,
        cell: CellId,
        payload: usize,
    ) -> Result<(), HeapIntegrationError> {
        self.require_known_cell(cell)?;
        if payload == 0 {
            return Err(HeapIntegrationError::ZeroCellPayload);
        }

        if let Some(existing) = self.payload_to_cell.get(&payload).copied() {
            if existing != cell {
                return Err(HeapIntegrationError::DuplicatePayloadBinding {
                    payload,
                    existing,
                    requested: cell,
                });
            }
        }

        if let Some(existing) = self.cell_to_payload.get(&cell).copied() {
            if existing != payload {
                return Err(HeapIntegrationError::DuplicateCellPayloadBinding {
                    cell,
                    existing,
                    requested: payload,
                });
            }
        }

        self.payload_to_cell.insert(payload, cell);
        self.cell_to_payload.insert(cell, payload);
        Ok(())
    }

    pub fn cell_for_payload(&self, payload: usize) -> Option<CellId> {
        self.payload_to_cell.get(&payload).copied()
    }

    pub fn payload_for_cell(&self, cell: CellId) -> Option<usize> {
        self.cell_to_payload.get(&cell).copied()
    }

    pub fn barrier_records(&self) -> &[WriteBarrierApplicationRecord] {
        &self.barriers
    }

    pub fn conservative_root_records(&self) -> &ConservativeRoots {
        &self.conservative_roots
    }

    pub fn snapshot_records(&self) -> &[HeapSnapshotRecord] {
        &self.snapshots
    }

    pub fn no_gc_scope_depth(&self) -> u32 {
        self.no_gc_scope_depth
    }

    pub fn full_activity_callback(&self) -> GcActivityCallbackState {
        self.full_activity
    }

    pub fn eden_activity_callback(&self) -> GcActivityCallbackState {
        self.eden_activity
    }

    pub fn subspace<T: TraceCell + ?Sized>(
        &self,
        name: &'static str,
        metadata: CellMetadata,
    ) -> Subspace<T> {
        Subspace::new(name, metadata)
    }

    pub fn allocation_plan<T: TraceCell>(&self, subspace: &Subspace<T>) -> AllocationPlan<T> {
        AllocationPlan {
            heap: self.id,
            subspace: subspace.name,
            metadata: subspace.metadata,
            mode: AllocationMode::Normal,
            may_trigger_collection: true,
            _cell: PhantomData,
        }
    }

    pub fn allocate<T: TraceCell>(
        &mut self,
        plan: AllocationPlan<T>,
        byte_size: usize,
    ) -> Result<HeapAllocationResponse, HeapIntegrationError> {
        let request = plan.request(byte_size);
        self.allocate_record(request)
    }

    pub fn allocate_record(
        &mut self,
        request: HeapAllocationRequest,
    ) -> Result<HeapAllocationResponse, HeapIntegrationError> {
        self.require_heap(request.heap)?;
        if request.byte_size == 0 {
            return Err(HeapIntegrationError::InvalidAllocationSize);
        }

        evaluate_heap_semantics(
            self.state_descriptor(),
            HeapMutationAuthority::Mutator,
            HeapSemanticOperation::Allocate {
                may_trigger_collection: request.may_trigger_collection,
            },
        )?;

        let cell = CellId(self.next_cell);
        self.next_cell += 1;
        let response = HeapAllocationResponse {
            heap: self.id,
            cell,
            epoch: self.epoch,
            metadata: request.metadata,
            byte_size: request.byte_size,
        };
        self.allocations.push(HeapAllocationRecord {
            request,
            response,
            lifecycle: CellLifecycleRecord::default(),
            published: false,
        });
        Ok(response)
    }

    pub fn publish_cell(&mut self, cell: CellId) -> Result<(), HeapIntegrationError> {
        let record = self
            .allocations
            .iter_mut()
            .find(|record| record.response.cell == cell)
            .ok_or(HeapIntegrationError::UnknownCell(cell))?;
        record.published = true;
        Ok(())
    }

    pub fn invalidate_cell(
        &mut self,
        request: HeapCellInvalidationRequest,
    ) -> Result<CellLifecycleRecord, HeapIntegrationError> {
        let record = self
            .allocations
            .iter_mut()
            .find(|record| record.response.cell == request.cell)
            .ok_or(HeapIntegrationError::UnknownCell(request.cell))?;
        record.lifecycle = CellLifecycleRecord {
            destruction_state: CellDestructionState::PendingDestruction,
            zap_reason: Some(request.reason),
        };
        Ok(record.lifecycle)
    }

    pub fn subspace_descriptor(
        &self,
        name: &'static str,
        metadata: CellMetadata,
    ) -> SubspaceDescriptor {
        SubspaceDescriptor::complete(name, metadata)
    }

    pub fn iso_subspace_descriptor(
        &self,
        name: &'static str,
        metadata: CellMetadata,
        lower_tier_precise_cells: u8,
    ) -> SubspaceDescriptor {
        SubspaceDescriptor::iso(name, metadata, lower_tier_precise_cells)
    }

    pub fn precise_subspace_descriptor(
        &self,
        name: &'static str,
        metadata: CellMetadata,
    ) -> SubspaceDescriptor {
        SubspaceDescriptor::precise(name, metadata)
    }

    pub fn queue_collection(
        &mut self,
        request: CollectionRequest,
    ) -> Result<(), HeapIntegrationError> {
        evaluate_heap_semantics(
            self.state_descriptor(),
            HeapMutationAuthority::Mutator,
            HeapSemanticOperation::QueueCollection,
        )?;
        if !self
            .requests
            .iter()
            .any(|existing| existing.subsumes(request))
        {
            self.requests.push(request);
        }
        Ok(())
    }

    pub fn slot_visitor_descriptor(&self, code_name: &'static str) -> SlotVisitorDescriptor {
        SlotVisitorDescriptor::new(self.id, code_name, self.epoch)
    }

    pub fn conservative_roots(&self) -> ConservativeRoots {
        self.conservative_roots.clone()
    }

    pub fn root_marking_plan(&self) -> RootMarkingPlan {
        RootMarkingPlan {
            precise_roots: self.roots.records.clone(),
            targeted_roots: self.targeted_roots.records.clone(),
            conservative_spans: self.conservative_roots.spans().to_vec(),
            source: crate::gc::ConservativeRootSource::MachineStack,
        }
    }

    pub fn heap_snapshot_builder(&self) -> HeapSnapshotBuilder {
        HeapSnapshotBuilder {
            snapshot: HeapSnapshotId(self.next_snapshot),
            kind: HeapSnapshotKind::Inspector,
            next_node: HeapSnapshotNodeId(1),
            include_weak_edges: false,
        }
    }

    pub fn statistics(&self) -> HeapStatistics {
        HeapStatistics {
            heap: self.id,
            epoch: self.epoch,
            ..HeapStatistics::default()
        }
    }

    pub fn allocation_profile(&self) -> AllocationProfile {
        AllocationProfile::default()
    }

    pub fn state_descriptor(&self) -> HeapStateDescriptor {
        HeapStateDescriptor {
            phase: self.phase,
            mutator_state: self.mutator_state,
            conductor: self.conductor,
            no_gc_scope_depth: self.no_gc_scope_depth,
            mutator_has_heap_access: self.mutator_has_heap_access,
            mutator_should_be_fenced: self.mutator_should_be_fenced,
        }
    }

    pub fn enter_phase(
        &mut self,
        phase: GcPhase,
        mutator_state: MutatorState,
        conductor: GcConductor,
    ) {
        self.phase = phase;
        self.mutator_state = mutator_state;
        self.conductor = conductor;
        self.mutator_has_heap_access = mutator_state == MutatorState::Running;
        self.mutator_should_be_fenced = matches!(phase, GcPhase::Concurrent | GcPhase::Reloop);
    }

    pub fn leave_phase(&mut self) {
        self.phase = GcPhase::NotRunning;
        self.mutator_state = MutatorState::Running;
        self.conductor = GcConductor::Mutator;
        self.mutator_has_heap_access = true;
        self.mutator_should_be_fenced = false;
    }

    pub fn enter_no_gc_scope(&mut self) -> NoGcScopeDepth {
        self.no_gc_scope_depth += 1;
        NoGcScopeDepth {
            heap: self.id,
            depth: self.no_gc_scope_depth,
        }
    }

    pub fn leave_no_gc_scope(&mut self, scope: NoGcScopeDepth) -> Result<(), HeapIntegrationError> {
        self.require_heap(scope.heap)?;
        if self.no_gc_scope_depth == 0 || scope.depth != self.no_gc_scope_depth {
            return Err(HeapIntegrationError::HeapSemantic(
                HeapSemanticError::NoGcScopeActive(HeapSemanticOperation::Observe),
            ));
        }
        self.no_gc_scope_depth -= 1;
        Ok(())
    }

    pub fn apply_root_mutation(
        &mut self,
        mutation: RootSetMutation,
    ) -> Result<RootSetMutationOutcome, HeapIntegrationError> {
        self.require_heap(mutation.record.heap)?;
        Ok(self.roots.apply_mutation(mutation)?)
    }

    pub fn register_targeted_root(
        &mut self,
        record: TargetedRootRecord,
        authority: RootSetMutationAuthority,
    ) -> Result<TargetedRootRecord, HeapIntegrationError> {
        self.require_targeted_root_record(record)?;
        Ok(self.targeted_roots.register(self.id, record, authority)?)
    }

    pub fn retarget_targeted_root(
        &mut self,
        record: TargetedRootRecord,
        authority: RootSetMutationAuthority,
    ) -> Result<TargetedRootRecord, HeapIntegrationError> {
        self.require_targeted_root_record(record)?;
        Ok(self.targeted_roots.retarget(self.id, record, authority)?)
    }

    pub fn unregister_targeted_root(
        &mut self,
        root: RootRecord,
        authority: RootSetMutationAuthority,
    ) -> Result<RootId, HeapIntegrationError> {
        self.require_heap(root.heap)?;
        Ok(self.targeted_roots.unregister(self.id, root, authority)?)
    }

    pub fn apply_write_barrier(
        &mut self,
        request: WriteBarrierApplicationRequest,
    ) -> Result<WriteBarrierApplicationRecord, HeapIntegrationError> {
        self.require_known_cell(request.owner)?;
        if let Some(target) = request.target {
            self.require_known_cell(target)?;
        }

        let operation = if request.context.initializing {
            HeapSemanticOperation::InitializeUnpublishedCell
        } else {
            HeapSemanticOperation::MutatePublishedCell
        };
        evaluate_heap_semantics(
            self.state_descriptor(),
            HeapMutationAuthority::Mutator,
            operation,
        )?;

        let outcome =
            static_barrier_schema_registry().evaluate_requirement(BarrierRequirementRequest {
                context: request.context,
                authority: request.authority,
                owner_is_published: request.owner_is_published,
            })?;
        let record = WriteBarrierApplicationRecord { request, outcome };
        self.barriers.push(record);
        Ok(record)
    }

    pub fn register_weak_set(
        &mut self,
        set: WeakSetDescriptor,
    ) -> Result<(), HeapIntegrationError> {
        self.require_heap(set.heap)?;
        self.weak.register_set(set)
    }

    pub fn register_weak(
        &mut self,
        record: WeakRegistrationRecord,
    ) -> Result<(), HeapIntegrationError> {
        if let Some(target) = record.target {
            self.require_known_cell(target)?;
        }
        self.weak.register_weak(record)
    }

    pub fn process_weak(
        &mut self,
        weak: WeakId,
        phase: WeakProcessingPhase,
        target_is_live: bool,
        owner: Option<WeakHandleOwnerContract>,
    ) -> Result<WeakProcessingTransitionRecord, HeapIntegrationError> {
        evaluate_heap_semantics(
            self.state_descriptor(),
            HeapMutationAuthority::WeakProcessor,
            HeapSemanticOperation::ProcessWeak,
        )?;
        self.weak.process_weak(weak, phase, target_is_live, owner)
    }

    pub fn register_finalizer_callback(
        &mut self,
        callback: HeapFinalizerCallback,
    ) -> Result<(), HeapIntegrationError> {
        self.finalizers.register_callback(callback)
    }

    pub fn register_finalizer(
        &mut self,
        record: FinalizerQueueRecord,
    ) -> Result<(), HeapIntegrationError> {
        self.require_known_cell(record.target)?;
        self.finalizers.register_finalizer(record)
    }

    pub fn process_finalizer(
        &mut self,
        finalizer: FinalizerId,
        target_is_live: bool,
    ) -> Result<FinalizerQueueTransitionRecord, HeapIntegrationError> {
        evaluate_heap_semantics(
            self.state_descriptor(),
            HeapMutationAuthority::Finalizer,
            HeapSemanticOperation::RunFinalizers,
        )?;
        self.finalizers.process_finalizer(finalizer, target_is_live)
    }

    pub fn ingest_conservative_roots(
        &mut self,
        roots: ConservativeRoots,
    ) -> Result<(), HeapIntegrationError> {
        let plan = RootMarkingPlan {
            precise_roots: Vec::new(),
            targeted_roots: Vec::new(),
            conservative_spans: roots.spans().to_vec(),
            source: crate::gc::ConservativeRootSource::MachineStack,
        };
        plan.validate()?;
        self.conservative_roots.extend(roots);
        Ok(())
    }

    pub fn begin_heap_snapshot(&mut self, kind: HeapSnapshotKind) -> HeapSnapshotId {
        let id = HeapSnapshotId(self.next_snapshot);
        self.next_snapshot += 1;
        let previous = self.snapshots.last().map(|snapshot| snapshot.id);
        self.snapshots
            .push(HeapSnapshotRecord::new(id, previous, kind, self.epoch));
        id
    }

    pub fn record_snapshot_node(
        &mut self,
        snapshot: HeapSnapshotId,
        cell: CellId,
        class_name: &'static str,
        retained_size: usize,
    ) -> Result<HeapSnapshotNodeId, HeapIntegrationError> {
        self.require_known_cell(cell)?;
        let snapshot = self.snapshot_mut(snapshot)?;
        if snapshot.finalized {
            return Err(HeapIntegrationError::SnapshotAlreadyFinalized(snapshot.id));
        }
        let node = HeapSnapshotNodeId(snapshot.nodes.len() as u32 + 1);
        snapshot.nodes.push(HeapSnapshotCellRecord {
            id: node,
            cell,
            class_name,
            retained_size,
        });
        snapshot.validate()?;
        Ok(node)
    }

    pub fn record_snapshot_edge(
        &mut self,
        snapshot: HeapSnapshotId,
        edge: HeapSnapshotEdge,
    ) -> Result<(), HeapIntegrationError> {
        let snapshot = self.snapshot_mut(snapshot)?;
        if snapshot.finalized {
            return Err(HeapIntegrationError::SnapshotAlreadyFinalized(snapshot.id));
        }
        snapshot.edges.push(edge);
        snapshot.validate()?;
        Ok(())
    }

    pub fn finish_heap_snapshot(
        &mut self,
        snapshot: HeapSnapshotId,
    ) -> Result<HeapSnapshotRecord, HeapIntegrationError> {
        let snapshot = self.snapshot_mut(snapshot)?;
        snapshot.validate()?;
        snapshot.finalized = true;
        Ok(snapshot.clone())
    }

    fn require_heap(&self, heap: HeapId) -> Result<(), HeapIntegrationError> {
        if heap == self.id {
            Ok(())
        } else {
            Err(HeapIntegrationError::HeapMismatch {
                expected: self.id,
                actual: heap,
            })
        }
    }

    fn require_known_cell(&self, cell: CellId) -> Result<(), HeapIntegrationError> {
        if self
            .allocations
            .iter()
            .any(|record| record.response.cell == cell)
        {
            Ok(())
        } else {
            Err(HeapIntegrationError::UnknownCell(cell))
        }
    }

    fn require_targeted_root_record(
        &self,
        record: TargetedRootRecord,
    ) -> Result<(), HeapIntegrationError> {
        self.require_heap(record.root.heap)?;
        validate_targeted_root_record(self.id, record)?;
        self.require_known_cell(record.target)
    }

    fn snapshot_mut(
        &mut self,
        snapshot: HeapSnapshotId,
    ) -> Result<&mut HeapSnapshotRecord, HeapIntegrationError> {
        self.snapshots
            .iter_mut()
            .find(|record| record.id == snapshot)
            .ok_or(HeapIntegrationError::InvalidSnapshotId(snapshot))
    }
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

/// Typed allocation domain.
///
/// A subspace classifies allocation requests for a payload type. It borrows
/// type metadata and does not allocate, sweep, or own individual cells.
#[derive(Debug)]
pub struct Subspace<T: ?Sized> {
    name: &'static str,
    metadata: CellMetadata,
    kind: SubspaceKind,
    _cell: PhantomData<T>,
}

impl<T: ?Sized> Subspace<T> {
    pub fn new(name: &'static str, metadata: CellMetadata) -> Self {
        Self {
            name,
            metadata,
            kind: SubspaceKind::Complete,
            _cell: PhantomData,
        }
    }

    pub fn with_kind(name: &'static str, metadata: CellMetadata, kind: SubspaceKind) -> Self {
        Self {
            name,
            metadata,
            kind,
            _cell: PhantomData,
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn metadata(&self) -> CellMetadata {
        self.metadata
    }

    pub fn kind(&self) -> SubspaceKind {
        self.kind
    }

    pub fn descriptor(&self) -> SubspaceDescriptor {
        SubspaceDescriptor {
            name: self.name,
            kind: self.kind,
            metadata: self.metadata,
            aligned_allocator: None,
            lower_tier_precise_cells: 0,
            first_directory: None,
            directory_for_empty_allocation: None,
            next_subspace_in_aligned_allocator: None,
            mutation_authority: crate::gc::SubspaceMutationAuthority::HeapInitialization,
        }
    }
}

/// Allocation area for cell-sized blocks.
///
/// The arena names a heap allocation domain. It owns allocation bookkeeping
/// only through the heap; cell identity is still assigned by the GC layer.
#[derive(Debug)]
pub struct CellArena {
    pub name: &'static str,
    pub heap: HeapId,
    pub epoch: HeapEpoch,
}

/// Inert allocation request that documents the decisions made before allocation.
///
/// This is a request descriptor, not an allocation token. It carries no cell
/// lifetime and grants no mutation authority by itself.
#[derive(Clone, Copy, Debug)]
pub struct AllocationPlan<T: ?Sized> {
    pub heap: HeapId,
    pub subspace: &'static str,
    pub metadata: CellMetadata,
    pub mode: AllocationMode,
    pub may_trigger_collection: bool,
    _cell: PhantomData<T>,
}

impl<T: ?Sized> AllocationPlan<T> {
    pub fn no_gc(mut self) -> Self {
        self.may_trigger_collection = false;
        self.mode = AllocationMode::NoCollection;
        self
    }

    pub fn request(&self, byte_size: usize) -> HeapAllocationRequest {
        HeapAllocationRequest {
            heap: self.heap,
            subspace: self.subspace,
            metadata: self.metadata,
            byte_size,
            mode: self.mode,
            may_trigger_collection: self.may_trigger_collection,
        }
    }
}

/// Unpublished allocation state before finish-creation.
///
/// Barrier-free initialization belongs behind this type. Once `finish` returns,
/// normal owner-aware barrier APIs are required.
/// The token borrows an unpublished heap cell for initialization; it does not
/// transfer storage ownership away from `Heap`.
#[derive(Debug)]
pub struct CellInit<T: TraceCell> {
    cell: GcRef<T>,
}

impl<T: TraceCell> CellInit<T> {
    pub fn new_unpublished(cell: GcRef<T>) -> Self {
        Self { cell }
    }

    pub fn cell(&self) -> GcRef<T> {
        self.cell
    }

    pub fn finish(self) -> GcRef<T> {
        self.cell
    }
}

/// Root category used by conservative and precise root visitors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootKind {
    Handle,
    ExplicitRoot,
    VMRegister,
    Stack,
    JitCode,
    Host,
}

/// Opaque root identity. It is not a pointer and does not imply liveness alone.
///
/// Root records are registry entries owned by the heap or VM handle set. They
/// must never be compared with or converted into `CellId`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct RootId(pub u64);

/// Metadata for one root slot owned by the VM, handle set, stack scan, or host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootRecord {
    pub id: RootId,
    pub kind: RootKind,
    pub heap: HeapId,
}

/// Precise root metadata plus the cell identity reached from that root slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TargetedRootRecord {
    pub root: RootRecord,
    pub target: CellId,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RootSetMutationAuthority {
    #[default]
    HandleScope,
    ExplicitRootRegistry,
    VmRegisterFile,
    JitCodeRegistry,
    HostIntegration,
    ConservativeScanner,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RootSetMutationKind {
    #[default]
    Register,
    Unregister,
    Retarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootSetMutation {
    pub kind: RootSetMutationKind,
    pub authority: RootSetMutationAuthority,
    pub record: RootRecord,
}

impl RootSetMutation {
    pub const fn register(record: RootRecord, authority: RootSetMutationAuthority) -> Self {
        Self {
            kind: RootSetMutationKind::Register,
            authority,
            record,
        }
    }

    pub const fn unregister(record: RootRecord, authority: RootSetMutationAuthority) -> Self {
        Self {
            kind: RootSetMutationKind::Unregister,
            authority,
            record,
        }
    }

    pub const fn retarget(record: RootRecord, authority: RootSetMutationAuthority) -> Self {
        Self {
            kind: RootSetMutationKind::Retarget,
            authority,
            record,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootSetMutationOutcome {
    Registered(RootRecord),
    Unregistered(RootId),
    Retargeted(RootRecord),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootReachabilitySemantics {
    pub root: RootRecord,
    pub reason: crate::gc::RootMarkReason,
    pub precise: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootSetSemanticError {
    InvalidRootId(RootId),
    InvalidRootTarget {
        root: RootId,
        target: CellId,
    },
    DuplicateRoot(RootId),
    UnknownRoot(RootId),
    HeapMismatch {
        expected: HeapId,
        actual: HeapId,
    },
    UnsupportedPreciseStackRoot(RootId),
    AuthorityMismatch {
        kind: RootKind,
        authority: RootSetMutationAuthority,
    },
}

/// Precise roots registered with the heap.
#[derive(Clone, Debug, Default)]
pub struct RootSet {
    records: Vec<RootRecord>,
}

impl RootSet {
    pub fn records(&self) -> &[RootRecord] {
        &self.records
    }

    pub fn from_records(records: Vec<RootRecord>) -> Result<Self, RootSetSemanticError> {
        let set = Self { records };
        set.validate()?;
        Ok(set)
    }

    pub fn validate(&self) -> Result<(), RootSetSemanticError> {
        // O(n) duplicate detection via a seen-set instead of the previous
        // O(n^2) `records[..index]` nested scan. This validate runs per
        // bytecode instruction (interpreter desired-root-set construction),
        // so the quadratic form dominated hot loops. `insert` returns false
        // on the second occurrence of an id, yielding the same first-duplicate
        // `DuplicateRoot(root.id)` error as the original scan.
        let mut seen = HashSet::with_capacity(self.records.len());
        for root in &self.records {
            validate_root_record(*root)?;
            if !seen.insert(root.id) {
                return Err(RootSetSemanticError::DuplicateRoot(root.id));
            }
        }

        Ok(())
    }

    pub fn evaluate_mutation(
        &self,
        mutation: RootSetMutation,
    ) -> Result<RootSetMutationOutcome, RootSetSemanticError> {
        validate_root_record(mutation.record)?;
        validate_root_authority(mutation.record.kind, mutation.authority)?;

        let existing = self
            .records
            .iter()
            .find(|record| record.id == mutation.record.id);
        if let Some(existing) = existing {
            if existing.heap != mutation.record.heap {
                return Err(RootSetSemanticError::HeapMismatch {
                    expected: existing.heap,
                    actual: mutation.record.heap,
                });
            }
        }

        match mutation.kind {
            RootSetMutationKind::Register if existing.is_some() => {
                Err(RootSetSemanticError::DuplicateRoot(mutation.record.id))
            }
            RootSetMutationKind::Register => {
                Ok(RootSetMutationOutcome::Registered(mutation.record))
            }
            RootSetMutationKind::Unregister if existing.is_none() => {
                Err(RootSetSemanticError::UnknownRoot(mutation.record.id))
            }
            RootSetMutationKind::Unregister => {
                Ok(RootSetMutationOutcome::Unregistered(mutation.record.id))
            }
            RootSetMutationKind::Retarget if existing.is_none() => {
                Err(RootSetSemanticError::UnknownRoot(mutation.record.id))
            }
            RootSetMutationKind::Retarget => {
                Ok(RootSetMutationOutcome::Retargeted(mutation.record))
            }
        }
    }

    pub fn apply_mutation(
        &mut self,
        mutation: RootSetMutation,
    ) -> Result<RootSetMutationOutcome, RootSetSemanticError> {
        let outcome = self.evaluate_mutation(mutation)?;
        match outcome {
            RootSetMutationOutcome::Registered(record) => self.records.push(record),
            RootSetMutationOutcome::Unregistered(root) => {
                self.records.retain(|record| record.id != root);
            }
            RootSetMutationOutcome::Retargeted(record) => {
                if let Some(existing) = self
                    .records
                    .iter_mut()
                    .find(|existing| existing.id == record.id)
                {
                    *existing = record;
                }
            }
        }
        Ok(outcome)
    }

    pub fn reachability_semantics(
        &self,
    ) -> Result<Vec<RootReachabilitySemantics>, RootSetSemanticError> {
        self.validate()?;
        Ok(self
            .records
            .iter()
            .map(|root| RootReachabilitySemantics {
                root: *root,
                reason: crate::gc::root_mark_reason_for_kind(root.kind),
                precise: true,
            })
            .collect())
    }
}

/// Precise roots paired with the cell IDs they keep reachable.
///
/// `records` is the authoritative live set. `index` mirrors it as a
/// `RootId -> Vec position` map so callers (notably the per-instruction
/// interpreter root sync) can resolve a record by root identity in O(1)
/// instead of scanning the whole set on every bytecode. The index is a
/// Rust-internal acceleration structure with no C++ JSC counterpart (C++ JSC
/// roots VM registers via conservative stack scanning rather than an eager
/// per-instruction precise targeted-root registry), so it is maintained in
/// lock-step with `records` on every register/retarget/unregister.
#[derive(Clone, Debug, Default)]
pub struct TargetedRootSet {
    records: Vec<TargetedRootRecord>,
    index: HashMap<RootId, usize>,
}

impl TargetedRootSet {
    pub fn records(&self) -> &[TargetedRootRecord] {
        &self.records
    }

    /// O(1) lookup of the live record for `root` (full `RootRecord` equality,
    /// matching the previous linear `find(|existing| existing.root == ...)`).
    pub fn record_for_root(&self, root: RootRecord) -> Option<&TargetedRootRecord> {
        self.index
            .get(&root.id)
            .map(|&position| &self.records[position])
            .filter(|record| record.root == root)
    }

    /// O(1) membership test for `root` (full `RootRecord` equality, matching the
    /// previous linear `any(|record| record.root == root)`).
    pub fn contains_root(&self, root: RootRecord) -> bool {
        self.record_for_root(root).is_some()
    }

    fn rebuild_index(&mut self) {
        self.index.clear();
        for (position, record) in self.records.iter().enumerate() {
            self.index.insert(record.root.id, position);
        }
    }

    pub fn from_records(
        heap: HeapId,
        records: Vec<TargetedRootRecord>,
    ) -> Result<Self, RootSetSemanticError> {
        let mut set = Self {
            records,
            index: HashMap::new(),
        };
        set.validate(heap)?;
        set.rebuild_index();
        Ok(set)
    }

    pub fn validate(&self, heap: HeapId) -> Result<(), RootSetSemanticError> {
        RootSet::from_records(self.records.iter().map(|record| record.root).collect())?;

        for record in &self.records {
            validate_targeted_root_record(heap, *record)?;
        }

        Ok(())
    }

    pub fn register(
        &mut self,
        heap: HeapId,
        record: TargetedRootRecord,
        authority: RootSetMutationAuthority,
    ) -> Result<TargetedRootRecord, RootSetSemanticError> {
        self.evaluate_targeted_mutation(
            heap,
            RootSetMutation::register(record.root, authority),
            Some(record),
        )?;
        // The new record lands at the tail; record its position in the index.
        let position = self.records.len();
        self.records.push(record);
        self.index.insert(record.root.id, position);
        Ok(record)
    }

    pub fn retarget(
        &mut self,
        heap: HeapId,
        record: TargetedRootRecord,
        authority: RootSetMutationAuthority,
    ) -> Result<TargetedRootRecord, RootSetSemanticError> {
        self.evaluate_targeted_mutation(
            heap,
            RootSetMutation::retarget(record.root, authority),
            Some(record),
        )?;
        // Retarget replaces the record at its existing slot in place; the root id
        // (and therefore the index entry and Vec position) is unchanged.
        if let Some(&position) = self.index.get(&record.root.id) {
            self.records[position] = record;
        }
        Ok(record)
    }

    pub fn unregister(
        &mut self,
        heap: HeapId,
        root: RootRecord,
        authority: RootSetMutationAuthority,
    ) -> Result<RootId, RootSetSemanticError> {
        let outcome = self.evaluate_targeted_mutation(
            heap,
            RootSetMutation::unregister(root, authority),
            None,
        )?;
        let root = match outcome {
            RootSetMutationOutcome::Unregistered(root) => root,
            _ => unreachable!("unregister mutation cannot produce another root outcome"),
        };
        self.records.retain(|record| record.root.id != root);
        // `retain` shifts the positions of every record after the removed slot,
        // so the position index must be rebuilt to stay consistent.
        self.rebuild_index();
        Ok(root)
    }

    /// Evaluate a single register/retarget/unregister against the live set in
    /// O(1) amortized time.
    ///
    /// The previous implementation re-validated the *entire* set on every
    /// mutation (`self.validate(heap)` plus a second
    /// `RootSet::from_records(...).evaluate_mutation(...)`), making each
    /// mutation O(records^2) and call-heavy interpreter code O(instructions x
    /// records^2). That full-set re-validation is redundant defensive work:
    /// `from_records`/`register`/`retarget`/`unregister` maintain the set
    /// invariant (no duplicate ids, every record valid for `heap`)
    /// incrementally, so if the set was valid before this mutation and the
    /// single mutation is valid, the set stays valid. We therefore drop the
    /// full-set re-validation and resolve existence through the O(1) `index`,
    /// while raising exactly the same errors in the same order as the old
    /// path:
    ///   1. `mutation.record.heap != heap` -> `HeapMismatch` (unchanged).
    ///   2. validate the single mutated record (targeted-record validation for
    ///      register/retarget, plain root validation for unregister) -
    ///      unchanged, only the *one* record is checked instead of every
    ///      record.
    ///   3. the body of `RootSet::evaluate_mutation`
    ///      (`validate_root_record` + `validate_root_authority` + existing-root
    ///      heap match + Register/Unregister/Retarget -> Duplicate/Unknown/OK),
    ///      with the linear `records.iter().find(id)` existence scan replaced by
    ///      an O(1) `self.index.get(&id)` lookup. Existence semantics are
    ///      identical because both match on `RootId` alone and `index` mirrors
    ///      `records` one-to-one.
    ///
    /// Safety: the dropped full-set `validate(heap)` could in principle have
    /// caught a pre-existing duplicate or invalid record already living in the
    /// set. That state is unreachable here: every prior mutation went through
    /// this same validated path (or `from_records`, which validates eagerly),
    /// so the set is always valid on entry. There is no public API that can
    /// inject an unvalidated record into `records`/`index`.
    fn evaluate_targeted_mutation(
        &self,
        heap: HeapId,
        mutation: RootSetMutation,
        record: Option<TargetedRootRecord>,
    ) -> Result<RootSetMutationOutcome, RootSetSemanticError> {
        if mutation.record.heap != heap {
            return Err(RootSetSemanticError::HeapMismatch {
                expected: heap,
                actual: mutation.record.heap,
            });
        }
        if let Some(record) = record {
            validate_targeted_root_record(heap, record)?;
        } else {
            validate_root_record(mutation.record)?;
        }

        // Inlined `RootSet::evaluate_mutation`, with the linear existence scan
        // replaced by the O(1) index lookup. Matches its checks and error
        // order exactly.
        validate_root_record(mutation.record)?;
        validate_root_authority(mutation.record.kind, mutation.authority)?;

        let existing = self
            .index
            .get(&mutation.record.id)
            .map(|&position| &self.records[position]);
        if let Some(existing) = existing {
            if existing.root.heap != mutation.record.heap {
                return Err(RootSetSemanticError::HeapMismatch {
                    expected: existing.root.heap,
                    actual: mutation.record.heap,
                });
            }
        }

        match mutation.kind {
            RootSetMutationKind::Register if existing.is_some() => {
                Err(RootSetSemanticError::DuplicateRoot(mutation.record.id))
            }
            RootSetMutationKind::Register => {
                Ok(RootSetMutationOutcome::Registered(mutation.record))
            }
            RootSetMutationKind::Unregister if existing.is_none() => {
                Err(RootSetSemanticError::UnknownRoot(mutation.record.id))
            }
            RootSetMutationKind::Unregister => {
                Ok(RootSetMutationOutcome::Unregistered(mutation.record.id))
            }
            RootSetMutationKind::Retarget if existing.is_none() => {
                Err(RootSetSemanticError::UnknownRoot(mutation.record.id))
            }
            RootSetMutationKind::Retarget => {
                Ok(RootSetMutationOutcome::Retargeted(mutation.record))
            }
        }
    }
}

fn validate_targeted_root_record(
    heap: HeapId,
    record: TargetedRootRecord,
) -> Result<(), RootSetSemanticError> {
    validate_root_record(record.root)?;
    if record.root.heap != heap {
        return Err(RootSetSemanticError::HeapMismatch {
            expected: heap,
            actual: record.root.heap,
        });
    }
    if record.target == CellId::default() {
        return Err(RootSetSemanticError::InvalidRootTarget {
            root: record.root.id,
            target: record.target,
        });
    }
    Ok(())
}

fn validate_root_record(root: RootRecord) -> Result<(), RootSetSemanticError> {
    if root.id == RootId::default() {
        return Err(RootSetSemanticError::InvalidRootId(root.id));
    }
    if root.kind == RootKind::Stack {
        return Err(RootSetSemanticError::UnsupportedPreciseStackRoot(root.id));
    }
    Ok(())
}

fn validate_root_authority(
    kind: RootKind,
    authority: RootSetMutationAuthority,
) -> Result<(), RootSetSemanticError> {
    let allowed = matches!(
        (kind, authority),
        (RootKind::Handle, RootSetMutationAuthority::HandleScope)
            | (
                RootKind::ExplicitRoot,
                RootSetMutationAuthority::ExplicitRootRegistry
            )
            | (
                RootKind::VMRegister,
                RootSetMutationAuthority::VmRegisterFile
            )
            | (RootKind::JitCode, RootSetMutationAuthority::JitCodeRegistry)
            | (RootKind::Host, RootSetMutationAuthority::HostIntegration)
    );
    if allowed {
        Ok(())
    } else {
        Err(RootSetSemanticError::AuthorityMismatch { kind, authority })
    }
}

/// Raw address range supplied by machine-stack or host conservative scanning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConservativeRootSpan {
    pub begin: usize,
    pub end: usize,
}

/// Visitor boundary for precise and conservative roots.
pub trait RootVisitor {
    fn visit_precise_root(&mut self, root: RootRecord);
    fn visit_conservative_span(&mut self, span: ConservativeRootSpan);
}

/// Weak processing phase. Weak clearing is distinct from finalization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WeakProcessingPhase {
    Discover,
    Validate,
    Clear,
}

/// Opaque weak slot identity owned by the heap's weak registry.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct WeakId(pub u64);

/// Registry for weak slots and ephemeron-like edges.
///
/// The registry owns weak-slot metadata and clearing phase state. It observes
/// borrowed cell references but does not replace `CellId` as raw identity.
#[derive(Clone, Debug, Default)]
pub struct WeakRegistry {
    generation: u64,
    sets: Vec<WeakSetDescriptor>,
    slots: Vec<WeakRegistrationRecord>,
    transitions: Vec<WeakProcessingTransitionRecord>,
}

impl WeakRegistry {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn sets(&self) -> &[WeakSetDescriptor] {
        &self.sets
    }

    pub fn slots(&self) -> &[WeakRegistrationRecord] {
        &self.slots
    }

    pub fn transitions(&self) -> &[WeakProcessingTransitionRecord] {
        &self.transitions
    }

    pub fn processing_phase(&self) -> Option<WeakProcessingPhase> {
        self.sets.iter().find_map(|set| set.active_phase)
    }

    pub fn register_set(&mut self, set: WeakSetDescriptor) -> Result<(), HeapIntegrationError> {
        if self.sets.iter().any(|existing| existing.id == set.id) {
            return Err(HeapIntegrationError::DuplicateWeakSet(set.id));
        }
        self.sets.push(set);
        self.generation += 1;
        Ok(())
    }

    pub fn register_weak(
        &mut self,
        record: WeakRegistrationRecord,
    ) -> Result<(), HeapIntegrationError> {
        if !self.sets.iter().any(|set| set.id == record.set) {
            return Err(HeapIntegrationError::UnknownWeakSet(record.set));
        }
        if self.slots.iter().any(|existing| existing.id == record.id) {
            return Err(HeapIntegrationError::DuplicateWeak(record.id));
        }
        self.slots.push(record);
        self.generation += 1;
        Ok(())
    }

    pub fn process_weak(
        &mut self,
        weak: WeakId,
        phase: WeakProcessingPhase,
        target_is_live: bool,
        owner: Option<WeakHandleOwnerContract>,
    ) -> Result<WeakProcessingTransitionRecord, HeapIntegrationError> {
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| slot.id == weak)
            .ok_or(HeapIntegrationError::UnknownWeak(weak))?;
        let mut request =
            WeakSlotTransitionRequest::new(slot.state, phase, slot.kind, target_is_live);
        if let Some(owner) = owner {
            request = request.owner(owner);
        }
        let outcome = WeakSlotState::transition(request)?;
        slot.state = outcome.to;
        if outcome.clears_target {
            slot.target = None;
        }
        let record = WeakProcessingTransitionRecord {
            weak,
            phase,
            target_was_live: target_is_live,
            outcome,
        };
        self.transitions.push(record);
        self.generation += 1;
        Ok(record)
    }
}

/// Finalizer callback identity. The callback body is a host/VM integration point.
///
/// This names a registry callback, not a finalizable cell. Target cells are
/// still identified through GC-owned cell references or `CellId` wrappers.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct FinalizerId(pub u64);

/// Finalization hooks run after the collector has decided an object is dead.
///
/// The registry owns callback records. Destruction authority is granted only by
/// collector/sweeper phase decisions, not by holding a `FinalizerId`.
#[derive(Clone, Debug, Default)]
pub struct FinalizationRegistry {
    generation: u64,
    finalizers: Vec<FinalizerRecord>,
    callbacks: Vec<HeapFinalizerCallback>,
    queue: Vec<FinalizerQueueRecord>,
    transitions: Vec<FinalizerQueueTransitionRecord>,
}

impl FinalizationRegistry {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn finalizers(&self) -> &[FinalizerRecord] {
        &self.finalizers
    }

    pub fn callbacks(&self) -> &[HeapFinalizerCallback] {
        &self.callbacks
    }

    pub fn queue(&self) -> &[FinalizerQueueRecord] {
        &self.queue
    }

    pub fn transitions(&self) -> &[FinalizerQueueTransitionRecord] {
        &self.transitions
    }

    pub fn register_callback(
        &mut self,
        callback: HeapFinalizerCallback,
    ) -> Result<(), HeapIntegrationError> {
        if self
            .callbacks
            .iter()
            .any(|existing| existing.id == callback.id)
        {
            return Err(HeapIntegrationError::DuplicateFinalizerCallback(
                callback.id,
            ));
        }
        self.callbacks.push(callback);
        self.generation += 1;
        Ok(())
    }

    pub fn register_finalizer(
        &mut self,
        record: FinalizerQueueRecord,
    ) -> Result<(), HeapIntegrationError> {
        if !self
            .callbacks
            .iter()
            .any(|callback| callback.id == record.callback)
        {
            return Err(HeapIntegrationError::UnknownFinalizerCallback(
                record.callback,
            ));
        }
        if self.queue.iter().any(|existing| existing.id == record.id) {
            return Err(HeapIntegrationError::DuplicateFinalizer(record.id));
        }
        FinalizerPlan::from_records(
            &self.callbacks,
            &[FinalizerPlanningRecord {
                id: record.id,
                callback: record.callback,
                target: record.target,
                owner: record.owner,
                kind: record.kind,
            }],
        )?;
        self.queue.push(record);
        self.generation += 1;
        Ok(())
    }

    pub fn process_finalizer(
        &mut self,
        finalizer: FinalizerId,
        target_is_live: bool,
    ) -> Result<FinalizerQueueTransitionRecord, HeapIntegrationError> {
        let record = self
            .queue
            .iter_mut()
            .find(|record| record.id == finalizer)
            .ok_or(HeapIntegrationError::UnknownFinalizer(finalizer))?;
        let callback_registered = self
            .callbacks
            .iter()
            .any(|callback| callback.id == record.callback);
        let outcome = FinalizerState::transition(FinalizerTransitionRequest {
            state: record.state,
            target_is_live,
            phase: GcPhase::End,
            callback_registered,
            kind: record.kind,
        })?;
        let transition = FinalizerQueueTransitionRecord {
            finalizer,
            target_was_live: target_is_live,
            from: outcome.from,
            to: outcome.to,
            invokes_callback: outcome.invokes_callback,
        };
        record.state = outcome.to;
        self.transitions.push(transition);
        self.generation += 1;
        Ok(transition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{
        static_allocation_schema_registry, static_cell_metadata_registry, BarrierFieldKind,
        CellState, CellType, CollectionKind, ConservativeRootSource, HeapSnapshotEdgeName,
        HeapSnapshotEdgeType, JsCellHeader, OpaqueRootId, OpaqueRootRecord, RootPlanStep, Trace,
        Tracer,
    };

    struct TestCell {
        header: JsCellHeader,
    }

    impl Trace for TestCell {
        fn trace(&self, _tracer: &mut dyn Tracer) {}
    }

    impl TraceCell for TestCell {
        fn cell_header(&self) -> &JsCellHeader {
            &self.header
        }
    }

    fn test_subspace(heap: &Heap) -> Subspace<TestCell> {
        heap.subspace::<TestCell>("object", object_metadata())
    }

    fn object_metadata() -> CellMetadata {
        static_cell_metadata_registry()
            .metadata_for_type(CellType::Object)
            .map(|descriptor| descriptor.metadata)
            .unwrap_or_default()
    }

    fn allocate_test_cell(heap: &mut Heap) -> CellId {
        let subspace = test_subspace(heap);
        let plan = heap.allocation_plan(&subspace);
        heap.allocate(plan, 64)
            .map(|response| response.cell)
            .expect("test allocation")
    }

    #[test]
    fn static_heap_schema_is_structurally_valid() {
        assert_eq!(static_heap_schema_descriptor().validate(), Ok(()));
    }

    #[test]
    fn heap_schema_builder_requires_registry_owners() {
        let descriptor =
            HeapSchemaDescriptorBuilder::new("heap", static_allocation_schema_registry()).build();

        assert_eq!(descriptor, Err(HeapSchemaValidationError::EmptyName));
    }

    #[test]
    fn heap_schema_builder_constructs_valid_descriptor() {
        let descriptor =
            HeapSchemaDescriptorBuilder::new("heap", static_allocation_schema_registry())
                .root_registry_owner("roots")
                .weak_registry_owner("weak")
                .finalizer_registry_owner("finalizers")
                .build();

        assert_eq!(descriptor.map(|descriptor| descriptor.name), Ok("heap"));
    }

    #[test]
    fn root_set_rejects_stack_root_as_precise_registration() {
        let root = RootRecord {
            id: RootId(1),
            kind: RootKind::Stack,
            heap: HeapId(7),
        };

        assert_eq!(
            RootSet::default().evaluate_mutation(RootSetMutation::register(
                root,
                RootSetMutationAuthority::ConservativeScanner
            )),
            Err(RootSetSemanticError::UnsupportedPreciseStackRoot(RootId(1)))
        );
    }

    #[test]
    fn root_set_rejects_wrong_mutation_authority() {
        let root = RootRecord {
            id: RootId(1),
            kind: RootKind::Handle,
            heap: HeapId(7),
        };

        assert_eq!(
            RootSet::default().evaluate_mutation(RootSetMutation::register(
                root,
                RootSetMutationAuthority::HostIntegration
            )),
            Err(RootSetSemanticError::AuthorityMismatch {
                kind: RootKind::Handle,
                authority: RootSetMutationAuthority::HostIntegration
            })
        );
    }

    #[test]
    fn root_set_reports_precise_root_mark_reason() {
        let set = RootSet::from_records(vec![RootRecord {
            id: RootId(1),
            kind: RootKind::ExplicitRoot,
            heap: HeapId(7),
        }])
        .expect("valid root set");

        assert_eq!(
            set.reachability_semantics().map(|entries| entries[0]),
            Ok(RootReachabilitySemantics {
                root: RootRecord {
                    id: RootId(1),
                    kind: RootKind::ExplicitRoot,
                    heap: HeapId(7)
                },
                reason: crate::gc::RootMarkReason::ProtectedValues,
                precise: true
            })
        );
    }

    #[test]
    fn targeted_root_set_accepts_valid_targeted_roots() {
        let roots = vec![
            TargetedRootRecord {
                root: RootRecord {
                    id: RootId(1),
                    kind: RootKind::ExplicitRoot,
                    heap: HeapId(7),
                },
                target: CellId(11),
            },
            TargetedRootRecord {
                root: RootRecord {
                    id: RootId(2),
                    kind: RootKind::Handle,
                    heap: HeapId(7),
                },
                target: CellId(12),
            },
        ];

        assert_eq!(
            TargetedRootSet::from_records(HeapId(7), roots).map(|set| set.records().len()),
            Ok(2)
        );
    }

    #[test]
    fn targeted_root_set_rejects_duplicate_roots_through_root_set() {
        let roots = vec![
            TargetedRootRecord {
                root: RootRecord {
                    id: RootId(1),
                    kind: RootKind::ExplicitRoot,
                    heap: HeapId(7),
                },
                target: CellId(11),
            },
            TargetedRootRecord {
                root: RootRecord {
                    id: RootId(1),
                    kind: RootKind::Handle,
                    heap: HeapId(7),
                },
                target: CellId(12),
            },
        ];

        assert_eq!(
            TargetedRootSet::from_records(HeapId(7), roots).map(|_| ()),
            Err(RootSetSemanticError::DuplicateRoot(RootId(1)))
        );
    }

    #[test]
    fn targeted_root_set_rejects_default_target() {
        let roots = vec![TargetedRootRecord {
            root: RootRecord {
                id: RootId(1),
                kind: RootKind::ExplicitRoot,
                heap: HeapId(7),
            },
            target: CellId::default(),
        }];

        assert_eq!(
            TargetedRootSet::from_records(HeapId(7), roots).map(|_| ()),
            Err(RootSetSemanticError::InvalidRootTarget {
                root: RootId(1),
                target: CellId::default()
            })
        );
    }

    #[test]
    fn targeted_root_set_rejects_heap_mismatch() {
        let roots = vec![TargetedRootRecord {
            root: RootRecord {
                id: RootId(1),
                kind: RootKind::ExplicitRoot,
                heap: HeapId(8),
            },
            target: CellId(11),
        }];

        assert_eq!(
            TargetedRootSet::from_records(HeapId(7), roots).map(|_| ()),
            Err(RootSetSemanticError::HeapMismatch {
                expected: HeapId(7),
                actual: HeapId(8)
            })
        );
    }

    #[test]
    fn heap_allocation_records_cell_identity_and_invalidation() {
        let mut heap = Heap::new();
        let subspace = test_subspace(&heap);
        let allocation = heap.allocate(heap.allocation_plan(&subspace), 64);

        assert_eq!(allocation.map(|response| response.cell), Ok(CellId(1)));
        assert_eq!(heap.allocation_records().len(), 1);

        let lifecycle = heap.invalidate_cell(HeapCellInvalidationRequest {
            cell: CellId(1),
            reason: CellZapReason::Destruction,
        });

        assert_eq!(
            lifecycle,
            Ok(CellLifecycleRecord {
                destruction_state: CellDestructionState::PendingDestruction,
                zap_reason: Some(CellZapReason::Destruction)
            })
        );
    }

    #[test]
    fn heap_cell_payload_binding_round_trips_through_heap() {
        let mut heap = Heap::new();
        let cell = allocate_test_cell(&mut heap);
        let payload = 0x1000;

        assert_eq!(heap.cell_for_payload(payload), None);
        assert_eq!(heap.payload_for_cell(cell), None);
        assert_eq!(heap.bind_cell_payload(cell, payload), Ok(()));
        assert_eq!(heap.cell_for_payload(payload), Some(cell));
        assert_eq!(heap.payload_for_cell(cell), Some(payload));
    }

    #[test]
    fn heap_cell_payload_binding_rejects_unknown_cell() {
        let mut heap = Heap::new();

        assert_eq!(
            heap.bind_cell_payload(CellId(99), 0x1000),
            Err(HeapIntegrationError::UnknownCell(CellId(99)))
        );
        assert_eq!(heap.cell_for_payload(0x1000), None);
    }

    #[test]
    fn heap_cell_payload_binding_rejects_zero_payload() {
        let mut heap = Heap::new();
        let cell = allocate_test_cell(&mut heap);

        assert_eq!(
            heap.bind_cell_payload(cell, 0),
            Err(HeapIntegrationError::ZeroCellPayload)
        );
        assert_eq!(heap.payload_for_cell(cell), None);
    }

    #[test]
    fn heap_cell_payload_binding_rejects_duplicate_payload() {
        let mut heap = Heap::new();
        let first = allocate_test_cell(&mut heap);
        let second = allocate_test_cell(&mut heap);
        let payload = 0x1000;

        assert_eq!(heap.bind_cell_payload(first, payload), Ok(()));
        assert_eq!(
            heap.bind_cell_payload(second, payload),
            Err(HeapIntegrationError::DuplicatePayloadBinding {
                payload,
                existing: first,
                requested: second
            })
        );
        assert_eq!(heap.cell_for_payload(payload), Some(first));
        assert_eq!(heap.payload_for_cell(second), None);
    }

    #[test]
    fn heap_cell_payload_binding_rejects_duplicate_cell_binding() {
        let mut heap = Heap::new();
        let cell = allocate_test_cell(&mut heap);
        let first_payload = 0x1000;
        let second_payload = 0x2000;

        assert_eq!(heap.bind_cell_payload(cell, first_payload), Ok(()));
        assert_eq!(
            heap.bind_cell_payload(cell, second_payload),
            Err(HeapIntegrationError::DuplicateCellPayloadBinding {
                cell,
                existing: first_payload,
                requested: second_payload
            })
        );
        assert_eq!(heap.payload_for_cell(cell), Some(first_payload));
        assert_eq!(heap.cell_for_payload(second_payload), None);
    }

    #[test]
    fn heap_cell_payload_binding_allows_idempotent_same_binding() {
        let mut heap = Heap::new();
        let cell = allocate_test_cell(&mut heap);
        let payload = 0x1000;

        assert_eq!(heap.bind_cell_payload(cell, payload), Ok(()));
        assert_eq!(heap.bind_cell_payload(cell, payload), Ok(()));
        assert_eq!(heap.cell_for_payload(payload), Some(cell));
        assert_eq!(heap.payload_for_cell(cell), Some(payload));
    }

    #[test]
    fn heap_no_gc_scope_blocks_collecting_allocation_and_collection_request() {
        let mut heap = Heap::new();
        let subspace = test_subspace(&heap);
        let scope = heap.enter_no_gc_scope();

        assert_eq!(
            heap.allocate(heap.allocation_plan(&subspace), 32),
            Err(HeapIntegrationError::HeapSemantic(
                HeapSemanticError::NoGcScopeActive(HeapSemanticOperation::Allocate {
                    may_trigger_collection: true
                })
            ))
        );
        assert_eq!(
            heap.queue_collection(CollectionRequest {
                kind: CollectionKind::Full,
                ..CollectionRequest::default()
            }),
            Err(HeapIntegrationError::HeapSemantic(
                HeapSemanticError::NoGcScopeActive(HeapSemanticOperation::QueueCollection)
            ))
        );
        assert!(heap
            .allocate(heap.allocation_plan(&subspace).no_gc(), 32)
            .is_ok());
        assert_eq!(heap.leave_no_gc_scope(scope), Ok(()));
    }

    #[test]
    fn heap_no_gc_scope_blocks_weak_processing() {
        let mut heap = Heap::new();
        let target = allocate_test_cell(&mut heap);
        heap.register_weak_set(WeakSetDescriptor {
            id: WeakSetId(1),
            heap: heap.id(),
            blocks: Vec::new(),
            allocator_block: None,
            next_allocator_block: None,
            active_phase: None,
        })
        .expect("weak set");
        heap.register_weak(WeakRegistrationRecord {
            id: WeakId(7),
            set: WeakSetId(1),
            owner: None,
            target: Some(target),
            kind: WeakEdgeKind::Ordinary,
            state: WeakSlotState::Live,
        })
        .expect("weak registration");
        let scope = heap.enter_no_gc_scope();
        heap.enter_phase(
            GcPhase::Fixpoint,
            MutatorState::Collecting,
            GcConductor::Collector,
        );

        assert_eq!(
            heap.process_weak(WeakId(7), WeakProcessingPhase::Validate, false, None),
            Err(HeapIntegrationError::HeapSemantic(
                HeapSemanticError::NoGcScopeActive(HeapSemanticOperation::ProcessWeak)
            ))
        );
        assert_eq!(heap.weak_registry().slots()[0].state, WeakSlotState::Live);
        assert!(heap.weak_registry().transitions().is_empty());

        heap.leave_phase();
        assert_eq!(heap.leave_no_gc_scope(scope), Ok(()));
    }

    #[test]
    fn heap_no_gc_scope_blocks_finalizer_processing() {
        let mut heap = Heap::new();
        let target = allocate_test_cell(&mut heap);
        heap.register_finalizer_callback(HeapFinalizerCallback {
            id: HeapFinalizerCallbackId(4),
            kind: FinalizerKind::CellDestructor,
            user_data_tag: 0,
        })
        .expect("callback registration");
        heap.register_finalizer(FinalizerQueueRecord {
            id: FinalizerId(5),
            callback: HeapFinalizerCallbackId(4),
            target,
            owner: None,
            kind: FinalizerKind::CellDestructor,
            state: FinalizerState::Registered,
        })
        .expect("finalizer registration");
        let scope = heap.enter_no_gc_scope();
        heap.enter_phase(
            GcPhase::End,
            MutatorState::Collecting,
            GcConductor::Collector,
        );

        assert_eq!(
            heap.process_finalizer(FinalizerId(5), false),
            Err(HeapIntegrationError::HeapSemantic(
                HeapSemanticError::NoGcScopeActive(HeapSemanticOperation::RunFinalizers)
            ))
        );
        assert_eq!(
            heap.finalization_registry().queue()[0].state,
            FinalizerState::Registered
        );
        assert!(heap.finalization_registry().transitions().is_empty());

        heap.leave_phase();
        assert_eq!(heap.leave_no_gc_scope(scope), Ok(()));
    }

    #[test]
    fn heap_root_registry_applies_register_and_unregister() {
        let mut heap = Heap::new();
        let root = RootRecord {
            id: RootId(9),
            kind: RootKind::ExplicitRoot,
            heap: heap.id(),
        };

        assert_eq!(
            heap.apply_root_mutation(RootSetMutation::register(
                root,
                RootSetMutationAuthority::ExplicitRootRegistry
            )),
            Ok(RootSetMutationOutcome::Registered(root))
        );
        assert_eq!(heap.roots().records().len(), 1);
        assert_eq!(
            heap.apply_root_mutation(RootSetMutation::unregister(
                root,
                RootSetMutationAuthority::ExplicitRootRegistry
            )),
            Ok(RootSetMutationOutcome::Unregistered(RootId(9)))
        );
        assert!(heap.roots().records().is_empty());
    }

    #[test]
    fn heap_targeted_root_registers_and_unregisters_record() {
        let mut heap = Heap::new();
        let target = allocate_test_cell(&mut heap);
        let root = RootRecord {
            id: RootId(10),
            kind: RootKind::ExplicitRoot,
            heap: heap.id(),
        };
        let record = TargetedRootRecord { root, target };

        assert_eq!(
            heap.register_targeted_root(record, RootSetMutationAuthority::ExplicitRootRegistry),
            Ok(record)
        );
        assert_eq!(heap.targeted_roots().records(), &[record]);
        assert!(heap.roots().records().is_empty());
        assert_eq!(
            heap.unregister_targeted_root(root, RootSetMutationAuthority::ExplicitRootRegistry),
            Ok(RootId(10))
        );
        assert!(heap.targeted_roots().records().is_empty());
    }

    #[test]
    fn heap_targeted_root_retargeting_updates_target_cell() {
        let mut heap = Heap::new();
        let first = allocate_test_cell(&mut heap);
        let second = allocate_test_cell(&mut heap);
        let root = RootRecord {
            id: RootId(11),
            kind: RootKind::Handle,
            heap: heap.id(),
        };
        let first_record = TargetedRootRecord {
            root,
            target: first,
        };
        let second_record = TargetedRootRecord {
            root,
            target: second,
        };

        assert_eq!(
            heap.register_targeted_root(first_record, RootSetMutationAuthority::HandleScope),
            Ok(first_record)
        );
        assert_eq!(
            heap.retarget_targeted_root(second_record, RootSetMutationAuthority::HandleScope),
            Ok(second_record)
        );
        assert_eq!(heap.targeted_roots().records(), &[second_record]);
    }

    #[test]
    fn heap_targeted_root_unregistering_rejects_unknown_root() {
        let mut heap = Heap::new();
        let root = RootRecord {
            id: RootId(12),
            kind: RootKind::ExplicitRoot,
            heap: heap.id(),
        };

        assert_eq!(
            heap.unregister_targeted_root(root, RootSetMutationAuthority::ExplicitRootRegistry),
            Err(HeapIntegrationError::Root(
                RootSetSemanticError::UnknownRoot(RootId(12))
            ))
        );
    }

    #[test]
    fn heap_targeted_root_rejects_unknown_target_cell() {
        let mut heap = Heap::new();
        let record = TargetedRootRecord {
            root: RootRecord {
                id: RootId(13),
                kind: RootKind::ExplicitRoot,
                heap: heap.id(),
            },
            target: CellId(99),
        };

        assert_eq!(
            heap.register_targeted_root(record, RootSetMutationAuthority::ExplicitRootRegistry),
            Err(HeapIntegrationError::UnknownCell(CellId(99)))
        );
        assert!(heap.targeted_roots().records().is_empty());
    }

    #[test]
    fn heap_targeted_root_rejects_default_target_cell() {
        let mut heap = Heap::new();
        let record = TargetedRootRecord {
            root: RootRecord {
                id: RootId(14),
                kind: RootKind::ExplicitRoot,
                heap: heap.id(),
            },
            target: CellId::default(),
        };

        assert_eq!(
            heap.register_targeted_root(record, RootSetMutationAuthority::ExplicitRootRegistry),
            Err(HeapIntegrationError::Root(
                RootSetSemanticError::InvalidRootTarget {
                    root: RootId(14),
                    target: CellId::default()
                }
            ))
        );
        assert!(heap.targeted_roots().records().is_empty());
    }

    #[test]
    fn heap_targeted_root_rejects_wrong_heap_root_record() {
        let mut heap = Heap::new();
        let target = allocate_test_cell(&mut heap);
        let record = TargetedRootRecord {
            root: RootRecord {
                id: RootId(15),
                kind: RootKind::ExplicitRoot,
                heap: HeapId(99),
            },
            target,
        };

        assert_eq!(
            heap.register_targeted_root(record, RootSetMutationAuthority::ExplicitRootRegistry),
            Err(HeapIntegrationError::HeapMismatch {
                expected: heap.id(),
                actual: HeapId(99)
            })
        );
        assert!(heap.targeted_roots().records().is_empty());
    }

    #[test]
    fn heap_targeted_root_marking_plan_exposes_targets_without_losing_roots() {
        let mut heap = Heap::new();
        let target = allocate_test_cell(&mut heap);
        let precise_root = RootRecord {
            id: RootId(16),
            kind: RootKind::ExplicitRoot,
            heap: heap.id(),
        };
        let targeted_root = RootRecord {
            id: RootId(17),
            kind: RootKind::Handle,
            heap: heap.id(),
        };
        let targeted_record = TargetedRootRecord {
            root: targeted_root,
            target,
        };

        heap.apply_root_mutation(RootSetMutation::register(
            precise_root,
            RootSetMutationAuthority::ExplicitRootRegistry,
        ))
        .expect("precise root registration");
        heap.register_targeted_root(targeted_record, RootSetMutationAuthority::HandleScope)
            .expect("targeted root registration");

        assert_eq!(
            heap.root_marking_plan().planned_steps(),
            Ok(vec![
                RootPlanStep::Precise {
                    root: precise_root,
                    reason: crate::gc::RootMarkReason::ProtectedValues
                },
                RootPlanStep::TargetedPrecise {
                    root: targeted_root,
                    target,
                    reason: crate::gc::RootMarkReason::StrongHandles
                }
            ])
        );
    }

    #[test]
    fn heap_barrier_application_records_required_marking_barrier() {
        let mut heap = Heap::new();
        let subspace = test_subspace(&heap);
        let owner = heap
            .allocate(heap.allocation_plan(&subspace), 64)
            .map(|response| response.cell)
            .expect("owner allocation");
        let target = heap
            .allocate(heap.allocation_plan(&subspace), 64)
            .map(|response| response.cell)
            .expect("target allocation");
        heap.publish_cell(owner).expect("publish owner");

        let record = heap.apply_write_barrier(WriteBarrierApplicationRequest {
            owner,
            target: Some(target),
            context: BarrierWriteContext::store(
                BarrierFieldKind::CellReference,
                CellState::PossiblyBlack,
                Some(CellState::PossiblyGrey),
            ),
            authority: BarrierMutationAuthority::MutatorFieldWrite,
            owner_is_published: true,
        });

        assert_eq!(
            record.map(|record| record.outcome),
            Ok(BarrierRequirementOutcome::Required(
                crate::gc::BarrierAction::MarkingBarrier
            ))
        );
        assert_eq!(heap.barrier_records().len(), 1);
    }

    #[test]
    fn heap_weak_processing_clears_dead_target() {
        let mut heap = Heap::new();
        let subspace = test_subspace(&heap);
        let target = heap
            .allocate(heap.allocation_plan(&subspace), 64)
            .map(|response| response.cell)
            .expect("target allocation");
        heap.register_weak_set(WeakSetDescriptor {
            id: WeakSetId(1),
            heap: heap.id(),
            blocks: Vec::new(),
            allocator_block: None,
            next_allocator_block: None,
            active_phase: None,
        })
        .expect("weak set");
        heap.register_weak(WeakRegistrationRecord {
            id: WeakId(7),
            set: WeakSetId(1),
            owner: None,
            target: Some(target),
            kind: WeakEdgeKind::Ordinary,
            state: WeakSlotState::Live,
        })
        .expect("weak registration");
        heap.enter_phase(
            GcPhase::Fixpoint,
            MutatorState::Collecting,
            GcConductor::Collector,
        );

        assert_eq!(
            heap.process_weak(WeakId(7), WeakProcessingPhase::Validate, false, None)
                .map(|transition| transition.outcome.to),
            Ok(WeakSlotState::ClearPending)
        );
        assert_eq!(
            heap.process_weak(WeakId(7), WeakProcessingPhase::Clear, false, None)
                .map(|transition| transition.outcome.clears_target),
            Ok(true)
        );
        assert_eq!(heap.weak_registry().slots()[0].target, None);
    }

    #[test]
    fn heap_finalization_queue_runs_ready_callback_boundary() {
        let mut heap = Heap::new();
        let subspace = test_subspace(&heap);
        let target = heap
            .allocate(heap.allocation_plan(&subspace), 64)
            .map(|response| response.cell)
            .expect("target allocation");
        heap.register_finalizer_callback(HeapFinalizerCallback {
            id: HeapFinalizerCallbackId(4),
            kind: FinalizerKind::CellDestructor,
            user_data_tag: 0,
        })
        .expect("callback registration");
        heap.register_finalizer(FinalizerQueueRecord {
            id: FinalizerId(5),
            callback: HeapFinalizerCallbackId(4),
            target,
            owner: None,
            kind: FinalizerKind::CellDestructor,
            state: FinalizerState::Registered,
        })
        .expect("finalizer registration");
        heap.enter_phase(
            GcPhase::End,
            MutatorState::Collecting,
            GcConductor::Collector,
        );

        assert_eq!(
            heap.process_finalizer(FinalizerId(5), false)
                .map(|transition| transition.to),
            Ok(FinalizerState::Ready)
        );
        assert_eq!(
            heap.process_finalizer(FinalizerId(5), false)
                .map(|transition| transition.invokes_callback),
            Ok(true)
        );
    }

    #[test]
    fn heap_ingests_conservative_scan_descriptors_into_root_plan() {
        let mut heap = Heap::new();
        let mut roots = ConservativeRoots::new();
        roots.add_span(ConservativeRootSpan {
            begin: 0x1000,
            end: 0x1010,
        });
        roots.add_candidate_address(0x1008);
        roots.add_opaque_root(OpaqueRootRecord {
            id: OpaqueRootId(3),
            address: 0x1008,
            reason: crate::gc::RootMarkReason::ConservativeScan,
        });

        assert_eq!(heap.ingest_conservative_roots(roots), Ok(()));
        assert_eq!(
            heap.conservative_root_records().candidate_addresses(),
            &[0x1008]
        );
        assert_eq!(
            heap.root_marking_plan().planned_steps(),
            Ok(vec![RootPlanStep::Conservative {
                span: ConservativeRootSpan {
                    begin: 0x1000,
                    end: 0x1010
                },
                source: ConservativeRootSource::MachineStack
            }])
        );
    }

    #[test]
    fn heap_snapshot_records_nodes_edges_and_finalization() {
        let mut heap = Heap::new();
        let subspace = test_subspace(&heap);
        let first = heap
            .allocate(heap.allocation_plan(&subspace), 64)
            .map(|response| response.cell)
            .expect("first allocation");
        let second = heap
            .allocate(heap.allocation_plan(&subspace), 64)
            .map(|response| response.cell)
            .expect("second allocation");
        let snapshot = heap.begin_heap_snapshot(HeapSnapshotKind::Inspector);
        let first_node = heap
            .record_snapshot_node(snapshot, first, "Object", 64)
            .expect("first node");
        let second_node = heap
            .record_snapshot_node(snapshot, second, "Object", 64)
            .expect("second node");
        heap.record_snapshot_edge(
            snapshot,
            HeapSnapshotEdge {
                from: first_node,
                to: second_node,
                edge_type: HeapSnapshotEdgeType::Property,
                name: HeapSnapshotEdgeName::String("child"),
            },
        )
        .expect("edge");

        let record = heap.finish_heap_snapshot(snapshot);

        assert_eq!(
            record.map(|record| (record.finalized, record.nodes.len(), record.edges.len())),
            Ok((true, 2, 1))
        );
    }
}
