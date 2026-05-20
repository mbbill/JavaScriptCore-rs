//! Runtime instance and global object placeholders.
//!
//! Instance layout, cached memory/table pointers, import wrappers, module
//! records, globals, and generated entrypoints are reserved for later
//! object-model and ABI work.

use crate::modules::ModuleRecordId;
use crate::runtime::ObjectId;
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
///
/// The instance owns Wasm runtime bindings and caches. Any `ModuleRecordId`
/// stored here is borrowed module-loader identity, not instance-owned state.
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
    pub anchor: Option<WasmInstanceAnchorDescriptor>,
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

/// Concurrent compiler anchor for a JS instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmInstanceAnchorDescriptor {
    pub instance: WasmInstanceId,
    pub module: WasmModuleId,
    pub js_instance_live: bool,
    pub guarded_by_anchor_lock: bool,
}

/// Runtime object allocation state owned by instance linking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmRuntimeObjectAllocationState {
    NotAllocated,
    AllocatingMemories,
    AllocatingTables,
    AllocatingGlobals,
    AllocatingTags,
    Complete,
    Failed,
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
    pub allocation_state: WasmRuntimeObjectAllocationState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmLinkStateSemanticDescriptor {
    pub state: WasmLinkState,
    pub is_terminal: bool,
    pub validates_imports: bool,
    pub imports_available: bool,
    pub runtime_objects_available: bool,
    pub exports_available: bool,
    pub failed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmLinkPlanSemanticDescriptor {
    pub module: WasmModuleId,
    pub instance: Option<WasmInstanceId>,
    pub state: WasmLinkStateSemanticDescriptor,
    pub import_count: u32,
    pub export_count: u32,
    pub required_js_to_wasm_bridge_count: usize,
    pub required_wasm_to_js_bridge_count: usize,
    pub allocation_state: WasmRuntimeObjectAllocationState,
    pub allocation_complete: bool,
}

pub fn describe_wasm_link_state_semantics(state: WasmLinkState) -> WasmLinkStateSemanticDescriptor {
    WasmLinkStateSemanticDescriptor {
        state,
        is_terminal: matches!(state, WasmLinkState::Instantiated | WasmLinkState::Failed),
        validates_imports: matches!(
            state,
            WasmLinkState::ValidatingImports
                | WasmLinkState::LinkingImports
                | WasmLinkState::ImportsLinked
                | WasmLinkState::AllocatingRuntimeObjects
                | WasmLinkState::Linked
                | WasmLinkState::Instantiating
                | WasmLinkState::Instantiated
        ),
        imports_available: matches!(
            state,
            WasmLinkState::ImportsLinked
                | WasmLinkState::AllocatingRuntimeObjects
                | WasmLinkState::Linked
                | WasmLinkState::Instantiating
                | WasmLinkState::Instantiated
        ),
        runtime_objects_available: matches!(
            state,
            WasmLinkState::Linked | WasmLinkState::Instantiating | WasmLinkState::Instantiated
        ),
        exports_available: matches!(state, WasmLinkState::Linked | WasmLinkState::Instantiated),
        failed: state == WasmLinkState::Failed,
    }
}

pub fn describe_wasm_link_plan_semantics(plan: &WasmLinkPlan) -> WasmLinkPlanSemanticDescriptor {
    WasmLinkPlanSemanticDescriptor {
        module: plan.module,
        instance: plan.instance,
        state: describe_wasm_link_state_semantics(plan.state),
        import_count: plan.import_count,
        export_count: plan.export_count,
        required_js_to_wasm_bridge_count: plan.required_js_to_wasm_bridges.len(),
        required_wasm_to_js_bridge_count: plan.required_wasm_to_js_bridges.len(),
        allocation_state: plan.allocation_state,
        allocation_complete: plan.allocation_state == WasmRuntimeObjectAllocationState::Complete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_state_semantics_mark_runtime_availability() {
        let linked = describe_wasm_link_state_semantics(WasmLinkState::Linked);

        assert!(linked.imports_available);
        assert!(linked.runtime_objects_available);
        assert!(linked.exports_available);
        assert!(!linked.is_terminal);
    }

    #[test]
    fn link_plan_semantics_count_bridges_without_instantiating() {
        let plan = WasmLinkPlan {
            module: WasmModuleId(10),
            instance: Some(WasmInstanceId(11)),
            import_count: 2,
            export_count: 1,
            state: WasmLinkState::AllocatingRuntimeObjects,
            required_js_to_wasm_bridges: Vec::new(),
            required_wasm_to_js_bridges: Vec::new(),
            allocation_state: WasmRuntimeObjectAllocationState::AllocatingGlobals,
        };

        let descriptor = describe_wasm_link_plan_semantics(&plan);

        assert_eq!(descriptor.module, WasmModuleId(10));
        assert_eq!(descriptor.import_count, 2);
        assert!(!descriptor.allocation_complete);
        assert!(descriptor.state.imports_available);
    }
}
