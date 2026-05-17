//! OfflineASM contracts.
//!
//! OfflineASM describes LLInt and thunk assembly in a portable DSL. This module
//! names parser products, lowering targets, and generated labels without
//! interpreting the DSL.

use crate::assembler::{AssemblerArchitecture, AssemblerLabel};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct OfflineAsmProgramId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OfflineAsmOpcodeKind {
    Macro,
    Instruction,
    SlowPath,
    CommonThunk,
    PlatformGuard,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfflineAsmLoweringPlan {
    pub program: OfflineAsmProgramId,
    pub target: AssemblerArchitecture,
    pub entry_label: Option<AssemblerLabel>,
    pub emits_cfi: bool,
    pub emits_metadata_table: bool,
}
