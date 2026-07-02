//! Structure descriptor/shape-transition VALIDATION contracts (static schema
//! tables + transition-plan validators), plus the [`IndexingMode`] vocabulary
//! they carry.
//!
//! Structures-as-cells Step 1 fork retirement (docs/design/structures-as-cells.md
//! §7): this file used to ALSO define a second, disconnected, GC-cell-shaped
//! `Structure` (`header: JsCellHeader` / `Trace`/`TraceCell` impls against the
//! pre-S4-arena `gc::cell`/`gc::trace`/`gc::barrier` generation) — a dead fork
//! of the live, arena-integrated `object::structure_cell::Structure`, with
//! exactly one (dead) external consumer (`vm::runtime::RuntimeStructures`'s
//! three `Option<Root<Structure>>` fields). That struct and its `Trace`/
//! `TraceCell` impls are DELETED per §7.2/§7.3.
//!
//! SCOPE-NOTE (design gap found during retirement, not present in §7's own
//! survey): §7.1 describes reading "object/structure.rs:1-181 for the
//! cell-shaped part" and §7.2 characterizes the file's OTHER types
//! (`StructureDictionaryKind`, `StructurePrototypeStorage`, `IndexingMode`) as
//! "deleted with it" — but this survey found `IndexingMode` is a REAL, live
//! import of `object::indexing_type` (`use super::structure::IndexingMode;`,
//! `indexing_type.rs:30`, feeding its `indexing_shape_and_writability_for_mode`
//! bridge + tests), and `StructureDescriptorTable`/`StructureDescriptorValidationError`/
//! `StructureSchemaOwner` are live, load-bearing types for `vm::runtime`'s
//! SEPARATE `VmStructureTableDescriptor`/`VmStaticDescriptorTables` validation
//! system (`vm/runtime.rs`, both its main body and its own test module).
//! `IndexingMode`/`StructureDictionaryKind`/`StructurePrototypeStorage` are
//! fields of the STILL-LIVE `StructureDescriptor`, not exclusively of the dead
//! `Structure` cell struct. Deleting the whole file (as §7's text reads at a
//! glance) would have broken those live consumers, so ONLY the dead cell-shaped
//! part (the former lines ~52-181: the `Structure` struct + `impl Structure` +
//! `impl Trace for Structure` + `impl TraceCell for Structure`) is removed
//! here; the descriptor/validation machinery below is retained unmodified.

use crate::gc::StructureId;

use crate::object::{
    PropertyAttributes, PropertyDescriptorValidationError, PropertyKey, PropertyLocation,
    PropertyOffset, StaticPropertyTableDescriptor,
};
use std::collections::HashSet;

/// Indexed storage representation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IndexingMode {
    #[default]
    None,
    UndecidedArray,
    Int32,
    Double,
    Contiguous,
    ArrayStorage,
    SlowPutArrayStorage,
    CopyOnWriteInt32,
    CopyOnWriteDouble,
    CopyOnWriteContiguous,
    Dictionary,
    IntegerIndexedExotic,
}

impl IndexingMode {
    pub const fn has_indexed_properties(self) -> bool {
        !matches!(self, Self::None)
    }

    pub const fn is_copy_on_write(self) -> bool {
        matches!(
            self,
            Self::CopyOnWriteInt32 | Self::CopyOnWriteDouble | Self::CopyOnWriteContiguous
        )
    }

    pub const fn needs_slow_put(self) -> bool {
        matches!(
            self,
            Self::SlowPutArrayStorage | Self::Dictionary | Self::IntegerIndexedExotic
        )
    }
}

// Structures-as-cells Step 1 (docs/design/structures-as-cells.md §7.2/§7.3):
// the dead, disconnected GC-cell-shaped `Structure` struct (`header:
// JsCellHeader`, a `WriteBarrier<JsCell>` prototype edge, an owned
// `PropertyTable`/`StructureTransitionMetadata`/`WatchpointSet`) plus its
// `impl Trace for Structure` / `impl TraceCell for Structure` were DELETED
// here. It targeted the pre-S4-arena `gc::cell`/`gc::trace`/`gc::barrier`
// header/visitor generation (`JsCellHeader` there has no fixed C++ offset
// layout, unlike the live arena's `marked_block::JsCellHeader`) and was never
// wired to `marked_block.rs`/`slot_visitor.rs`/`interpreter::object_store`.
// Its one external consumer (`vm::runtime::RuntimeStructures`'s three
// `Option<Root<Structure>>` fields) was itself dead — zero readers/writers
// anywhere in the crate — and was removed in the same batch. The live
// replacement is `object::structure_cell::Structure` (re-exported as
// `StructureCell`), the arena-integrated Structure this whole module's
// descriptor/validation types (below) describe metadata FOR, not duplicate.
//
// Harvested from the deleted code (design §7.2, as REQUIREMENTS, not literal
// code — the concrete types below target the live `structure_cell::Structure`
// instead): "a cell has a fixed header prefix, the rest is payload" (already
// the live arena convention); "the prototype must be a real traced
// write-barrier edge, not a bare pointer" (a Structures-as-cells Step 3 item,
// docs/design/structures-as-cells.md §3.3, not yet wired).

/// Add/delete/attribute/prototype transition descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructureTransition {
    AddProperty {
        key: PropertyKey,
        attributes: PropertyAttributes,
    },
    DeleteProperty {
        key: PropertyKey,
    },
    ChangeAttributes {
        key: PropertyKey,
        attributes: PropertyAttributes,
    },
    ChangePrototype,
    ChangeGlobalProxyTarget,
    EnterDictionaryMode,
    EnterUncacheableDictionaryMode,
    Seal,
    Freeze,
    PreventExtensions,
    SetPrivateBrand {
        key: PropertyKey,
    },
    BecomePrototype,
    ChangeIndexingMode(IndexingMode),
}

/// Planned transition away from a structure. The new structure allocation and
/// storage migration are separate responsibilities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StructureTransitionPlan {
    pub base: StructureId,
    pub transition: StructureTransition,
    pub invalidates_watchpoints: bool,
}

/// Owner of immutable structure schema metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructureSchemaOwner {
    VmStructures,
    RealmIntrinsics,
    BuiltinObject,
    GlobalObject,
    HostObject,
    GeneratedStaticData,
}

/// Provenance for structure descriptors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructureSchemaProvenance {
    HandAuthoredRust,
    GeneratedFromEngineMetadata,
    Ecma262Intrinsic,
    HostBinding,
}

/// Immutable descriptor for one structure shape.
///
/// Structure ids are canonical metadata handles owned by the GC/structure table.
/// The descriptor only names layout facts and borrowed static property metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StructureDescriptor {
    pub name: &'static str,
    pub structure: StructureId,
    pub owner: StructureSchemaOwner,
    pub provenance: StructureSchemaProvenance,
    pub indexing_mode: IndexingMode,
    pub dictionary_kind: StructureDictionaryKind,
    pub prototype_storage: StructurePrototypeStorage,
    pub inline_capacity: u16,
    pub out_of_line_capacity: u16,
    property_table: Option<&'static StaticPropertyTableDescriptor>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StructureDescriptorValidationError {
    EmptyName,
    InvalidStructureId,
    DuplicateStructureId(StructureId),
    DuplicateName(&'static str),
    PropertyTable(PropertyDescriptorValidationError),
    PropertyCapacityExceeded {
        structure: StructureId,
        required: u32,
        available: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructureTransitionValidationError {
    BaseMismatch {
        expected: StructureId,
        actual: StructureId,
    },
    PropertyAlreadyExists(PropertyKey),
    PropertyMissing(PropertyKey),
    WatchpointInvalidationMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StructureDescriptorBuilder {
    descriptor: StructureDescriptor,
}

impl StructureDescriptorBuilder {
    pub const fn new(
        name: &'static str,
        structure: StructureId,
        owner: StructureSchemaOwner,
        provenance: StructureSchemaProvenance,
    ) -> Self {
        Self {
            descriptor: StructureDescriptor {
                name,
                structure,
                owner,
                provenance,
                indexing_mode: IndexingMode::None,
                dictionary_kind: StructureDictionaryKind::None,
                prototype_storage: StructurePrototypeStorage::Mono,
                inline_capacity: 0,
                out_of_line_capacity: 0,
                property_table: None,
            },
        }
    }

    pub const fn indexing_mode(mut self, indexing_mode: IndexingMode) -> Self {
        self.descriptor.indexing_mode = indexing_mode;
        self
    }

    pub const fn dictionary_kind(mut self, dictionary_kind: StructureDictionaryKind) -> Self {
        self.descriptor.dictionary_kind = dictionary_kind;
        self
    }

    pub const fn prototype_storage(mut self, prototype_storage: StructurePrototypeStorage) -> Self {
        self.descriptor.prototype_storage = prototype_storage;
        self
    }

    pub const fn inline_capacity(mut self, inline_capacity: u16) -> Self {
        self.descriptor.inline_capacity = inline_capacity;
        self
    }

    pub const fn out_of_line_capacity(mut self, out_of_line_capacity: u16) -> Self {
        self.descriptor.out_of_line_capacity = out_of_line_capacity;
        self
    }

    pub const fn property_table(
        mut self,
        property_table: &'static StaticPropertyTableDescriptor,
    ) -> Self {
        self.descriptor.property_table = Some(property_table);
        self
    }

    pub fn build(self) -> Result<StructureDescriptor, StructureDescriptorValidationError> {
        validate_structure_descriptor(&self.descriptor)?;
        Ok(self.descriptor)
    }
}

impl StructureDescriptor {
    pub const fn new(
        name: &'static str,
        structure: StructureId,
        owner: StructureSchemaOwner,
        provenance: StructureSchemaProvenance,
        indexing_mode: IndexingMode,
        property_table: Option<&'static StaticPropertyTableDescriptor>,
    ) -> Self {
        Self {
            name,
            structure,
            owner,
            provenance,
            indexing_mode,
            dictionary_kind: StructureDictionaryKind::None,
            prototype_storage: StructurePrototypeStorage::Mono,
            inline_capacity: 0,
            out_of_line_capacity: 0,
            property_table,
        }
    }

    /// Returns the borrowed immutable property table for this structure.
    pub const fn property_table(self) -> Option<&'static StaticPropertyTableDescriptor> {
        self.property_table
    }

    pub fn validate(&self) -> Result<(), StructureDescriptorValidationError> {
        validate_structure_descriptor(self)
    }
}

/// Static table of structure descriptors owned by a VM or realm.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StructureDescriptorTable {
    pub name: &'static str,
    pub owner: StructureSchemaOwner,
    descriptors: &'static [StructureDescriptor],
}

impl StructureDescriptorTable {
    pub const fn new(
        name: &'static str,
        owner: StructureSchemaOwner,
        descriptors: &'static [StructureDescriptor],
    ) -> Self {
        Self {
            name,
            owner,
            descriptors,
        }
    }

    /// Returns structure descriptors as immutable static metadata.
    pub const fn descriptors(&self) -> &'static [StructureDescriptor] {
        self.descriptors
    }

    /// Returns one existing structure descriptor by table index.
    pub const fn descriptor_at(&self, index: usize) -> Option<&'static StructureDescriptor> {
        if index < self.descriptors.len() {
            Some(&self.descriptors[index])
        } else {
            None
        }
    }

    pub fn validate(&self) -> Result<(), StructureDescriptorValidationError> {
        validate_structure_descriptor_table(self)
    }
}

pub fn validate_structure_descriptor(
    descriptor: &StructureDescriptor,
) -> Result<(), StructureDescriptorValidationError> {
    if descriptor.name.is_empty() {
        return Err(StructureDescriptorValidationError::EmptyName);
    }

    if descriptor.structure == StructureId::INVALID {
        return Err(StructureDescriptorValidationError::InvalidStructureId);
    }

    if let Some(table) = descriptor.property_table {
        table
            .validate()
            .map_err(StructureDescriptorValidationError::PropertyTable)?;

        let available =
            u32::from(descriptor.inline_capacity) + u32::from(descriptor.out_of_line_capacity);
        let mut required = 0;
        for entry in table.entries() {
            if entry.location == PropertyLocation::InlineOrOutOfLine {
                required = required.max(entry.offset.raw() as u32 + 1);
            }
        }

        if required > available {
            return Err(
                StructureDescriptorValidationError::PropertyCapacityExceeded {
                    structure: descriptor.structure,
                    required,
                    available,
                },
            );
        }
    }

    Ok(())
}

pub fn validate_structure_descriptor_table(
    table: &StructureDescriptorTable,
) -> Result<(), StructureDescriptorValidationError> {
    if table.name.is_empty() {
        return Err(StructureDescriptorValidationError::EmptyName);
    }

    let mut seen_ids = HashSet::new();
    let mut seen_names = HashSet::new();
    for descriptor in table.descriptors {
        validate_structure_descriptor(descriptor)?;
        if !seen_ids.insert(descriptor.structure) {
            return Err(StructureDescriptorValidationError::DuplicateStructureId(
                descriptor.structure,
            ));
        }
        if !seen_names.insert(descriptor.name) {
            return Err(StructureDescriptorValidationError::DuplicateName(
                descriptor.name,
            ));
        }
    }
    Ok(())
}

pub fn validate_structure_transition_plan(
    descriptor: &StructureDescriptor,
    plan: StructureTransitionPlan,
) -> Result<(), StructureTransitionValidationError> {
    if plan.base != descriptor.structure {
        return Err(StructureTransitionValidationError::BaseMismatch {
            expected: descriptor.structure,
            actual: plan.base,
        });
    }

    let has_property = |key| {
        descriptor
            .property_table
            .is_some_and(|table| table.entries().iter().any(|entry| entry.key == key))
    };

    match plan.transition {
        StructureTransition::AddProperty { key, .. }
        | StructureTransition::SetPrivateBrand { key }
            if has_property(key) =>
        {
            return Err(StructureTransitionValidationError::PropertyAlreadyExists(
                key,
            ));
        }
        StructureTransition::DeleteProperty { key }
        | StructureTransition::ChangeAttributes { key, .. }
            if !has_property(key) =>
        {
            return Err(StructureTransitionValidationError::PropertyMissing(key));
        }
        _ => {}
    }

    if plan.invalidates_watchpoints != transition_invalidates_watchpoints(plan.transition) {
        return Err(StructureTransitionValidationError::WatchpointInvalidationMismatch);
    }

    Ok(())
}

pub fn plan_structure_transition(
    descriptor: &StructureDescriptor,
    transition: StructureTransition,
) -> Result<StructureTransitionPlan, StructureTransitionValidationError> {
    let plan = StructureTransitionPlan {
        base: descriptor.structure,
        transition,
        invalidates_watchpoints: transition_invalidates_watchpoints(transition),
    };
    validate_structure_transition_plan(descriptor, plan)?;
    Ok(plan)
}

pub const fn transition_invalidates_watchpoints(transition: StructureTransition) -> bool {
    matches!(
        transition,
        StructureTransition::ChangePrototype
            | StructureTransition::DeleteProperty { .. }
            | StructureTransition::ChangeAttributes { .. }
            | StructureTransition::ChangeGlobalProxyTarget
            | StructureTransition::EnterDictionaryMode
            | StructureTransition::EnterUncacheableDictionaryMode
            | StructureTransition::Seal
            | StructureTransition::Freeze
            | StructureTransition::PreventExtensions
            | StructureTransition::SetPrivateBrand { .. }
            | StructureTransition::BecomePrototype
            | StructureTransition::ChangeIndexingMode(_)
    )
}

/// Dictionary mode selected for a structure's property table.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StructureDictionaryKind {
    #[default]
    None,
    Cached,
    Uncached,
}

/// Prototype representation used by the structure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StructurePrototypeStorage {
    #[default]
    Mono,
    Poly,
}

/// Lifecycle state for a structure cell.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StructureLifecycle {
    #[default]
    Allocating,
    FinishingCreation,
    Published,
    TransitionedFrom,
    DictionaryMutated,
}

/// Concurrency authority for structure lookup or mutation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StructureMutationAuthority {
    #[default]
    MainThreadVm,
    ConcurrentLookupOnly,
    ExistingStructureTransition,
    DictionaryMutationWithStructureLock,
}

/// Metadata carried with structure transitions and rare-data edges.
///
/// The previous-structure edge may later be replaced by rare data. C++ uses a
/// tagged `m_previousOrRareData` field; this contract keeps the distinction
/// explicit so Rust code does not treat previous IDs and rare-data cells as the
/// same identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StructureTransitionMetadata {
    pub previous: Option<StructureId>,
    pub rare_data: Option<StructureRareDataId>,
    pub lifecycle: StructureLifecycle,
    pub mutation_authority: StructureMutationAuthority,
    pub transition_count_estimate: u16,
    pub max_offset: PropertyOffset,
    pub transition_offset: PropertyOffset,
    pub realm_is_immutable_after_creation: bool,
}

impl StructureTransitionMetadata {
    pub const fn new_unpublished(_id: StructureId) -> Self {
        Self {
            previous: None,
            rare_data: None,
            lifecycle: StructureLifecycle::Allocating,
            mutation_authority: StructureMutationAuthority::MainThreadVm,
            transition_count_estimate: 0,
            max_offset: PropertyOffset::INVALID,
            transition_offset: PropertyOffset::INVALID,
            realm_is_immutable_after_creation: true,
        }
    }
}

/// Rare-data cell associated with a structure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct StructureRareDataId(pub u32);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::{AtomId, Identifier};

    fn key(slot: u32) -> PropertyKey {
        PropertyKey::from_identifier(Identifier::from_atom(AtomId::from_table_slot(slot)))
    }

    #[test]
    fn structure_transition_planner_sets_watchpoint_invalidation() {
        let descriptor = StructureDescriptorBuilder::new(
            "object",
            StructureId::new(1),
            StructureSchemaOwner::VmStructures,
            StructureSchemaProvenance::HandAuthoredRust,
        )
        .build()
        .unwrap();

        let plan =
            plan_structure_transition(&descriptor, StructureTransition::ChangePrototype).unwrap();

        assert_eq!(plan.base, StructureId::new(1));
        assert!(plan.invalidates_watchpoints);
    }

    #[test]
    fn structure_transition_planner_rejects_deleting_missing_property() {
        let descriptor = StructureDescriptorBuilder::new(
            "object",
            StructureId::new(1),
            StructureSchemaOwner::VmStructures,
            StructureSchemaProvenance::HandAuthoredRust,
        )
        .build()
        .unwrap();

        let error = plan_structure_transition(
            &descriptor,
            StructureTransition::DeleteProperty { key: key(7) },
        )
        .unwrap_err();

        assert_eq!(
            error,
            StructureTransitionValidationError::PropertyMissing(key(7))
        );
    }
}

/// Cacheability decision derived from structure flags and type info.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StructurePropertyCacheability {
    #[default]
    Cacheable,
    UncacheableDictionary,
    PrototypeQueriesUncacheable,
    ImpureGetOwnPropertySlot,
    ImpureAbsenceCheck,
    NeedsImpurePropertyWatchpoint,
}
