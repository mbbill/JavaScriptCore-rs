//! Disassembly metadata for generated code diagnostics.
//!
//! Disassembly is optional side data. The real disassembler owns instruction
//! decoding, symbol lookup, and formatting; this module records how decoded
//! output can be associated with code origins and patchpoints.

use crate::jit::{CodeOrigin, JitCodeId, MachineCodeRange, PatchpointDescriptor};

/// Decoder or source of disassembly records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisassemblySource {
    JitDisassembler,
    AirDisassembler,
    DfgDisassembler,
    YarrDisassembler,
    ExternalTool,
}

/// Formatting target for diagnostic output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisassemblyFormat {
    PlainText,
    Json,
    PerfMap,
    GdbJit,
}

/// Section within generated code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisassemblySection {
    Prologue,
    Body,
    SlowPath,
    OsrExit,
    ExceptionHandler,
    InlineCacheStub,
    Data,
}

/// Annotation attached to an instruction range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisassemblyAnnotation {
    pub section: DisassemblySection,
    pub origin: Option<CodeOrigin>,
    pub patchpoint: Option<PatchpointDescriptor>,
    pub label: Option<&'static str>,
}

/// One decoded instruction or data directive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisassemblyInstruction {
    pub offset: u32,
    pub size_bytes: u8,
    pub text: Option<&'static str>,
    pub annotation: Option<DisassemblyAnnotation>,
}

/// Disassembly metadata associated with a code artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisassemblyMetadata {
    pub code: JitCodeId,
    pub source: DisassemblySource,
    pub format: DisassemblyFormat,
    pub range: Option<MachineCodeRange>,
    pub instructions: Vec<DisassemblyInstruction>,
}
