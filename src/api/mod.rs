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
    ApiCallbackObject, ApiCallbackPrivatePropertyMap, ApiClass, ApiClassAttributes,
    ApiClassCallbacks, ApiClassContextData, ApiClassPrototypePolicy, ApiClassStaticTable,
    ApiConvertToTypeCallback, ApiDeletePropertyCallback, ApiFinalizeCallback,
    ApiGetPropertyCallback, ApiGetPropertyNamesCallback, ApiHasInstanceCallback,
    ApiHasPropertyCallback, ApiHostConstructor, ApiHostFunction, ApiInitializeCallback,
    ApiPropertyAttributes, ApiSetPropertyCallback, ApiStaticFunction, ApiStaticValue, ApiValueType,
};
pub use exception::{
    ApiExceptionResult, ApiExceptionSlot, ApiExceptionSlotPolicy, ApiOperationResult,
    ApiThrowDisposition,
};
pub use handles::{
    ApiCastContract, ApiCastDirection, ApiClassRef, ApiContextGroup, ApiContextRef,
    ApiGlobalContext, ApiHandleKind, ApiInternalHandle, ApiObjectRef, ApiOpaqueHandle,
    ApiPropertyNameAccumulatorRef, ApiPropertyNameArrayRef, ApiReferenceOwnership, ApiScriptRef,
    ApiStringRef, ApiValueRef, ApiValueRefKind, ApiWeakValueRef,
};
pub use lock::{
    ApiEntryKind, ApiEntryScope, ApiLock, ApiLockDropState, ApiLockPolicy, ApiLockState,
    ApiReentrancy,
};
pub use protect::{
    ApiProtectionAction, ApiProtectionCount, ApiProtectionOwner, ApiProtectionSlot, ProtectedValue,
};
