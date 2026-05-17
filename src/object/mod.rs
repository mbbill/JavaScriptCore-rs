//! Object identity, structures, property metadata, and storage contracts.
//!
//! This module models objects without implementing JavaScript property lookup.
//! Shape transitions, storage mutation, and watchpoint invalidation are named
//! here so future behavior lands behind stable boundaries.

#![deny(unsafe_op_in_unsafe_fn)]

mod identity;
mod operations;
mod property;
mod storage;
mod structure;
mod watchpoint;

pub use self::identity::{JsObject, ObjectFlags, ObjectHeader};
pub use crate::gc::StructureId;
pub use operations::{
    DefineOwnPropertyOutcome, DeletePropertyOutcome, ExoticObjectKind, GetPropertyOutcome,
    HasPropertyOutcome, ObjectInternalMethodContext, ObjectInternalMethodHooks,
    ObjectInternalMethodKind, ObjectMethodTableCapabilities, ObjectOperationError,
    ObjectOperationResult, OrdinaryPropertyLookupPlan, PrivateBrandCheck, PrivateBrandCheckResult,
    PrivateBrandRequirement, PropertyDefinitionPolicy, PropertyMutationContext, PropertyReceiver,
    PrototypeTraversalAction, PrototypeTraversalPlan, PrototypeTraversalStep, PutPropertyOutcome,
};
pub use property::{
    AccessorDescriptor, AtomId, CompletePropertyDescriptor, DataDescriptor, EnumerationBucket,
    EnumerationOrder, EnumerationRecord, PrivateFieldDescriptor, PrivateFieldKind, PrivateNameId,
    PropertyAttributes, PropertyCacheability, PropertyDescriptor, PropertyDescriptorKind,
    PropertyDescriptorState, PropertyKey, PropertyLocation, PropertyLookupMode, PropertyOffset,
    PropertySlot, PropertySlotBase, PropertySlotKind, PropertyTable, PropertyTableMode,
    PropertyValueDescriptor, SymbolId,
};
pub use storage::{
    ArrayLengthContract, ArrayStorageMetadata, Butterfly, ButterflyGrowth, ButterflyGrowthReason,
    ButterflyHandle, ButterflyLayout, IndexedStorage, IndexedStorageKind, IndexingHeader,
    IndexingHistory, InlineStorage, OutOfLineStorage, SparseIndexMetadata, TypedArrayBufferEdge,
    TypedArrayContentType, TypedArrayElementType, TypedArrayMode, TypedArrayStorageContract,
    TypedArrayViewLength,
};
pub use structure::{IndexingMode, Structure, StructureTransition, StructureTransitionPlan};
pub use watchpoint::{Watchpoint, WatchpointKind, WatchpointSet, WatchpointState};
