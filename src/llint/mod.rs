//! Low-Level Interpreter contracts.
//!
//! LLInt and offlineasm-generated entrypoints are planned as a boundary around
//! bytecode dispatch, slow paths, and call-frame layout. This module reserves
//! that boundary without an interpreter loop.

pub(crate) mod dispatch;
pub(crate) mod entrypoint;
pub(crate) mod exception;
pub(crate) mod handoff;
pub(crate) mod slow_path;

pub use dispatch::{
    LLIntCodePtr, LLIntDispatchAuthority, LLIntDispatchBases, LLIntDispatchEntry, LLIntOpcodeMaps,
    LLIntRegister, LLIntRegisterContract, OpcodeSizeClass,
};
pub use entrypoint::{
    select_llint_entrypoint_kinds, select_llint_entrypoint_table, LLIntEntrypoint,
    LLIntEntrypointInstall, LLIntEntrypointKind, LLIntEntrypointRegistryMutationAuthority,
    LLIntEntrypointSchemaOwner, LLIntEntrypointSchemaRegistry, LLIntEntrypointState,
    LLIntEntrypointTable, LLIntEntrypointValidationError, LLIntReturnPoint,
    LLIntReturnPointPurpose, LLIntThunkSet, StaticLLIntEntrypointSchema,
    LLINT_ENTRYPOINT_SCHEMA_REGISTRY, STATIC_LLINT_ENTRYPOINT_SCHEMAS,
};
pub use exception::{
    LLIntCatchHandler, LLIntExceptionInstruction, LLIntExceptionResume, LLIntExceptionTable,
};
pub use handoff::{
    select_llint_entry_for_interpreter_frame, LLIntInterpreterEntryHandoff,
    LLIntInterpreterHandoffError, LLIntInterpreterHandoffReason,
};
pub use slow_path::{
    LLIntAbi, LLIntHelperPath, LLIntHelperPurpose, LLIntSlowPath, LLIntSlowPathBoundaryKind,
    LLIntSlowPathBoundaryRecord, LLIntSlowPathCallSite, LLIntSlowPathId, LLIntSlowPathKind,
    LLIntSlowPathParameter, LLIntSlowPathRegistry, LLIntSlowPathRegistryMutationAuthority,
    LLIntSlowPathResult, LLIntSlowPathResume, LLIntSlowPathSchemaOwner, LLIntSlowPathSignature,
    LLIntSlowPathValidationError, SlowPathOriginPolicy, StaticLLIntSlowPathRegistry,
    StaticLLIntSlowPathSchema, STATIC_LLINT_SLOW_PATH_REGISTRY, STATIC_LLINT_SLOW_PATH_SCHEMAS,
};
