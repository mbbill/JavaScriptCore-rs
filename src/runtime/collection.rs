use crate::runtime::exception::JsResult;
use crate::runtime::state::{ObjectId, RuntimeValue, StructureId, WatchpointGeneration};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct OrderedHashTableId(pub u32);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JsMapObject {
    pub object: Option<ObjectId>,
    pub structure: Option<StructureId>,
    pub table: OrderedHashTableId,
    pub iteration_watchpoint: WatchpointGeneration,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JsSetObject {
    pub object: Option<ObjectId>,
    pub structure: Option<StructureId>,
    pub table: OrderedHashTableId,
    pub iteration_watchpoint: WatchpointGeneration,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CollectionEntry {
    pub key: RuntimeValue,
    pub value: RuntimeValue,
    pub state: CollectionEntryState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CollectionEntryState {
    #[default]
    Occupied,
    Deleted,
    Empty,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CollectionIterator {
    pub object: Option<ObjectId>,
    pub iterated_collection: Option<ObjectId>,
    pub next_index: u64,
    pub kind: CollectionIterationKind,
    pub table_snapshot_generation: WatchpointGeneration,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CollectionIterationKind {
    Keys,
    #[default]
    Values,
    Entries,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CollectionKind {
    #[default]
    Map,
    Set,
    WeakMap,
    WeakSet,
}

pub trait CollectionOperations {
    fn map_get(&self, map: ObjectId, key: RuntimeValue) -> JsResult<Option<RuntimeValue>>;
    fn map_set(&mut self, map: ObjectId, key: RuntimeValue, value: RuntimeValue) -> JsResult<()>;
    fn map_delete(&mut self, map: ObjectId, key: RuntimeValue) -> JsResult<bool>;
    fn set_add(&mut self, set: ObjectId, value: RuntimeValue) -> JsResult<()>;
    fn set_has(&self, set: ObjectId, value: RuntimeValue) -> JsResult<bool>;
    fn set_delete(&mut self, set: ObjectId, value: RuntimeValue) -> JsResult<bool>;
    fn create_collection_iterator(
        &mut self,
        collection: ObjectId,
        kind: CollectionIterationKind,
    ) -> JsResult<ObjectId>;
}
