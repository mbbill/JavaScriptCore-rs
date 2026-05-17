use crate::bytecode::code_block::{BytecodeIndex, RuntimeSlot};
use crate::bytecode::ic::CallLinkInfo;
use crate::bytecode::opcode::Opcode;
use crate::bytecode::origin::CodeOrigin;
use crate::bytecode::register::VirtualRegister;

/// Declarative registry of LLInt slow paths referenced from generated code.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LLIntSlowPathRegistry {
    pub paths: Vec<LLIntSlowPath>,
    pub helpers: Vec<LLIntHelperPath>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LLIntSlowPath {
    pub id: LLIntSlowPathId,
    pub symbol: &'static str,
    pub kind: LLIntSlowPathKind,
    pub signature: LLIntSlowPathSignature,
    pub origin_policy: SlowPathOriginPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct LLIntSlowPathId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LLIntSlowPathKind {
    Trace,
    EntryOsr,
    LoopOsr,
    Replacement,
    ObjectAllocation,
    ArrayAllocation,
    RegExpAllocation,
    PropertyAccess,
    PrivateName,
    PrivateBrand,
    Iterator,
    Branch,
    Compare,
    Switch,
    FunctionAllocation,
    VarargsFrame,
    Call,
    DirectEval,
    ArgumentsObject,
    StringConcat,
    Conversion,
    Throw,
    Trap,
    Debug,
    Exception,
    Scope,
    CatchProfile,
    ShadowChicken,
    OutOfLineJumpTarget,
    ArityCheck,
    CheckpointOsrExit,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LLIntSlowPathSignature {
    pub parameters: Vec<LLIntSlowPathParameter>,
    pub result: LLIntSlowPathResult,
    pub abi: LLIntAbi,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LLIntSlowPathParameter {
    CallFrame,
    ProgramCounter,
    Vm,
    ProtoCallFrame,
    NewStackPointer,
    EncodedValue,
    VirtualRegister(VirtualRegister),
    OperandIndex(i32),
    CallLinkInfo,
    Cell,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LLIntSlowPathResult {
    #[default]
    UGeneralPurposePair,
    Void,
    NoReturn,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LLIntAbi {
    #[default]
    SysV,
    CLoop,
    PlatformDefault,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum SlowPathOriginPolicy {
    #[default]
    CurrentBytecode,
    CurrentCheckpoint,
    CallLink(CodeOrigin),
    None,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LLIntHelperPath {
    pub symbol: &'static str,
    pub purpose: LLIntHelperPurpose,
    pub signature: LLIntSlowPathSignature,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LLIntHelperPurpose {
    TraceOperand,
    TraceValue,
    DefaultCall,
    VirtualCall,
    PolymorphicCall,
    WriteBarrier,
    StackCheck,
    VmEntryPermission,
    Crash,
}

/// Metadata passed by generated code when a slow path is entered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LLIntSlowPathCallSite {
    pub bytecode_index: BytecodeIndex,
    pub opcode: Opcode,
    pub origin: CodeOrigin,
    pub metadata_slot: Option<RuntimeSlot>,
    pub call_link_info: Option<CallLinkInfo>,
}
