use crate::runtime::exception::JsResult;
use crate::runtime::property::{PropertyDescriptor, PutPropertySlot, RuntimePropertyKey};
use crate::runtime::state::{ObjectId, RuntimeValue, StructureId, WatchpointGeneration};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JsArray {
    /// JSArray state relevant to indexed exotic semantics.
    ///
    /// Element storage and butterfly ownership belong to the object/GC layers.
    /// This contract records the observable length, indexing strategy, and
    /// conditions that block fast non-observable array operations.
    pub object: Option<ObjectId>,
    pub structure: Option<StructureId>,
    pub length: ArrayLengthSlot,
    pub indexing: ArrayIndexingProfile,
    pub storage: IndexedStorageContract,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ArrayLengthSlot {
    pub public_length: u64,
    pub vector_length: u32,
    pub writable: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ArrayIndexingProfile {
    pub indexing_type: ArrayIndexingType,
    pub holes_forward_to_prototype: bool,
    pub copy_on_write: bool,
    pub may_have_indexed_accessors: bool,
    pub watchpoint: WatchpointGeneration,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ArrayIndexingType {
    #[default]
    Undecided,
    Int32,
    Double,
    Contiguous,
    ArrayStorage,
    SlowPutArrayStorage,
    Sparse,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IndexedStorageContract {
    pub capacity: u32,
    pub initialized_length: u32,
    pub sparse_map_present: bool,
    pub index_bias: i32,
    pub owns_butterfly: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ArraySpeciesContract {
    pub constructor: Option<ObjectId>,
    pub species_constructor: Option<ObjectId>,
    pub can_use_fast_array_species: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ArrayIterationKind {
    #[default]
    Values,
    Keys,
    Entries,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ArrayMutationRequest {
    pub receiver: ObjectId,
    pub start: u64,
    pub delete_count: u64,
    pub insert_count: u64,
    pub length_before: u64,
    pub should_throw: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ArrayLengthWriteOutcome {
    #[default]
    Accepted,
    RejectedReadOnly,
    RejectedNonConfigurableElement,
    ExceedsMaximumLength,
    RequiresSparseStorage,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ArrayCopyContract {
    pub source: ObjectId,
    pub target: ObjectId,
    pub source_length: u64,
    pub target_offset: u64,
    pub fill_holes_with_undefined: bool,
    pub must_use_gc_safe_ops: bool,
}

/// Indexed exotic operations shared by arrays and array prototypes.
pub trait ArrayExoticOperations {
    fn define_length(
        &mut self,
        array: ObjectId,
        descriptor: PropertyDescriptor,
        should_throw: bool,
    ) -> JsResult<ArrayLengthWriteOutcome>;
    fn get_index(&self, array: ObjectId, index: u64) -> JsResult<Option<RuntimeValue>>;
    fn put_index(
        &mut self,
        array: ObjectId,
        index: u64,
        value: RuntimeValue,
        slot: PutPropertySlot,
    ) -> JsResult<bool>;
    fn delete_index(&mut self, array: ObjectId, index: u64, should_throw: bool) -> JsResult<bool>;
    fn set_public_length(
        &mut self,
        array: ObjectId,
        new_length: u64,
        should_throw: bool,
    ) -> JsResult<ArrayLengthWriteOutcome>;
    fn species_create(
        &self,
        array: ObjectId,
        requested_length: u64,
    ) -> JsResult<ArraySpeciesContract>;
    fn create_array_iterator(
        &mut self,
        array: ObjectId,
        kind: ArrayIterationKind,
    ) -> JsResult<ObjectId>;
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ArrayPrototypeMethodContract {
    /// Names the observable hooks each Array.prototype method must honor.
    ///
    /// `length` reads, `HasProperty`, `Get`, `Set`, species construction, and
    /// callback calls must remain visible to the future implementation.
    pub method: ArrayPrototypeMethod,
    pub reads_length: bool,
    pub consults_has_property: bool,
    pub calls_user_callback: bool,
    pub uses_species_constructor: bool,
    pub mutates_receiver: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ArrayPrototypeMethod {
    #[default]
    Values,
    Entries,
    Keys,
    At,
    Concat,
    CopyWithin,
    Fill,
    Filter,
    Find,
    FindIndex,
    FindLast,
    FindLastIndex,
    Flat,
    FlatMap,
    ForEach,
    Includes,
    IndexOf,
    Join,
    LastIndexOf,
    Map,
    Pop,
    Push,
    Reduce,
    ReduceRight,
    Reverse,
    Shift,
    Slice,
    Some,
    Sort,
    Splice,
    ToReversed,
    ToSorted,
    ToSpliced,
    Unshift,
    With,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IndexedExoticDefineRequest {
    pub object: ObjectId,
    pub key: RuntimePropertyKey,
    pub descriptor: PropertyDescriptor,
    pub should_throw: bool,
}
