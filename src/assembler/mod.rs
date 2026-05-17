//! Assembler and MacroAssembler contracts.
//!
//! JSC treats assembler buffers, labels, jumps, calls, relocations, and link
//! buffers as a substrate shared by LLInt, baseline JIT, DFG, FTL, Yarr, and
//! Wasm. This module names those ownership boundaries without emitting bytes.

use crate::jit::{CodePatchPlan, ExecutableAllocationId};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct AssemblerBufferId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct AssemblerLabel(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct AssemblerJumpId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssemblerArchitecture {
    X86,
    X86_64,
    Arm64,
    Riscv64,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssemblerRelocationKind {
    CodeLabel,
    DataLabel,
    NearCall,
    FarCall,
    Jump,
    AbsolutePointer,
    ExternalReference,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssemblerRelocation {
    pub kind: AssemblerRelocationKind,
    pub at_offset: u32,
    pub target: Option<AssemblerLabel>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AssemblerBufferDescriptor {
    pub id: AssemblerBufferId,
    pub architecture: Option<AssemblerArchitecture>,
    pub byte_len: u32,
    pub labels: Vec<AssemblerLabel>,
    pub jumps: Vec<AssemblerJumpId>,
    pub relocations: Vec<AssemblerRelocation>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinkBufferPlan {
    pub source: AssemblerBufferId,
    pub allocation: Option<ExecutableAllocationId>,
    pub patches: Vec<CodePatchPlan>,
}
