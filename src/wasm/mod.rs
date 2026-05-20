//! Deferred WebAssembly integration contracts for the Rust JavaScriptCore skeleton.
//!
//! This module reserves module-loader, object-model, bridge, memory/table/global,
//! validation, and compilation-plan attachment points. It does not parse,
//! compile, instantiate, or execute WebAssembly.

#![forbid(unsafe_code)]

pub(crate) mod bridge;
pub(crate) mod compile;
pub(crate) mod debugger;
pub(crate) mod execution;
pub(crate) mod instance;
pub(crate) mod memory;
pub(crate) mod module;
pub(crate) mod table;

pub use bridge::{
    BridgeAbi, BridgeConversionPolicy, BridgeEntrypoint, BridgeEntrypointOwner,
    BridgeExceptionPolicy, JsToWasmBridge, WasmExportedFunctionBridge, WasmFunctionSignature,
    WasmGlobalBridge, WasmImportedFunctionBridge, WasmToJsBridge, WasmValueType,
};
pub use compile::{
    describe_wasm_compilation_fallback, plan_wasm_compilation_tiers,
    validate_wasm_compilation_request, validate_wasm_compilation_tier_descriptors,
    validate_wasm_validation_request, wasm_compilation_tier_descriptor,
    wasm_compilation_tier_descriptors, WasmCompilationCancellation, WasmCompilationCompletionTask,
    WasmCompilationDiagnosticKind, WasmCompilationDiagnosticRecord, WasmCompilationError,
    WasmCompilationErrorState, WasmCompilationFallbackRecord, WasmCompilationFallbackTarget,
    WasmCompilationKind, WasmCompilationPlan, WasmCompilationPlanId, WasmCompilationPriority,
    WasmCompilationProduct, WasmCompilationRequest, WasmCompilationRequestBuilder,
    WasmCompilationState, WasmCompilationTier, WasmCompilationTierDescriptor,
    WasmCompilationValidationError, WasmCompilerMode, WasmRuntimeMemoryMode, WasmTierPlan,
    WasmTierPlanningConfig, WasmValidationMode, WasmValidationProduct, WasmValidationRequest,
    WasmValidationRequestBuilder,
};
pub use debugger::{
    WasmDebugBreakpoint, WasmDebugBreakpointKind, WasmDebugInfoState, WasmDebugLocation,
    WasmDebugServerDescriptor, WasmDebugServerState, WasmDebugTransport,
    WasmDebuggerInstanceRegistration, WasmModuleDebugInfo, WasmVirtualAddress,
};
pub use execution::{
    describe_wasm_execution_result, describe_wasm_export_invocation,
    describe_wasm_host_call_invocation, describe_wasm_import_invocation,
    describe_wasm_instance_entry_boundary, WasmBoundaryValueSlot, WasmCallBoundaryKind,
    WasmExecutionBoundaryError, WasmExecutionResultKind, WasmExecutionResultRecord,
    WasmExportInvocationDescriptor, WasmHostCallInvocationDescriptor,
    WasmImportInvocationDescriptor, WasmInstanceEntryBoundaryDescriptor,
    WasmInstanceEntryBoundaryRecord, WasmInstanceEntryKind, WasmRootBoundaryKind,
    WasmRootBoundaryRecord, WasmTrapKind, WasmTrapRecord,
};
pub use instance::{
    describe_wasm_link_plan_semantics, describe_wasm_link_state_semantics, WasmExportBinding,
    WasmGlobalId, WasmGlobalMutability, WasmGlobalObject, WasmGlobalStorage, WasmImportBinding,
    WasmImportLinkStatus, WasmInstanceAnchorDescriptor, WasmInstanceExportKind, WasmInstanceId,
    WasmInstanceLifecycleEvent, WasmInstanceObject, WasmLinkPlan, WasmLinkPlanSemanticDescriptor,
    WasmLinkState, WasmLinkStateSemanticDescriptor, WasmRuntimeObjectAllocationState,
};
pub use memory::{
    WasmAddressType, WasmGlobalKind, WasmMemoryCacheSlot, WasmMemoryDescriptor,
    WasmMemoryGrowCallback, WasmMemoryGrowthState, WasmMemoryHandleDescriptor, WasmMemoryId,
    WasmMemoryIndex, WasmMemoryObject, WasmMemorySharing, WasmMemoryStyle,
};
pub use module::{
    derive_wasm_module_required_features, describe_wasm_export_semantics,
    describe_wasm_import_semantics, describe_wasm_module_linking_semantics,
    describe_wasm_validation_semantics, plan_wasm_module_validation,
    validate_wasm_module_enabled_features, validate_wasm_module_info,
    validate_wasm_module_schema_registry, wasm_feature_descriptor, wasm_module_feature_mask,
    wasm_module_schema_registry, wasm_section_descriptor, WasmArrayTypeDescriptor, WasmBranchHint,
    WasmBranchHintDescriptor, WasmCustomSectionDescriptor, WasmCustomSectionPurpose,
    WasmDataSegmentDescriptor, WasmDataSegmentIndex, WasmElementSegmentDescriptor,
    WasmElementSegmentIndex, WasmExportDescriptor, WasmExportIndex, WasmExportKind,
    WasmExportSemanticDescriptor, WasmFeatureDescriptor, WasmFeatureStatus, WasmFieldMutability,
    WasmFunctionCodeIndex, WasmFunctionIndex, WasmFunctionTypeDescriptor,
    WasmFunctionValidationSummary, WasmGlobalDescriptor, WasmGlobalIndex, WasmHeapType,
    WasmImportDescriptor, WasmImportIndex, WasmImportKind, WasmImportSemanticDescriptor,
    WasmJsWrapperDescriptor, WasmJsWrapperKind, WasmModuleFeature, WasmModuleId, WasmModuleInfo,
    WasmModuleInfoBuilder, WasmModuleLinkingSemanticDescriptor, WasmModuleObject, WasmModuleRecord,
    WasmModuleRecordState, WasmModuleSchemaRegistry, WasmModuleValidationError,
    WasmModuleValidationPlan, WasmPackedType, WasmRegistryAuthority, WasmSection,
    WasmSectionDescriptor, WasmSourceKind, WasmStructFieldDescriptor, WasmStructTypeDescriptor,
    WasmTagDescriptor, WasmTagIndex, WasmTypeDescriptor, WasmTypeKind, WasmTypeSignatureIndex,
    WasmValidationSemanticOutcome, WasmValidationSemanticStatus, WasmValidationState,
    WASM_MODULE_SCHEMA_REGISTRY,
};
pub use table::{
    WasmCalleeDescriptor, WasmCalleeGroup, WasmCalleeGroupId, WasmCalleeGroupState,
    WasmDirectCallerSet, WasmFuncRefTableEntry, WasmIndirectCallEntry, WasmOptimizedCalleeSlot,
    WasmTableCacheSlot, WasmTableDescriptor, WasmTableElementType, WasmTableId, WasmTableIndex,
    WasmTableObject, WasmTableStorageKind,
};
