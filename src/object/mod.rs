//! Object identity, structures, property metadata, and storage contracts.
//!
//! This module models objects without implementing JavaScript property lookup.
//! Shape transitions, storage mutation, and watchpoint invalidation are named
//! here so future behavior lands behind stable boundaries.

#![deny(unsafe_op_in_unsafe_fn)]

pub mod auxiliary;
pub mod butterfly_handle;
mod identity;
mod indexing_type;
mod operations;
mod property;
mod property_offset;
mod property_table;
mod storage;
mod structure;
mod structure_cell;
mod structure_transition_table;
mod watchpoint;

pub use self::identity::{
    JsObject, ObjectFlags, ObjectHeader, ObjectMutationAuthority, ObjectPublicationState,
};
pub use crate::gc::StructureId;
pub use operations::{
    adapt_define_own_property_for_execution, adapt_delete_property_for_execution,
    adapt_get_own_property_for_execution, adapt_has_own_property_for_execution,
    adapt_put_property_for_execution, ordinary_define_own_property, ordinary_delete_property,
    ordinary_get_own_property, ordinary_has_property, ordinary_put_property,
    plan_object_property_mutation_barrier, validate_ordinary_descriptor_compatibility,
    DefineOwnPropertyOutcome, DeletePropertyOutcome, ExecutionPropertyOperation,
    ExecutionPropertyOperationRecord, ExoticObjectKind, GetPropertyOutcome, HasPropertyOutcome,
    ObjectInternalMethodContext, ObjectInternalMethodHooks, ObjectInternalMethodKind,
    ObjectMethodSideEffect, ObjectMethodSlotDescriptor, ObjectMethodTableCapabilities,
    ObjectMethodTableDescriptor, ObjectMethodTableOwner, ObjectMethodTableProvenance,
    ObjectOperationError, ObjectOperationGcBoundary, ObjectOperationResult,
    ObjectPropertyMutationBarrierRecord, ObjectPropertyMutationBarrierRequest,
    OrdinaryDefineOwnPropertyPlan, OrdinaryObjectState, OrdinaryPropertyLookupPlan,
    PrivateBrandCheck, PrivateBrandCheckResult, PrivateBrandRequirement, PropertyDefinitionPolicy,
    PropertyDescriptorCompatibility, PropertyMutationContext, PropertyReceiver,
    PrototypeTraversalAction, PrototypeTraversalPlan, PrototypeTraversalStep, PutPropertyOutcome,
};
pub use property::{
    analyze_static_property_table, enumerate_static_property_table, validate_property_descriptor,
    validate_property_descriptors, AccessorDescriptor, AtomId, CompletePropertyDescriptor,
    DataDescriptor, DomAttributeSlot, EnumerationBucket, EnumerationOrder, EnumerationRecord,
    ModuleNamespaceSlot, PrivateFieldDescriptor, PrivateFieldKind, PrivateName, PropertyAttributes,
    PropertyCacheability, PropertyDescriptor, PropertyDescriptorBuilder, PropertyDescriptorKind,
    PropertyDescriptorState, PropertyDescriptorValidationError, PropertyKey, PropertyLocation,
    PropertyLookupMode, PropertyOffset, PropertySchemaOwner, PropertySchemaProvenance,
    PropertySlot, PropertySlotAdditionalData, PropertySlotBase, PropertySlotKind, PropertyTable,
    PropertyTableBuilder, PropertyTableMode, PropertyValueDescriptor, PutPropertySlotContext,
    PutPropertySlotKind, StaticPropertyKind, StaticPropertyTableAnalysis,
    StaticPropertyTableDescriptor,
};
// gc-r4 B1a: `ButterflyHandle` (a value-type-agnostic slab index) moved out of
// `storage.rs` into `butterfly_handle.rs`, the home of the LIVE butterfly rep
// over `RuntimeValue`. `ButterflyAllocation` is that live rep. storage.rs's
// `Butterfly`/`OutOfLineStorage`/etc. (re-exported below) remain the NON-LIVE
// contract/skeleton types over `JsValue`, retired in a later GAP-D cleanup.
pub use butterfly_handle::{ButterflyAllocation, ButterflyHandle};
pub use storage::{
    typed_array_content_type, typed_array_element_size, validate_array_storage_metadata,
    validate_butterfly_layout, validate_indexing_header, validate_typed_array_storage_contract,
    ArrayLengthContract, ArrayStorageMetadata, Butterfly, ButterflyGrowth, ButterflyGrowthReason,
    ButterflyLayout, ButterflyLayoutBuilder, IndexedStorage, IndexedStorageKind, IndexingHeader,
    IndexingHistory, InlineStorage, OutOfLineStorage, SparseIndexMetadata, StorageValidationError,
    TypedArrayBufferEdge, TypedArrayContentType, TypedArrayElementType, TypedArrayMode,
    TypedArrayStorageContract, TypedArrayStorageContractBuilder, TypedArrayViewLength,
};
pub use structure::{
    plan_structure_transition, transition_invalidates_watchpoints, validate_structure_descriptor,
    validate_structure_descriptor_table, validate_structure_transition_plan, IndexingMode,
    Structure, StructureDescriptor, StructureDescriptorBuilder, StructureDescriptorTable,
    StructureDescriptorValidationError, StructureDictionaryKind, StructureLifecycle,
    StructureMutationAuthority, StructurePropertyCacheability, StructurePrototypeStorage,
    StructureRareDataId, StructureSchemaOwner, StructureSchemaProvenance, StructureTransition,
    StructureTransitionMetadata, StructureTransitionPlan, StructureTransitionValidationError,
};
pub use watchpoint::{Watchpoint, WatchpointKind, WatchpointSet, WatchpointState};

// gc-r4 Batch 2 (Structure-wire): the ported faithful Structure registry + offset
// math, now mounted as the interpreter's live property-offset authority. The
// `structure_cell::Structure` JSCell is re-exported as `StructureCell` to avoid
// colliding with the still-standalone Rust-only `structure::Structure` descriptor.
pub use indexing_type::NON_ARRAY;
pub use property_offset::{
    offset_for_property_number, FIRST_OUT_OF_LINE_OFFSET as STRUCTURE_FIRST_OUT_OF_LINE_OFFSET,
};
pub use structure_cell::{PrototypePointer, Structure as StructureCell, StructureIdTable};
