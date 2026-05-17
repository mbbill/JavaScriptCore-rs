//! Deferred WebAssembly integration contracts for the Rust JavaScriptCore skeleton.
//!
//! This module reserves module-loader, object-model, bridge, memory/table/global,
//! and compilation-plan attachment points. It does not parse, validate, compile,
//! instantiate, or execute WebAssembly.

#![forbid(unsafe_code)]

pub(crate) mod bridge;
pub(crate) mod compile;
pub(crate) mod debugger;
pub(crate) mod instance;
pub(crate) mod memory;
pub(crate) mod module;
pub(crate) mod table;

pub use bridge::{
    BridgeAbi, BridgeConversionPolicy, BridgeEntrypoint, BridgeExceptionPolicy, JsToWasmBridge,
    WasmExportedFunctionBridge, WasmFunctionSignature, WasmGlobalBridge,
    WasmImportedFunctionBridge, WasmToJsBridge, WasmValueType,
};
pub use compile::{
    WasmCompilationCancellation, WasmCompilationKind, WasmCompilationPlan, WasmCompilationPlanId,
    WasmCompilationPriority, WasmCompilationProduct, WasmCompilationRequest, WasmCompilationState,
    WasmCompilerMode, WasmValidationMode, WasmValidationProduct, WasmValidationRequest,
};
pub use debugger::{
    WasmDebugBreakpoint, WasmDebugBreakpointKind, WasmDebugInfoState, WasmDebugLocation,
    WasmDebugServerDescriptor, WasmDebugServerState, WasmDebugTransport,
    WasmDebuggerInstanceRegistration, WasmModuleDebugInfo, WasmVirtualAddress,
};
pub use instance::{
    WasmExportBinding, WasmGlobalId, WasmGlobalMutability, WasmGlobalObject, WasmGlobalStorage,
    WasmImportBinding, WasmImportLinkStatus, WasmInstanceExportKind, WasmInstanceId,
    WasmInstanceLifecycleEvent, WasmInstanceObject, WasmLinkPlan, WasmLinkState,
};
pub use memory::{
    WasmAddressType, WasmGlobalKind, WasmMemoryCacheSlot, WasmMemoryDescriptor,
    WasmMemoryGrowthState, WasmMemoryId, WasmMemoryIndex, WasmMemoryObject, WasmMemorySharing,
    WasmMemoryStyle,
};
pub use module::{
    WasmExportDescriptor, WasmExportIndex, WasmExportKind, WasmFunctionCodeIndex,
    WasmFunctionIndex, WasmImportDescriptor, WasmImportIndex, WasmImportKind,
    WasmJsWrapperDescriptor, WasmJsWrapperKind, WasmModuleFeature, WasmModuleId, WasmModuleInfo,
    WasmModuleObject, WasmModuleRecord, WasmModuleRecordState, WasmSourceKind, WasmValidationState,
};
pub use table::{
    WasmCalleeDescriptor, WasmCalleeGroup, WasmCalleeGroupId, WasmCalleeGroupState,
    WasmIndirectCallEntry, WasmTableCacheSlot, WasmTableDescriptor, WasmTableElementType,
    WasmTableId, WasmTableIndex, WasmTableObject,
};
