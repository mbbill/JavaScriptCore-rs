use crate::bytecode::debug::{ExceptionHandlerRecord, ExceptionTarget};
use crate::bytecode::opcode::OperandWidth;
use crate::llint::dispatch::{LLIntCodePtr, OpcodeSizeClass};

/// Exception routing targets owned by LLInt.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LLIntExceptionTable {
    pub return_to_throw: Option<LLIntExceptionInstruction>,
    pub call_to_throw: Option<LLIntCodePtr>,
    pub handle_uncaught_exception: Option<LLIntCodePtr>,
    pub catch_handlers: Vec<LLIntCatchHandler>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LLIntExceptionInstruction {
    pub base: LLIntCodePtr,
    pub advance_slots_min: u8,
    pub advance_slots_max: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LLIntCatchHandler {
    pub opcode_size: OpcodeSizeClass,
    pub handler: ExceptionHandlerRecord,
    pub target: ExceptionTarget,
    pub code: Option<LLIntCodePtr>,
}

/// Contract for resuming dispatch after an exception handler is selected.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LLIntExceptionResume {
    pub target: ExceptionTarget,
    pub operand_width: OperandWidth,
    pub clears_pending_exception: bool,
    pub profiles_catch_value: bool,
}
