use crate::bytecode::code_block::{
    BytecodeIndex, CallSiteIndex, SourcePosition, SourceProviderId, SourceRange,
};
use crate::runtime::CodeBlockId;

/// Opaque handle to an inline-call-frame descriptor owned by a future JIT tier.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct InlineCallFrameRef(pub u32);

/// Bytecode position plus optional inline stack context.
///
/// JSC packs `CodeOrigin` aggressively for generated code. The Rust skeleton
/// keeps the semantic shape visible while leaving pointer packing and lifetime
/// ownership to the JIT/runtime layers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CodeOrigin {
    pub bytecode_index: BytecodeIndex,
    pub inline_call_frame: Option<InlineCallFrameRef>,
}

impl CodeOrigin {
    pub const fn new(bytecode_index: BytecodeIndex) -> Self {
        Self {
            bytecode_index,
            inline_call_frame: None,
        }
    }

    pub const fn with_inline_call_frame(
        bytecode_index: BytecodeIndex,
        inline_call_frame: InlineCallFrameRef,
    ) -> Self {
        Self {
            bytecode_index,
            inline_call_frame: Some(inline_call_frame),
        }
    }

    pub const fn is_set(self) -> bool {
        self.bytecode_index.is_valid()
    }
}

/// Code origin resolved against the owning code block.
///
/// The code-block cell identity is owned by the runtime layer. Origin tables
/// borrow it only to qualify bytecode offsets for stack traces, debugging, and
/// deoptimization metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct FullCodeOrigin {
    pub code_block: CodeBlockId,
    pub origin: CodeOrigin,
}

/// One inlining edge in a generated-code inline stack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineCallFrameRecord {
    pub reference: InlineCallFrameRef,
    pub caller: CodeOrigin,
    pub callee: CodeBlockId,
    pub call_site: Option<CallSiteIndex>,
    pub source_range: Option<SourceRange>,
}

/// Side table used by debugger, profiler, stack traces, and deoptimization.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CodeOriginTable {
    pub inline_call_frames: Vec<InlineCallFrameRecord>,
    pub pc_mappings: Vec<ProgramCounterOrigin>,
    pub source_mappings: Vec<BytecodeSourceMapping>,
    pub semantic_mappings: SourceOriginSemanticMap,
}

impl CodeOriginTable {
    pub fn origin_for_pc_offset(&self, pc_offset: u32) -> Option<CodeOrigin> {
        self.pc_mappings
            .iter()
            .filter(|mapping| mapping.pc_offset <= pc_offset)
            .max_by_key(|mapping| mapping.pc_offset)
            .map(|mapping| mapping.origin)
    }

    pub fn source_mapping_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<&BytecodeSourceMapping> {
        self.source_mappings
            .iter()
            .find(|mapping| mapping.bytecode_index == bytecode_index)
    }

    pub fn execution_diagnostic_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<ExecutionDiagnosticMapping> {
        let source = self.source_mapping_for_bytecode_index(bytecode_index)?;
        let semantic = self
            .semantic_mappings
            .entry_for_bytecode_index(bytecode_index);
        Some(ExecutionDiagnosticMapping {
            bytecode_index,
            provider: source.provider,
            source_range: source.source_range,
            position_kind: source.position_kind,
            semantic_kind: semantic.map(|entry| entry.semantic_kind),
            strict: semantic.is_some_and(|entry| entry.strict),
            synthetic: semantic.is_some_and(|entry| entry.synthetic),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ProgramCounterOrigin {
    pub pc_offset: u32,
    pub origin: CodeOrigin,
    pub width: ProgramCounterMappingWidth,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ProgramCounterMappingWidth {
    Byte,
    NativeInstruction,
    ReturnPoint,
    SlowPath,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct BytecodeSourceMapping {
    pub bytecode_index: BytecodeIndex,
    pub provider: Option<SourceProviderId>,
    pub source_range: SourceRange,
    pub position_kind: SourcePositionKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SourcePositionKind {
    Expression,
    Statement,
    Call,
    Construct,
    Return,
    Throw,
    Synthetic,
}

/// Result shape for source-note lookups without committing to a lookup engine.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SourceNoteLookup {
    pub bytecode_index: BytecodeIndex,
    pub position: SourcePosition,
    pub range: SourceRange,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceOriginSemanticMap {
    pub entries: Vec<SourceOriginSemanticEntry>,
}

impl SourceOriginSemanticMap {
    pub fn entry_for_bytecode_index(
        &self,
        bytecode_index: BytecodeIndex,
    ) -> Option<&SourceOriginSemanticEntry> {
        self.entries
            .iter()
            .find(|entry| entry.bytecode_index == bytecode_index)
    }

    pub fn validate(&self) -> SourceOriginSemanticValidationReport {
        let mut findings = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if !entry.bytecode_index.is_valid() {
                findings.push(
                    SourceOriginSemanticValidationFinding::InvalidBytecodeIndex {
                        entry: index as u32,
                    },
                );
            }
            if source_position_after(entry.source_range.start, entry.source_range.end) {
                findings.push(
                    SourceOriginSemanticValidationFinding::UnorderedSourceRange {
                        entry: index as u32,
                        range: entry.source_range,
                    },
                );
            }
            if self.entries[..index]
                .iter()
                .any(|candidate| candidate.bytecode_index == entry.bytecode_index)
            {
                findings.push(
                    SourceOriginSemanticValidationFinding::DuplicateBytecodeIndex {
                        bytecode_index: entry.bytecode_index,
                    },
                );
            }
        }
        SourceOriginSemanticValidationReport { findings }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ExecutionDiagnosticMapping {
    pub bytecode_index: BytecodeIndex,
    pub provider: Option<SourceProviderId>,
    pub source_range: SourceRange,
    pub position_kind: SourcePositionKind,
    pub semantic_kind: Option<SourceOriginSemanticKind>,
    pub strict: bool,
    pub synthetic: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SourceOriginSemanticEntry {
    pub bytecode_index: BytecodeIndex,
    pub provider: Option<SourceProviderId>,
    pub source_range: SourceRange,
    pub semantic_kind: SourceOriginSemanticKind,
    pub strict: bool,
    pub synthetic: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SourceOriginSemanticKind {
    DirectivePrologue,
    Declaration,
    LexicalBinding,
    PrivateName,
    ClassFieldInitializer,
    FunctionBody,
    EvalBody,
    ModuleItem,
    Expression,
    Synthetic,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceOriginSemanticValidationReport {
    pub findings: Vec<SourceOriginSemanticValidationFinding>,
}

impl SourceOriginSemanticValidationReport {
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceOriginSemanticValidationFinding {
    InvalidBytecodeIndex { entry: u32 },
    UnorderedSourceRange { entry: u32, range: SourceRange },
    DuplicateBytecodeIndex { bytecode_index: BytecodeIndex },
}

fn source_position_after(left: SourcePosition, right: SourcePosition) -> bool {
    (left.line, left.column, left.offset) > (right.line, right.column, right.offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(offset: u32) -> SourcePosition {
        SourcePosition {
            offset,
            line: 1,
            column: offset,
        }
    }

    #[test]
    fn source_origin_semantic_map_validates_boundary_records() {
        let bytecode_index = BytecodeIndex::from_offset(0);
        let map = SourceOriginSemanticMap {
            entries: vec![
                SourceOriginSemanticEntry {
                    bytecode_index,
                    provider: None,
                    source_range: SourceRange {
                        start: position(3),
                        end: position(2),
                    },
                    semantic_kind: SourceOriginSemanticKind::ClassFieldInitializer,
                    strict: true,
                    synthetic: false,
                },
                SourceOriginSemanticEntry {
                    bytecode_index,
                    provider: None,
                    source_range: SourceRange {
                        start: position(4),
                        end: position(5),
                    },
                    semantic_kind: SourceOriginSemanticKind::Synthetic,
                    strict: true,
                    synthetic: true,
                },
            ],
        };

        let findings = map.validate().findings;

        assert!(findings.contains(
            &SourceOriginSemanticValidationFinding::UnorderedSourceRange {
                entry: 0,
                range: SourceRange {
                    start: position(3),
                    end: position(2),
                },
            }
        ));
        assert!(findings.contains(
            &SourceOriginSemanticValidationFinding::DuplicateBytecodeIndex { bytecode_index }
        ));
    }

    #[test]
    fn code_origin_table_resolves_execution_diagnostics() {
        let bytecode_index = BytecodeIndex::from_offset(2);
        let range = SourceRange {
            start: position(7),
            end: position(9),
        };
        let table = CodeOriginTable {
            pc_mappings: vec![ProgramCounterOrigin {
                pc_offset: 4,
                origin: CodeOrigin::new(bytecode_index),
                width: ProgramCounterMappingWidth::ReturnPoint,
            }],
            source_mappings: vec![BytecodeSourceMapping {
                bytecode_index,
                provider: Some(SourceProviderId(11)),
                source_range: range,
                position_kind: SourcePositionKind::Call,
            }],
            semantic_mappings: SourceOriginSemanticMap {
                entries: vec![SourceOriginSemanticEntry {
                    bytecode_index,
                    provider: Some(SourceProviderId(11)),
                    source_range: range,
                    semantic_kind: SourceOriginSemanticKind::Expression,
                    strict: true,
                    synthetic: false,
                }],
            },
            ..CodeOriginTable::default()
        };

        let diagnostic = table
            .execution_diagnostic_for_bytecode_index(bytecode_index)
            .expect("diagnostic");

        assert_eq!(
            table.origin_for_pc_offset(8),
            Some(CodeOrigin::new(bytecode_index))
        );
        assert_eq!(diagnostic.provider, Some(SourceProviderId(11)));
        assert_eq!(
            diagnostic.semantic_kind,
            Some(SourceOriginSemanticKind::Expression)
        );
        assert!(diagnostic.strict);
        assert!(!diagnostic.synthetic);
    }
}
