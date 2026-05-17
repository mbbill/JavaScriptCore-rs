//! Runtime instance and global object placeholders.
//!
//! Instance layout, cached memory/table pointers, import wrappers, module
//! records, globals, and generated entrypoints are reserved for later
//! object-model and ABI work.

use crate::runtime::{ModuleRecordId, ObjectId};
use crate::wasm::{
    JsToWasmBridge, WasmCalleeGroupId, WasmFunctionIndex, WasmGlobalKind, WasmMemoryCacheSlot,
    WasmMemoryId, WasmModuleId, WasmTableCacheSlot, WasmTableId, WasmToJsBridge,
};

/// Stable identity for a Wasm instance object.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmInstanceId(pub u64);

/// Linkage lifecycle for a Wasm instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmLinkState {
    Unlinked,
    ValidatingImports,
    LinkingImports,
    ImportsLinked,
    AllocatingRuntimeObjects,
    Linked,
    Instantiating,
    Instantiated,
    Failed,
}

/// GC-owned public instance wrapper reserved for future WebAssembly.Instance.
#[derive(Clone, Debug)]
pub struct WasmInstanceObject {
    pub id: WasmInstanceId,
    pub object: Option<ObjectId>,
    pub module: WasmModuleId,
    pub module_record: Option<ModuleRecordId>,
    pub link_state: WasmLinkState,
    pub memories: Vec<WasmMemoryId>,
    pub tables: Vec<WasmTableId>,
    pub globals: Vec<WasmGlobalId>,
    pub imports: Vec<WasmImportBinding>,
    pub exports: Vec<WasmExportBinding>,
    pub memory_cache_slots: Vec<WasmMemoryCacheSlot>,
    pub table_cache_slots: Vec<WasmTableCacheSlot>,
    pub callee_group: Option<WasmCalleeGroupId>,
    pub js_to_wasm: Vec<JsToWasmBridge>,
    pub wasm_to_js: Vec<WasmToJsBridge>,
}

/// Import binding installed into an instance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmImportBinding {
    pub function: Option<WasmFunctionIndex>,
    pub object: Option<ObjectId>,
    pub memory: Option<WasmMemoryId>,
    pub table: Option<WasmTableId>,
    pub global: Option<WasmGlobalId>,
    pub link_status: WasmImportLinkStatus,
}

/// Link status for a single import binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmImportLinkStatus {
    Unchecked,
    TypeMatched,
    WrappedJsCallable,
    LinkedWasmExport,
    Rejected,
}

/// Export binding exposed by an instance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmExportBinding {
    pub name: Option<u32>,
    pub kind: WasmInstanceExportKind,
    pub function: Option<WasmFunctionIndex>,
    pub object: Option<ObjectId>,
}

/// Export category after instantiation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmInstanceExportKind {
    Function,
    Memory,
    Table,
    Global,
    Tag,
}

/// Stable identity for a Wasm global object.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmGlobalId(pub u64);

/// Mutability category for future WebAssembly.Global wrappers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmGlobalMutability {
    Immutable,
    Mutable,
}

/// GC-owned public global wrapper reserved for future WebAssembly.Global.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmGlobalObject {
    pub id: WasmGlobalId,
    pub object: Option<ObjectId>,
    pub kind: WasmGlobalKind,
    pub mutability: WasmGlobalMutability,
    pub owner_instance: Option<WasmInstanceId>,
    pub storage: WasmGlobalStorage,
}

/// Storage policy for a global.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmGlobalStorage {
    EmbeddedInInstance,
    ImportedBinding,
    JsWrapperOwned,
    Deferred,
}

/// Lifecycle event for instance integration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmInstanceLifecycleEvent {
    pub instance: WasmInstanceId,
    pub from: WasmLinkState,
    pub to: WasmLinkState,
}

/// Import/export linking plan for an instance.
#[derive(Clone, Debug)]
pub struct WasmLinkPlan {
    pub module: WasmModuleId,
    pub instance: Option<WasmInstanceId>,
    pub import_count: u32,
    pub export_count: u32,
    pub state: WasmLinkState,
    pub required_js_to_wasm_bridges: Vec<JsToWasmBridge>,
    pub required_wasm_to_js_bridges: Vec<WasmToJsBridge>,
}
