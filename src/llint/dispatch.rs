use crate::bytecode::opcode::{Opcode, OpcodeId, OperandWidth};

/// Width-specific opcode dispatch maps populated by offlineasm or C-loop setup.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LLIntOpcodeMaps {
    pub narrow: Vec<LLIntDispatchEntry>,
    pub wide16: Vec<LLIntDispatchEntry>,
    pub wide32: Vec<LLIntDispatchEntry>,
    pub bases: LLIntDispatchBases,
    /// OfflineASM and C-loop setup own population of dispatch maps. Runtime
    /// dispatch observes these entries but must not rewrite them.
    pub authority: LLIntDispatchAuthority,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LLIntDispatchAuthority {
    #[default]
    OfflineAsmGenerated,
    CLoopGenerated,
    LinkTimeInstalled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LLIntDispatchEntry {
    pub opcode_id: OpcodeId,
    pub opcode: Opcode,
    pub width: OperandWidth,
    pub target: LLIntCodePtr,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct LLIntDispatchBases {
    pub bytecode: Option<LLIntCodePtr>,
    pub gc: Option<LLIntCodePtr>,
    pub conversion: Option<LLIntCodePtr>,
    pub simd: Option<LLIntCodePtr>,
    pub atomic: Option<LLIntCodePtr>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct LLIntCodePtr(pub u64);

/// Register contract used by generated LLInt code.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct LLIntRegisterContract {
    pub pc: LLIntRegister,
    pub payload_base: LLIntRegister,
    pub metadata_table: LLIntRegister,
    pub call_frame: LLIntRegister,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LLIntRegister {
    #[default]
    Unassigned,
    Gpr(u8),
    Fpr(u8),
    StackSlot(i32),
}

/// Bytecode-size class used by generic return points and exception handlers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum OpcodeSizeClass {
    Narrow,
    Wide16,
    Wide32,
}
