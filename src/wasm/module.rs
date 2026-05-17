//! WebAssembly module and module-record integration placeholders.
//!
//! The module loader can reserve these records for WebAssembly source types,
//! while unsupported construction should fail at the host boundary until Wasm is
//! implemented. Module information is immutable after validation in JSC; this
//! skeleton mirrors that ownership by separating parsed module metadata from
//! GC-owned JS wrappers and runtime instances.

use crate::runtime::{ModuleRecordId, ObjectId, SourceProviderId};
use crate::wasm::{WasmFunctionSignature, WasmMemoryDescriptor, WasmTableDescriptor};

/// Stable identity for parsed module information.
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
}

/// Static export metadata used by wrappers and module records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WasmExportDescriptor {
    pub index: WasmExportIndex,
    pub kind: WasmExportKind,
    pub name: Option<u32>,
    pub function: Option<WasmFunctionIndex>,
}

/// Parsed-module information reserved outside GC-owned wrappers.
#[derive(Clone, Debug)]
pub struct WasmModuleInfo {
    pub id: WasmModuleId,
    pub source_kind: WasmSourceKind,
    pub source: Option<SourceProviderId>,
    pub features: Vec<WasmModuleFeature>,
    pub imports: Vec<WasmImportDescriptor>,
    pub exports: Vec<WasmExportDescriptor>,
    pub function_signatures: Vec<WasmFunctionSignature>,
    pub memories: Vec<WasmMemoryDescriptor>,
    pub tables: Vec<WasmTableDescriptor>,
    pub declared_exports: Vec<WasmExportKind>,
    pub function_import_count: u32,
    pub function_count: u32,
    pub validation_state: WasmValidationState,
    pub start_function: Option<WasmFunctionIndex>,
}

/// Module-system integration record for Wasm modules.
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
