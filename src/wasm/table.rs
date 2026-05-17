//! WebAssembly table and callee-group placeholders.
//!
//! Future implementations will attach compiled callees, wrappers, anchors,
//! indirect-call entrypoints, optimized replacements, and GC liveness edges
//! here.

use crate::jit::{JitCodeId, JitType};
use crate::runtime::ObjectId;
use crate::wasm::{
    JsToWasmBridge, WasmFunctionIndex, WasmInstanceId, WasmMemoryId, WasmModuleId, WasmToJsBridge,
};

/// Stable identity for a Wasm table object.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmTableId(pub u64);

/// Table index in module table index space.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmTableIndex(pub u32);

/// GC-owned public table wrapper reserved for future WebAssembly.Table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmTableObject {
    pub id: WasmTableId,
    pub object: Option<ObjectId>,
    pub element_type: WasmTableElementType,
    pub minimum_elements: u32,
    pub maximum_elements: Option<u32>,
}

/// Static table declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmTableDescriptor {
    pub index: WasmTableIndex,
    pub element_type: WasmTableElementType,
    pub minimum_elements: u32,
    pub maximum_elements: Option<u32>,
}

/// Table element family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmTableElementType {
    ExternRef,
    FuncRef,
    AnyRef,
    EqRef,
    ConcreteHeapType(u32),
}

/// Instance-local cached table slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmTableCacheSlot {
    pub instance: WasmInstanceId,
    pub table: WasmTableId,
    pub index: WasmTableIndex,
    pub generation: u64,
    pub cached_length: u32,
}

/// Stable identity for compiled callee groups.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmCalleeGroupId(pub u64);

/// Reserved compiled-callee set for a module and memory mode.
#[derive(Clone, Debug)]
pub struct WasmCalleeGroup {
    pub id: WasmCalleeGroupId,
    pub module: WasmModuleId,
    pub memory: Option<WasmMemoryId>,
    pub compilation_state: WasmCalleeGroupState,
    pub callees: Vec<WasmCalleeDescriptor>,
    pub js_to_wasm: Vec<JsToWasmBridge>,
    pub wasm_to_js: Vec<WasmToJsBridge>,
}

/// Callee-group lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmCalleeGroupState {
    CreatedFromInterpreter,
    Compiling,
    Runnable,
    Failed,
    Invalidated,
}

/// Reserved callee descriptor for interpreter, BBQ, OMG, or bridge code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmCalleeDescriptor {
    pub function: WasmFunctionIndex,
    pub tier: JitType,
    pub entrypoint_code: Option<JitCodeId>,
    pub replacement_for: Option<JitCodeId>,
    pub may_be_replaced: bool,
}

/// Indirect call table entry metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmIndirectCallEntry {
    pub table: WasmTableId,
    pub function: Option<WasmFunctionIndex>,
    pub target_instance: Option<WasmInstanceId>,
    pub entrypoint_code: Option<JitCodeId>,
}
