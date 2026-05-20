//! Public embedding API contracts.
//!
//! This module defines opaque handles, API entry discipline, protection/rooting
//! placeholders, callback metadata, and exception bridging. It does not expose
//! Rust ownership of VM internals across the embedding boundary.

mod callbacks;
mod exception;
mod handles;
mod lock;
mod protect;

pub use callbacks::{
    ApiCallbackDescriptor, ApiCallbackKind, ApiCallbackObject, ApiCallbackPrivatePropertyMap,
    ApiCallbackResultKind, ApiCallbackSemanticError, ApiCallbackSemanticOutcome, ApiClass,
    ApiClassAttributes, ApiClassBuilder, ApiClassCallbacks, ApiClassContextData,
    ApiClassDescriptor, ApiClassDescriptorBuilder, ApiClassLifecycle, ApiClassPrototypePolicy,
    ApiClassPrototypeSchema, ApiClassSchemaRegistry, ApiClassSelection, ApiClassStaticTable,
    ApiClassValidationError, ApiConvertToTypeCallback, ApiDeletePropertyCallback,
    ApiDescriptorSelectionError, ApiFinalizeCallback, ApiGetPropertyCallback,
    ApiGetPropertyNamesCallback, ApiHasInstanceCallback, ApiHasPropertyCallback,
    ApiHostConstructor, ApiHostFunction, ApiInitializeCallback, ApiObjectCreationKind,
    ApiPrivateDataAuthority, ApiPropertyAttributes, ApiPropertyFallback,
    ApiRegistryMutationAuthority, ApiSchemaOwner, ApiSchemaProvenance, ApiSetPropertyCallback,
    ApiStaticFunction, ApiStaticMemberDescriptor, ApiStaticMemberKind, ApiStaticMemberSelection,
    ApiStaticValue, ApiValueType, API_CLASS_CALLBACK_DESCRIPTORS, API_CLASS_SCHEMA_REGISTRY,
};
pub use exception::{
    ApiExceptionResult, ApiExceptionSemanticError, ApiExceptionSlot, ApiExceptionSlotPolicy,
    ApiExceptionSlotUpdate, ApiExecutionDiagnosticSummary, ApiExecutionResultKind,
    ApiExecutionResultRecord, ApiGcDiagnosticSummary, ApiGcEventResultKind, ApiGcEventResultRecord,
    ApiOperationResult, ApiThrowDisposition, ApiTierDiagnosticSummary,
};
pub use handles::{
    ApiCastContract, ApiCastDirection, ApiClassRef, ApiContextGroup, ApiContextRef,
    ApiGlobalContext, ApiHandleKind, ApiInternalHandle, ApiObjectRef, ApiOpaqueHandle,
    ApiPropertyNameAccumulatorRef, ApiPropertyNameArrayRef, ApiReferenceOwnership, ApiScriptRef,
    ApiScriptSourceLifetime, ApiStringRef, ApiStringStorageKind, ApiValueRef, ApiValueRefKind,
    ApiWeakRefState, ApiWeakValueRef,
};
pub use lock::{
    ApiEntryKind, ApiEntryObservationRecord, ApiEntryScope, ApiExitObservationRecord, ApiLock,
    ApiLockDropState, ApiLockPolicy, ApiLockState, ApiReentrancy,
};
pub use protect::{
    ApiHeapFinalizerRegistration, ApiProtectionAction, ApiProtectionCount, ApiProtectionOutcome,
    ApiProtectionOwner, ApiProtectionRegistry, ApiProtectionRegistryEntry,
    ApiProtectionRegistryError, ApiProtectionRootMutationOutcome, ApiProtectionSlot,
    ApiProtectionTarget, ProtectedValue,
};
