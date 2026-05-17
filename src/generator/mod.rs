//! Generated-source and table-generation contracts.
//!
//! JavaScriptCore relies on generated opcode tables, builtin bindings,
//! offlineasm products, and Unicode data. This module records generated artifact
//! provenance without running generators.

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
