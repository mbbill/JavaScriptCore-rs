use crate::builtins::ids::{
    BuiltinGeneratedArtifact, BuiltinGenerationPhase, BuiltinId, BuiltinInitializationEdge,
    BuiltinInitializationStage, BuiltinInitializationStep, BuiltinTableRevision, BuiltinVisibility,
};
use crate::builtins::intrinsics::{BuiltinIntrinsic, BuiltinIntrinsicDescriptor};
use crate::builtins::names::WellKnownBuiltinName;

/// Authorship form for builtin code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinSourceKind {
    JavaScriptSource,
    RustIr,
    GeneratedMetadataOnly,
    DefaultConstructor,
}

/// Source range in the generated combined builtin source provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinSourceRange {
    start_offset: u32,
    end_offset: u32,
}

impl BuiltinSourceRange {
    pub const fn new(start_offset: u32, end_offset: u32) -> Self {
        Self {
            start_offset,
            end_offset,
        }
    }

    pub const fn start_offset(self) -> u32 {
        self.start_offset
    }

    pub const fn end_offset(self) -> u32 {
        self.end_offset
    }
}

/// Builtin source identity and metadata.
///
/// This type intentionally does not store source text. The VM/global executable
/// layer owns source providers, parser metadata validation, and the eventual
/// unlinked executable. This module records enough identity to describe that
/// boundary and the generated-source ordering.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuiltinSource {
    kind: BuiltinSourceKind,
    name: WellKnownBuiltinName,
    range: Option<BuiltinSourceRange>,
}

/// Generated pipeline artifact and its initialization dependency.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinGeneratedPipelineStep {
    phase: BuiltinGenerationPhase,
    artifact: BuiltinGeneratedArtifact,
    revision: BuiltinTableRevision,
    init_edge: Option<BuiltinInitializationEdge>,
}

impl BuiltinGeneratedPipelineStep {
    pub const fn new(
        phase: BuiltinGenerationPhase,
        artifact: BuiltinGeneratedArtifact,
        revision: BuiltinTableRevision,
    ) -> Self {
        Self {
            phase,
            artifact,
            revision,
            init_edge: None,
        }
    }

    pub const fn with_init_edge(
        phase: BuiltinGenerationPhase,
        artifact: BuiltinGeneratedArtifact,
        revision: BuiltinTableRevision,
        init_edge: BuiltinInitializationEdge,
    ) -> Self {
        Self {
            phase,
            artifact,
            revision,
            init_edge: Some(init_edge),
        }
    }
}

impl BuiltinSource {
    pub const fn new(kind: BuiltinSourceKind, name: WellKnownBuiltinName) -> Self {
        Self {
            kind,
            name,
            range: None,
        }
    }

    pub const fn with_generated_range(
        kind: BuiltinSourceKind,
        name: WellKnownBuiltinName,
        range: BuiltinSourceRange,
    ) -> Self {
        Self {
            kind,
            name,
            range: Some(range),
        }
    }

    pub const fn kind(&self) -> BuiltinSourceKind {
        self.kind
    }

    pub const fn name(&self) -> WellKnownBuiltinName {
        self.name
    }

    pub const fn range(&self) -> Option<BuiltinSourceRange> {
        self.range
    }
}

/// Parser/executable metadata that is generated beside builtin source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinExecutableMetadata {
    declared_length: u16,
    constructor_kind: BuiltinConstructorKind,
    construct_ability: BuiltinConstructAbility,
    inline_policy: BuiltinInlinePolicy,
}

impl BuiltinExecutableMetadata {
    pub const fn new(
        declared_length: u16,
        constructor_kind: BuiltinConstructorKind,
        construct_ability: BuiltinConstructAbility,
        inline_policy: BuiltinInlinePolicy,
    ) -> Self {
        Self {
            declared_length,
            constructor_kind,
            construct_ability,
            inline_policy,
        }
    }

    pub const fn declared_length(self) -> u16 {
        self.declared_length
    }

    pub const fn constructor_kind(self) -> BuiltinConstructorKind {
        self.constructor_kind
    }

    pub const fn construct_ability(self) -> BuiltinConstructAbility {
        self.construct_ability
    }

    pub const fn inline_policy(self) -> BuiltinInlinePolicy {
        self.inline_policy
    }
}

/// Constructor behavior attached to a builtin executable descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinConstructorKind {
    None,
    Base,
    Derived,
    Naked,
}

/// Whether the builtin executable can be used as a constructor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinConstructAbility {
    CannotConstruct,
    CanConstruct,
}

/// Inline metadata generated with a builtin function.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinInlinePolicy {
    Never,
    Hint,
    Always,
}

/// Metadata entry for a generated builtin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuiltinDescriptor {
    id: BuiltinId,
    source: BuiltinSource,
    visibility: BuiltinVisibility,
    intrinsic: Option<BuiltinIntrinsic>,
    executable: Option<BuiltinExecutableMetadata>,
    init_step: BuiltinInitializationStep,
}

impl BuiltinDescriptor {
    pub const fn new(
        id: BuiltinId,
        source: BuiltinSource,
        visibility: BuiltinVisibility,
        intrinsic: Option<BuiltinIntrinsic>,
    ) -> Self {
        Self {
            id,
            source,
            visibility,
            intrinsic,
            executable: None,
            init_step: BuiltinInitializationStep::new(
                id,
                BuiltinInitializationStage::MetadataRegistered,
                None,
            ),
        }
    }

    pub const fn with_executable_metadata(
        id: BuiltinId,
        source: BuiltinSource,
        visibility: BuiltinVisibility,
        intrinsic: Option<BuiltinIntrinsic>,
        executable: BuiltinExecutableMetadata,
        init_step: BuiltinInitializationStep,
    ) -> Self {
        Self {
            id,
            source,
            visibility,
            intrinsic,
            executable: Some(executable),
            init_step,
        }
    }

    pub const fn id(&self) -> BuiltinId {
        self.id
    }

    pub const fn source(&self) -> &BuiltinSource {
        &self.source
    }

    pub const fn visibility(&self) -> BuiltinVisibility {
        self.visibility
    }

    pub const fn intrinsic(&self) -> Option<BuiltinIntrinsic> {
        self.intrinsic
    }

    pub const fn executable(&self) -> Option<BuiltinExecutableMetadata> {
        self.executable
    }

    pub const fn init_step(&self) -> BuiltinInitializationStep {
        self.init_step
    }
}

/// Handle to a lazily created unlinked builtin executable.
///
/// The concrete code-block type belongs to bytecode/executable modules. This
/// module only names the cache boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BuiltinExecutableHandle(u32);

impl BuiltinExecutableHandle {
    pub const fn from_cache_slot(slot: u32) -> Self {
        Self(slot)
    }

    pub const fn cache_slot(self) -> u32 {
        self.0
    }
}

/// Lifecycle of a cache slot for a builtin executable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinExecutableCacheState {
    Empty,
    SourceMaterialized,
    MetadataValidated,
    UnlinkedExecutableCached,
    InvalidatedForVmTeardown,
}

/// Cache entry metadata without owning bytecode or GC cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinExecutableCacheEntry {
    id: BuiltinId,
    handle: BuiltinExecutableHandle,
    revision: BuiltinTableRevision,
    state: BuiltinExecutableCacheState,
}

impl BuiltinExecutableCacheEntry {
    pub const fn new(
        id: BuiltinId,
        handle: BuiltinExecutableHandle,
        revision: BuiltinTableRevision,
        state: BuiltinExecutableCacheState,
    ) -> Self {
        Self {
            id,
            handle,
            revision,
            state,
        }
    }

    pub const fn state(self) -> BuiltinExecutableCacheState {
        self.state
    }
}

/// Lazy executable cache for builtin code.
///
/// Cache mutation may allocate, parse, validate metadata, and install GC-owned
/// executable state. Implementations must route those operations through the VM.
#[derive(Debug, Default)]
pub struct BuiltinExecutableCache {
    _sealed: (),
}

impl BuiltinExecutableCache {
    pub const fn new_uninitialized() -> Self {
        Self { _sealed: () }
    }
}

/// Overall registry lifecycle for VM-owned builtin metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinRegistryState {
    Uninitialized,
    NamesReady,
    DescriptorsReady,
    IntrinsicsReady,
    ExecutablesReady,
    Sealed,
}

/// Host intrinsic hook published by another runtime subsystem.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostIntrinsicHook {
    descriptor: BuiltinIntrinsicDescriptor,
    required_stage: BuiltinInitializationStage,
}

impl HostIntrinsicHook {
    pub const fn new(
        descriptor: BuiltinIntrinsicDescriptor,
        required_stage: BuiltinInitializationStage,
    ) -> Self {
        Self {
            descriptor,
            required_stage,
        }
    }

    pub const fn descriptor(self) -> BuiltinIntrinsicDescriptor {
        self.descriptor
    }

    pub const fn required_stage(self) -> BuiltinInitializationStage {
        self.required_stage
    }
}

/// Registry for builtin descriptors and generated metadata.
#[derive(Debug, Default)]
pub struct BuiltinRegistry {
    _sealed: (),
}

impl BuiltinRegistry {
    pub const fn new_uninitialized() -> Self {
        Self { _sealed: () }
    }
}
