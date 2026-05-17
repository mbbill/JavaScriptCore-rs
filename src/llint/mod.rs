//! Low-Level Interpreter contracts.
//!
//! LLInt and offlineasm-generated entrypoints are planned as a boundary around
//! bytecode dispatch, slow paths, and call-frame layout. This module reserves
//! that boundary without an interpreter loop.

pub(crate) mod dispatch;
pub(crate) mod entrypoint;
pub(crate) mod exception;
pub(crate) mod slow_path;

pub use dispatch::{
    LLIntCodePtr, LLIntDispatchBases, LLIntDispatchEntry, LLIntOpcodeMaps, LLIntRegister,
    LLIntRegisterContract, OpcodeSizeClass,
};
pub use entrypoint::{
    LLIntEntrypoint, LLIntEntrypointInstall, LLIntEntrypointKind, LLIntEntrypointTable,
    LLIntReturnPoint, LLIntReturnPointPurpose, LLIntThunkSet,
};
pub use exception::{
    LLIntCatchHandler, LLIntExceptionInstruction, LLIntExceptionResume, LLIntExceptionTable,
};
pub use slow_path::{
    LLIntAbi, LLIntHelperPath, LLIntHelperPurpose, LLIntSlowPath, LLIntSlowPathCallSite,
    LLIntSlowPathId, LLIntSlowPathKind, LLIntSlowPathParameter, LLIntSlowPathRegistry,
    LLIntSlowPathResult, LLIntSlowPathSignature, SlowPathOriginPolicy,
};
