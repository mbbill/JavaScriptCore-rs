use crate::bytecode::code_block::{BytecodeIndex, RuntimeSlot};
use crate::bytecode::opcode::{MetadataFieldSpec, Opcode, OpcodeId, OpcodeSchemaVersion};

/// Generated metadata layout for opcodes that carry runtime feedback.
///
/// This mirrors the split between `UnlinkedMetadataTable` and linked
/// `MetadataTable`: unlinked code records counts, offsets, alignment, and value
/// profile slots; linked code materializes mutable storage beside the
/// interpreter metadata pointer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MetadataLayout {
    pub schema_version: OpcodeSchemaVersion,
    pub offset_table: MetadataOffsetTable,
    pub opcode_records: Vec<OpcodeMetadataLayout>,
    pub value_profiles: MetadataValueProfileRegion,
    pub memory: MetadataTableMemoryLayout,
    pub phase: MetadataTablePhase,
}

impl MetadataLayout {
    pub fn validate(&self) -> MetadataValidationReport {
        let mut findings = Vec::new();
        validate_offset_table(
            self.schema_version,
            self.offset_table.encoding,
            &self.offset_table.entries,
            &mut findings,
        );
        validate_opcode_layouts(self.schema_version, &self.opcode_records, &mut findings);
        validate_memory_layout(
            self.schema_version,
            self.memory,
            &self.opcode_records,
            self.value_profiles,
            &mut findings,
        );
        MetadataValidationReport { findings }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MetadataOffsetTable {
    pub encoding: MetadataOffsetEncoding,
    pub entries: Vec<MetadataOffsetEntry>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum MetadataOffsetEncoding {
    #[default]
    Empty,
    Offset16,
    Offset32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct MetadataOffsetEntry {
    pub opcode_id: OpcodeId,
    pub start_offset: u32,
    pub end_offset: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpcodeMetadataLayout {
    pub opcode: Opcode,
    pub alignment: MetadataAlignment,
    pub stride: u32,
    pub count: u32,
    pub fields: Vec<MetadataFieldSpec>,
    pub semantic: BytecodeMetadataSemanticContract,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum MetadataAlignment {
    #[default]
    One,
    Two,
    Four,
    Eight,
    Pointer,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct MetadataValueProfileRegion {
    pub count: u32,
    pub first_negative_offset: i32,
    pub storage_order: ValueProfileStorageOrder,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ValueProfileStorageOrder {
    #[default]
    BeforeLinkingData,
    InlineWithOpcodeMetadata,
    SideTable,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct MetadataTableMemoryLayout {
    pub value_profile_bytes: u32,
    pub linking_data_bytes: u32,
    pub offset_table_bytes: u32,
    pub metadata_content_bytes: u32,
    pub maximum_alignment: MetadataAlignment,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum MetadataTablePhase {
    #[default]
    OpenForGeneration,
    FinalizedUnlinked,
    LinkedRuntime,
    DetachedForDestruction,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MetadataLinkingData {
    pub unlinked_table: Option<UnlinkedMetadataTableRef>,
    pub ref_count_slot: Option<RuntimeSlot>,
    pub did_optimize: MetadataTriState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct UnlinkedMetadataTableRef(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum MetadataTriState {
    Yes,
    No,
    #[default]
    Indeterminate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstructionMetadataPlan {
    pub bytecode_index: BytecodeIndex,
    pub opcode: Opcode,
    pub metadata_id: u32,
    pub fields: Vec<InstructionMetadataFieldPlan>,
    pub semantic: BytecodeMetadataSemanticContract,
}

impl Default for InstructionMetadataPlan {
    fn default() -> Self {
        Self {
            bytecode_index: BytecodeIndex::default(),
            opcode: Opcode::Reserved,
            metadata_id: 0,
            fields: Vec::new(),
            semantic: BytecodeMetadataSemanticContract::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstructionMetadataFieldPlan {
    pub spec: MetadataFieldSpec,
    pub binding: MetadataBinding,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum MetadataBinding {
    #[default]
    Unassigned,
    InlineOffset(u32),
    ValueProfileOffset(u32),
    RuntimeSlot(RuntimeSlot),
    GeneratedOpaque(u32),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct BytecodeMetadataSemanticContract {
    pub side_effects: MetadataSideEffectSet,
    pub exception: MetadataExceptionContract,
    pub observable_order: MetadataObservableOrder,
}

impl BytecodeMetadataSemanticContract {
    pub const fn none() -> Self {
        Self {
            side_effects: MetadataSideEffectSet::none(),
            exception: MetadataExceptionContract::CannotThrow,
            observable_order: MetadataObservableOrder::NoObservableEffect,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct MetadataSideEffectSet {
    pub reads_heap: bool,
    pub writes_heap: bool,
    pub allocates: bool,
    pub calls_host: bool,
    pub mutates_environment: bool,
    pub mutates_metadata: bool,
}

impl MetadataSideEffectSet {
    pub const fn none() -> Self {
        Self {
            reads_heap: false,
            writes_heap: false,
            allocates: false,
            calls_host: false,
            mutates_environment: false,
            mutates_metadata: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum MetadataExceptionContract {
    #[default]
    CannotThrow,
    MayThrow,
    AlwaysThrows,
    ThrowsOnTdz,
    ThrowsOnPrivateBrand,
    ThrowsOnUserCode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum MetadataObservableOrder {
    #[default]
    NoObservableEffect,
    BeforeValueProfile,
    AfterValueProfile,
    AroundHostCall,
}

/// Component that owns immutable bytecode metadata layout descriptors.
///
/// Layout descriptors are generated beside opcode schemas. Runtime metadata
/// storage may mutate under `CodeBlock` authority, but these records remain
/// static generated data.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum MetadataLayoutOwner {
    #[default]
    OpcodeSchemaGenerator,
    BytecodeRuntime,
    TestFixture,
}

/// Generated-data provenance for a metadata layout snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct MetadataLayoutProvenance {
    pub generator: &'static str,
    pub source: &'static str,
    pub schema_version: OpcodeSchemaVersion,
}

impl MetadataLayoutProvenance {
    pub const fn new(
        generator: &'static str,
        source: &'static str,
        schema_version: OpcodeSchemaVersion,
    ) -> Self {
        Self {
            generator,
            source,
            schema_version,
        }
    }
}

/// Immutable offset table layout for generated metadata records.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StaticMetadataOffsetTable {
    pub encoding: MetadataOffsetEncoding,
    pub entries: &'static [MetadataOffsetEntry],
}

impl StaticMetadataOffsetTable {
    pub const fn new(
        encoding: MetadataOffsetEncoding,
        entries: &'static [MetadataOffsetEntry],
    ) -> Self {
        Self { encoding, entries }
    }

    pub const fn entries(self) -> &'static [MetadataOffsetEntry] {
        self.entries
    }
}

/// Immutable per-opcode metadata layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticOpcodeMetadataLayout {
    pub opcode: Opcode,
    pub alignment: MetadataAlignment,
    pub stride: u32,
    pub count: u32,
    pub fields: &'static [MetadataFieldSpec],
    pub semantic: BytecodeMetadataSemanticContract,
}

impl StaticOpcodeMetadataLayout {
    pub const fn fields(self) -> &'static [MetadataFieldSpec] {
        self.fields
    }
}

/// Immutable generated metadata table layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticMetadataLayout {
    pub schema_version: OpcodeSchemaVersion,
    pub offset_table: StaticMetadataOffsetTable,
    pub opcode_records: &'static [StaticOpcodeMetadataLayout],
    pub value_profiles: MetadataValueProfileRegion,
    pub memory: MetadataTableMemoryLayout,
    pub phase: MetadataTablePhase,
    pub owner: MetadataLayoutOwner,
    pub provenance: MetadataLayoutProvenance,
}

impl StaticMetadataLayout {
    pub const fn opcode_records(self) -> &'static [StaticOpcodeMetadataLayout] {
        self.opcode_records
    }

    pub fn layout_for_opcode(self, opcode: Opcode) -> Option<&'static StaticOpcodeMetadataLayout> {
        self.opcode_records
            .iter()
            .find(|record| record.opcode == opcode)
    }

    pub fn offset_for_opcode_id(self, opcode_id: OpcodeId) -> Option<MetadataOffsetEntry> {
        self.offset_table
            .entries()
            .iter()
            .find(|entry| entry.opcode_id == opcode_id)
            .copied()
    }

    pub fn execution_lookup(self) -> ExecutionMetadataLookup {
        ExecutionMetadataLookup { layout: self }
    }

    pub fn validate(self) -> MetadataValidationReport {
        let mut findings = Vec::new();
        if self.provenance.schema_version != self.schema_version {
            findings.push(MetadataValidationFinding::ProvenanceSchemaMismatch {
                layout: self.schema_version,
                provenance: self.provenance.schema_version,
            });
        }
        if matches!(self.phase, MetadataTablePhase::LinkedRuntime) {
            findings.push(MetadataValidationFinding::StaticLayoutHasLinkedPhase {
                schema_version: self.schema_version,
            });
        }
        validate_offset_table(
            self.schema_version,
            self.offset_table.encoding,
            self.offset_table.entries(),
            &mut findings,
        );
        validate_static_opcode_layouts(self.schema_version, self.opcode_records(), &mut findings);
        validate_static_memory_layout(
            self.schema_version,
            self.memory,
            self.opcode_records(),
            self.value_profiles,
            &mut findings,
        );
        MetadataValidationReport { findings }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionMetadataLookup {
    layout: StaticMetadataLayout,
}

impl ExecutionMetadataLookup {
    pub const fn layout(self) -> StaticMetadataLayout {
        self.layout
    }

    pub fn record_for_opcode(self, opcode: Opcode) -> Option<&'static StaticOpcodeMetadataLayout> {
        self.layout.layout_for_opcode(opcode)
    }

    pub fn field_for_opcode(
        self,
        opcode: Opcode,
        name: &str,
    ) -> Option<&'static MetadataFieldSpec> {
        self.record_for_opcode(opcode)
            .and_then(|record| record.fields().iter().find(|field| field.name == name))
    }

    pub fn offset_for_opcode_id(self, opcode_id: OpcodeId) -> Option<MetadataOffsetEntry> {
        self.layout.offset_for_opcode_id(opcode_id)
    }

    pub fn execution_contract_for_opcode(
        self,
        opcode: Opcode,
    ) -> Option<BytecodeMetadataSemanticContract> {
        self.record_for_opcode(opcode).map(|record| record.semantic)
    }
}

/// Registry of generated metadata layouts keyed by opcode schema version.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MetadataLayoutRegistry {
    pub layouts: &'static [StaticMetadataLayout],
}

impl MetadataLayoutRegistry {
    pub const fn new(layouts: &'static [StaticMetadataLayout]) -> Self {
        Self { layouts }
    }

    pub const fn layouts(self) -> &'static [StaticMetadataLayout] {
        self.layouts
    }

    pub fn layout_for_schema(
        self,
        schema_version: OpcodeSchemaVersion,
    ) -> Option<&'static StaticMetadataLayout> {
        self.layouts
            .iter()
            .find(|layout| layout.schema_version == schema_version)
    }

    pub fn validate(self) -> MetadataValidationReport {
        let mut findings = Vec::new();
        for (index, layout) in self.layouts.iter().enumerate() {
            if self.layouts[..index]
                .iter()
                .any(|candidate| candidate.schema_version == layout.schema_version)
            {
                findings.push(MetadataValidationFinding::DuplicateSchemaVersion {
                    schema_version: layout.schema_version,
                });
            }
            findings.extend(layout.validate().findings);
        }
        MetadataValidationReport { findings }
    }
}

fn validate_offset_table(
    schema_version: OpcodeSchemaVersion,
    encoding: MetadataOffsetEncoding,
    entries: &[MetadataOffsetEntry],
    findings: &mut Vec<MetadataValidationFinding>,
) {
    if encoding == MetadataOffsetEncoding::Empty && !entries.is_empty() {
        findings.push(MetadataValidationFinding::OffsetEncodingMismatch { schema_version });
    }
    if encoding != MetadataOffsetEncoding::Empty && entries.is_empty() {
        findings.push(MetadataValidationFinding::OffsetEncodingMismatch { schema_version });
    }

    for (index, entry) in entries.iter().enumerate() {
        if entry.start_offset > entry.end_offset {
            findings.push(MetadataValidationFinding::UnorderedOffsetEntry {
                schema_version,
                opcode_id: entry.opcode_id,
            });
        }
        if let Some(previous) = entries.get(index.wrapping_sub(1)) {
            if index > 0 && previous.end_offset > entry.start_offset {
                findings.push(MetadataValidationFinding::OverlappingOffsetEntry {
                    schema_version,
                    previous: previous.opcode_id,
                    current: entry.opcode_id,
                });
            }
        }
    }
}

fn validate_opcode_layouts(
    schema_version: OpcodeSchemaVersion,
    records: &[OpcodeMetadataLayout],
    findings: &mut Vec<MetadataValidationFinding>,
) {
    for (index, record) in records.iter().enumerate() {
        validate_record_shape(
            schema_version,
            record.opcode,
            record.stride,
            record.count,
            &record.fields,
            record.semantic,
            findings,
        );
        if records[..index]
            .iter()
            .any(|candidate| candidate.opcode == record.opcode)
        {
            findings.push(MetadataValidationFinding::DuplicateOpcodeLayout {
                schema_version,
                opcode: record.opcode,
            });
        }
    }
}

fn validate_static_opcode_layouts(
    schema_version: OpcodeSchemaVersion,
    records: &[StaticOpcodeMetadataLayout],
    findings: &mut Vec<MetadataValidationFinding>,
) {
    for (index, record) in records.iter().enumerate() {
        validate_record_shape(
            schema_version,
            record.opcode,
            record.stride,
            record.count,
            record.fields,
            record.semantic,
            findings,
        );
        if records[..index]
            .iter()
            .any(|candidate| candidate.opcode == record.opcode)
        {
            findings.push(MetadataValidationFinding::DuplicateOpcodeLayout {
                schema_version,
                opcode: record.opcode,
            });
        }
    }
}

fn validate_record_shape(
    schema_version: OpcodeSchemaVersion,
    opcode: Opcode,
    stride: u32,
    count: u32,
    fields: &[MetadataFieldSpec],
    semantic: BytecodeMetadataSemanticContract,
    findings: &mut Vec<MetadataValidationFinding>,
) {
    if count > 0 && stride == 0 {
        findings.push(MetadataValidationFinding::ZeroStrideWithRecords {
            schema_version,
            opcode,
        });
    }
    if semantic.side_effects.mutates_metadata && fields.is_empty() {
        findings.push(MetadataValidationFinding::MetadataMutationWithoutFields {
            schema_version,
            opcode,
        });
    }
    if (semantic.side_effects.calls_host || semantic.side_effects.allocates)
        && semantic.exception == MetadataExceptionContract::CannotThrow
    {
        findings.push(MetadataValidationFinding::EffectRequiresExceptionContract {
            schema_version,
            opcode,
            exception: semantic.exception,
        });
    }
}

fn validate_memory_layout(
    schema_version: OpcodeSchemaVersion,
    memory: MetadataTableMemoryLayout,
    records: &[OpcodeMetadataLayout],
    value_profiles: MetadataValueProfileRegion,
    findings: &mut Vec<MetadataValidationFinding>,
) {
    let maximum_alignment = records
        .iter()
        .map(|record| record.alignment)
        .max_by_key(|alignment| alignment_rank(*alignment))
        .unwrap_or(MetadataAlignment::One);
    validate_memory_shape(
        schema_version,
        memory,
        maximum_alignment,
        value_profiles,
        findings,
    );
}

fn validate_static_memory_layout(
    schema_version: OpcodeSchemaVersion,
    memory: MetadataTableMemoryLayout,
    records: &[StaticOpcodeMetadataLayout],
    value_profiles: MetadataValueProfileRegion,
    findings: &mut Vec<MetadataValidationFinding>,
) {
    let maximum_alignment = records
        .iter()
        .map(|record| record.alignment)
        .max_by_key(|alignment| alignment_rank(*alignment))
        .unwrap_or(MetadataAlignment::One);
    validate_memory_shape(
        schema_version,
        memory,
        maximum_alignment,
        value_profiles,
        findings,
    );
}

fn validate_memory_shape(
    schema_version: OpcodeSchemaVersion,
    memory: MetadataTableMemoryLayout,
    required_alignment: MetadataAlignment,
    value_profiles: MetadataValueProfileRegion,
    findings: &mut Vec<MetadataValidationFinding>,
) {
    if alignment_rank(memory.maximum_alignment) < alignment_rank(required_alignment) {
        findings.push(MetadataValidationFinding::MaximumAlignmentTooSmall {
            schema_version,
            required: required_alignment,
            actual: memory.maximum_alignment,
        });
    }
    if value_profiles.count > 0 && memory.value_profile_bytes == 0 {
        findings.push(MetadataValidationFinding::MissingValueProfileStorage { schema_version });
    }
}

fn alignment_rank(alignment: MetadataAlignment) -> u8 {
    match alignment {
        MetadataAlignment::One => 1,
        MetadataAlignment::Two => 2,
        MetadataAlignment::Four => 4,
        MetadataAlignment::Eight => 8,
        MetadataAlignment::Pointer => 8,
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MetadataValidationReport {
    pub findings: Vec<MetadataValidationFinding>,
}

impl MetadataValidationReport {
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataValidationFinding {
    ProvenanceSchemaMismatch {
        layout: OpcodeSchemaVersion,
        provenance: OpcodeSchemaVersion,
    },
    StaticLayoutHasLinkedPhase {
        schema_version: OpcodeSchemaVersion,
    },
    DuplicateSchemaVersion {
        schema_version: OpcodeSchemaVersion,
    },
    OffsetEncodingMismatch {
        schema_version: OpcodeSchemaVersion,
    },
    UnorderedOffsetEntry {
        schema_version: OpcodeSchemaVersion,
        opcode_id: OpcodeId,
    },
    OverlappingOffsetEntry {
        schema_version: OpcodeSchemaVersion,
        previous: OpcodeId,
        current: OpcodeId,
    },
    DuplicateOpcodeLayout {
        schema_version: OpcodeSchemaVersion,
        opcode: Opcode,
    },
    ZeroStrideWithRecords {
        schema_version: OpcodeSchemaVersion,
        opcode: Opcode,
    },
    MaximumAlignmentTooSmall {
        schema_version: OpcodeSchemaVersion,
        required: MetadataAlignment,
        actual: MetadataAlignment,
    },
    MissingValueProfileStorage {
        schema_version: OpcodeSchemaVersion,
    },
    MetadataMutationWithoutFields {
        schema_version: OpcodeSchemaVersion,
        opcode: Opcode,
    },
    EffectRequiresExceptionContract {
        schema_version: OpcodeSchemaVersion,
        opcode: Opcode,
        exception: MetadataExceptionContract,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIELD: MetadataFieldSpec = MetadataFieldSpec {
        name: "profile",
        kind: crate::bytecode::opcode::MetadataFieldKind::ValueProfile,
        mutability: crate::bytecode::opcode::MetadataMutability::LinkedMutable,
    };
    const FIELDS: &[MetadataFieldSpec] = &[FIELD];
    const RECORDS: &[StaticOpcodeMetadataLayout] = &[StaticOpcodeMetadataLayout {
        opcode: Opcode::Reserved,
        alignment: MetadataAlignment::Four,
        stride: 4,
        count: 1,
        fields: FIELDS,
        semantic: BytecodeMetadataSemanticContract {
            side_effects: MetadataSideEffectSet {
                mutates_metadata: true,
                ..MetadataSideEffectSet::none()
            },
            ..BytecodeMetadataSemanticContract::none()
        },
    }];

    #[test]
    fn metadata_validation_accepts_static_layout() {
        let layout = StaticMetadataLayout {
            schema_version: OpcodeSchemaVersion(1),
            offset_table: StaticMetadataOffsetTable::new(MetadataOffsetEncoding::Empty, &[]),
            opcode_records: RECORDS,
            value_profiles: MetadataValueProfileRegion::default(),
            memory: MetadataTableMemoryLayout {
                maximum_alignment: MetadataAlignment::Four,
                metadata_content_bytes: 4,
                ..MetadataTableMemoryLayout::default()
            },
            phase: MetadataTablePhase::FinalizedUnlinked,
            owner: MetadataLayoutOwner::TestFixture,
            provenance: MetadataLayoutProvenance::new("test", "test", OpcodeSchemaVersion(1)),
        };

        assert!(layout.validate().is_valid());
    }

    #[test]
    fn metadata_validation_reports_descriptor_mismatches() {
        const BAD_RECORDS: &[StaticOpcodeMetadataLayout] = &[StaticOpcodeMetadataLayout {
            opcode: Opcode::Reserved,
            alignment: MetadataAlignment::Eight,
            stride: 0,
            count: 1,
            fields: FIELDS,
            semantic: BytecodeMetadataSemanticContract {
                side_effects: MetadataSideEffectSet {
                    calls_host: true,
                    ..MetadataSideEffectSet::none()
                },
                ..BytecodeMetadataSemanticContract::none()
            },
        }];
        const BAD_OFFSETS: &[MetadataOffsetEntry] = &[MetadataOffsetEntry {
            opcode_id: OpcodeId::from_generated_index(0),
            start_offset: 4,
            end_offset: 2,
        }];
        let layout = StaticMetadataLayout {
            schema_version: OpcodeSchemaVersion(1),
            offset_table: StaticMetadataOffsetTable::new(
                MetadataOffsetEncoding::Offset16,
                BAD_OFFSETS,
            ),
            opcode_records: BAD_RECORDS,
            value_profiles: MetadataValueProfileRegion {
                count: 1,
                ..MetadataValueProfileRegion::default()
            },
            memory: MetadataTableMemoryLayout::default(),
            phase: MetadataTablePhase::LinkedRuntime,
            owner: MetadataLayoutOwner::TestFixture,
            provenance: MetadataLayoutProvenance::new("test", "test", OpcodeSchemaVersion(2)),
        };

        let findings = layout.validate().findings;
        assert!(
            findings.contains(&MetadataValidationFinding::ProvenanceSchemaMismatch {
                layout: OpcodeSchemaVersion(1),
                provenance: OpcodeSchemaVersion(2),
            })
        );
        assert!(
            findings.contains(&MetadataValidationFinding::StaticLayoutHasLinkedPhase {
                schema_version: OpcodeSchemaVersion(1),
            })
        );
        assert!(
            findings.contains(&MetadataValidationFinding::ZeroStrideWithRecords {
                schema_version: OpcodeSchemaVersion(1),
                opcode: Opcode::Reserved,
            })
        );
        assert!(findings.contains(
            &MetadataValidationFinding::EffectRequiresExceptionContract {
                schema_version: OpcodeSchemaVersion(1),
                opcode: Opcode::Reserved,
                exception: MetadataExceptionContract::CannotThrow,
            }
        ));
    }

    #[test]
    fn execution_metadata_lookup_resolves_records_fields_and_offsets() {
        const OFFSETS: &[MetadataOffsetEntry] = &[MetadataOffsetEntry {
            opcode_id: OpcodeId::from_generated_index(9),
            start_offset: 16,
            end_offset: 20,
        }];
        let layout = StaticMetadataLayout {
            schema_version: OpcodeSchemaVersion(1),
            offset_table: StaticMetadataOffsetTable::new(MetadataOffsetEncoding::Offset16, OFFSETS),
            opcode_records: RECORDS,
            value_profiles: MetadataValueProfileRegion::default(),
            memory: MetadataTableMemoryLayout {
                maximum_alignment: MetadataAlignment::Four,
                metadata_content_bytes: 4,
                ..MetadataTableMemoryLayout::default()
            },
            phase: MetadataTablePhase::FinalizedUnlinked,
            owner: MetadataLayoutOwner::TestFixture,
            provenance: MetadataLayoutProvenance::new("test", "test", OpcodeSchemaVersion(1)),
        };

        let lookup = layout.execution_lookup();

        assert_eq!(
            lookup.record_for_opcode(Opcode::Reserved).unwrap().stride,
            4
        );
        assert_eq!(
            lookup.field_for_opcode(Opcode::Reserved, "profile"),
            Some(&FIELD)
        );
        assert_eq!(
            lookup.offset_for_opcode_id(OpcodeId::from_generated_index(9)),
            Some(OFFSETS[0])
        );
        assert_eq!(
            lookup.execution_contract_for_opcode(Opcode::Reserved),
            Some(RECORDS[0].semantic)
        );
    }
}
