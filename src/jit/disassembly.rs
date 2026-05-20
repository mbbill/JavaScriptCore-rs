//! Disassembly metadata for generated code diagnostics.
//!
//! Disassembly is optional side data. The real disassembler owns instruction
//! decoding, symbol lookup, and formatting; this module records how decoded
//! output can be associated with code origins and patchpoints.

use crate::bytecode::BytecodeIndex;
use crate::jit::{
    CallBoundaryId, CodeOrigin, EffectSummary, InlineCacheSlotId, JitCodeId, JitType,
    MachineCodeRange, PatchpointDescriptor, TierFallbackReason,
};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisassemblySemanticKind {
    TierEntry,
    InlineCache,
    SpeculationCheck,
    OsrExit,
    SlowPath,
    ExceptionEdge,
    DomJitBoundary,
    DeoptContinuation,
    Data,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisassemblySemanticAnnotation {
    pub kind: DisassemblySemanticKind,
    pub effects: EffectSummary,
    pub semantic_id: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisassemblyExecutionKind {
    InterpreterEntry,
    SlowPathBoundary,
    TierFallback,
    InlineCacheMiss,
    ExceptionUnwind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisassemblyExecutionAnnotation {
    pub kind: DisassemblyExecutionKind,
    pub bytecode_index: Option<BytecodeIndex>,
    pub boundary: Option<CallBoundaryId>,
    pub tier: Option<JitType>,
    pub fallback_reason: Option<TierFallbackReason>,
    pub inline_cache_slot: Option<InlineCacheSlotId>,
}

/// Annotation attached to an instruction range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisassemblyAnnotation {
    pub section: DisassemblySection,
    pub origin: Option<CodeOrigin>,
    pub patchpoint: Option<PatchpointDescriptor>,
    pub label: Option<&'static str>,
    pub semantic: Option<DisassemblySemanticAnnotation>,
    pub execution: Option<DisassemblyExecutionAnnotation>,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisassemblyMetadataValidationError {
    EmptyInstruction,
    InstructionOffsetsNotMonotonic,
    InstructionOutsideRange,
    PatchpointOwnerMismatch,
    EmptyRange,
    SemanticSectionMismatch,
    ExecutionSectionMismatch,
    ExecutionDetailMissing,
}

impl DisassemblyMetadata {
    pub fn validate(&self) -> Result<(), DisassemblyMetadataValidationError> {
        if let Some(range) = self.range {
            if range.size_bytes == 0 {
                return Err(DisassemblyMetadataValidationError::EmptyRange);
            }
        }

        let mut previous = None;
        for instruction in &self.instructions {
            instruction.validate(self.code)?;
            if previous.is_some_and(|offset| instruction.offset < offset) {
                return Err(DisassemblyMetadataValidationError::InstructionOffsetsNotMonotonic);
            }
            if let Some(range) = self.range {
                let end = instruction
                    .offset
                    .saturating_add(instruction.size_bytes as u32);
                if instruction.offset < range.start_offset
                    || end > range.start_offset.saturating_add(range.size_bytes)
                {
                    return Err(DisassemblyMetadataValidationError::InstructionOutsideRange);
                }
            }
            previous = Some(instruction.offset);
        }

        Ok(())
    }
}

impl DisassemblyInstruction {
    pub fn validate(&self, code: JitCodeId) -> Result<(), DisassemblyMetadataValidationError> {
        if self.size_bytes == 0 {
            return Err(DisassemblyMetadataValidationError::EmptyInstruction);
        }
        if let Some(annotation) = &self.annotation {
            annotation.validate()?;
            if annotation
                .patchpoint
                .is_some_and(|patchpoint| patchpoint.owner_code != Some(code))
            {
                return Err(DisassemblyMetadataValidationError::PatchpointOwnerMismatch);
            }
        }

        Ok(())
    }
}

impl DisassemblyAnnotation {
    pub fn validate(&self) -> Result<(), DisassemblyMetadataValidationError> {
        if let Some(semantic) = self.semantic {
            match (self.section, semantic.kind) {
                (DisassemblySection::OsrExit, DisassemblySemanticKind::OsrExit)
                | (DisassemblySection::ExceptionHandler, DisassemblySemanticKind::ExceptionEdge)
                | (DisassemblySection::InlineCacheStub, DisassemblySemanticKind::InlineCache)
                | (DisassemblySection::SlowPath, DisassemblySemanticKind::SlowPath)
                | (DisassemblySection::Data, DisassemblySemanticKind::Data) => {}
                (
                    DisassemblySection::Body | DisassemblySection::Prologue,
                    DisassemblySemanticKind::TierEntry
                    | DisassemblySemanticKind::SpeculationCheck
                    | DisassemblySemanticKind::DomJitBoundary
                    | DisassemblySemanticKind::DeoptContinuation,
                ) => {}
                _ => return Err(DisassemblyMetadataValidationError::SemanticSectionMismatch),
            }
        }
        if let Some(execution) = self.execution {
            execution.validate_for_section(self.section)?;
        }

        Ok(())
    }
}

impl DisassemblyExecutionAnnotation {
    pub fn validate_for_section(
        self,
        section: DisassemblySection,
    ) -> Result<(), DisassemblyMetadataValidationError> {
        match (section, self.kind) {
            (
                DisassemblySection::Prologue | DisassemblySection::Body,
                DisassemblyExecutionKind::InterpreterEntry,
            )
            | (DisassemblySection::SlowPath, DisassemblyExecutionKind::SlowPathBoundary)
            | (
                DisassemblySection::Body
                | DisassemblySection::SlowPath
                | DisassemblySection::OsrExit,
                DisassemblyExecutionKind::TierFallback,
            )
            | (
                DisassemblySection::InlineCacheStub | DisassemblySection::SlowPath,
                DisassemblyExecutionKind::InlineCacheMiss,
            )
            | (
                DisassemblySection::ExceptionHandler | DisassemblySection::OsrExit,
                DisassemblyExecutionKind::ExceptionUnwind,
            ) => {}
            _ => return Err(DisassemblyMetadataValidationError::ExecutionSectionMismatch),
        }

        if self.kind == DisassemblyExecutionKind::TierFallback && self.fallback_reason.is_none() {
            return Err(DisassemblyMetadataValidationError::ExecutionDetailMissing);
        }
        if self.kind == DisassemblyExecutionKind::InlineCacheMiss
            && self.inline_cache_slot.is_none()
        {
            return Err(DisassemblyMetadataValidationError::ExecutionDetailMissing);
        }
        if matches!(
            self.kind,
            DisassemblyExecutionKind::SlowPathBoundary | DisassemblyExecutionKind::InlineCacheMiss
        ) && self.boundary.is_none()
        {
            return Err(DisassemblyMetadataValidationError::ExecutionDetailMissing);
        }

        Ok(())
    }
}
