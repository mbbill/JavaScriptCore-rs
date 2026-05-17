//! Fuzzilli instrumentation contracts.
//!
//! Fuzzilli is a testing and coverage boundary. It must never become part of
//! normal execution ownership; this module only names instrumentation hooks and
//! coverage records.

use crate::bytecode::BytecodeIndex;
use crate::runtime::CodeBlockId;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct FuzzilliCoverageSiteId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FuzzilliEventKind {
    BasicBlock,
    Edge,
    Compare,
    BuiltinCall,
    WasmOperation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FuzzilliCoverageSite {
    pub id: FuzzilliCoverageSiteId,
    pub owner: Option<CodeBlockId>,
    pub bytecode_index: Option<BytecodeIndex>,
    pub kind: FuzzilliEventKind,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FuzzilliInstrumentationPlan {
    pub sites: Vec<FuzzilliCoverageSite>,
    pub expose_reprl_hooks: bool,
    pub deterministic_gc: bool,
}
