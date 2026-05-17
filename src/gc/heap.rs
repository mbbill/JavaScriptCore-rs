//! Heap ownership and allocation contracts.

use core::marker::PhantomData;

use crate::gc::{
    AllocationMode, AllocationProfile, CellMetadata, CollectionRequest, ConservativeRoots,
    FinalizerRecord, GcActivityCallbackState, GcPhase, GcRef, HeapFinalizerCallback,
    HeapSnapshotBuilder, HeapStatistics, MarkedSpaceDescriptor, RootMarkingPlan,
    SlotVisitorDescriptor, SubspaceDescriptor, SubspaceKind, TraceCell, WeakSetDescriptor,
};

/// Opaque identity for one VM-owned heap.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct HeapId(pub u64);

/// Monotonic marker used to separate allocation and marking epochs.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct HeapEpoch(pub u64);

/// Owner of JavaScript-managed memory.
#[derive(Debug, Default)]
pub struct Heap {
    id: HeapId,
    phase: GcPhase,
    epoch: HeapEpoch,
    roots: RootSet,
    weak: WeakRegistry,
    finalizers: FinalizationRegistry,
    object_space: MarkedSpaceDescriptor,
    requests: Vec<CollectionRequest>,
    full_activity: GcActivityCallbackState,
    eden_activity: GcActivityCallbackState,
}

impl Heap {
    pub fn new() -> Self {
        let id = HeapId::default();
        Self {
            id,
            phase: GcPhase::Idle,
            epoch: HeapEpoch::default(),
            roots: RootSet::default(),
            weak: WeakRegistry::default(),
            finalizers: FinalizationRegistry::default(),
            object_space: MarkedSpaceDescriptor::new(id),
            requests: Vec::new(),
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
        // This is a planning boundary only. A future allocator will turn this
        // request into an unpublished cell and return it through `CellInit`.
        AllocationPlan {
            heap: self.id,
            subspace: subspace.name,
            metadata: subspace.metadata,
            mode: AllocationMode::Normal,
            may_trigger_collection: true,
            _cell: PhantomData,
        }
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

    pub fn queue_collection(&mut self, request: CollectionRequest) {
        if !self
            .requests
            .iter()
            .any(|existing| existing.subsumes(request))
        {
            self.requests.push(request);
        }
    }

    pub fn slot_visitor_descriptor(&self, code_name: &'static str) -> SlotVisitorDescriptor {
        SlotVisitorDescriptor::new(self.id, code_name, self.epoch)
    }

    pub fn conservative_roots(&self) -> ConservativeRoots {
        ConservativeRoots::new()
    }

    pub fn root_marking_plan(&self) -> RootMarkingPlan {
        RootMarkingPlan {
            precise_roots: self.roots.records.clone(),
            conservative_spans: Vec::new(),
            source: crate::gc::ConservativeRootSource::MachineStack,
        }
    }

    pub fn heap_snapshot_builder(&self) -> HeapSnapshotBuilder {
        HeapSnapshotBuilder::default()
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
}

/// Typed allocation domain.
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
        }
    }
}

/// Allocation area for cell-sized blocks.
#[derive(Debug)]
pub struct CellArena {
    pub name: &'static str,
    pub heap: HeapId,
    pub epoch: HeapEpoch,
}

/// Inert allocation request that documents the decisions made before allocation.
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
}

/// Unpublished allocation state before finish-creation.
///
/// Barrier-free initialization belongs behind this type. Once `finish` returns,
/// normal owner-aware barrier APIs are required.
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

/// Precise roots registered with the heap.
#[derive(Clone, Debug, Default)]
pub struct RootSet {
    records: Vec<RootRecord>,
}

impl RootSet {
    pub fn records(&self) -> &[RootRecord] {
        &self.records
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
#[derive(Clone, Debug, Default)]
pub struct WeakRegistry {
    generation: u64,
    sets: Vec<WeakSetDescriptor>,
}

impl WeakRegistry {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn sets(&self) -> &[WeakSetDescriptor] {
        &self.sets
    }

    pub fn processing_phase(&self) -> Option<WeakProcessingPhase> {
        self.sets.iter().find_map(|set| set.active_phase)
    }
}

/// Finalizer callback identity. The callback body is a host/VM integration point.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct FinalizerId(pub u64);

/// Finalization hooks run after the collector has decided an object is dead.
#[derive(Clone, Debug, Default)]
pub struct FinalizationRegistry {
    generation: u64,
    finalizers: Vec<FinalizerRecord>,
    callbacks: Vec<HeapFinalizerCallback>,
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
}
