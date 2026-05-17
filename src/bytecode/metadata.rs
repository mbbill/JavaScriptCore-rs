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
}

impl Default for InstructionMetadataPlan {
    fn default() -> Self {
        Self {
            bytecode_index: BytecodeIndex::default(),
            opcode: Opcode::Reserved,
            metadata_id: 0,
            fields: Vec::new(),
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
