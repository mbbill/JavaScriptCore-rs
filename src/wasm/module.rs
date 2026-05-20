//! WebAssembly module and module-record integration placeholders.
//!
//! The module loader can reserve these records for WebAssembly source types,
//! while unsupported construction should fail at the host boundary until Wasm is
//! implemented. Module information is immutable after validation in JSC; this
//! skeleton mirrors that ownership by separating parsed module metadata from
//! GC-owned JS wrappers and runtime instances.

use crate::bytecode::SourceProviderId;
use crate::modules::ModuleRecordId;
use crate::runtime::ObjectId;
use crate::wasm::{WasmFunctionSignature, WasmMemoryDescriptor, WasmTableDescriptor};

/// Stable identity for parsed Wasm module information.
///
/// This is separate from `modules::ModuleRecordId`: module records own JS
/// loader state, while `WasmModuleId` owns validated Wasm metadata.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmModuleId(pub u64);

/// Function index in Wasm function index space.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmFunctionIndex(pub u32);

/// Function index after import-space mapping is removed.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmFunctionCodeIndex(pub u32);

/// Import index in module import order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmImportIndex(pub u32);

/// Export index in module export order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmExportIndex(pub u32);

/// Global index in module global index space.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmGlobalIndex(pub u32);

/// Exception/tag index in module exception index space.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmTagIndex(pub u32);

/// Data segment index in module data segment order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmDataSegmentIndex(pub u32);

/// Element segment index in module element segment order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmElementSegmentIndex(pub u32);

/// Canonical type-signature table index.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WasmTypeSignatureIndex(pub u32);

/// Heap type family used by reference type descriptors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmHeapType {
    Func,
    Extern,
    Any,
    Eq,
    I31,
    Struct,
    Array,
    Concrete(WasmTypeSignatureIndex),
}

/// Type descriptor category in a parsed Wasm module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmTypeKind {
    Function,
    Struct,
    Array,
    Continuation,
}

/// Field mutability for typed objects and globals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmFieldMutability {
    Const,
    Var,
}

/// Packed storage family for Wasm GC fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmPackedType {
    I8,
    I16,
}

/// Static struct-field descriptor for Wasm GC types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmStructFieldDescriptor {
    pub value_type: crate::wasm::WasmValueType,
    pub mutability: WasmFieldMutability,
    pub packed_type: Option<WasmPackedType>,
}

/// Static function type descriptor from the type section.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmFunctionTypeDescriptor {
    pub index: WasmTypeSignatureIndex,
    pub params: Vec<crate::wasm::WasmValueType>,
    pub results: Vec<crate::wasm::WasmValueType>,
    pub supertype: Option<WasmTypeSignatureIndex>,
    pub is_final: bool,
}

/// Static struct type descriptor from the type section.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmStructTypeDescriptor {
    pub index: WasmTypeSignatureIndex,
    pub fields: Vec<WasmStructFieldDescriptor>,
    pub supertype: Option<WasmTypeSignatureIndex>,
    pub is_final: bool,
}

/// Static array type descriptor from the type section.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmArrayTypeDescriptor {
    pub index: WasmTypeSignatureIndex,
    pub element: WasmStructFieldDescriptor,
    pub supertype: Option<WasmTypeSignatureIndex>,
    pub is_final: bool,
}

/// Static module-local type descriptor.
///
/// Validation owns mutation while decoding the type section. After module
/// publication, import/export wrappers and compile tiers borrow these shapes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmTypeDescriptor {
    pub index: WasmTypeSignatureIndex,
    pub kind: WasmTypeKind,
    pub function: Option<WasmFunctionTypeDescriptor>,
    pub struct_type: Option<WasmStructTypeDescriptor>,
    pub array_type: Option<WasmArrayTypeDescriptor>,
    pub recursive_group: Option<u32>,
}

/// Source path through which Wasm entered the engine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmSourceKind {
    BinaryModule,
    StreamingBinary,
    ModuleImport,
    HostProvided,
}

/// Validation stage reached by parsed module information.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmValidationState {
    BytesReceived,
    HeaderValidated,
    SectionsDecoded,
    TypesValidated,
    ImportsValidated,
    FunctionsValidated,
    DataAndElementsValidated,
    Complete,
    Failed,
}

/// Known section ordering tracked during validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmSection {
    Begin,
    Type,
    Import,
    Function,
    Table,
    Memory,
    Global,
    Export,
    Start,
    Element,
    Code,
    Data,
    DataCount,
    Exception,
    Custom,
}

/// Registry authority for immutable Wasm schema rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmRegistryAuthority {
    StaticSpecTable,
    FeatureConfiguration,
    ModuleValidation,
}

/// Static section descriptor from the binary format.
///
/// The binary-format table is immutable spec data. Validation records actual
/// section occurrences in `WasmModuleInfo` but must not mutate this registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmSectionDescriptor {
    pub section: WasmSection,
    pub binary_id: Option<u8>,
    pub canonical_name: &'static str,
    pub order: u8,
    pub repeatable: bool,
    pub required_feature: Option<WasmModuleFeature>,
    pub authority: WasmRegistryAuthority,
}

/// Export category reserved for module linking and JS wrappers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmExportKind {
    Function,
    Memory,
    Table,
    Global,
    Tag,
}

/// Import category reserved for linking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmImportKind {
    Function,
    Memory,
    Table,
    Global,
    Tag,
}

/// Module compile option surface that affects later runtime ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmModuleFeature {
    ReferenceTypes,
    Gc,
    Simd,
    Threads,
    Exceptions,
    Memory64,
    TailCalls,
    ImportedStringConstants,
    BuiltinSets,
}

/// How a WebAssembly feature is exposed to the engine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmFeatureStatus {
    AlwaysOn,
    RuntimeFlag,
    Experimental,
}

/// Static descriptor for a WebAssembly proposal or implementation feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmFeatureDescriptor {
    pub feature: WasmModuleFeature,
    pub name: &'static str,
    pub status: WasmFeatureStatus,
    pub required_section: Option<WasmSection>,
    pub authority: WasmRegistryAuthority,
}

/// Static import metadata. Names remain opaque identities until string modules
/// exist; object and module resolution is owned by the JS module loader.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmImportDescriptor {
    pub index: WasmImportIndex,
    pub kind: WasmImportKind,
    pub module_name: Option<u32>,
    pub field_name: Option<u32>,
    pub function: Option<WasmFunctionIndex>,
    pub type_signature: Option<WasmTypeSignatureIndex>,
    pub memory: Option<crate::wasm::WasmMemoryIndex>,
    pub table: Option<crate::wasm::WasmTableIndex>,
    pub global: Option<WasmGlobalIndex>,
    pub tag: Option<WasmTagIndex>,
    pub hidden_from_reflection: bool,
}

/// Static export metadata used by wrappers and module records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmExportDescriptor {
    pub index: WasmExportIndex,
    pub kind: WasmExportKind,
    pub name: Option<u32>,
    pub function: Option<WasmFunctionIndex>,
    pub memory: Option<crate::wasm::WasmMemoryIndex>,
    pub table: Option<crate::wasm::WasmTableIndex>,
    pub global: Option<WasmGlobalIndex>,
    pub tag: Option<WasmTagIndex>,
    pub type_signature: Option<WasmTypeSignatureIndex>,
}

/// Static global declaration from a module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmGlobalDescriptor {
    pub index: WasmGlobalIndex,
    pub kind: crate::wasm::WasmGlobalKind,
    pub mutable: bool,
    pub imported: bool,
    pub initializer_expression: Option<u32>,
}

/// Static exception/tag declaration from a module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmTagDescriptor {
    pub index: WasmTagIndex,
    pub type_signature: WasmTypeSignatureIndex,
    pub imported: bool,
}

/// Data segment metadata decoded from the data section.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmDataSegmentDescriptor {
    pub index: WasmDataSegmentIndex,
    pub memory: Option<crate::wasm::WasmMemoryIndex>,
    pub initializer_expression: Option<u32>,
    pub byte_count: u32,
    pub passive: bool,
}

/// Element segment metadata decoded from the element section.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmElementSegmentDescriptor {
    pub index: WasmElementSegmentIndex,
    pub table: Option<crate::wasm::WasmTableIndex>,
    pub initializer_expression: Option<u32>,
    pub element_count: u32,
    pub passive: bool,
}

/// Custom section retained for names, source maps, branch hints, and tooling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmCustomSectionDescriptor {
    pub name: Option<u32>,
    pub byte_offset: u32,
    pub byte_count: u32,
    pub section_index: u32,
    pub purpose: WasmCustomSectionPurpose,
}

/// Engine responsibility associated with a custom section.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmCustomSectionPurpose {
    Name,
    SourceMappingUrl,
    BranchHints,
    DebugInfo,
    User,
}

const WASM_SECTION_DESCRIPTORS: &[WasmSectionDescriptor] = &[
    WasmSectionDescriptor {
        section: WasmSection::Custom,
        binary_id: Some(0),
        canonical_name: "custom",
        order: 0,
        repeatable: true,
        required_feature: None,
        authority: WasmRegistryAuthority::StaticSpecTable,
    },
    WasmSectionDescriptor {
        section: WasmSection::Type,
        binary_id: Some(1),
        canonical_name: "type",
        order: 1,
        repeatable: false,
        required_feature: None,
        authority: WasmRegistryAuthority::StaticSpecTable,
    },
    WasmSectionDescriptor {
        section: WasmSection::Import,
        binary_id: Some(2),
        canonical_name: "import",
        order: 2,
        repeatable: false,
        required_feature: None,
        authority: WasmRegistryAuthority::StaticSpecTable,
    },
    WasmSectionDescriptor {
        section: WasmSection::Function,
        binary_id: Some(3),
        canonical_name: "function",
        order: 3,
        repeatable: false,
        required_feature: None,
        authority: WasmRegistryAuthority::StaticSpecTable,
    },
    WasmSectionDescriptor {
        section: WasmSection::Table,
        binary_id: Some(4),
        canonical_name: "table",
        order: 4,
        repeatable: false,
        required_feature: None,
        authority: WasmRegistryAuthority::StaticSpecTable,
    },
    WasmSectionDescriptor {
        section: WasmSection::Memory,
        binary_id: Some(5),
        canonical_name: "memory",
        order: 5,
        repeatable: false,
        required_feature: None,
        authority: WasmRegistryAuthority::StaticSpecTable,
    },
    WasmSectionDescriptor {
        section: WasmSection::Global,
        binary_id: Some(6),
        canonical_name: "global",
        order: 6,
        repeatable: false,
        required_feature: None,
        authority: WasmRegistryAuthority::StaticSpecTable,
    },
    WasmSectionDescriptor {
        section: WasmSection::Export,
        binary_id: Some(7),
        canonical_name: "export",
        order: 8,
        repeatable: false,
        required_feature: None,
        authority: WasmRegistryAuthority::StaticSpecTable,
    },
    WasmSectionDescriptor {
        section: WasmSection::Start,
        binary_id: Some(8),
        canonical_name: "start",
        order: 9,
        repeatable: false,
        required_feature: None,
        authority: WasmRegistryAuthority::StaticSpecTable,
    },
    WasmSectionDescriptor {
        section: WasmSection::Element,
        binary_id: Some(9),
        canonical_name: "element",
        order: 10,
        repeatable: false,
        required_feature: None,
        authority: WasmRegistryAuthority::StaticSpecTable,
    },
    WasmSectionDescriptor {
        section: WasmSection::Code,
        binary_id: Some(10),
        canonical_name: "code",
        order: 12,
        repeatable: false,
        required_feature: None,
        authority: WasmRegistryAuthority::StaticSpecTable,
    },
    WasmSectionDescriptor {
        section: WasmSection::Data,
        binary_id: Some(11),
        canonical_name: "data",
        order: 13,
        repeatable: false,
        required_feature: None,
        authority: WasmRegistryAuthority::StaticSpecTable,
    },
    WasmSectionDescriptor {
        section: WasmSection::DataCount,
        binary_id: Some(12),
        canonical_name: "data_count",
        order: 11,
        repeatable: false,
        required_feature: None,
        authority: WasmRegistryAuthority::StaticSpecTable,
    },
    WasmSectionDescriptor {
        section: WasmSection::Exception,
        binary_id: Some(13),
        canonical_name: "tag",
        order: 7,
        repeatable: false,
        required_feature: Some(WasmModuleFeature::Exceptions),
        authority: WasmRegistryAuthority::StaticSpecTable,
    },
];

const WASM_FEATURE_DESCRIPTORS: &[WasmFeatureDescriptor] = &[
    WasmFeatureDescriptor {
        feature: WasmModuleFeature::ReferenceTypes,
        name: "reference-types",
        status: WasmFeatureStatus::AlwaysOn,
        required_section: None,
        authority: WasmRegistryAuthority::FeatureConfiguration,
    },
    WasmFeatureDescriptor {
        feature: WasmModuleFeature::Gc,
        name: "gc",
        status: WasmFeatureStatus::RuntimeFlag,
        required_section: Some(WasmSection::Type),
        authority: WasmRegistryAuthority::FeatureConfiguration,
    },
    WasmFeatureDescriptor {
        feature: WasmModuleFeature::Simd,
        name: "simd",
        status: WasmFeatureStatus::RuntimeFlag,
        required_section: None,
        authority: WasmRegistryAuthority::FeatureConfiguration,
    },
    WasmFeatureDescriptor {
        feature: WasmModuleFeature::Threads,
        name: "threads",
        status: WasmFeatureStatus::RuntimeFlag,
        required_section: None,
        authority: WasmRegistryAuthority::FeatureConfiguration,
    },
    WasmFeatureDescriptor {
        feature: WasmModuleFeature::Exceptions,
        name: "exceptions",
        status: WasmFeatureStatus::RuntimeFlag,
        required_section: Some(WasmSection::Exception),
        authority: WasmRegistryAuthority::FeatureConfiguration,
    },
    WasmFeatureDescriptor {
        feature: WasmModuleFeature::Memory64,
        name: "memory64",
        status: WasmFeatureStatus::RuntimeFlag,
        required_section: Some(WasmSection::Memory),
        authority: WasmRegistryAuthority::FeatureConfiguration,
    },
    WasmFeatureDescriptor {
        feature: WasmModuleFeature::TailCalls,
        name: "tail-calls",
        status: WasmFeatureStatus::Experimental,
        required_section: None,
        authority: WasmRegistryAuthority::FeatureConfiguration,
    },
    WasmFeatureDescriptor {
        feature: WasmModuleFeature::ImportedStringConstants,
        name: "imported-string-constants",
        status: WasmFeatureStatus::Experimental,
        required_section: Some(WasmSection::Import),
        authority: WasmRegistryAuthority::FeatureConfiguration,
    },
    WasmFeatureDescriptor {
        feature: WasmModuleFeature::BuiltinSets,
        name: "builtin-sets",
        status: WasmFeatureStatus::Experimental,
        required_section: None,
        authority: WasmRegistryAuthority::FeatureConfiguration,
    },
];

/// Immutable registry facade for Wasm binary/module schema metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmModuleSchemaRegistry {
    pub sections: &'static [WasmSectionDescriptor],
    pub features: &'static [WasmFeatureDescriptor],
    pub authority: WasmRegistryAuthority,
}

pub const WASM_MODULE_SCHEMA_REGISTRY: WasmModuleSchemaRegistry = WasmModuleSchemaRegistry {
    sections: WASM_SECTION_DESCRIPTORS,
    features: WASM_FEATURE_DESCRIPTORS,
    authority: WasmRegistryAuthority::StaticSpecTable,
};

pub const fn wasm_module_schema_registry() -> &'static WasmModuleSchemaRegistry {
    &WASM_MODULE_SCHEMA_REGISTRY
}

pub fn wasm_section_descriptor(section: WasmSection) -> Option<&'static WasmSectionDescriptor> {
    WASM_SECTION_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.section == section)
}

pub fn wasm_feature_descriptor(
    feature: WasmModuleFeature,
) -> Option<&'static WasmFeatureDescriptor> {
    WASM_FEATURE_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.feature == feature)
}

/// Branch hint metadata keyed by function and branch bytecode offsets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmBranchHintDescriptor {
    pub function: WasmFunctionCodeIndex,
    pub function_offset: u32,
    pub branch_offset: u32,
    pub hint: WasmBranchHint,
}

/// Branch-hint category without embedding parser policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmBranchHint {
    Invalid,
    Unlikely,
    Likely,
}

/// Validation-time function summary owned by ModuleInformation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmFunctionValidationSummary {
    pub code_index: WasmFunctionCodeIndex,
    pub start_offset: u32,
    pub end_offset: u32,
    pub finished_validating: bool,
    pub uses_simd: bool,
    pub uses_exceptions: bool,
    pub uses_atomics: bool,
    pub declared: bool,
    pub referenced: bool,
}

/// Parsed-module information reserved outside GC-owned wrappers.
/// Validation owns mutation until `validation_state` reaches `Complete`; after
/// that, Module, CalleeGroup, instances, and JS wrappers hold shared immutable
/// references except for concurrent reference-tracking summaries.
#[derive(Clone, Debug)]
pub struct WasmModuleInfo {
    pub id: WasmModuleId,
    pub source_kind: WasmSourceKind,
    pub source: Option<SourceProviderId>,
    pub features: Vec<WasmModuleFeature>,
    pub sections: Vec<WasmSectionDescriptor>,
    pub types: Vec<WasmTypeDescriptor>,
    pub imports: Vec<WasmImportDescriptor>,
    pub exports: Vec<WasmExportDescriptor>,
    pub function_signatures: Vec<WasmFunctionSignature>,
    pub memories: Vec<WasmMemoryDescriptor>,
    pub tables: Vec<WasmTableDescriptor>,
    pub globals: Vec<WasmGlobalDescriptor>,
    pub tags: Vec<WasmTagDescriptor>,
    pub data_segments: Vec<WasmDataSegmentDescriptor>,
    pub element_segments: Vec<WasmElementSegmentDescriptor>,
    pub custom_sections: Vec<WasmCustomSectionDescriptor>,
    pub branch_hints: Vec<WasmBranchHintDescriptor>,
    pub function_summaries: Vec<WasmFunctionValidationSummary>,
    pub declared_exports: Vec<WasmExportKind>,
    pub function_import_count: u32,
    pub function_count: u32,
    pub exception_import_count: u32,
    pub exception_count: u32,
    pub total_function_size: u64,
    pub small_function_count: u32,
    pub validation_state: WasmValidationState,
    pub start_function: Option<WasmFunctionIndex>,
    pub data_segment_count: Option<u32>,
    pub source_mapping_url: Option<u32>,
}

/// Structural error reported by Wasm module builders and validators.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WasmModuleValidationError {
    EmptyDescriptorName,
    DuplicateSection(WasmSection),
    DuplicateSectionBinaryId(u8),
    DuplicateFeature(WasmModuleFeature),
    UnknownSection(WasmSection),
    UnknownFeature(WasmModuleFeature),
    SectionOutOfOrder {
        previous: WasmSection,
        next: WasmSection,
    },
    SectionNotRepeatable(WasmSection),
    InvalidTypeDescriptor(WasmTypeSignatureIndex),
    InvalidTypeReference(WasmTypeSignatureIndex),
    InvalidImport {
        index: WasmImportIndex,
        kind: WasmImportKind,
    },
    InvalidExport {
        index: WasmExportIndex,
        kind: WasmExportKind,
    },
    InvalidIndexOrder {
        expected: u32,
        actual: u32,
    },
    InvalidLimit {
        minimum: u64,
        maximum: u64,
    },
    InvalidFunctionReference(WasmFunctionIndex),
    InvalidMemoryReference(crate::wasm::WasmMemoryIndex),
    InvalidTableReference(crate::wasm::WasmTableIndex),
    InvalidGlobalReference(WasmGlobalIndex),
    InvalidTagReference(WasmTagIndex),
    InvalidDataSegmentReference(WasmDataSegmentIndex),
    InvalidElementSegmentReference(WasmElementSegmentIndex),
    InvalidFunctionSummary(WasmFunctionCodeIndex),
    InvalidCustomSectionRange {
        byte_offset: u32,
        byte_count: u32,
    },
    DataSegmentCountMismatch {
        declared: u32,
        actual: usize,
    },
    FunctionCountMismatch {
        declared: u32,
        actual: usize,
    },
    ImportCountMismatch {
        declared: u32,
        actual: usize,
    },
    ExceptionCountMismatch {
        declared: u32,
        actual: usize,
    },
    RequiredFeatureDisabled(WasmModuleFeature),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmModuleValidationPlan {
    pub module: WasmModuleId,
    pub validation_state: WasmValidationState,
    pub required_features: Vec<WasmModuleFeature>,
    pub section_order: Vec<WasmSection>,
    pub type_count: usize,
    pub function_import_count: u32,
    pub function_count: u32,
    pub import_count: usize,
    pub export_count: usize,
    pub memory_count: usize,
    pub table_count: usize,
    pub tag_count: usize,
}

/// Non-executing semantic outcome for a validated module artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmValidationSemanticStatus {
    Incomplete,
    Accepted,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmValidationSemanticOutcome {
    pub module: WasmModuleId,
    pub status: WasmValidationSemanticStatus,
    pub validation_state: WasmValidationState,
    pub required_features: Vec<WasmModuleFeature>,
    pub enabled_feature_mask: u32,
    pub has_start_function: bool,
    pub import_count: usize,
    pub export_count: usize,
    pub function_count: u32,
    pub memory_count: usize,
    pub table_count: usize,
    pub global_count: usize,
    pub tag_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmImportSemanticDescriptor {
    pub index: WasmImportIndex,
    pub kind: WasmImportKind,
    pub module_name: Option<u32>,
    pub field_name: Option<u32>,
    pub type_signature: Option<WasmTypeSignatureIndex>,
    pub hidden_from_reflection: bool,
    pub requires_import_object_value: bool,
    pub creates_wasm_to_js_bridge: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmExportSemanticDescriptor {
    pub index: WasmExportIndex,
    pub kind: WasmExportKind,
    pub name: Option<u32>,
    pub type_signature: Option<WasmTypeSignatureIndex>,
    pub creates_js_wrapper: bool,
    pub exposes_runtime_object: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmModuleLinkingSemanticDescriptor {
    pub module: WasmModuleId,
    pub imports: Vec<WasmImportSemanticDescriptor>,
    pub exports: Vec<WasmExportSemanticDescriptor>,
    pub required_js_to_wasm_bridge_count: usize,
    pub required_wasm_to_js_bridge_count: usize,
    pub has_start_function: bool,
    pub allocates_memories: bool,
    pub allocates_tables: bool,
    pub allocates_globals: bool,
    pub exposes_hidden_imports: bool,
}

#[derive(Clone, Debug)]
pub struct WasmModuleInfoBuilder {
    info: WasmModuleInfo,
}

impl WasmModuleInfoBuilder {
    pub fn new(id: WasmModuleId, source_kind: WasmSourceKind) -> Self {
        Self {
            info: WasmModuleInfo {
                id,
                source_kind,
                source: None,
                features: Vec::new(),
                sections: Vec::new(),
                types: Vec::new(),
                imports: Vec::new(),
                exports: Vec::new(),
                function_signatures: Vec::new(),
                memories: Vec::new(),
                tables: Vec::new(),
                globals: Vec::new(),
                tags: Vec::new(),
                data_segments: Vec::new(),
                element_segments: Vec::new(),
                custom_sections: Vec::new(),
                branch_hints: Vec::new(),
                function_summaries: Vec::new(),
                declared_exports: Vec::new(),
                function_import_count: 0,
                function_count: 0,
                exception_import_count: 0,
                exception_count: 0,
                total_function_size: 0,
                small_function_count: 0,
                validation_state: WasmValidationState::BytesReceived,
                start_function: None,
                data_segment_count: None,
                source_mapping_url: None,
            },
        }
    }

    pub fn source(mut self, source: SourceProviderId) -> Self {
        self.info.source = Some(source);
        self
    }

    pub fn feature(mut self, feature: WasmModuleFeature) -> Self {
        self.info.features.push(feature);
        self
    }

    pub fn section(mut self, section: WasmSectionDescriptor) -> Self {
        self.info.sections.push(section);
        self
    }

    pub fn type_descriptor(mut self, descriptor: WasmTypeDescriptor) -> Self {
        self.info.types.push(descriptor);
        self
    }

    pub fn import(mut self, descriptor: WasmImportDescriptor) -> Self {
        self.info.imports.push(descriptor);
        self
    }

    pub fn export(mut self, descriptor: WasmExportDescriptor) -> Self {
        self.info.exports.push(descriptor);
        self
    }

    pub fn function_signature(mut self, signature: WasmFunctionSignature) -> Self {
        self.info.function_signatures.push(signature);
        self
    }

    pub fn memory(mut self, descriptor: WasmMemoryDescriptor) -> Self {
        self.info.memories.push(descriptor);
        self
    }

    pub fn table(mut self, descriptor: WasmTableDescriptor) -> Self {
        self.info.tables.push(descriptor);
        self
    }

    pub fn global(mut self, descriptor: WasmGlobalDescriptor) -> Self {
        self.info.globals.push(descriptor);
        self
    }

    pub fn tag(mut self, descriptor: WasmTagDescriptor) -> Self {
        self.info.tags.push(descriptor);
        self
    }

    pub fn data_segment(mut self, descriptor: WasmDataSegmentDescriptor) -> Self {
        self.info.data_segments.push(descriptor);
        self
    }

    pub fn element_segment(mut self, descriptor: WasmElementSegmentDescriptor) -> Self {
        self.info.element_segments.push(descriptor);
        self
    }

    pub fn custom_section(mut self, descriptor: WasmCustomSectionDescriptor) -> Self {
        self.info.custom_sections.push(descriptor);
        self
    }

    pub fn branch_hint(mut self, descriptor: WasmBranchHintDescriptor) -> Self {
        self.info.branch_hints.push(descriptor);
        self
    }

    pub fn function_summary(mut self, summary: WasmFunctionValidationSummary) -> Self {
        self.info.function_summaries.push(summary);
        self
    }

    pub fn declared_export(mut self, export: WasmExportKind) -> Self {
        self.info.declared_exports.push(export);
        self
    }

    pub fn function_counts(mut self, imported: u32, internal: u32) -> Self {
        self.info.function_import_count = imported;
        self.info.function_count = internal;
        self
    }

    pub fn exception_counts(mut self, imported: u32, internal: u32) -> Self {
        self.info.exception_import_count = imported;
        self.info.exception_count = internal;
        self
    }

    pub fn validation_state(mut self, state: WasmValidationState) -> Self {
        self.info.validation_state = state;
        self
    }

    pub fn start_function(mut self, function: WasmFunctionIndex) -> Self {
        self.info.start_function = Some(function);
        self
    }

    pub fn data_segment_count(mut self, count: u32) -> Self {
        self.info.data_segment_count = Some(count);
        self
    }

    pub fn source_mapping_url(mut self, name: u32) -> Self {
        self.info.source_mapping_url = Some(name);
        self
    }

    pub fn build(self) -> Result<WasmModuleInfo, WasmModuleValidationError> {
        validate_wasm_module_info(&self.info)?;
        Ok(self.info)
    }
}

pub fn validate_wasm_module_schema_registry(
    registry: &WasmModuleSchemaRegistry,
) -> Result<(), WasmModuleValidationError> {
    for (index, section) in registry.sections.iter().enumerate() {
        if section.canonical_name.is_empty() {
            return Err(WasmModuleValidationError::EmptyDescriptorName);
        }
        if matches!(section.section, WasmSection::Begin) {
            return Err(WasmModuleValidationError::UnknownSection(section.section));
        }
        for other in registry.sections.iter().skip(index + 1) {
            if section.section == other.section {
                return Err(WasmModuleValidationError::DuplicateSection(section.section));
            }
            if let (Some(left), Some(right)) = (section.binary_id, other.binary_id) {
                if left == right {
                    return Err(WasmModuleValidationError::DuplicateSectionBinaryId(left));
                }
            }
        }
        if let Some(feature) = section.required_feature {
            if !registry
                .features
                .iter()
                .any(|entry| entry.feature == feature)
            {
                return Err(WasmModuleValidationError::UnknownFeature(feature));
            }
        }
    }

    for (index, feature) in registry.features.iter().enumerate() {
        if feature.name.is_empty() {
            return Err(WasmModuleValidationError::EmptyDescriptorName);
        }
        if let Some(section) = feature.required_section {
            if !registry
                .sections
                .iter()
                .any(|descriptor| descriptor.section == section)
            {
                return Err(WasmModuleValidationError::UnknownSection(section));
            }
        }
        for other in registry.features.iter().skip(index + 1) {
            if feature.feature == other.feature {
                return Err(WasmModuleValidationError::DuplicateFeature(feature.feature));
            }
        }
    }

    Ok(())
}

pub fn validate_wasm_module_info(info: &WasmModuleInfo) -> Result<(), WasmModuleValidationError> {
    validate_wasm_module_schema_registry(wasm_module_schema_registry())?;
    validate_module_features(&info.features)?;
    validate_module_sections(&info.sections)?;
    validate_types(&info.types)?;
    validate_signatures(&info.function_signatures, info.types.len())?;
    validate_memories(&info.memories)?;
    validate_tables(&info.tables)?;
    validate_globals(&info.globals)?;
    validate_tags(&info.tags, info.types.len())?;
    validate_imports(info)?;
    validate_exports(info)?;
    validate_data_segments(info)?;
    validate_element_segments(info)?;
    validate_custom_sections(&info.custom_sections)?;
    validate_function_summaries(info)?;

    if let Some(start) = info.start_function {
        validate_function_index(info, start)?;
    }

    let function_imports = info
        .imports
        .iter()
        .filter(|import| import.kind == WasmImportKind::Function)
        .count();
    if info.function_import_count as usize != function_imports {
        return Err(WasmModuleValidationError::ImportCountMismatch {
            declared: info.function_import_count,
            actual: function_imports,
        });
    }

    let internal_functions = info.function_summaries.len();
    if info.function_count as usize != internal_functions {
        return Err(WasmModuleValidationError::FunctionCountMismatch {
            declared: info.function_count,
            actual: internal_functions,
        });
    }

    let exception_imports = info
        .imports
        .iter()
        .filter(|import| import.kind == WasmImportKind::Tag)
        .count();
    if info.exception_import_count as usize != exception_imports {
        return Err(WasmModuleValidationError::ExceptionCountMismatch {
            declared: info.exception_import_count,
            actual: exception_imports,
        });
    }

    let internal_exceptions = info.tags.iter().filter(|tag| !tag.imported).count();
    if info.exception_count as usize != internal_exceptions {
        return Err(WasmModuleValidationError::ExceptionCountMismatch {
            declared: info.exception_count,
            actual: internal_exceptions,
        });
    }

    Ok(())
}

pub fn plan_wasm_module_validation(
    info: &WasmModuleInfo,
) -> Result<WasmModuleValidationPlan, WasmModuleValidationError> {
    validate_wasm_module_info(info)?;
    Ok(WasmModuleValidationPlan {
        module: info.id,
        validation_state: info.validation_state,
        required_features: derive_wasm_module_required_features(info)?,
        section_order: info
            .sections
            .iter()
            .map(|section| section.section)
            .collect(),
        type_count: info.types.len(),
        function_import_count: info.function_import_count,
        function_count: info.function_count,
        import_count: info.imports.len(),
        export_count: info.exports.len(),
        memory_count: info.memories.len(),
        table_count: info.tables.len(),
        tag_count: info.tags.len(),
    })
}

pub fn validate_wasm_module_enabled_features(
    info: &WasmModuleInfo,
    enabled_features: &[WasmModuleFeature],
) -> Result<WasmModuleValidationPlan, WasmModuleValidationError> {
    let plan = plan_wasm_module_validation(info)?;
    for feature in &plan.required_features {
        let descriptor = wasm_feature_descriptor(*feature)
            .ok_or(WasmModuleValidationError::UnknownFeature(*feature))?;
        if descriptor.status != WasmFeatureStatus::AlwaysOn && !enabled_features.contains(feature) {
            return Err(WasmModuleValidationError::RequiredFeatureDisabled(*feature));
        }
    }
    Ok(plan)
}

pub fn describe_wasm_validation_semantics(
    info: &WasmModuleInfo,
    enabled_features: &[WasmModuleFeature],
) -> Result<WasmValidationSemanticOutcome, WasmModuleValidationError> {
    let plan = validate_wasm_module_enabled_features(info, enabled_features)?;
    let status = match info.validation_state {
        WasmValidationState::Complete => WasmValidationSemanticStatus::Accepted,
        WasmValidationState::Failed => WasmValidationSemanticStatus::Failed,
        _ => WasmValidationSemanticStatus::Incomplete,
    };

    Ok(WasmValidationSemanticOutcome {
        module: info.id,
        status,
        validation_state: info.validation_state,
        required_features: plan.required_features,
        enabled_feature_mask: wasm_module_feature_mask(enabled_features)?,
        has_start_function: info.start_function.is_some(),
        import_count: info.imports.len(),
        export_count: info.exports.len(),
        function_count: info
            .function_import_count
            .saturating_add(info.function_count),
        memory_count: info.memories.len(),
        table_count: info.tables.len(),
        global_count: info.globals.len(),
        tag_count: info.tags.len(),
    })
}

pub fn describe_wasm_module_linking_semantics(
    info: &WasmModuleInfo,
) -> Result<WasmModuleLinkingSemanticDescriptor, WasmModuleValidationError> {
    validate_wasm_module_info(info)?;
    let imports: Vec<_> = info
        .imports
        .iter()
        .map(describe_wasm_import_semantics)
        .collect();
    let exports: Vec<_> = info
        .exports
        .iter()
        .map(describe_wasm_export_semantics)
        .collect();
    let required_js_to_wasm_bridge_count = exports
        .iter()
        .filter(|export| export.creates_js_wrapper)
        .count();
    let required_wasm_to_js_bridge_count = imports
        .iter()
        .filter(|import| import.creates_wasm_to_js_bridge)
        .count();
    let exposes_hidden_imports = imports.iter().any(|import| import.hidden_from_reflection);

    Ok(WasmModuleLinkingSemanticDescriptor {
        module: info.id,
        imports,
        exports,
        required_js_to_wasm_bridge_count,
        required_wasm_to_js_bridge_count,
        has_start_function: info.start_function.is_some(),
        allocates_memories: info.memories.iter().any(|memory| !memory.imported),
        allocates_tables: info.tables.iter().any(|table| !table.imported),
        allocates_globals: info.globals.iter().any(|global| !global.imported),
        exposes_hidden_imports,
    })
}

pub fn describe_wasm_import_semantics(
    import: &WasmImportDescriptor,
) -> WasmImportSemanticDescriptor {
    WasmImportSemanticDescriptor {
        index: import.index,
        kind: import.kind,
        module_name: import.module_name,
        field_name: import.field_name,
        type_signature: import.type_signature,
        hidden_from_reflection: import.hidden_from_reflection,
        requires_import_object_value: !import.hidden_from_reflection,
        creates_wasm_to_js_bridge: import.kind == WasmImportKind::Function,
    }
}

pub fn describe_wasm_export_semantics(
    export: &WasmExportDescriptor,
) -> WasmExportSemanticDescriptor {
    WasmExportSemanticDescriptor {
        index: export.index,
        kind: export.kind,
        name: export.name,
        type_signature: export.type_signature,
        creates_js_wrapper: export.kind == WasmExportKind::Function,
        exposes_runtime_object: matches!(
            export.kind,
            WasmExportKind::Memory
                | WasmExportKind::Table
                | WasmExportKind::Global
                | WasmExportKind::Tag
        ),
    }
}

pub fn wasm_module_feature_mask(
    features: &[WasmModuleFeature],
) -> Result<u32, WasmModuleValidationError> {
    let mut mask = 0u32;
    for feature in features {
        let Some(index) = WASM_FEATURE_DESCRIPTORS
            .iter()
            .position(|descriptor| descriptor.feature == *feature)
        else {
            return Err(WasmModuleValidationError::UnknownFeature(*feature));
        };
        mask |= 1u32 << index;
    }
    Ok(mask)
}

pub fn derive_wasm_module_required_features(
    info: &WasmModuleInfo,
) -> Result<Vec<WasmModuleFeature>, WasmModuleValidationError> {
    let mut features = Vec::new();

    for section in &info.sections {
        let descriptor = wasm_section_descriptor(section.section)
            .ok_or(WasmModuleValidationError::UnknownSection(section.section))?;
        if let Some(feature) = descriptor.required_feature {
            push_unique_feature(&mut features, feature);
        }
    }

    for feature in &info.features {
        if wasm_feature_descriptor(*feature).is_none() {
            return Err(WasmModuleValidationError::UnknownFeature(*feature));
        }
        push_unique_feature(&mut features, *feature);
    }

    for descriptor in &info.types {
        if descriptor.kind != WasmTypeKind::Function {
            push_unique_feature(&mut features, WasmModuleFeature::Gc);
        }
        if descriptor.recursive_group.is_some() {
            push_unique_feature(&mut features, WasmModuleFeature::Gc);
        }
    }

    for memory in &info.memories {
        if memory.address_type == crate::wasm::WasmAddressType::I64 {
            push_unique_feature(&mut features, WasmModuleFeature::Memory64);
        }
        if memory.sharing == crate::wasm::WasmMemorySharing::Shared {
            push_unique_feature(&mut features, WasmModuleFeature::Threads);
        }
    }

    for table in &info.tables {
        if !matches!(
            table.element_type,
            crate::wasm::WasmTableElementType::FuncRef
                | crate::wasm::WasmTableElementType::ExternRef
        ) {
            push_unique_feature(&mut features, WasmModuleFeature::ReferenceTypes);
        }
    }

    for import in &info.imports {
        if import.kind == WasmImportKind::Tag {
            push_unique_feature(&mut features, WasmModuleFeature::Exceptions);
        }
        if import.hidden_from_reflection {
            push_unique_feature(&mut features, WasmModuleFeature::ImportedStringConstants);
        }
    }

    if !info.tags.is_empty() {
        push_unique_feature(&mut features, WasmModuleFeature::Exceptions);
    }

    for summary in &info.function_summaries {
        if summary.uses_simd {
            push_unique_feature(&mut features, WasmModuleFeature::Simd);
        }
        if summary.uses_exceptions {
            push_unique_feature(&mut features, WasmModuleFeature::Exceptions);
        }
        if summary.uses_atomics {
            push_unique_feature(&mut features, WasmModuleFeature::Threads);
        }
    }

    Ok(features)
}

fn push_unique_feature(features: &mut Vec<WasmModuleFeature>, feature: WasmModuleFeature) {
    if !features.contains(&feature) {
        features.push(feature);
    }
}

fn validate_module_features(
    features: &[WasmModuleFeature],
) -> Result<(), WasmModuleValidationError> {
    for (index, feature) in features.iter().enumerate() {
        if wasm_feature_descriptor(*feature).is_none() {
            return Err(WasmModuleValidationError::UnknownFeature(*feature));
        }
        for other in features.iter().skip(index + 1) {
            if feature == other {
                return Err(WasmModuleValidationError::DuplicateFeature(*feature));
            }
        }
    }
    Ok(())
}

fn validate_module_sections(
    sections: &[WasmSectionDescriptor],
) -> Result<(), WasmModuleValidationError> {
    let mut previous_known = WasmSection::Begin;

    for (index, section) in sections.iter().enumerate() {
        let registry_section = wasm_section_descriptor(section.section)
            .ok_or(WasmModuleValidationError::UnknownSection(section.section))?;
        if section.binary_id != registry_section.binary_id
            || section.order != registry_section.order
        {
            return Err(WasmModuleValidationError::UnknownSection(section.section));
        }
        if section.section != WasmSection::Custom {
            if wasm_section_descriptor(previous_known)
                .map(|previous| previous.order >= section.order)
                .unwrap_or(false)
            {
                return Err(WasmModuleValidationError::SectionOutOfOrder {
                    previous: previous_known,
                    next: section.section,
                });
            }
            previous_known = section.section;
        }
        if !section.repeatable {
            for other in sections.iter().skip(index + 1) {
                if section.section == other.section {
                    return Err(WasmModuleValidationError::SectionNotRepeatable(
                        section.section,
                    ));
                }
            }
        }
    }

    Ok(())
}

fn validate_types(types: &[WasmTypeDescriptor]) -> Result<(), WasmModuleValidationError> {
    for (index, descriptor) in types.iter().enumerate() {
        validate_index(index, descriptor.index.0)?;
        let payload_count = descriptor.function.is_some() as u8
            + descriptor.struct_type.is_some() as u8
            + descriptor.array_type.is_some() as u8;
        if payload_count != 1 {
            return Err(WasmModuleValidationError::InvalidTypeDescriptor(
                descriptor.index,
            ));
        }

        match descriptor.kind {
            WasmTypeKind::Function if descriptor.function.is_none() => {
                return Err(WasmModuleValidationError::InvalidTypeDescriptor(
                    descriptor.index,
                ));
            }
            WasmTypeKind::Struct if descriptor.struct_type.is_none() => {
                return Err(WasmModuleValidationError::InvalidTypeDescriptor(
                    descriptor.index,
                ));
            }
            WasmTypeKind::Array if descriptor.array_type.is_none() => {
                return Err(WasmModuleValidationError::InvalidTypeDescriptor(
                    descriptor.index,
                ));
            }
            WasmTypeKind::Continuation => {
                return Err(WasmModuleValidationError::InvalidTypeDescriptor(
                    descriptor.index,
                ));
            }
            _ => {}
        }

        if let Some(supertype) = descriptor
            .function
            .as_ref()
            .and_then(|payload| payload.supertype)
            .or_else(|| {
                descriptor
                    .struct_type
                    .as_ref()
                    .and_then(|payload| payload.supertype)
            })
            .or_else(|| descriptor.array_type.and_then(|payload| payload.supertype))
        {
            validate_type_index(types.len(), supertype)?;
            if supertype == descriptor.index {
                return Err(WasmModuleValidationError::InvalidTypeReference(supertype));
            }
        }
    }
    Ok(())
}

fn validate_signatures(
    signatures: &[WasmFunctionSignature],
    type_count: usize,
) -> Result<(), WasmModuleValidationError> {
    for signature in signatures {
        if let Some(index) = signature.canonical_type_index {
            validate_type_index(type_count, index)?;
        }
    }
    Ok(())
}

fn validate_memories(memories: &[WasmMemoryDescriptor]) -> Result<(), WasmModuleValidationError> {
    for (index, memory) in memories.iter().enumerate() {
        validate_index(index, memory.index.0)?;
        if let Some(maximum) = memory.maximum_pages {
            if memory.minimum_pages > maximum {
                return Err(WasmModuleValidationError::InvalidLimit {
                    minimum: memory.minimum_pages,
                    maximum,
                });
            }
        }
    }
    Ok(())
}

fn validate_tables(tables: &[WasmTableDescriptor]) -> Result<(), WasmModuleValidationError> {
    for (index, table) in tables.iter().enumerate() {
        validate_index(index, table.index.0)?;
        if let Some(maximum) = table.maximum_elements {
            if table.minimum_elements > maximum {
                return Err(WasmModuleValidationError::InvalidLimit {
                    minimum: table.minimum_elements as u64,
                    maximum: maximum as u64,
                });
            }
        }
    }
    Ok(())
}

fn validate_globals(globals: &[WasmGlobalDescriptor]) -> Result<(), WasmModuleValidationError> {
    for (index, global) in globals.iter().enumerate() {
        validate_index(index, global.index.0)?;
    }
    Ok(())
}

fn validate_tags(
    tags: &[WasmTagDescriptor],
    type_count: usize,
) -> Result<(), WasmModuleValidationError> {
    for (index, tag) in tags.iter().enumerate() {
        validate_index(index, tag.index.0)?;
        validate_type_index(type_count, tag.type_signature)?;
    }
    Ok(())
}

fn validate_imports(info: &WasmModuleInfo) -> Result<(), WasmModuleValidationError> {
    for (index, import) in info.imports.iter().enumerate() {
        validate_index(index, import.index.0)?;
        match import.kind {
            WasmImportKind::Function => {
                let (Some(function), Some(type_signature)) =
                    (import.function, import.type_signature)
                else {
                    return Err(WasmModuleValidationError::InvalidImport {
                        index: import.index,
                        kind: import.kind,
                    });
                };
                if import.memory.is_some()
                    || import.table.is_some()
                    || import.global.is_some()
                    || import.tag.is_some()
                {
                    return Err(WasmModuleValidationError::InvalidImport {
                        index: import.index,
                        kind: import.kind,
                    });
                }
                validate_type_index(info.types.len(), type_signature)?;
                validate_function_index(info, function)?;
            }
            WasmImportKind::Memory => {
                let Some(memory) = import.memory else {
                    return Err(WasmModuleValidationError::InvalidImport {
                        index: import.index,
                        kind: import.kind,
                    });
                };
                if import.function.is_some()
                    || import.type_signature.is_some()
                    || import.table.is_some()
                    || import.global.is_some()
                    || import.tag.is_some()
                {
                    return Err(WasmModuleValidationError::InvalidImport {
                        index: import.index,
                        kind: import.kind,
                    });
                }
                validate_memory_index(info, memory)?;
            }
            WasmImportKind::Table => {
                let Some(table) = import.table else {
                    return Err(WasmModuleValidationError::InvalidImport {
                        index: import.index,
                        kind: import.kind,
                    });
                };
                if import.function.is_some()
                    || import.type_signature.is_some()
                    || import.memory.is_some()
                    || import.global.is_some()
                    || import.tag.is_some()
                {
                    return Err(WasmModuleValidationError::InvalidImport {
                        index: import.index,
                        kind: import.kind,
                    });
                }
                validate_table_index(info, table)?;
            }
            WasmImportKind::Global => {
                let Some(global) = import.global else {
                    return Err(WasmModuleValidationError::InvalidImport {
                        index: import.index,
                        kind: import.kind,
                    });
                };
                if import.function.is_some()
                    || import.type_signature.is_some()
                    || import.memory.is_some()
                    || import.table.is_some()
                    || import.tag.is_some()
                {
                    return Err(WasmModuleValidationError::InvalidImport {
                        index: import.index,
                        kind: import.kind,
                    });
                }
                validate_global_index(info, global)?;
            }
            WasmImportKind::Tag => {
                let (Some(tag), Some(type_signature)) = (import.tag, import.type_signature) else {
                    return Err(WasmModuleValidationError::InvalidImport {
                        index: import.index,
                        kind: import.kind,
                    });
                };
                if import.function.is_some()
                    || import.memory.is_some()
                    || import.table.is_some()
                    || import.global.is_some()
                {
                    return Err(WasmModuleValidationError::InvalidImport {
                        index: import.index,
                        kind: import.kind,
                    });
                }
                validate_type_index(info.types.len(), type_signature)?;
                validate_tag_index(info, tag)?;
            }
        }
    }
    Ok(())
}

fn validate_exports(info: &WasmModuleInfo) -> Result<(), WasmModuleValidationError> {
    for (index, export) in info.exports.iter().enumerate() {
        validate_index(index, export.index.0)?;
        match export.kind {
            WasmExportKind::Function => {
                let Some(function) = export.function else {
                    return Err(WasmModuleValidationError::InvalidExport {
                        index: export.index,
                        kind: export.kind,
                    });
                };
                if export.memory.is_some()
                    || export.table.is_some()
                    || export.global.is_some()
                    || export.tag.is_some()
                {
                    return Err(WasmModuleValidationError::InvalidExport {
                        index: export.index,
                        kind: export.kind,
                    });
                }
                validate_function_index(info, function)?;
            }
            WasmExportKind::Memory => {
                let Some(memory) = export.memory else {
                    return Err(WasmModuleValidationError::InvalidExport {
                        index: export.index,
                        kind: export.kind,
                    });
                };
                if export.function.is_some()
                    || export.table.is_some()
                    || export.global.is_some()
                    || export.tag.is_some()
                {
                    return Err(WasmModuleValidationError::InvalidExport {
                        index: export.index,
                        kind: export.kind,
                    });
                }
                validate_memory_index(info, memory)?;
            }
            WasmExportKind::Table => {
                let Some(table) = export.table else {
                    return Err(WasmModuleValidationError::InvalidExport {
                        index: export.index,
                        kind: export.kind,
                    });
                };
                if export.function.is_some()
                    || export.memory.is_some()
                    || export.global.is_some()
                    || export.tag.is_some()
                {
                    return Err(WasmModuleValidationError::InvalidExport {
                        index: export.index,
                        kind: export.kind,
                    });
                }
                validate_table_index(info, table)?;
            }
            WasmExportKind::Global => {
                let Some(global) = export.global else {
                    return Err(WasmModuleValidationError::InvalidExport {
                        index: export.index,
                        kind: export.kind,
                    });
                };
                if export.function.is_some()
                    || export.memory.is_some()
                    || export.table.is_some()
                    || export.tag.is_some()
                {
                    return Err(WasmModuleValidationError::InvalidExport {
                        index: export.index,
                        kind: export.kind,
                    });
                }
                validate_global_index(info, global)?;
            }
            WasmExportKind::Tag => {
                let Some(tag) = export.tag else {
                    return Err(WasmModuleValidationError::InvalidExport {
                        index: export.index,
                        kind: export.kind,
                    });
                };
                if export.function.is_some()
                    || export.memory.is_some()
                    || export.table.is_some()
                    || export.global.is_some()
                {
                    return Err(WasmModuleValidationError::InvalidExport {
                        index: export.index,
                        kind: export.kind,
                    });
                }
                validate_tag_index(info, tag)?;
            }
        }
    }
    Ok(())
}

fn validate_data_segments(info: &WasmModuleInfo) -> Result<(), WasmModuleValidationError> {
    for (index, segment) in info.data_segments.iter().enumerate() {
        validate_index(index, segment.index.0)?;
        if let Some(memory) = segment.memory {
            validate_memory_index(info, memory)?;
        }
    }
    if let Some(count) = info.data_segment_count {
        if count as usize != info.data_segments.len() {
            return Err(WasmModuleValidationError::DataSegmentCountMismatch {
                declared: count,
                actual: info.data_segments.len(),
            });
        }
    }
    Ok(())
}

fn validate_element_segments(info: &WasmModuleInfo) -> Result<(), WasmModuleValidationError> {
    for (index, segment) in info.element_segments.iter().enumerate() {
        validate_index(index, segment.index.0)?;
        if let Some(table) = segment.table {
            validate_table_index(info, table)?;
        }
    }
    Ok(())
}

fn validate_custom_sections(
    sections: &[WasmCustomSectionDescriptor],
) -> Result<(), WasmModuleValidationError> {
    for section in sections {
        if section
            .byte_offset
            .checked_add(section.byte_count)
            .is_none()
        {
            return Err(WasmModuleValidationError::InvalidCustomSectionRange {
                byte_offset: section.byte_offset,
                byte_count: section.byte_count,
            });
        }
    }
    Ok(())
}

fn validate_function_summaries(info: &WasmModuleInfo) -> Result<(), WasmModuleValidationError> {
    for (index, summary) in info.function_summaries.iter().enumerate() {
        validate_index(index, summary.code_index.0)?;
        if summary.start_offset > summary.end_offset {
            return Err(WasmModuleValidationError::InvalidFunctionSummary(
                summary.code_index,
            ));
        }
    }

    for hint in &info.branch_hints {
        if hint.function.0 >= info.function_count {
            return Err(WasmModuleValidationError::InvalidFunctionSummary(
                hint.function,
            ));
        }
    }

    Ok(())
}

fn validate_index(expected: usize, actual: u32) -> Result<(), WasmModuleValidationError> {
    if expected as u32 != actual {
        return Err(WasmModuleValidationError::InvalidIndexOrder {
            expected: expected as u32,
            actual,
        });
    }
    Ok(())
}

fn validate_type_index(
    type_count: usize,
    index: WasmTypeSignatureIndex,
) -> Result<(), WasmModuleValidationError> {
    if index.0 as usize >= type_count {
        return Err(WasmModuleValidationError::InvalidTypeReference(index));
    }
    Ok(())
}

fn validate_function_index(
    info: &WasmModuleInfo,
    index: WasmFunctionIndex,
) -> Result<(), WasmModuleValidationError> {
    if index.0
        >= info
            .function_import_count
            .saturating_add(info.function_count)
    {
        return Err(WasmModuleValidationError::InvalidFunctionReference(index));
    }
    Ok(())
}

fn validate_memory_index(
    info: &WasmModuleInfo,
    index: crate::wasm::WasmMemoryIndex,
) -> Result<(), WasmModuleValidationError> {
    if index.0 as usize >= info.memories.len() {
        return Err(WasmModuleValidationError::InvalidMemoryReference(index));
    }
    Ok(())
}

fn validate_table_index(
    info: &WasmModuleInfo,
    index: crate::wasm::WasmTableIndex,
) -> Result<(), WasmModuleValidationError> {
    if index.0 as usize >= info.tables.len() {
        return Err(WasmModuleValidationError::InvalidTableReference(index));
    }
    Ok(())
}

fn validate_global_index(
    info: &WasmModuleInfo,
    index: WasmGlobalIndex,
) -> Result<(), WasmModuleValidationError> {
    if index.0 as usize >= info.globals.len() {
        return Err(WasmModuleValidationError::InvalidGlobalReference(index));
    }
    Ok(())
}

fn validate_tag_index(
    info: &WasmModuleInfo,
    index: WasmTagIndex,
) -> Result<(), WasmModuleValidationError> {
    if index.0 as usize >= info.tags.len() {
        return Err(WasmModuleValidationError::InvalidTagReference(index));
    }
    Ok(())
}

/// Module-system integration record for Wasm modules.
///
/// `module_record` borrows the canonical module-system identity; mutation of
/// loader/link/evaluate state remains in the modules layer.
#[derive(Clone, Debug)]
pub struct WasmModuleRecord {
    pub module: WasmModuleId,
    pub module_record: Option<ModuleRecordId>,
    pub js_module_object: Option<ObjectId>,
    pub source_kind: WasmSourceKind,
    pub export_count: usize,
    pub link_state: WasmModuleRecordState,
}

/// JS module record lifecycle for Wasm modules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmModuleRecordState {
    Allocated,
    Linking,
    Linked,
    Evaluated,
    Failed,
}

/// GC-owned public module wrapper reserved for future WebAssembly.Module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmModuleObject {
    pub object: Option<ObjectId>,
    pub module: WasmModuleId,
    pub source_kind: WasmSourceKind,
}

/// JS-visible WebAssembly namespace wrapper surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WasmJsWrapperKind {
    ModuleConstructor,
    InstanceConstructor,
    MemoryConstructor,
    TableConstructor,
    GlobalConstructor,
    FunctionExport,
    TagConstructor,
}

/// Wrapper object identity and its backing Wasm entity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WasmJsWrapperDescriptor {
    pub object: Option<ObjectId>,
    pub wrapper_kind: WasmJsWrapperKind,
    pub module: Option<WasmModuleId>,
    pub export: Option<WasmExportIndex>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_static_registries_are_cross_referenced() {
        let registry = wasm_module_schema_registry();

        assert!(validate_wasm_module_schema_registry(registry).is_ok());
        assert!(registry
            .sections
            .iter()
            .any(|descriptor| descriptor.repeatable && descriptor.section == WasmSection::Custom));
        assert!(registry.features.iter().all(|feature| feature
            .required_section
            .map(|section| wasm_section_descriptor(section).is_some())
            .unwrap_or(true)));
        assert_eq!(
            wasm_feature_descriptor(WasmModuleFeature::Exceptions)
                .and_then(|feature| feature.required_section),
            Some(WasmSection::Exception)
        );
    }

    #[test]
    fn wasm_module_builder_accepts_structural_function_export() {
        let type_index = WasmTypeSignatureIndex(0);
        let module = WasmModuleInfoBuilder::new(WasmModuleId(1), WasmSourceKind::BinaryModule)
            .type_descriptor(WasmTypeDescriptor {
                index: type_index,
                kind: WasmTypeKind::Function,
                function: Some(WasmFunctionTypeDescriptor {
                    index: type_index,
                    params: vec![crate::wasm::WasmValueType::I32],
                    results: vec![crate::wasm::WasmValueType::I32],
                    supertype: None,
                    is_final: true,
                }),
                struct_type: None,
                array_type: None,
                recursive_group: None,
            })
            .function_signature(WasmFunctionSignature {
                params: vec![crate::wasm::WasmValueType::I32],
                results: vec![crate::wasm::WasmValueType::I32],
                module_type_index: Some(0),
                canonical_type_index: Some(type_index),
            })
            .function_summary(WasmFunctionValidationSummary {
                code_index: WasmFunctionCodeIndex(0),
                start_offset: 0,
                end_offset: 8,
                finished_validating: true,
                uses_simd: false,
                uses_exceptions: false,
                uses_atomics: false,
                declared: true,
                referenced: true,
            })
            .export(WasmExportDescriptor {
                index: WasmExportIndex(0),
                kind: WasmExportKind::Function,
                name: Some(0),
                function: Some(WasmFunctionIndex(0)),
                memory: None,
                table: None,
                global: None,
                tag: None,
                type_signature: Some(type_index),
            })
            .function_counts(0, 1)
            .validation_state(WasmValidationState::Complete)
            .build()
            .unwrap();

        assert!(validate_wasm_module_info(&module).is_ok());
    }

    #[test]
    fn wasm_module_builder_rejects_memory_limit_inversion() {
        let error = WasmModuleInfoBuilder::new(WasmModuleId(1), WasmSourceKind::BinaryModule)
            .memory(WasmMemoryDescriptor {
                index: crate::wasm::WasmMemoryIndex(0),
                minimum_pages: 10,
                maximum_pages: Some(1),
                sharing: crate::wasm::WasmMemorySharing::Unshared,
                address_type: crate::wasm::WasmAddressType::I32,
                imported: false,
            })
            .build()
            .unwrap_err();

        assert_eq!(
            error,
            WasmModuleValidationError::InvalidLimit {
                minimum: 10,
                maximum: 1,
            }
        );
    }

    #[test]
    fn wasm_module_validation_plan_derives_memory64_feature() {
        let module = WasmModuleInfoBuilder::new(WasmModuleId(2), WasmSourceKind::BinaryModule)
            .memory(WasmMemoryDescriptor {
                index: crate::wasm::WasmMemoryIndex(0),
                minimum_pages: 1,
                maximum_pages: Some(2),
                sharing: crate::wasm::WasmMemorySharing::Unshared,
                address_type: crate::wasm::WasmAddressType::I64,
                imported: false,
            })
            .validation_state(WasmValidationState::Complete)
            .build()
            .unwrap();

        let plan = plan_wasm_module_validation(&module).unwrap();

        assert_eq!(plan.memory_count, 1);
        assert!(plan
            .required_features
            .contains(&WasmModuleFeature::Memory64));
        assert_eq!(
            wasm_module_feature_mask(&plan.required_features).unwrap()
                & wasm_module_feature_mask(&[WasmModuleFeature::Memory64]).unwrap(),
            wasm_module_feature_mask(&[WasmModuleFeature::Memory64]).unwrap()
        );
    }

    #[test]
    fn wasm_module_enabled_feature_validation_rejects_disabled_feature() {
        let module = WasmModuleInfoBuilder::new(WasmModuleId(3), WasmSourceKind::BinaryModule)
            .memory(WasmMemoryDescriptor {
                index: crate::wasm::WasmMemoryIndex(0),
                minimum_pages: 1,
                maximum_pages: None,
                sharing: crate::wasm::WasmMemorySharing::Shared,
                address_type: crate::wasm::WasmAddressType::I32,
                imported: false,
            })
            .build()
            .unwrap();

        let error = validate_wasm_module_enabled_features(&module, &[]).unwrap_err();

        assert_eq!(
            error,
            WasmModuleValidationError::RequiredFeatureDisabled(WasmModuleFeature::Threads)
        );
    }

    #[test]
    fn wasm_validation_semantics_report_complete_outcome() {
        let module = WasmModuleInfoBuilder::new(WasmModuleId(4), WasmSourceKind::BinaryModule)
            .validation_state(WasmValidationState::Complete)
            .build()
            .unwrap();

        let outcome = describe_wasm_validation_semantics(&module, &[]).unwrap();

        assert_eq!(outcome.status, WasmValidationSemanticStatus::Accepted);
        assert_eq!(outcome.module, WasmModuleId(4));
        assert_eq!(outcome.enabled_feature_mask, 0);
    }

    #[test]
    fn wasm_linking_semantics_count_bridge_boundaries() {
        let type_index = WasmTypeSignatureIndex(0);
        let module = WasmModuleInfoBuilder::new(WasmModuleId(5), WasmSourceKind::BinaryModule)
            .type_descriptor(WasmTypeDescriptor {
                index: type_index,
                kind: WasmTypeKind::Function,
                function: Some(WasmFunctionTypeDescriptor {
                    index: type_index,
                    params: Vec::new(),
                    results: Vec::new(),
                    supertype: None,
                    is_final: true,
                }),
                struct_type: None,
                array_type: None,
                recursive_group: None,
            })
            .function_signature(WasmFunctionSignature {
                params: Vec::new(),
                results: Vec::new(),
                module_type_index: Some(0),
                canonical_type_index: Some(type_index),
            })
            .import(WasmImportDescriptor {
                index: WasmImportIndex(0),
                kind: WasmImportKind::Function,
                module_name: Some(1),
                field_name: Some(2),
                function: Some(WasmFunctionIndex(0)),
                type_signature: Some(type_index),
                memory: None,
                table: None,
                global: None,
                tag: None,
                hidden_from_reflection: false,
            })
            .export(WasmExportDescriptor {
                index: WasmExportIndex(0),
                kind: WasmExportKind::Function,
                name: Some(3),
                function: Some(WasmFunctionIndex(0)),
                memory: None,
                table: None,
                global: None,
                tag: None,
                type_signature: Some(type_index),
            })
            .function_counts(1, 0)
            .build()
            .unwrap();

        let descriptor = describe_wasm_module_linking_semantics(&module).unwrap();

        assert_eq!(descriptor.required_wasm_to_js_bridge_count, 1);
        assert_eq!(descriptor.required_js_to_wasm_bridge_count, 1);
        assert!(!descriptor.exposes_hidden_imports);
    }
}
