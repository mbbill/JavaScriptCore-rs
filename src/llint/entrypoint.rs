use crate::bytecode::code_block::{CodeBlockEntrypoints, InterpreterEntrySlot};
use crate::llint::dispatch::{LLIntCodePtr, OpcodeSizeClass};

/// LLInt entrypoint set installed on a linked code block.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LLIntEntrypointTable {
    pub call: Option<LLIntEntrypoint>,
    pub construct: Option<LLIntEntrypoint>,
    pub arity_check_call: Option<LLIntEntrypoint>,
    pub arity_check_construct: Option<LLIntEntrypoint>,
    pub return_points: Vec<LLIntReturnPoint>,
    pub thunks: LLIntThunkSet,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LLIntEntrypoint {
    pub kind: LLIntEntrypointKind,
    pub slot: InterpreterEntrySlot,
    pub code: Option<LLIntCodePtr>,
    pub frame_register_count: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LLIntEntrypointKind {
    Program,
    Eval,
    Module,
    FunctionForCall,
    FunctionForConstruct,
    HostCallReturnValue,
    FuzzerReturnEarlyFromLoopHint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LLIntReturnPoint {
    pub opcode_size: OpcodeSizeClass,
    pub code: LLIntCodePtr,
    pub purpose: LLIntReturnPointPurpose,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LLIntReturnPointPurpose {
    Generic,
    ExceptionCatch,
    ExceptionUncaught,
    CheckpointOsrExit,
    ArraySortComparator,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct LLIntThunkSet {
    pub default_call: Option<LLIntCodePtr>,
    pub arity_fixup: Option<LLIntCodePtr>,
    pub handle_uncaught_exception: Option<LLIntCodePtr>,
    pub call_to_throw: Option<LLIntCodePtr>,
}

/// Pending entrypoint installation request.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LLIntEntrypointInstall {
    pub entrypoints: CodeBlockEntrypoints,
    pub table: LLIntEntrypointTable,
    pub frame_register_count: Option<u32>,
}
