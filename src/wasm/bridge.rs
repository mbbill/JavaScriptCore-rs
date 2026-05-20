//! JS/Wasm bridge attachment points.
//!
//! Future bridge code owns ABI-sensitive entrypoints, generated thunks,
//! import/export wrappers, exception translation, and value conversion. This
//! skeleton only names those boundaries.

use crate::jit::{CallBoundaryId, EntryAbi, Entrypoint, JitCodeId};
use crate::runtime::{HostHookId, ObjectId};
use crate::wasm::{WasmFunctionIndex, WasmGlobalKind, WasmInstanceId, WasmModuleId};

/// ABI family reserved for JS/Wasm calls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeAbi {
    Deferred,
    JsToWasm,
    JsToWasmInlineCache,
    WasmToJs,
    WasmToWasm,
    WasmBuiltin,
    WasmToHost,
}

/// JavaScript-to-Wasm call bridge descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsToWasmBridge {
    pub abi: BridgeAbi,
    pub module: WasmModuleId,
    pub function: WasmFunctionIndex,
    pub entry: BridgeEntrypoint,
    pub signature: WasmFunctionSignature,
    pub conversion: BridgeConversionPolicy,
}

/// Wasm-to-JavaScript call bridge descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmToJsBridge {
    pub abi: BridgeAbi,
    pub instance: Option<WasmInstanceId>,
    pub imported_function: WasmFunctionIndex,
    pub callee: Option<ObjectId>,
    pub host_hook: Option<HostHookId>,
    pub entry: BridgeEntrypoint,
    pub signature: WasmFunctionSignature,
    pub exception_policy: BridgeExceptionPolicy,
}

/// Symbolic bridge entry metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeEntrypoint {
    pub entry_slot: Option<u32>,
    pub boundary: Option<CallBoundaryId>,
    pub entrypoint: Option<Entrypoint>,
    pub code: Option<JitCodeId>,
    pub abi: EntryAbi,
}

/// Ownership of a bridge entrypoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeEntrypointOwner {
    Module,
    Instance,
    CalleeGroup,
    ImportWrapper,
    ExportWrapper,
    Host,
}

/// Wasm value type at a bridge boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmValueType {
    I32,
    I64,
    F32,
    F64,
    V128,
    ExternRef,
    FuncRef,
    EqRef,
    AnyRef,
    StructRef,
    ArrayRef,
}

/// Function signature metadata. It is not a validator or canonical type table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmFunctionSignature {
    pub params: Vec<WasmValueType>,
    pub results: Vec<WasmValueType>,
    pub module_type_index: Option<u32>,
    pub canonical_type_index: Option<crate::wasm::WasmTypeSignatureIndex>,
}

/// Conversion policy for JS values entering or leaving Wasm.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeConversionPolicy {
    StrictWasmJs,
    ImportObjectCoercion,
    ExportWrapper,
    HostFunction,
    Deferred,
}

/// Exception/trap behavior at the bridge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeExceptionPolicy {
    PropagateJsException,
    ConvertTrapToRuntimeError,
    PreserveWasmException,
    HostBoundary,
}

/// Exported JS wrapper for a Wasm function.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmExportedFunctionBridge {
    pub module: WasmModuleId,
    pub function: WasmFunctionIndex,
    pub wrapper_object: Option<ObjectId>,
    pub js_to_wasm: JsToWasmBridge,
}

/// Imported JS callable slot used by Wasm.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmImportedFunctionBridge {
    pub instance: WasmInstanceId,
    pub function: WasmFunctionIndex,
    pub callable: Option<ObjectId>,
    pub wasm_to_js: WasmToJsBridge,
}

/// Global bridge metadata for JS-visible WebAssembly.Global wrappers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmGlobalBridge {
    pub global_object: Option<ObjectId>,
    pub kind: WasmGlobalKind,
    pub mutable: bool,
}
