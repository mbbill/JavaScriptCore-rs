//! Disassembler contracts.
//!
//! Disassembly is diagnostic infrastructure for generated code. It must observe
//! code ranges and metadata without owning executable memory or decoding
//! semantics into behavior.

use crate::jit::{JitCodeId, JitType};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisassemblerBackend {
    Arm64,
    Zydis,
    TextOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisassemblyRequest {
    pub code: JitCodeId,
    pub tier: JitType,
    pub backend: DisassemblerBackend,
    pub include_source: bool,
    pub include_relocations: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DisassemblyLine {
    pub offset: u32,
    pub text_ordinal: u32,
    pub annotation_ordinal: Option<u32>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DisassemblyReport {
    pub lines: Vec<DisassemblyLine>,
    pub truncated: bool,
}
