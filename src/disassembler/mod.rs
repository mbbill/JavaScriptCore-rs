//! Disassembler contracts.
//!
//! Disassembly is diagnostic infrastructure for generated code. It must observe
//! code ranges and metadata without owning executable memory or decoding
//! semantics into behavior.

use crate::jit::{DisassemblyMetadata, JitCodeId, JitType, TierFallbackResultRecord};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisassemblerBackend {
    Arm64,
    Zydis,
    TextOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisassemblyAuthority {
    ObserveOnly,
    AnnotateMetadata,
    ExternalTool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisassemblyRequest {
    pub code: JitCodeId,
    pub tier: JitType,
    pub backend: DisassemblerBackend,
    pub include_source: bool,
    pub include_relocations: bool,
    /// Disassembly never owns executable memory. This field records whether it
    /// may attach diagnostic metadata or only render an existing snapshot.
    pub authority: DisassemblyAuthority,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisassemblerValidationError {
    EmptyName,
    EmptyProvenance(&'static str),
    DuplicateFormat(crate::jit::DisassemblyFormat),
    EmptyBackends(&'static str),
    BackendUnsupported(DisassemblerBackend),
    SourceAnnotationsUnsupported,
    RelocationsUnsupported,
    FormatUnsupported(crate::jit::DisassemblyFormat),
    MetadataInvalid(crate::jit::DisassemblyMetadataValidationError),
    ReportOffsetsNotMonotonic,
    EmptyLineText,
}

impl DisassemblyRequest {
    pub fn validate_against(
        &self,
        schema: &StaticDisassemblyFormatSchema,
    ) -> Result<(), DisassemblerValidationError> {
        if !schema.backends.contains(&self.backend) {
            return Err(DisassemblerValidationError::BackendUnsupported(
                self.backend,
            ));
        }
        if self.include_source && !schema.supports_source_annotations {
            return Err(DisassemblerValidationError::SourceAnnotationsUnsupported);
        }
        if self.include_relocations && !schema.supports_relocations {
            return Err(DisassemblerValidationError::RelocationsUnsupported);
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DisassemblyLine {
    pub offset: u32,
    pub text_ordinal: u32,
    pub annotation_ordinal: Option<u32>,
    pub semantic_ordinal: Option<u32>,
    pub execution_ordinal: Option<u32>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DisassemblyReport {
    pub lines: Vec<DisassemblyLine>,
    pub truncated: bool,
}

impl DisassemblyReport {
    pub fn validate(&self) -> Result<(), DisassemblerValidationError> {
        let mut previous = None;
        for line in &self.lines {
            if line.text_ordinal == 0 {
                return Err(DisassemblerValidationError::EmptyLineText);
            }
            if previous.is_some_and(|offset| line.offset < offset) {
                return Err(DisassemblerValidationError::ReportOffsetsNotMonotonic);
            }
            previous = Some(line.offset);
        }

        Ok(())
    }
}

/// Disassembly diagnostics joined with tier fallback visibility.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisassemblyDiagnosticReport {
    pub disassembly: DisassemblyReport,
    pub tier_fallbacks: Vec<TierFallbackResultRecord>,
    pub execution_annotation_count: usize,
    pub fallback_annotation_count: usize,
}

pub fn disassembly_diagnostic_report(
    request: &DisassemblyRequest,
    metadata: &DisassemblyMetadata,
    registry: DisassemblyFormatRegistry,
    tier_fallbacks: Vec<TierFallbackResultRecord>,
) -> Result<DisassemblyDiagnosticReport, DisassemblerValidationError> {
    let disassembly = format_disassembly_metadata(request, metadata, registry)?;
    let execution_annotation_count = disassembly
        .lines
        .iter()
        .filter(|line| line.execution_ordinal.is_some())
        .count();
    let fallback_annotation_count = metadata
        .instructions
        .iter()
        .filter(|instruction| {
            instruction
                .annotation
                .as_ref()
                .and_then(|annotation| annotation.execution)
                .is_some_and(|execution| {
                    execution.kind == crate::jit::DisassemblyExecutionKind::TierFallback
                })
        })
        .count();
    Ok(DisassemblyDiagnosticReport {
        disassembly,
        tier_fallbacks,
        execution_annotation_count,
        fallback_annotation_count,
    })
}

pub fn format_disassembly_metadata(
    request: &DisassemblyRequest,
    metadata: &DisassemblyMetadata,
    registry: DisassemblyFormatRegistry,
) -> Result<DisassemblyReport, DisassemblerValidationError> {
    let schema = registry
        .schema_for_format(request_format(request, metadata))
        .ok_or(DisassemblerValidationError::FormatUnsupported(
            metadata.format,
        ))?;
    request.validate_against(schema)?;
    metadata
        .validate()
        .map_err(DisassemblerValidationError::MetadataInvalid)?;

    let lines = metadata
        .instructions
        .iter()
        .enumerate()
        .map(|(index, instruction)| DisassemblyLine {
            offset: instruction.offset,
            text_ordinal: instruction
                .text
                .map_or(index as u32 + 1, |_| index as u32 + 1),
            annotation_ordinal: instruction
                .annotation
                .as_ref()
                .map(|annotation| disassembly_section_ordinal(annotation.section)),
            semantic_ordinal: instruction
                .annotation
                .as_ref()
                .and_then(|annotation| annotation.semantic)
                .map(|semantic| disassembly_semantic_ordinal(semantic.kind)),
            execution_ordinal: instruction
                .annotation
                .as_ref()
                .and_then(|annotation| annotation.execution)
                .map(|execution| disassembly_execution_ordinal(execution.kind)),
        })
        .collect();
    let report = DisassemblyReport {
        lines,
        truncated: false,
    };
    report.validate()?;
    Ok(report)
}

const fn request_format(
    _request: &DisassemblyRequest,
    metadata: &DisassemblyMetadata,
) -> crate::jit::DisassemblyFormat {
    metadata.format
}

const fn disassembly_section_ordinal(section: crate::jit::DisassemblySection) -> u32 {
    match section {
        crate::jit::DisassemblySection::Prologue => 1,
        crate::jit::DisassemblySection::Body => 2,
        crate::jit::DisassemblySection::SlowPath => 3,
        crate::jit::DisassemblySection::OsrExit => 4,
        crate::jit::DisassemblySection::ExceptionHandler => 5,
        crate::jit::DisassemblySection::InlineCacheStub => 6,
        crate::jit::DisassemblySection::Data => 7,
    }
}

const fn disassembly_semantic_ordinal(kind: crate::jit::DisassemblySemanticKind) -> u32 {
    match kind {
        crate::jit::DisassemblySemanticKind::TierEntry => 1,
        crate::jit::DisassemblySemanticKind::InlineCache => 2,
        crate::jit::DisassemblySemanticKind::SpeculationCheck => 3,
        crate::jit::DisassemblySemanticKind::OsrExit => 4,
        crate::jit::DisassemblySemanticKind::SlowPath => 5,
        crate::jit::DisassemblySemanticKind::ExceptionEdge => 6,
        crate::jit::DisassemblySemanticKind::DomJitBoundary => 7,
        crate::jit::DisassemblySemanticKind::DeoptContinuation => 8,
        crate::jit::DisassemblySemanticKind::Data => 9,
    }
}

const fn disassembly_execution_ordinal(kind: crate::jit::DisassemblyExecutionKind) -> u32 {
    match kind {
        crate::jit::DisassemblyExecutionKind::InterpreterEntry => 1,
        crate::jit::DisassemblyExecutionKind::SlowPathBoundary => 2,
        crate::jit::DisassemblyExecutionKind::TierFallback => 3,
        crate::jit::DisassemblyExecutionKind::InlineCacheMiss => 4,
        crate::jit::DisassemblyExecutionKind::ExceptionUnwind => 5,
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DisassemblerSchemaOwner {
    #[default]
    DisassemblerRegistry,
    JitDiagnostics,
    ExternalTooling,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DisassemblerRegistryMutationAuthority {
    #[default]
    GeneratedStaticDataRefresh,
    CrateInitialization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticDisassemblyFormatSchema {
    pub format: crate::jit::DisassemblyFormat,
    pub name: &'static str,
    pub backends: &'static [DisassemblerBackend],
    pub supports_source_annotations: bool,
    pub supports_relocations: bool,
    pub owner: DisassemblerSchemaOwner,
    pub mutation_authority: DisassemblerRegistryMutationAuthority,
    pub provenance: &'static str,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DisassemblyFormatRegistry {
    pub formats: &'static [StaticDisassemblyFormatSchema],
}

impl DisassemblyFormatRegistry {
    pub const fn new(formats: &'static [StaticDisassemblyFormatSchema]) -> Self {
        Self { formats }
    }

    pub const fn formats(self) -> &'static [StaticDisassemblyFormatSchema] {
        self.formats
    }

    pub fn schema_for_format(
        self,
        format: crate::jit::DisassemblyFormat,
    ) -> Option<&'static StaticDisassemblyFormatSchema> {
        self.formats.iter().find(|schema| schema.format == format)
    }

    pub fn validate(self) -> Result<(), DisassemblerValidationError> {
        for (index, format) in self.formats.iter().enumerate() {
            format.validate()?;
            if self.formats[index + 1..]
                .iter()
                .any(|other| other.format == format.format)
            {
                return Err(DisassemblerValidationError::DuplicateFormat(format.format));
            }
        }

        Ok(())
    }
}

impl StaticDisassemblyFormatSchema {
    pub fn validate(&self) -> Result<(), DisassemblerValidationError> {
        if self.name.is_empty() {
            return Err(DisassemblerValidationError::EmptyName);
        }
        if self.provenance.is_empty() {
            return Err(DisassemblerValidationError::EmptyProvenance(self.name));
        }
        if self.backends.is_empty() {
            return Err(DisassemblerValidationError::EmptyBackends(self.name));
        }

        Ok(())
    }
}

const TEXT_BACKENDS: &[DisassemblerBackend] = &[
    DisassemblerBackend::Arm64,
    DisassemblerBackend::Zydis,
    DisassemblerBackend::TextOnly,
];
const EXTERNAL_BACKENDS: &[DisassemblerBackend] = &[DisassemblerBackend::TextOnly];

pub const STATIC_DISASSEMBLY_FORMAT_SCHEMAS: &[StaticDisassemblyFormatSchema] = &[
    StaticDisassemblyFormatSchema {
        format: crate::jit::DisassemblyFormat::PlainText,
        name: "plain-text",
        backends: TEXT_BACKENDS,
        supports_source_annotations: true,
        supports_relocations: true,
        owner: DisassemblerSchemaOwner::JitDiagnostics,
        mutation_authority: DisassemblerRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust disassembly format schema",
    },
    StaticDisassemblyFormatSchema {
        format: crate::jit::DisassemblyFormat::Json,
        name: "json",
        backends: TEXT_BACKENDS,
        supports_source_annotations: true,
        supports_relocations: true,
        owner: DisassemblerSchemaOwner::JitDiagnostics,
        mutation_authority: DisassemblerRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust disassembly format schema",
    },
    StaticDisassemblyFormatSchema {
        format: crate::jit::DisassemblyFormat::PerfMap,
        name: "perf-map",
        backends: EXTERNAL_BACKENDS,
        supports_source_annotations: false,
        supports_relocations: false,
        owner: DisassemblerSchemaOwner::ExternalTooling,
        mutation_authority: DisassemblerRegistryMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust disassembly format schema",
    },
];

pub const DISASSEMBLY_FORMAT_REGISTRY: DisassemblyFormatRegistry =
    DisassemblyFormatRegistry::new(STATIC_DISASSEMBLY_FORMAT_SCHEMAS);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::{DisassemblyFormat, JitCodeId, JitType};

    #[test]
    fn static_disassembly_registry_validates() {
        assert_eq!(DISASSEMBLY_FORMAT_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn perf_map_request_rejects_source_annotations() {
        let schema = DISASSEMBLY_FORMAT_REGISTRY
            .schema_for_format(DisassemblyFormat::PerfMap)
            .expect("perf-map schema");
        let request = DisassemblyRequest {
            code: JitCodeId(1),
            tier: JitType::Ftl,
            backend: DisassemblerBackend::TextOnly,
            include_source: true,
            include_relocations: false,
            authority: DisassemblyAuthority::ObserveOnly,
        };

        assert_eq!(
            request.validate_against(schema),
            Err(DisassemblerValidationError::SourceAnnotationsUnsupported)
        );
    }

    #[test]
    fn metadata_formatting_produces_monotonic_report_lines() {
        let request = DisassemblyRequest {
            code: JitCodeId(2),
            tier: JitType::Baseline,
            backend: DisassemblerBackend::TextOnly,
            include_source: false,
            include_relocations: false,
            authority: DisassemblyAuthority::ObserveOnly,
        };
        let metadata = DisassemblyMetadata {
            code: JitCodeId(2),
            source: crate::jit::DisassemblySource::JitDisassembler,
            format: DisassemblyFormat::PlainText,
            range: None,
            instructions: vec![
                crate::jit::DisassemblyInstruction {
                    offset: 0,
                    size_bytes: 4,
                    text: Some("mov"),
                    annotation: None,
                },
                crate::jit::DisassemblyInstruction {
                    offset: 4,
                    size_bytes: 4,
                    text: Some("ret"),
                    annotation: None,
                },
            ],
        };

        assert_eq!(
            format_disassembly_metadata(&request, &metadata, DISASSEMBLY_FORMAT_REGISTRY)
                .map(|report| report.lines.len()),
            Ok(2)
        );
    }

    #[test]
    fn semantic_annotations_flow_into_report_lines() {
        let request = DisassemblyRequest {
            code: JitCodeId(3),
            tier: JitType::Dfg,
            backend: DisassemblerBackend::TextOnly,
            include_source: false,
            include_relocations: false,
            authority: DisassemblyAuthority::AnnotateMetadata,
        };
        let metadata = DisassemblyMetadata {
            code: JitCodeId(3),
            source: crate::jit::DisassemblySource::DfgDisassembler,
            format: DisassemblyFormat::PlainText,
            range: None,
            instructions: vec![crate::jit::DisassemblyInstruction {
                offset: 12,
                size_bytes: 4,
                text: Some("check"),
                annotation: Some(crate::jit::DisassemblyAnnotation {
                    section: crate::jit::DisassemblySection::Body,
                    origin: None,
                    patchpoint: None,
                    label: Some("speculation"),
                    semantic: Some(crate::jit::DisassemblySemanticAnnotation {
                        kind: crate::jit::DisassemblySemanticKind::SpeculationCheck,
                        effects: crate::jit::EffectSummary::for_check(),
                        semantic_id: Some(7),
                    }),
                    execution: None,
                }),
            }],
        };

        let report =
            format_disassembly_metadata(&request, &metadata, DISASSEMBLY_FORMAT_REGISTRY).unwrap();

        assert_eq!(report.lines[0].semantic_ordinal, Some(3));
    }

    #[test]
    fn execution_annotations_flow_into_report_lines() {
        let request = DisassemblyRequest {
            code: JitCodeId(4),
            tier: JitType::Baseline,
            backend: DisassemblerBackend::TextOnly,
            include_source: false,
            include_relocations: false,
            authority: DisassemblyAuthority::AnnotateMetadata,
        };
        let metadata = DisassemblyMetadata {
            code: JitCodeId(4),
            source: crate::jit::DisassemblySource::JitDisassembler,
            format: DisassemblyFormat::PlainText,
            range: None,
            instructions: vec![crate::jit::DisassemblyInstruction {
                offset: 32,
                size_bytes: 4,
                text: Some("call"),
                annotation: Some(crate::jit::DisassemblyAnnotation {
                    section: crate::jit::DisassemblySection::SlowPath,
                    origin: None,
                    patchpoint: None,
                    label: Some("slow-path"),
                    semantic: None,
                    execution: Some(crate::jit::DisassemblyExecutionAnnotation {
                        kind: crate::jit::DisassemblyExecutionKind::SlowPathBoundary,
                        bytecode_index: None,
                        boundary: Some(crate::jit::CallBoundaryId(30)),
                        tier: Some(JitType::Baseline),
                        fallback_reason: None,
                        inline_cache_slot: None,
                    }),
                }),
            }],
        };

        let report =
            format_disassembly_metadata(&request, &metadata, DISASSEMBLY_FORMAT_REGISTRY).unwrap();

        assert_eq!(report.lines[0].execution_ordinal, Some(2));
    }

    #[test]
    fn diagnostic_report_counts_tier_fallback_annotations() {
        let request = DisassemblyRequest {
            code: JitCodeId(5),
            tier: JitType::Baseline,
            backend: DisassemblerBackend::TextOnly,
            include_source: false,
            include_relocations: false,
            authority: DisassemblyAuthority::AnnotateMetadata,
        };
        let metadata = DisassemblyMetadata {
            code: JitCodeId(5),
            source: crate::jit::DisassemblySource::JitDisassembler,
            format: DisassemblyFormat::PlainText,
            range: None,
            instructions: vec![crate::jit::DisassemblyInstruction {
                offset: 40,
                size_bytes: 4,
                text: Some("fallback"),
                annotation: Some(crate::jit::DisassemblyAnnotation {
                    section: crate::jit::DisassemblySection::SlowPath,
                    origin: None,
                    patchpoint: None,
                    label: Some("fallback"),
                    semantic: None,
                    execution: Some(crate::jit::DisassemblyExecutionAnnotation {
                        kind: crate::jit::DisassemblyExecutionKind::TierFallback,
                        bytecode_index: Some(crate::bytecode::BytecodeIndex::from_offset(4)),
                        boundary: None,
                        tier: Some(JitType::Baseline),
                        fallback_reason: Some(crate::jit::TierFallbackReason::UnsupportedTier),
                        inline_cache_slot: None,
                    }),
                }),
            }],
        };
        let fallback = TierFallbackResultRecord {
            owner: crate::runtime::CodeBlockId(crate::gc::CellId(9)),
            from_tier: JitType::Baseline,
            attempted_tier: JitType::Dfg,
            reason: crate::jit::TierFallbackReason::UnsupportedTier,
            target: crate::jit::TierFallbackTarget::ReturnToInterpreter,
            bytecode_index: Some(crate::bytecode::BytecodeIndex::from_offset(4)),
            resume: crate::jit::TierFallbackResumeKind::ContinueInInterpreter,
            preserves_profile: true,
            should_count_invalidation: true,
            clears_active_request: true,
        };

        let report = disassembly_diagnostic_report(
            &request,
            &metadata,
            DISASSEMBLY_FORMAT_REGISTRY,
            vec![fallback],
        )
        .expect("diagnostic report");

        assert_eq!(report.execution_annotation_count, 1);
        assert_eq!(report.fallback_annotation_count, 1);
        assert_eq!(report.tier_fallbacks.len(), 1);
    }
}
