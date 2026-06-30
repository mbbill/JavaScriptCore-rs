//! Owner-aware barrier slots.
//!
//! Writes from a GC-owned object to another GC thing must carry owner context.
//! The barrier algorithm is deferred; these APIs reserve the mutation boundary.

use crate::gc::{CellState, GcRef};

/// Barrier family selected by the collector and field kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BarrierKind {
    Initialization,
    Store,
    StoreCellValue,
    StoreStructureId,
    AuxiliaryOwner,
    RememberedSet,
    MutatorFence,
}

/// Static owner for barrier schema rows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BarrierSchemaOwner {
    /// The GC barrier module owns the schema shape and static rows.
    #[default]
    GcBarrierSchema,
    /// A future generated table owns rows derived from object field metadata.
    GeneratedFieldTable,
}

/// Registry mutation authority for barrier schema data.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BarrierRegistryAuthority {
    /// Barrier schema rows are compiled static data.
    #[default]
    StaticReadOnly,
    /// A generated field metadata refresh may replace the compiled table.
    GeneratedSourceRefresh,
}

/// Category of field protected by a barrier.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BarrierFieldKind {
    /// Field stores a GC cell reference.
    #[default]
    CellReference,
    /// Field stores a JavaScript value whose bits may encode a cell.
    Value,
    /// Field stores structure metadata.
    StructureId,
    /// Field stores auxiliary owner linkage.
    AuxiliaryOwner,
}

/// JIT/CPP write-barrier use site classification.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WriteBarrierUseKind {
    PropertyAccess,
    VariableAccess,
    #[default]
    GenericAccess,
}

/// Profiling counters are diagnostic authority, not barrier semantics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WriteBarrierCounterSet {
    pub uses_with_barrier_from_cpp: usize,
    pub uses_without_barrier_from_cpp: usize,
    pub uses_with_barrier_from_jit: usize,
    pub uses_for_properties_from_jit: usize,
    pub uses_for_variables_from_jit: usize,
    pub uses_without_barrier_from_jit: usize,
}

/// Threshold that determines when a write needs a slow barrier.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BarrierThreshold {
    #[default]
    None,
    PossiblyGrey,
    PossiblyBlack,
}

/// Static barrier row for a named field family.
///
/// The row describes which barrier a write site must plan for. It does not
/// perform a write, enqueue work, inspect mark bits, or touch remembered sets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BarrierSchemaDescriptor {
    pub name: &'static str,
    pub field_kind: BarrierFieldKind,
    pub barrier_kind: BarrierKind,
    pub threshold: BarrierThreshold,
    pub use_kind: WriteBarrierUseKind,
    pub owner: BarrierSchemaOwner,
}

/// Immutable registry of barrier schemas.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BarrierSchemaRegistry {
    pub name: &'static str,
    pub authority: BarrierRegistryAuthority,
    pub schemas: &'static [BarrierSchemaDescriptor],
}

impl BarrierSchemaRegistry {
    pub const fn schemas(&self) -> &'static [BarrierSchemaDescriptor] {
        self.schemas
    }

    pub fn schema_for_kind(&self, kind: BarrierKind) -> Option<&'static BarrierSchemaDescriptor> {
        self.schemas
            .iter()
            .find(|descriptor| descriptor.barrier_kind == kind)
    }

    pub fn plan_write(
        &self,
        context: BarrierWriteContext,
    ) -> Result<BarrierDecision, BarrierDecisionError> {
        self.validate()
            .map_err(BarrierDecisionError::InvalidSchema)?;
        let barrier_kind = context.preferred_barrier_kind();
        let schema = self
            .schema_for_kind(barrier_kind)
            .ok_or(BarrierDecisionError::MissingSchema(barrier_kind))?;
        if schema.field_kind != context.field_kind
            && schema.barrier_kind != BarrierKind::MutatorFence
        {
            return Err(BarrierDecisionError::FieldKindMismatch {
                expected: context.field_kind,
                actual: schema.field_kind,
            });
        }

        let action = if context.target_state.is_none()
            || matches!(
                schema.barrier_kind,
                BarrierKind::Initialization | BarrierKind::StoreStructureId
            ) {
            BarrierAction::NoBarrier
        } else if context.force_mutator_fence {
            BarrierAction::MutatorFence
        } else if context.needs_remembered_set {
            BarrierAction::RememberedSet
        } else if threshold_requires_barrier(
            schema.threshold,
            context.owner_state,
            context.target_state,
        ) {
            BarrierAction::MarkingBarrier
        } else {
            BarrierAction::NoBarrier
        };

        Ok(BarrierDecision {
            schema,
            action,
            threshold: schema.threshold,
        })
    }

    pub fn evaluate_requirement(
        &self,
        request: BarrierRequirementRequest,
    ) -> Result<BarrierRequirementOutcome, BarrierDecisionError> {
        if request.context.initializing
            && (request.owner_is_published
                || request.authority != BarrierMutationAuthority::UnpublishedCellInitialization)
        {
            return Err(BarrierDecisionError::InvalidMutationAuthority {
                context: request.context,
                authority: request.authority,
            });
        }

        if !request.context.initializing
            && request.authority == BarrierMutationAuthority::UnpublishedCellInitialization
        {
            return Err(BarrierDecisionError::InvalidMutationAuthority {
                context: request.context,
                authority: request.authority,
            });
        }

        let decision = self.plan_write(request.context)?;
        Ok(decision.requirement_outcome(request.context))
    }

    pub fn validate(&self) -> Result<(), BarrierSchemaValidationError> {
        if self.name.is_empty() {
            return Err(BarrierSchemaValidationError::EmptyRegistryName);
        }

        for (index, descriptor) in self.schemas.iter().enumerate() {
            descriptor.validate()?;
            if self.schemas[..index]
                .iter()
                .any(|previous| previous.name == descriptor.name)
            {
                return Err(BarrierSchemaValidationError::DuplicateSchemaName(
                    descriptor.name,
                ));
            }
            if self.schemas[..index]
                .iter()
                .any(|previous| previous.barrier_kind == descriptor.barrier_kind)
            {
                return Err(BarrierSchemaValidationError::DuplicateBarrierKind(
                    descriptor.barrier_kind,
                ));
            }
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BarrierSchemaValidationError {
    EmptyRegistryName,
    EmptySchemaName,
    DuplicateSchemaName(&'static str),
    DuplicateBarrierKind(BarrierKind),
    FieldKindMismatch {
        name: &'static str,
        field_kind: BarrierFieldKind,
        barrier_kind: BarrierKind,
    },
    ThresholdMismatch {
        name: &'static str,
        barrier_kind: BarrierKind,
        threshold: BarrierThreshold,
    },
}

impl BarrierSchemaDescriptor {
    pub const fn new(
        name: &'static str,
        field_kind: BarrierFieldKind,
        barrier_kind: BarrierKind,
    ) -> Self {
        Self {
            name,
            field_kind,
            barrier_kind,
            threshold: BarrierThreshold::None,
            use_kind: WriteBarrierUseKind::GenericAccess,
            owner: BarrierSchemaOwner::GcBarrierSchema,
        }
    }

    pub fn validate(&self) -> Result<(), BarrierSchemaValidationError> {
        if self.name.is_empty() {
            return Err(BarrierSchemaValidationError::EmptySchemaName);
        }

        let matches_field = matches!(
            (self.field_kind, self.barrier_kind),
            (BarrierFieldKind::CellReference, BarrierKind::Initialization)
                | (BarrierFieldKind::CellReference, BarrierKind::Store)
                | (BarrierFieldKind::Value, BarrierKind::StoreCellValue)
                | (BarrierFieldKind::StructureId, BarrierKind::StoreStructureId)
                | (
                    BarrierFieldKind::AuxiliaryOwner,
                    BarrierKind::AuxiliaryOwner
                )
                | (BarrierFieldKind::CellReference, BarrierKind::RememberedSet)
                | (_, BarrierKind::MutatorFence)
        );
        if !matches_field {
            return Err(BarrierSchemaValidationError::FieldKindMismatch {
                name: self.name,
                field_kind: self.field_kind,
                barrier_kind: self.barrier_kind,
            });
        }

        let threshold_matches = match self.barrier_kind {
            BarrierKind::Initialization
            | BarrierKind::StoreStructureId
            | BarrierKind::AuxiliaryOwner
            | BarrierKind::MutatorFence => self.threshold == BarrierThreshold::None,
            BarrierKind::Store | BarrierKind::StoreCellValue => {
                self.threshold == BarrierThreshold::PossiblyGrey
            }
            BarrierKind::RememberedSet => self.threshold == BarrierThreshold::PossiblyBlack,
        };
        if !threshold_matches {
            return Err(BarrierSchemaValidationError::ThresholdMismatch {
                name: self.name,
                barrier_kind: self.barrier_kind,
                threshold: self.threshold,
            });
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BarrierSchemaDescriptorBuilder {
    descriptor: BarrierSchemaDescriptor,
}

impl BarrierSchemaDescriptorBuilder {
    pub const fn new(
        name: &'static str,
        field_kind: BarrierFieldKind,
        barrier_kind: BarrierKind,
    ) -> Self {
        Self {
            descriptor: BarrierSchemaDescriptor::new(name, field_kind, barrier_kind),
        }
    }

    pub const fn threshold(mut self, threshold: BarrierThreshold) -> Self {
        self.descriptor.threshold = threshold;
        self
    }

    pub const fn use_kind(mut self, use_kind: WriteBarrierUseKind) -> Self {
        self.descriptor.use_kind = use_kind;
        self
    }

    pub const fn owner(mut self, owner: BarrierSchemaOwner) -> Self {
        self.descriptor.owner = owner;
        self
    }

    pub fn build(self) -> Result<BarrierSchemaDescriptor, BarrierSchemaValidationError> {
        self.descriptor.validate()?;
        Ok(self.descriptor)
    }
}

/// Canonical barrier schemas owned by `gc::barrier`.
pub const STATIC_BARRIER_SCHEMAS: &[BarrierSchemaDescriptor] = &[
    BarrierSchemaDescriptor {
        name: "initialization",
        field_kind: BarrierFieldKind::CellReference,
        barrier_kind: BarrierKind::Initialization,
        threshold: BarrierThreshold::None,
        use_kind: WriteBarrierUseKind::GenericAccess,
        owner: BarrierSchemaOwner::GcBarrierSchema,
    },
    BarrierSchemaDescriptor {
        name: "cell-store",
        field_kind: BarrierFieldKind::CellReference,
        barrier_kind: BarrierKind::Store,
        threshold: BarrierThreshold::PossiblyGrey,
        use_kind: WriteBarrierUseKind::GenericAccess,
        owner: BarrierSchemaOwner::GcBarrierSchema,
    },
    BarrierSchemaDescriptor {
        name: "value-store",
        field_kind: BarrierFieldKind::Value,
        barrier_kind: BarrierKind::StoreCellValue,
        threshold: BarrierThreshold::PossiblyGrey,
        use_kind: WriteBarrierUseKind::GenericAccess,
        owner: BarrierSchemaOwner::GcBarrierSchema,
    },
    BarrierSchemaDescriptor {
        name: "structure-id-store",
        field_kind: BarrierFieldKind::StructureId,
        barrier_kind: BarrierKind::StoreStructureId,
        threshold: BarrierThreshold::None,
        use_kind: WriteBarrierUseKind::PropertyAccess,
        owner: BarrierSchemaOwner::GcBarrierSchema,
    },
    BarrierSchemaDescriptor {
        name: "remembered-set-owner",
        field_kind: BarrierFieldKind::CellReference,
        barrier_kind: BarrierKind::RememberedSet,
        threshold: BarrierThreshold::PossiblyBlack,
        use_kind: WriteBarrierUseKind::GenericAccess,
        owner: BarrierSchemaOwner::GcBarrierSchema,
    },
    BarrierSchemaDescriptor {
        name: "auxiliary-owner",
        field_kind: BarrierFieldKind::AuxiliaryOwner,
        barrier_kind: BarrierKind::AuxiliaryOwner,
        threshold: BarrierThreshold::None,
        use_kind: WriteBarrierUseKind::GenericAccess,
        owner: BarrierSchemaOwner::GcBarrierSchema,
    },
    BarrierSchemaDescriptor {
        name: "mutator-fence",
        field_kind: BarrierFieldKind::CellReference,
        barrier_kind: BarrierKind::MutatorFence,
        threshold: BarrierThreshold::None,
        use_kind: WriteBarrierUseKind::GenericAccess,
        owner: BarrierSchemaOwner::GcBarrierSchema,
    },
];

pub const STATIC_BARRIER_SCHEMA_REGISTRY: BarrierSchemaRegistry = BarrierSchemaRegistry {
    name: "gc.barrier.static-schema",
    authority: BarrierRegistryAuthority::StaticReadOnly,
    schemas: STATIC_BARRIER_SCHEMAS,
};

pub const fn static_barrier_schemas() -> &'static [BarrierSchemaDescriptor] {
    STATIC_BARRIER_SCHEMAS
}

pub const fn static_barrier_schema_registry() -> &'static BarrierSchemaRegistry {
    &STATIC_BARRIER_SCHEMA_REGISTRY
}

/// Owner edge recorded by a barrier. This deliberately avoids exposing card
/// tables, remembered sets, or snapshot algorithms.
///
/// The edge borrows both endpoints. The owning cell authorizes the field
/// mutation; the target cell remains owned by the heap.
#[derive(Clone, Copy, Debug)]
pub struct BarrierEdge<O: ?Sized, T: ?Sized> {
    pub owner: GcRef<O>,
    pub target: Option<GcRef<T>>,
    pub kind: BarrierKind,
}

/// Remembered-set entry reserved by an inter-generational or incremental write.
///
/// This is collector bookkeeping about an owner, not ownership of the owner.
#[derive(Clone, Copy, Debug)]
pub struct RememberedSetEntry<O: ?Sized> {
    pub owner: GcRef<O>,
    pub kind: BarrierKind,
}

/// Descriptor for the barrier work a write would enqueue.
#[derive(Clone, Copy, Debug)]
pub struct WriteBarrierPlan<O: ?Sized, T: ?Sized> {
    pub edge: BarrierEdge<O, T>,
    pub threshold: BarrierThreshold,
    pub remembered_set: Option<RememberedSetEntry<O>>,
    pub use_kind: WriteBarrierUseKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BarrierWriteContext {
    pub field_kind: BarrierFieldKind,
    pub owner_state: CellState,
    pub target_state: Option<CellState>,
    pub initializing: bool,
    pub needs_remembered_set: bool,
    pub force_mutator_fence: bool,
}

impl BarrierWriteContext {
    pub const fn store(
        field_kind: BarrierFieldKind,
        owner_state: CellState,
        target_state: Option<CellState>,
    ) -> Self {
        Self {
            field_kind,
            owner_state,
            target_state,
            initializing: false,
            needs_remembered_set: false,
            force_mutator_fence: false,
        }
    }

    pub const fn initializing(
        field_kind: BarrierFieldKind,
        owner_state: CellState,
        target_state: Option<CellState>,
    ) -> Self {
        Self {
            field_kind,
            owner_state,
            target_state,
            initializing: true,
            needs_remembered_set: false,
            force_mutator_fence: false,
        }
    }

    pub const fn remembered_set(mut self, needs_remembered_set: bool) -> Self {
        self.needs_remembered_set = needs_remembered_set;
        self
    }

    pub const fn mutator_fence(mut self, force_mutator_fence: bool) -> Self {
        self.force_mutator_fence = force_mutator_fence;
        self
    }

    fn preferred_barrier_kind(self) -> BarrierKind {
        if self.force_mutator_fence {
            return BarrierKind::MutatorFence;
        }
        if self.needs_remembered_set {
            return BarrierKind::RememberedSet;
        }
        if self.initializing && self.field_kind == BarrierFieldKind::CellReference {
            return BarrierKind::Initialization;
        }
        match self.field_kind {
            BarrierFieldKind::CellReference => BarrierKind::Store,
            BarrierFieldKind::Value => BarrierKind::StoreCellValue,
            BarrierFieldKind::StructureId => BarrierKind::StoreStructureId,
            BarrierFieldKind::AuxiliaryOwner => BarrierKind::AuxiliaryOwner,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BarrierMutationAuthority {
    /// Initialization of an unpublished cell may use the initialization path.
    UnpublishedCellInitialization,
    /// Mutator-owned field mutation of a published cell.
    #[default]
    MutatorFieldWrite,
    /// Collector-side metadata rewrite that still must preserve visibility.
    CollectorMetadataRewrite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BarrierRequirementRequest {
    pub context: BarrierWriteContext,
    pub authority: BarrierMutationAuthority,
    pub owner_is_published: bool,
}

impl BarrierRequirementRequest {
    pub const fn new(context: BarrierWriteContext) -> Self {
        Self {
            context,
            authority: BarrierMutationAuthority::MutatorFieldWrite,
            owner_is_published: true,
        }
    }

    pub const fn authority(mut self, authority: BarrierMutationAuthority) -> Self {
        self.authority = authority;
        self
    }

    pub const fn owner_is_published(mut self, owner_is_published: bool) -> Self {
        self.owner_is_published = owner_is_published;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BarrierAction {
    NoBarrier,
    MarkingBarrier,
    RememberedSet,
    MutatorFence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BarrierNotRequiredReason {
    NullOrNonCellTarget,
    UnpublishedInitialization,
    StructureOrAuxiliaryMetadata,
    TargetAlreadyVisible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BarrierRequirementOutcome {
    NotRequired(BarrierNotRequiredReason),
    Required(BarrierAction),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BarrierDecision {
    pub schema: &'static BarrierSchemaDescriptor,
    pub action: BarrierAction,
    pub threshold: BarrierThreshold,
}

impl BarrierDecision {
    pub fn requirement_outcome(self, context: BarrierWriteContext) -> BarrierRequirementOutcome {
        match self.action {
            BarrierAction::NoBarrier if context.target_state.is_none() => {
                BarrierRequirementOutcome::NotRequired(
                    BarrierNotRequiredReason::NullOrNonCellTarget,
                )
            }
            BarrierAction::NoBarrier if context.initializing => {
                BarrierRequirementOutcome::NotRequired(
                    BarrierNotRequiredReason::UnpublishedInitialization,
                )
            }
            BarrierAction::NoBarrier
                if matches!(
                    context.field_kind,
                    BarrierFieldKind::StructureId | BarrierFieldKind::AuxiliaryOwner
                ) =>
            {
                BarrierRequirementOutcome::NotRequired(
                    BarrierNotRequiredReason::StructureOrAuxiliaryMetadata,
                )
            }
            BarrierAction::NoBarrier => BarrierRequirementOutcome::NotRequired(
                BarrierNotRequiredReason::TargetAlreadyVisible,
            ),
            action => BarrierRequirementOutcome::Required(action),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BarrierDecisionError {
    InvalidSchema(BarrierSchemaValidationError),
    MissingSchema(BarrierKind),
    FieldKindMismatch {
        expected: BarrierFieldKind,
        actual: BarrierFieldKind,
    },
    InvalidMutationAuthority {
        context: BarrierWriteContext,
        authority: BarrierMutationAuthority,
    },
}

fn threshold_requires_barrier(
    threshold: BarrierThreshold,
    owner_state: CellState,
    target_state: Option<CellState>,
) -> bool {
    match (threshold, owner_state, target_state) {
        (BarrierThreshold::None, _, _) | (_, _, None) => false,
        (
            BarrierThreshold::PossiblyGrey,
            CellState::PossiblyBlack,
            Some(CellState::PossiblyGrey),
        )
        | (
            BarrierThreshold::PossiblyGrey,
            CellState::PossiblyBlack,
            Some(CellState::DefinitelyWhite),
        ) => true,
        (BarrierThreshold::PossiblyBlack, CellState::PossiblyBlack, Some(state)) => {
            state != CellState::PossiblyBlack
        }
        _ => false,
    }
}

/// Barriered reference field inside a GC-owned object.
///
/// The field owns the stored edge value. It does not own the target cell, and
/// post-publication mutation requires the caller to pass the owning cell.
#[derive(Debug, Default)]
pub struct WriteBarrier<T: ?Sized> {
    slot: Option<GcRef<T>>,
    initialized: bool,
}

impl<T: ?Sized> WriteBarrier<T> {
    pub fn empty() -> Self {
        Self {
            slot: None,
            initialized: false,
        }
    }

    pub fn get(&self) -> Option<GcRef<T>> {
        self.slot
    }

    pub fn initialize_without_barrier(&mut self, value: Option<GcRef<T>>) {
        // Initialization-only path for unpublished cells. Callers must not use
        // this after the owning cell can be observed by the mutator or GC.
        self.slot = value;
        self.initialized = true;
    }

    pub fn set<O: ?Sized>(
        &mut self,
        owner: GcRef<O>,
        value: Option<GcRef<T>>,
    ) -> BarrierEdge<O, T> {
        // Future implementation performs the selected write barrier here.
        self.slot = value;
        self.initialized = true;
        BarrierEdge {
            owner,
            target: value,
            kind: BarrierKind::Store,
        }
    }

    pub fn set_with_plan<O: ?Sized>(
        &mut self,
        owner: GcRef<O>,
        value: Option<GcRef<T>>,
        threshold: BarrierThreshold,
    ) -> WriteBarrierPlan<O, T> {
        let edge = self.set(owner, value);
        WriteBarrierPlan {
            edge,
            threshold,
            remembered_set: Some(RememberedSetEntry {
                owner,
                kind: BarrierKind::RememberedSet,
            }),
            use_kind: WriteBarrierUseKind::GenericAccess,
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

/// Barriered JavaScript value field.
///
/// Kept generic so `gc` remains independent from the concrete `JsValue`
/// module while still naming the mutation boundary.
/// The field owns value bits. If those bits encode a cell, liveness and write
/// visibility still belong to heap/barrier authority.
#[derive(Clone, Copy, Debug)]
pub struct ValueBarrier<V> {
    value: V,
    initialized: bool,
}

impl<V: Copy> ValueBarrier<V> {
    pub fn new_initial(value: V) -> Self {
        Self {
            value,
            initialized: true,
        }
    }

    pub fn get(&self) -> V {
        self.value
    }

    pub fn initialize_without_barrier(&mut self, value: V) {
        // Initialization-only path for unpublished cells.
        self.value = value;
        self.initialized = true;
    }

    pub fn set<O: ?Sized>(&mut self, _owner: GcRef<O>, value: V) -> BarrierKind {
        // Future implementation inspects cell-containing values and records the
        // owner-to-child edge required by the collector.
        self.value = value;
        self.initialized = true;
        BarrierKind::StoreCellValue
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

/// gc-r4 Batch 5 Step 2 — the object generational barrier for a butterfly
/// REALLOCATION. C++ `JSObject::setButterfly` (after `createOrGrowPropertyStorage`
/// reallocates the butterfly) runs `vm.heap.writeBarrier(this)`: the object now
/// points at a NEW butterfly, so a generational/incremental collector must remember
/// the object and rescan its out-of-line storage. This reserves that mutation
/// boundary at every butterfly-pointer (cell+8) rewrite site (`sync_butterfly_base`).
///
/// NO-OP today: the live collector is not wired (`force_collect` re-marks the whole
/// live closure from roots each cycle, so there is no remembered set to update), and
/// `apply_value_store_write_barrier` likewise classifies a white owner as NotRequired
/// while not fenced. The faithful remembered-set update lands with the live
/// incremental collector (gc-r4 follow-up); this marker keeps the barrier intent at
/// the code site so the rewrite is not silently un-barriered when that lands.
#[inline]
pub fn butterfly_reallocation_barrier() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_barrier_schema_registry_is_read_only() {
        assert_eq!(
            static_barrier_schema_registry().authority,
            BarrierRegistryAuthority::StaticReadOnly
        );
        assert_eq!(static_barrier_schema_registry().validate(), Ok(()));
    }

    #[test]
    fn barrier_builder_constructs_store_schema() {
        let schema = BarrierSchemaDescriptorBuilder::new(
            "store",
            BarrierFieldKind::CellReference,
            BarrierKind::Store,
        )
        .threshold(BarrierThreshold::PossiblyGrey)
        .build();

        assert_eq!(
            schema.map(|schema| schema.barrier_kind),
            Ok(BarrierKind::Store)
        );
    }

    #[test]
    fn barrier_validator_rejects_structure_field_for_cell_store() {
        let schema = BarrierSchemaDescriptorBuilder::new(
            "bad",
            BarrierFieldKind::StructureId,
            BarrierKind::Store,
        )
        .threshold(BarrierThreshold::PossiblyGrey)
        .build();

        assert_eq!(
            schema,
            Err(BarrierSchemaValidationError::FieldKindMismatch {
                name: "bad",
                field_kind: BarrierFieldKind::StructureId,
                barrier_kind: BarrierKind::Store
            })
        );
    }

    #[test]
    fn barrier_decision_selects_marking_barrier_for_black_to_grey_store() {
        let decision = static_barrier_schema_registry().plan_write(BarrierWriteContext::store(
            BarrierFieldKind::CellReference,
            CellState::PossiblyBlack,
            Some(CellState::PossiblyGrey),
        ));

        assert_eq!(
            decision.map(|decision| decision.action),
            Ok(BarrierAction::MarkingBarrier)
        );
    }

    #[test]
    fn barrier_decision_skips_initialization_barrier() {
        let decision =
            static_barrier_schema_registry().plan_write(BarrierWriteContext::initializing(
                BarrierFieldKind::CellReference,
                CellState::DefinitelyWhite,
                Some(CellState::DefinitelyWhite),
            ));

        assert_eq!(
            decision.map(|decision| (decision.schema.barrier_kind, decision.action)),
            Ok((BarrierKind::Initialization, BarrierAction::NoBarrier))
        );
    }

    #[test]
    fn barrier_decision_prefers_remembered_set_when_requested() {
        let decision = static_barrier_schema_registry().plan_write(
            BarrierWriteContext::store(
                BarrierFieldKind::CellReference,
                CellState::PossiblyBlack,
                Some(CellState::PossiblyBlack),
            )
            .remembered_set(true),
        );

        assert_eq!(
            decision.map(|decision| decision.action),
            Ok(BarrierAction::RememberedSet)
        );
    }

    #[test]
    fn barrier_requirement_reports_required_marking_barrier() {
        let outcome = static_barrier_schema_registry().evaluate_requirement(
            BarrierRequirementRequest::new(BarrierWriteContext::store(
                BarrierFieldKind::CellReference,
                CellState::PossiblyBlack,
                Some(CellState::DefinitelyWhite),
            )),
        );

        assert_eq!(
            outcome,
            Ok(BarrierRequirementOutcome::Required(
                BarrierAction::MarkingBarrier
            ))
        );
    }

    #[test]
    fn barrier_requirement_rejects_published_initialization_path() {
        let context = BarrierWriteContext::initializing(
            BarrierFieldKind::CellReference,
            CellState::DefinitelyWhite,
            Some(CellState::DefinitelyWhite),
        );
        let outcome = static_barrier_schema_registry().evaluate_requirement(
            BarrierRequirementRequest::new(context)
                .authority(BarrierMutationAuthority::UnpublishedCellInitialization),
        );

        assert_eq!(
            outcome,
            Err(BarrierDecisionError::InvalidMutationAuthority {
                context,
                authority: BarrierMutationAuthority::UnpublishedCellInitialization
            })
        );
    }

    #[test]
    fn barrier_requirement_accepts_unpublished_initialization() {
        let outcome = static_barrier_schema_registry().evaluate_requirement(
            BarrierRequirementRequest::new(BarrierWriteContext::initializing(
                BarrierFieldKind::CellReference,
                CellState::DefinitelyWhite,
                Some(CellState::DefinitelyWhite),
            ))
            .authority(BarrierMutationAuthority::UnpublishedCellInitialization)
            .owner_is_published(false),
        );

        assert_eq!(
            outcome,
            Ok(BarrierRequirementOutcome::NotRequired(
                BarrierNotRequiredReason::UnpublishedInitialization
            ))
        );
    }
}
