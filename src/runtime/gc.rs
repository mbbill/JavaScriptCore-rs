//! Runtime-facing GC integration records.
//!
//! These records connect runtime semantic operations to the existing `gc`
//! authority APIs. They do not allocate storage, run finalizers, clear weak
//! slots, or mutate public JavaScript semantics.

use crate::gc::{
    evaluate_heap_semantics, CellId, CellMetadata, CellType, FinalizerId, FinalizerKind,
    FinalizerPlan, FinalizerPlanningError, FinalizerPlanningRecord, GcPhase, HeapFinalizerCallback,
    HeapFinalizerCallbackId, HeapId, HeapMutationAuthority, HeapSemanticError,
    HeapSemanticOperation, HeapStateDescriptor, NoGcScopeContract, RootId, RootKind, RootRecord,
    RootSetSemanticError, StructureId, TargetedRootRecord, TargetedRootSet,
    WeakHandleOwnerContract, WeakHandleOwnerId, WeakId, WeakProcessingPhase, WeakSlotState,
    WeakSlotTransitionOutcome, WeakSlotTransitionRequest, WeakStateTransitionError,
};

use crate::runtime::state::ObjectId;

/// Runtime allocation family selected before entering a heap allocation path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeAllocationKind {
    Object,
    GlobalObject,
    Function,
    Scope,
    ModuleNamespace,
    ModuleRecord,
    String,
    Symbol,
    Executable,
    CodeBlock,
}

/// Allocation request tied to a structure, cell type, heap, and GC permission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeAllocationRequest {
    pub heap: HeapId,
    pub structure: StructureId,
    pub cell_type: CellType,
    pub metadata: CellMetadata,
    pub kind: RuntimeAllocationKind,
    pub may_trigger_collection: bool,
}

impl RuntimeAllocationRequest {
    pub const fn new(
        heap: HeapId,
        structure: StructureId,
        cell_type: CellType,
        metadata: CellMetadata,
        kind: RuntimeAllocationKind,
    ) -> Self {
        Self {
            heap,
            structure,
            cell_type,
            metadata,
            kind,
            may_trigger_collection: true,
        }
    }

    pub const fn object(heap: HeapId, structure: StructureId, metadata: CellMetadata) -> Self {
        Self::new(
            heap,
            structure,
            CellType::Object,
            metadata,
            RuntimeAllocationKind::Object,
        )
    }

    pub const fn no_gc(mut self) -> Self {
        self.may_trigger_collection = false;
        self
    }

    pub fn evaluate(
        self,
        state: HeapStateDescriptor,
    ) -> Result<RuntimeAllocationGrant, HeapSemanticError> {
        let grant = evaluate_heap_semantics(
            state,
            HeapMutationAuthority::Mutator,
            HeapSemanticOperation::Allocate {
                may_trigger_collection: self.may_trigger_collection,
            },
        )?;
        Ok(RuntimeAllocationGrant {
            request: self,
            phase: grant.phase,
            requires_world_suspension: grant.requires_world_suspension,
        })
    }
}

/// Successful semantic grant for a runtime allocation request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeAllocationGrant {
    pub request: RuntimeAllocationRequest,
    pub phase: GcPhase,
    pub requires_world_suspension: bool,
}

/// Runtime roots retained outside object fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeRootKind {
    Realm,
    GlobalObject,
    GlobalThis,
    Intrinsic,
    RuntimeCache,
    ModuleRegistry,
    ModuleRecord,
    ValueStack,
    HostSlot,
}

/// A runtime root plus the target cell identity it keeps discoverable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeRootRecord {
    pub kind: RuntimeRootKind,
    pub root: RootRecord,
    pub target: CellId,
}

impl RuntimeRootRecord {
    pub const fn explicit(
        kind: RuntimeRootKind,
        heap: HeapId,
        root: RootId,
        target: CellId,
    ) -> Self {
        Self {
            kind,
            root: RootRecord {
                id: root,
                kind: RootKind::ExplicitRoot,
                heap,
            },
            target,
        }
    }

    pub const fn targeted_root(self) -> TargetedRootRecord {
        TargetedRootRecord {
            root: self.root,
            target: self.target,
        }
    }
}

impl From<RuntimeRootRecord> for TargetedRootRecord {
    fn from(record: RuntimeRootRecord) -> Self {
        record.targeted_root()
    }
}

/// Root plan for runtime-owned entries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeRootPlan {
    pub heap: HeapId,
    pub roots: Vec<RuntimeRootRecord>,
}

impl RuntimeRootPlan {
    pub fn from_records(
        heap: HeapId,
        roots: Vec<RuntimeRootRecord>,
    ) -> Result<Self, RootSetSemanticError> {
        TargetedRootSet::from_records(
            heap,
            roots
                .iter()
                .copied()
                .map(TargetedRootRecord::from)
                .collect(),
        )?;
        Ok(Self { heap, roots })
    }
}

/// Runtime slot family that may hold a weak edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeWeakSlotKind {
    CachedPrototype,
    StructureWatchpoint,
    ModuleRegistryEntry,
    HostObject,
    FinalizationTarget,
}

/// Weak reference metadata retained by a runtime slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeWeakSlot {
    pub id: WeakId,
    pub kind: RuntimeWeakSlotKind,
    pub target: Option<CellId>,
    pub state: WeakSlotState,
    pub owner: Option<WeakHandleOwnerId>,
}

impl RuntimeWeakSlot {
    pub const fn new(id: WeakId, kind: RuntimeWeakSlotKind, target: CellId) -> Self {
        Self {
            id,
            kind,
            target: Some(target),
            state: WeakSlotState::Live,
            owner: None,
        }
    }

    pub const fn owner(mut self, owner: WeakHandleOwnerId) -> Self {
        self.owner = Some(owner);
        self
    }

    pub fn transition(
        self,
        phase: WeakProcessingPhase,
        target_is_live: bool,
        owner_contract: Option<WeakHandleOwnerContract>,
    ) -> Result<RuntimeWeakSlotTransition, WeakStateTransitionError> {
        let mut request = WeakSlotTransitionRequest::new(
            self.state,
            phase,
            crate::gc::WeakEdgeKind::Ordinary,
            target_is_live,
        );
        if let Some(owner) = owner_contract {
            request = request.owner(owner);
        }
        let outcome = WeakSlotState::transition(request)?;
        Ok(RuntimeWeakSlotTransition {
            slot: self,
            outcome,
            target_after_transition: if outcome.clears_target {
                None
            } else {
                self.target
            },
        })
    }
}

/// Planned weak-slot state change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeWeakSlotTransition {
    pub slot: RuntimeWeakSlot,
    pub outcome: WeakSlotTransitionOutcome,
    pub target_after_transition: Option<CellId>,
}

/// Runtime object finalization hook.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeObjectFinalizationHook {
    pub id: FinalizerId,
    pub object: ObjectId,
    pub callback: HeapFinalizerCallbackId,
    pub owner: Option<WeakHandleOwnerId>,
    pub kind: FinalizerKind,
}

impl RuntimeObjectFinalizationHook {
    pub const fn cell_destructor(
        id: FinalizerId,
        object: ObjectId,
        callback: HeapFinalizerCallbackId,
    ) -> Self {
        Self {
            id,
            object,
            callback,
            owner: None,
            kind: FinalizerKind::CellDestructor,
        }
    }

    pub const fn planning_record(self) -> FinalizerPlanningRecord {
        FinalizerPlanningRecord {
            id: self.id,
            callback: self.callback,
            target: self.object.0,
            owner: self.owner,
            kind: self.kind,
        }
    }
}

pub fn plan_runtime_finalizers(
    callbacks: &[HeapFinalizerCallback],
    hooks: &[RuntimeObjectFinalizationHook],
) -> Result<FinalizerPlan, FinalizerPlanningError> {
    let records = hooks
        .iter()
        .map(|hook| hook.planning_record())
        .collect::<Vec<_>>();
    FinalizerPlan::from_records(callbacks, &records)
}

/// Runtime semantic operation whose GC boundary must be explicit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeSemanticOperation {
    AllocateObject,
    MutateObjectProperty,
    ResolveModuleRecord,
    EvaluateModuleRecord,
    SnapshotValueStack,
    VisitRuntimeRoots,
    ProcessRuntimeWeakSlots,
    RunRuntimeFinalizers,
}

/// No-GC requirement attached to a runtime semantic operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeNoGcRequirement {
    pub operation: RuntimeSemanticOperation,
    pub heap_operation: HeapSemanticOperation,
    pub requires_no_gc_scope: bool,
}

impl RuntimeNoGcRequirement {
    pub const fn for_operation(operation: RuntimeSemanticOperation) -> Self {
        let heap_operation = match operation {
            RuntimeSemanticOperation::AllocateObject => HeapSemanticOperation::Allocate {
                may_trigger_collection: true,
            },
            RuntimeSemanticOperation::MutateObjectProperty => {
                HeapSemanticOperation::MutatePublishedCell
            }
            RuntimeSemanticOperation::VisitRuntimeRoots => HeapSemanticOperation::TraceRoots,
            RuntimeSemanticOperation::ProcessRuntimeWeakSlots => HeapSemanticOperation::ProcessWeak,
            RuntimeSemanticOperation::RunRuntimeFinalizers => HeapSemanticOperation::RunFinalizers,
            RuntimeSemanticOperation::ResolveModuleRecord
            | RuntimeSemanticOperation::EvaluateModuleRecord
            | RuntimeSemanticOperation::SnapshotValueStack => HeapSemanticOperation::Observe,
        };
        let requires_no_gc_scope = matches!(
            operation,
            RuntimeSemanticOperation::ResolveModuleRecord
                | RuntimeSemanticOperation::SnapshotValueStack
        );
        Self {
            operation,
            heap_operation,
            requires_no_gc_scope,
        }
    }

    pub fn validate_contract(
        self,
        contract: Option<NoGcScopeContract>,
    ) -> Result<(), RuntimeNoGcRequirementError> {
        if self.requires_no_gc_scope && contract.is_none() {
            return Err(RuntimeNoGcRequirementError::MissingNoGcScope(
                self.operation,
            ));
        }
        if let Some(contract) = contract {
            if !contract.allows(self.heap_operation) {
                return Err(RuntimeNoGcRequirementError::OperationNotAllowed {
                    operation: self.operation,
                    heap_operation: self.heap_operation,
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeNoGcRequirementError {
    MissingNoGcScope(RuntimeSemanticOperation),
    OperationNotAllowed {
        operation: RuntimeSemanticOperation,
        heap_operation: HeapSemanticOperation,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{CollectionKind, CollectionRequest, CollectionTriggerKind, Synchronousness};

    #[test]
    fn runtime_allocation_request_rejects_collecting_allocation_in_no_gc_scope() {
        let request = RuntimeAllocationRequest::object(
            HeapId(1),
            StructureId::new(7),
            CellMetadata::default(),
        );
        let state = HeapStateDescriptor::default().in_no_gc_scope(1);

        assert_eq!(
            request.evaluate(state),
            Err(HeapSemanticError::NoGcScopeActive(
                HeapSemanticOperation::Allocate {
                    may_trigger_collection: true
                }
            ))
        );
    }

    #[test]
    fn runtime_allocation_request_accepts_no_gc_allocation_descriptor() {
        let request = RuntimeAllocationRequest::object(
            HeapId(1),
            StructureId::new(7),
            CellMetadata::default(),
        )
        .no_gc();

        assert_eq!(
            request
                .evaluate(HeapStateDescriptor::default())
                .map(|grant| grant.request.may_trigger_collection),
            Ok(false)
        );
    }

    #[test]
    fn runtime_roots_validate_through_targeted_root_set() {
        let roots = vec![RuntimeRootRecord::explicit(
            RuntimeRootKind::GlobalObject,
            HeapId(3),
            RootId(11),
            CellId(9),
        )];

        assert_eq!(
            RuntimeRootPlan::from_records(HeapId(3), roots).map(|plan| plan.roots[0].kind),
            Ok(RuntimeRootKind::GlobalObject)
        );
    }

    #[test]
    fn runtime_roots_reject_default_target_through_targeted_root_set() {
        let roots = vec![RuntimeRootRecord::explicit(
            RuntimeRootKind::GlobalObject,
            HeapId(3),
            RootId(11),
            CellId::default(),
        )];

        assert_eq!(
            RuntimeRootPlan::from_records(HeapId(3), roots),
            Err(RootSetSemanticError::InvalidRootTarget {
                root: RootId(11),
                target: CellId::default()
            })
        );
    }

    #[test]
    fn runtime_weak_slot_clear_transition_drops_target() {
        let slot = RuntimeWeakSlot {
            id: WeakId(4),
            kind: RuntimeWeakSlotKind::HostObject,
            target: Some(CellId(12)),
            state: WeakSlotState::ClearPending,
            owner: None,
        };

        assert_eq!(
            slot.transition(WeakProcessingPhase::Clear, false, None)
                .map(|transition| transition.target_after_transition),
            Ok(None)
        );
    }

    #[test]
    fn runtime_finalizer_plan_uses_registered_callbacks() {
        let callbacks = [HeapFinalizerCallback {
            id: HeapFinalizerCallbackId(5),
            kind: FinalizerKind::CellDestructor,
            user_data_tag: 0,
        }];
        let hooks = [RuntimeObjectFinalizationHook::cell_destructor(
            FinalizerId(6),
            ObjectId(CellId(7)),
            HeapFinalizerCallbackId(5),
        )];

        assert_eq!(
            plan_runtime_finalizers(&callbacks, &hooks).map(|plan| plan.entries.len()),
            Ok(1)
        );
    }

    #[test]
    fn no_gc_requirement_rejects_missing_scope_for_stack_snapshot() {
        let requirement =
            RuntimeNoGcRequirement::for_operation(RuntimeSemanticOperation::SnapshotValueStack);

        assert_eq!(
            requirement.validate_contract(None),
            Err(RuntimeNoGcRequirementError::MissingNoGcScope(
                RuntimeSemanticOperation::SnapshotValueStack
            ))
        );
    }

    #[test]
    fn collection_request_subsumption_still_comes_from_gc_contracts() {
        let full = CollectionRequest {
            kind: CollectionKind::Full,
            synchronousness: Synchronousness::Sync,
            trigger: CollectionTriggerKind::API,
            requested_bytes: 0,
            did_finish_end_phase: None,
        };
        assert!(full.subsumes(CollectionRequest::default()));
    }
}
