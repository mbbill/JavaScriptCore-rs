//! Generated-source and table-generation contracts.
//!
//! JavaScriptCore relies on generated opcode tables, builtin bindings,
//! offlineasm products, and Unicode data. This module records generated artifact
//! provenance without running generators.

/// Generator-local artifact identity.
///
/// The generator pipeline owns assignment and ordering. Consumers may borrow
/// this ID to connect generated sections to inputs, but it is not bytecode
/// opcode, builtin, source-provider, or file-system identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct GeneratedArtifactId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedArtifactKind {
    OpcodeTable,
    BuiltinNames,
    BuiltinSource,
    LLIntAssembly,
    UnicodeTable,
    InspectorProtocol,
    WasmOpcodeTable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedArtifactFreshness {
    Unknown,
    SourceMatched,
    NeedsRegeneration,
    GeneratedOutOfTree,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedArtifactRecord {
    pub id: GeneratedArtifactId,
    pub kind: GeneratedArtifactKind,
    pub source_ordinal: u32,
    pub freshness: GeneratedArtifactFreshness,
}

/// Source file consumed by a generator pass.
///
/// Ruby generator inputs such as `BytecodeList.rb`, builtin JS sources, and
/// Unicode tables have different formats, but they share provenance needs:
/// source order, revision identity, and whether generated output can be
/// considered matched to the checked-in sources.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratorSource {
    pub ordinal: u32,
    pub kind: GeneratorSourceKind,
    pub path: String,
    pub revision: GeneratorRevision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratorSourceKind {
    RubyDsl,
    BuiltinJavaScript,
    UnicodeData,
    OfflineAssembly,
    Template,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct GeneratorRevision(pub u64);

/// Generated file boundary.
///
/// The generator may own templates and section ordering, but checked-in Rust or
/// C++ code owns how generated files are included. This contract keeps output
/// description separate from file-system mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedFilePlan {
    pub artifact: GeneratedArtifactId,
    pub path: String,
    pub sections: Vec<GeneratedSectionPlan>,
    pub comment_style: GeneratedCommentStyle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedFilePlanBuilder {
    artifact: GeneratedArtifactId,
    path: String,
    sections: Vec<GeneratedSectionPlan>,
    comment_style: GeneratedCommentStyle,
}

impl GeneratedFilePlanBuilder {
    pub fn new(artifact: GeneratedArtifactId, path: impl Into<String>) -> Self {
        Self {
            artifact,
            path: path.into(),
            sections: Vec::new(),
            comment_style: GeneratedCommentStyle::default(),
        }
    }

    pub fn with_comment_style(mut self, comment_style: GeneratedCommentStyle) -> Self {
        self.comment_style = comment_style;
        self
    }

    pub fn add_section(&mut self, section: GeneratedSectionPlan) {
        self.sections.push(section);
    }

    pub fn finish(self) -> GeneratedFilePlan {
        GeneratedFilePlan {
            artifact: self.artifact,
            path: self.path,
            sections: self.sections,
            comment_style: self.comment_style,
        }
    }
}

impl GeneratedFilePlan {
    pub fn validate(&self, sources: &[GeneratorSource]) -> GeneratedValidationReport {
        let mut findings = Vec::new();
        validate_generated_path(self.artifact, &self.path, &mut findings);
        validate_dynamic_sections(self.artifact, &self.sections, sources.len(), &mut findings);
        GeneratedValidationReport { findings }
    }

    pub fn assemble(&self, sources: &[GeneratorSource]) -> GeneratedAssemblyPlan {
        let validation = self.validate(sources);
        let mut sections = self.sections.clone();
        sections.sort_by_key(|section| section.order);
        let needs_regeneration = sections
            .iter()
            .any(|section| section.freshness != GeneratedArtifactFreshness::SourceMatched);
        let header = generated_header(self.comment_style, self.artifact, &self.path);
        GeneratedAssemblyPlan {
            artifact: self.artifact,
            path: self.path.clone(),
            header,
            ordered_sections: sections,
            needs_regeneration,
            validation,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedAssemblyPlan {
    pub artifact: GeneratedArtifactId,
    pub path: String,
    pub header: String,
    pub ordered_sections: Vec<GeneratedSectionPlan>,
    pub needs_regeneration: bool,
    pub validation: GeneratedValidationReport,
}

impl GeneratedAssemblyPlan {
    pub fn is_ready_to_emit(&self) -> bool {
        self.validation.is_valid() && !self.needs_regeneration
    }
}

fn generated_header(
    style: GeneratedCommentStyle,
    artifact: GeneratedArtifactId,
    path: &str,
) -> String {
    let text = format!("Generated artifact {} for {}", artifact.0, path);
    match style {
        GeneratedCommentStyle::Cpp => format!("// {text}"),
        GeneratedCommentStyle::Rust => format!("// {text}"),
        GeneratedCommentStyle::Ruby => format!("# {text}"),
        GeneratedCommentStyle::Plain => text,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedSectionPlan {
    pub name: String,
    pub order: u32,
    pub source: Option<GeneratorSourceRef>,
    pub freshness: GeneratedArtifactFreshness,
}

/// Borrowed reference to a generator input record.
///
/// `GeneratorSource` owns the source path and revision metadata. This handle is
/// valid only inside the generated-file plan that produced it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct GeneratorSourceRef(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GeneratedCommentStyle {
    #[default]
    Cpp,
    Rust,
    Ruby,
    Plain,
}

/// Opcode-generator contract extracted from `generator/Opcode.rb`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpcodeGeneratorRecord {
    pub name: String,
    pub section: String,
    pub arguments: Vec<GeneratorArgument>,
    pub metadata: Vec<GeneratorMetadataField>,
    pub checkpoints: Vec<GeneratorCheckpoint>,
    pub temporaries: Vec<GeneratorTemporary>,
    pub assertions: Vec<GeneratorAssertion>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratorArgument {
    pub name: String,
    pub ty: GeneratorType,
    pub role: GeneratorArgumentRole,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratorArgumentRole {
    Destination,
    Source,
    Immediate,
    Metadata,
    ControlFlow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratorType {
    pub name: String,
    pub storage: GeneratorStorageKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratorStorageKind {
    Register,
    Immediate,
    Pointer,
    MetadataIndex,
    Opaque,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratorMetadataField {
    pub name: String,
    pub ty: GeneratorType,
    pub mutability: GeneratorMetadataMutability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratorMetadataMutability {
    Frozen,
    LinkedMutable,
    MainThreadOnly,
    Locked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratorCheckpoint {
    pub name: String,
    pub ordinal: u8,
    pub records_value_profile: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratorTemporary {
    pub name: String,
    pub ty: GeneratorType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratorAssertion {
    pub name: String,
    pub phase: GeneratorAssertionPhase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratorAssertionPhase {
    ParseDsl,
    ValidateFits,
    EmitMetadata,
    EmitInstructionStructs,
}

/// Component that owns generated file descriptor tables.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GeneratedFileDescriptorOwner {
    #[default]
    GeneratorPipeline,
    CheckedInSourceTree,
    TestFixture,
}

/// Authority allowed to replace generated file descriptors.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GeneratedFileMutationAuthority {
    #[default]
    GeneratorRun,
    SourceTreeRefresh,
}

/// Static source file consumed by a generator pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticGeneratorSource {
    pub ordinal: u32,
    pub kind: GeneratorSourceKind,
    pub path: &'static str,
    pub revision: GeneratorRevision,
}

/// Static generated section descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticGeneratedSection {
    pub name: &'static str,
    pub order: u32,
    pub source: Option<GeneratorSourceRef>,
    pub freshness: GeneratedArtifactFreshness,
}

/// Immutable generated file descriptor.
///
/// It records generated-data provenance and section ownership only. File-system
/// writes and regeneration remain outside this descriptor layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedFileDescriptor {
    pub artifact: GeneratedArtifactId,
    pub kind: GeneratedArtifactKind,
    pub path: &'static str,
    pub sections: &'static [StaticGeneratedSection],
    pub comment_style: GeneratedCommentStyle,
    pub owner: GeneratedFileDescriptorOwner,
    pub mutation_authority: GeneratedFileMutationAuthority,
}

impl GeneratedFileDescriptor {
    pub const fn sections(self) -> &'static [StaticGeneratedSection] {
        self.sections
    }
}

/// Immutable registry for generated file descriptors and source provenance.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GeneratedFileRegistry {
    pub sources: &'static [StaticGeneratorSource],
    pub files: &'static [GeneratedFileDescriptor],
}

impl GeneratedFileRegistry {
    pub const fn new(
        sources: &'static [StaticGeneratorSource],
        files: &'static [GeneratedFileDescriptor],
    ) -> Self {
        Self { sources, files }
    }

    pub const fn sources(self) -> &'static [StaticGeneratorSource] {
        self.sources
    }

    pub const fn files(self) -> &'static [GeneratedFileDescriptor] {
        self.files
    }

    pub fn file_for_artifact(
        self,
        artifact: GeneratedArtifactId,
    ) -> Option<&'static GeneratedFileDescriptor> {
        self.files.iter().find(|file| file.artifact == artifact)
    }

    pub fn validate(self) -> GeneratedValidationReport {
        let mut findings = Vec::new();
        for (index, source) in self.sources.iter().enumerate() {
            if source.path.is_empty() {
                findings.push(GeneratedValidationFinding::EmptySourcePath {
                    ordinal: source.ordinal,
                });
            }
            if self.sources[..index]
                .iter()
                .any(|candidate| candidate.ordinal == source.ordinal)
            {
                findings.push(GeneratedValidationFinding::DuplicateSourceOrdinal {
                    ordinal: source.ordinal,
                });
            }
        }

        for (index, file) in self.files.iter().enumerate() {
            validate_generated_path(file.artifact, file.path, &mut findings);
            if self.files[..index]
                .iter()
                .any(|candidate| candidate.artifact == file.artifact)
            {
                findings.push(GeneratedValidationFinding::DuplicateGeneratedArtifact {
                    artifact: file.artifact,
                });
            }
            validate_static_sections(
                file.artifact,
                file.sections(),
                self.sources.len(),
                &mut findings,
            );
        }

        GeneratedValidationReport { findings }
    }
}

/// Static borrowed type descriptor from an opcode generator record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticGeneratorType {
    pub name: &'static str,
    pub storage: GeneratorStorageKind,
}

/// Static borrowed opcode-generator argument descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticGeneratorArgument {
    pub name: &'static str,
    pub ty: StaticGeneratorType,
    pub role: GeneratorArgumentRole,
}

/// Static borrowed opcode-generator metadata field descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticGeneratorMetadataField {
    pub name: &'static str,
    pub ty: StaticGeneratorType,
    pub mutability: GeneratorMetadataMutability,
}

/// Static borrowed opcode-generator checkpoint descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticGeneratorCheckpoint {
    pub name: &'static str,
    pub ordinal: u8,
    pub records_value_profile: bool,
}

/// Static borrowed opcode-generator temporary descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticGeneratorTemporary {
    pub name: &'static str,
    pub ty: StaticGeneratorType,
}

/// Static borrowed opcode-generator assertion descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticGeneratorAssertion {
    pub name: &'static str,
    pub phase: GeneratorAssertionPhase,
}

/// Immutable opcode-generator record extracted from generated DSL metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticOpcodeGeneratorRecord {
    pub name: &'static str,
    pub section: &'static str,
    pub arguments: &'static [StaticGeneratorArgument],
    pub metadata: &'static [StaticGeneratorMetadataField],
    pub checkpoints: &'static [StaticGeneratorCheckpoint],
    pub temporaries: &'static [StaticGeneratorTemporary],
    pub assertions: &'static [StaticGeneratorAssertion],
}

impl StaticOpcodeGeneratorRecord {
    pub const fn arguments(self) -> &'static [StaticGeneratorArgument] {
        self.arguments
    }

    pub const fn metadata(self) -> &'static [StaticGeneratorMetadataField] {
        self.metadata
    }
}

/// Immutable registry of opcode-generator records.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OpcodeGeneratorRegistry {
    pub records: &'static [StaticOpcodeGeneratorRecord],
}

impl OpcodeGeneratorRegistry {
    pub const fn new(records: &'static [StaticOpcodeGeneratorRecord]) -> Self {
        Self { records }
    }

    pub const fn records(self) -> &'static [StaticOpcodeGeneratorRecord] {
        self.records
    }

    pub fn record_for_name(self, name: &str) -> Option<&'static StaticOpcodeGeneratorRecord> {
        self.records.iter().find(|record| record.name == name)
    }

    pub fn validate(self) -> GeneratedValidationReport {
        let mut findings = Vec::new();
        for (index, record) in self.records.iter().enumerate() {
            if record.name.is_empty() {
                findings.push(GeneratedValidationFinding::EmptyOpcodeRecordName);
            }
            if self.records[..index]
                .iter()
                .any(|candidate| candidate.name == record.name)
            {
                findings.push(GeneratedValidationFinding::DuplicateOpcodeRecordName {
                    name: record.name,
                });
            }
            validate_opcode_record(*record, &mut findings);
        }
        GeneratedValidationReport { findings }
    }
}

fn validate_generated_path(
    artifact: GeneratedArtifactId,
    path: &str,
    findings: &mut Vec<GeneratedValidationFinding>,
) {
    if path.is_empty() {
        findings.push(GeneratedValidationFinding::EmptyGeneratedFilePath { artifact });
    }
}

fn validate_dynamic_sections(
    artifact: GeneratedArtifactId,
    sections: &[GeneratedSectionPlan],
    source_count: usize,
    findings: &mut Vec<GeneratedValidationFinding>,
) {
    for (index, section) in sections.iter().enumerate() {
        if section.name.is_empty() {
            findings.push(GeneratedValidationFinding::EmptySectionName { artifact });
        }
        if sections[..index]
            .iter()
            .any(|candidate| candidate.order == section.order)
        {
            findings.push(GeneratedValidationFinding::DuplicateSectionOrder {
                artifact,
                order: section.order,
            });
        }
        if let Some(source) = section.source {
            if usize::try_from(source.0)
                .ok()
                .is_none_or(|index| index >= source_count)
            {
                findings
                    .push(GeneratedValidationFinding::MissingSectionSource { artifact, source });
            }
        }
    }
}

fn validate_static_sections(
    artifact: GeneratedArtifactId,
    sections: &[StaticGeneratedSection],
    source_count: usize,
    findings: &mut Vec<GeneratedValidationFinding>,
) {
    for (index, section) in sections.iter().enumerate() {
        if section.name.is_empty() {
            findings.push(GeneratedValidationFinding::EmptySectionName { artifact });
        }
        if sections[..index]
            .iter()
            .any(|candidate| candidate.order == section.order)
        {
            findings.push(GeneratedValidationFinding::DuplicateSectionOrder {
                artifact,
                order: section.order,
            });
        }
        if let Some(source) = section.source {
            if usize::try_from(source.0)
                .ok()
                .is_none_or(|index| index >= source_count)
            {
                findings
                    .push(GeneratedValidationFinding::MissingSectionSource { artifact, source });
            }
        }
    }
}

fn validate_opcode_record(
    record: StaticOpcodeGeneratorRecord,
    findings: &mut Vec<GeneratedValidationFinding>,
) {
    for (index, argument) in record.arguments.iter().enumerate() {
        if argument.name.is_empty() {
            findings.push(GeneratedValidationFinding::EmptyOpcodeArgumentName {
                record: record.name,
            });
        }
        if record.arguments[..index]
            .iter()
            .any(|candidate| candidate.name == argument.name)
        {
            findings.push(GeneratedValidationFinding::DuplicateOpcodeArgument {
                record: record.name,
                name: argument.name,
            });
        }
    }
    for (index, field) in record.metadata.iter().enumerate() {
        if field.name.is_empty() {
            findings.push(GeneratedValidationFinding::EmptyOpcodeMetadataName {
                record: record.name,
            });
        }
        if record.metadata[..index]
            .iter()
            .any(|candidate| candidate.name == field.name)
        {
            findings.push(GeneratedValidationFinding::DuplicateOpcodeMetadata {
                record: record.name,
                name: field.name,
            });
        }
    }
    for (index, checkpoint) in record.checkpoints.iter().enumerate() {
        if checkpoint.name.is_empty() {
            findings.push(GeneratedValidationFinding::EmptyOpcodeCheckpointName {
                record: record.name,
            });
        }
        if record.checkpoints[..index]
            .iter()
            .any(|candidate| candidate.ordinal == checkpoint.ordinal)
        {
            findings.push(GeneratedValidationFinding::DuplicateOpcodeCheckpoint {
                record: record.name,
                ordinal: checkpoint.ordinal,
            });
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GeneratedValidationReport {
    pub findings: Vec<GeneratedValidationFinding>,
}

impl GeneratedValidationReport {
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedValidationFinding {
    EmptySourcePath {
        ordinal: u32,
    },
    DuplicateSourceOrdinal {
        ordinal: u32,
    },
    EmptyGeneratedFilePath {
        artifact: GeneratedArtifactId,
    },
    DuplicateGeneratedArtifact {
        artifact: GeneratedArtifactId,
    },
    EmptySectionName {
        artifact: GeneratedArtifactId,
    },
    DuplicateSectionOrder {
        artifact: GeneratedArtifactId,
        order: u32,
    },
    MissingSectionSource {
        artifact: GeneratedArtifactId,
        source: GeneratorSourceRef,
    },
    EmptyOpcodeRecordName,
    DuplicateOpcodeRecordName {
        name: &'static str,
    },
    EmptyOpcodeArgumentName {
        record: &'static str,
    },
    DuplicateOpcodeArgument {
        record: &'static str,
        name: &'static str,
    },
    EmptyOpcodeMetadataName {
        record: &'static str,
    },
    DuplicateOpcodeMetadata {
        record: &'static str,
        name: &'static str,
    },
    EmptyOpcodeCheckpointName {
        record: &'static str,
    },
    DuplicateOpcodeCheckpoint {
        record: &'static str,
        ordinal: u8,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedArtifactIntegrationSummary {
    pub artifact: GeneratedArtifactId,
    pub kind: GeneratedArtifactKind,
    pub path: &'static str,
    pub source_count: u32,
    pub section_count: u32,
    pub stale_section_count: u32,
    pub owner: GeneratedFileDescriptorOwner,
    pub mutation_authority: GeneratedFileMutationAuthority,
    pub ready_for_consumers: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedArtifactIntegrationReport {
    pub artifacts: Vec<GeneratedArtifactIntegrationSummary>,
    pub validation: GeneratedValidationReport,
}

impl GeneratedArtifactIntegrationReport {
    pub fn is_ready_for_consumers(&self) -> bool {
        self.validation.is_valid()
            && self
                .artifacts
                .iter()
                .all(|artifact| artifact.ready_for_consumers)
    }
}

pub fn summarize_generated_artifact_integration(
    registry: GeneratedFileRegistry,
) -> GeneratedArtifactIntegrationReport {
    let validation = registry.validate();
    let artifacts = registry
        .files()
        .iter()
        .map(|file| {
            let source_count = file
                .sections()
                .iter()
                .filter(|section| section.source.is_some())
                .count() as u32;
            let stale_section_count = file
                .sections()
                .iter()
                .filter(|section| section.freshness != GeneratedArtifactFreshness::SourceMatched)
                .count() as u32;
            GeneratedArtifactIntegrationSummary {
                artifact: file.artifact,
                kind: file.kind,
                path: file.path,
                source_count,
                section_count: file.sections().len() as u32,
                stale_section_count,
                owner: file.owner,
                mutation_authority: file.mutation_authority,
                ready_for_consumers: stale_section_count == 0
                    && validation
                        .findings
                        .iter()
                        .all(|finding| !finding_mentions_artifact(*finding, file.artifact)),
            }
        })
        .collect();

    GeneratedArtifactIntegrationReport {
        artifacts,
        validation,
    }
}

fn finding_mentions_artifact(
    finding: GeneratedValidationFinding,
    artifact: GeneratedArtifactId,
) -> bool {
    match finding {
        GeneratedValidationFinding::EmptyGeneratedFilePath { artifact: found }
        | GeneratedValidationFinding::DuplicateGeneratedArtifact { artifact: found }
        | GeneratedValidationFinding::EmptySectionName { artifact: found }
        | GeneratedValidationFinding::DuplicateSectionOrder {
            artifact: found, ..
        }
        | GeneratedValidationFinding::MissingSectionSource {
            artifact: found, ..
        } => found == artifact,
        GeneratedValidationFinding::EmptySourcePath { .. }
        | GeneratedValidationFinding::DuplicateSourceOrdinal { .. }
        | GeneratedValidationFinding::EmptyOpcodeRecordName
        | GeneratedValidationFinding::DuplicateOpcodeRecordName { .. }
        | GeneratedValidationFinding::EmptyOpcodeArgumentName { .. }
        | GeneratedValidationFinding::DuplicateOpcodeArgument { .. }
        | GeneratedValidationFinding::EmptyOpcodeMetadataName { .. }
        | GeneratedValidationFinding::DuplicateOpcodeMetadata { .. }
        | GeneratedValidationFinding::EmptyOpcodeCheckpointName { .. }
        | GeneratedValidationFinding::DuplicateOpcodeCheckpoint { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTIFACT: GeneratedArtifactId = GeneratedArtifactId(1);
    const SOURCE: StaticGeneratorSource = StaticGeneratorSource {
        ordinal: 0,
        kind: GeneratorSourceKind::RubyDsl,
        path: "bytecode.rb",
        revision: GeneratorRevision(1),
    };
    const SECTION: StaticGeneratedSection = StaticGeneratedSection {
        name: "opcodes",
        order: 0,
        source: Some(GeneratorSourceRef(0)),
        freshness: GeneratedArtifactFreshness::SourceMatched,
    };
    const FILE: GeneratedFileDescriptor = GeneratedFileDescriptor {
        artifact: ARTIFACT,
        kind: GeneratedArtifactKind::OpcodeTable,
        path: "opcodes.rs",
        sections: &[SECTION],
        comment_style: GeneratedCommentStyle::Rust,
        owner: GeneratedFileDescriptorOwner::TestFixture,
        mutation_authority: GeneratedFileMutationAuthority::SourceTreeRefresh,
    };
    const TYPE: StaticGeneratorType = StaticGeneratorType {
        name: "VirtualRegister",
        storage: GeneratorStorageKind::Register,
    };
    const ARG: StaticGeneratorArgument = StaticGeneratorArgument {
        name: "dst",
        ty: TYPE,
        role: GeneratorArgumentRole::Destination,
    };
    const RECORD: StaticOpcodeGeneratorRecord = StaticOpcodeGeneratorRecord {
        name: "op_test",
        section: "test",
        arguments: &[ARG],
        metadata: &[],
        checkpoints: &[],
        temporaries: &[],
        assertions: &[],
    };

    #[test]
    fn generated_file_registry_validation_accepts_consistent_descriptors() {
        let registry = GeneratedFileRegistry::new(&[SOURCE], &[FILE]);

        assert!(registry.validate().is_valid());
    }

    #[test]
    fn generated_file_builder_creates_checkable_plan() {
        let mut builder = GeneratedFilePlanBuilder::new(ARTIFACT, "opcodes.rs")
            .with_comment_style(GeneratedCommentStyle::Rust);
        builder.add_section(GeneratedSectionPlan {
            name: "opcodes".to_string(),
            order: 0,
            source: Some(GeneratorSourceRef(0)),
            freshness: GeneratedArtifactFreshness::SourceMatched,
        });
        let source = GeneratorSource {
            ordinal: 0,
            kind: GeneratorSourceKind::RubyDsl,
            path: "bytecode.rb".to_string(),
            revision: GeneratorRevision(1),
        };

        assert!(builder.finish().validate(&[source]).is_valid());
    }

    #[test]
    fn generated_file_plan_assembly_orders_sections_and_tracks_freshness() {
        let mut builder = GeneratedFilePlanBuilder::new(ARTIFACT, "opcodes.rs")
            .with_comment_style(GeneratedCommentStyle::Rust);
        builder.add_section(GeneratedSectionPlan {
            name: "late".to_string(),
            order: 2,
            source: Some(GeneratorSourceRef(0)),
            freshness: GeneratedArtifactFreshness::NeedsRegeneration,
        });
        builder.add_section(GeneratedSectionPlan {
            name: "early".to_string(),
            order: 1,
            source: Some(GeneratorSourceRef(0)),
            freshness: GeneratedArtifactFreshness::SourceMatched,
        });
        let source = GeneratorSource {
            ordinal: 0,
            kind: GeneratorSourceKind::RubyDsl,
            path: "bytecode.rb".to_string(),
            revision: GeneratorRevision(1),
        };

        let assembly = builder.finish().assemble(&[source]);

        assert_eq!(assembly.ordered_sections[0].name, "early");
        assert!(assembly.needs_regeneration);
        assert!(!assembly.is_ready_to_emit());
    }

    #[test]
    fn opcode_generator_registry_validation_reports_duplicates() {
        let registry = OpcodeGeneratorRegistry::new(&[RECORD, RECORD]);

        assert_eq!(
            registry.validate().findings,
            vec![GeneratedValidationFinding::DuplicateOpcodeRecordName { name: "op_test" }]
        );
    }

    #[test]
    fn generated_artifact_integration_reports_ready_artifacts() {
        let registry = GeneratedFileRegistry::new(&[SOURCE], &[FILE]);

        let report = summarize_generated_artifact_integration(registry);

        assert!(report.is_ready_for_consumers());
        assert_eq!(report.artifacts[0].artifact, ARTIFACT);
        assert_eq!(report.artifacts[0].section_count, 1);
        assert_eq!(report.artifacts[0].source_count, 1);
        assert_eq!(report.artifacts[0].stale_section_count, 0);
    }

    #[test]
    fn generated_artifact_integration_marks_stale_sections_not_ready() {
        const STALE_SECTION: StaticGeneratedSection = StaticGeneratedSection {
            name: "opcodes",
            order: 0,
            source: Some(GeneratorSourceRef(0)),
            freshness: GeneratedArtifactFreshness::NeedsRegeneration,
        };
        const STALE_FILE: GeneratedFileDescriptor = GeneratedFileDescriptor {
            artifact: ARTIFACT,
            kind: GeneratedArtifactKind::OpcodeTable,
            path: "opcodes.rs",
            sections: &[STALE_SECTION],
            comment_style: GeneratedCommentStyle::Rust,
            owner: GeneratedFileDescriptorOwner::TestFixture,
            mutation_authority: GeneratedFileMutationAuthority::SourceTreeRefresh,
        };

        let report = summarize_generated_artifact_integration(GeneratedFileRegistry::new(
            &[SOURCE],
            &[STALE_FILE],
        ));

        assert!(!report.is_ready_for_consumers());
        assert_eq!(report.artifacts[0].stale_section_count, 1);
    }
}
