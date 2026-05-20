use crate::builtins::ids::{
    BuiltinCodeIndex, BuiltinGeneratedArtifact, BuiltinGenerationPhase, BuiltinId,
    BuiltinInitializationEdge, BuiltinInitializationStage, BuiltinInitializationStep,
    BuiltinTableRevision, BuiltinVisibility,
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
    code_index: Option<BuiltinCodeIndex>,
    declared_length: u16,
    constructor_kind: BuiltinConstructorKind,
    construct_ability: BuiltinConstructAbility,
    inline_policy: BuiltinInlinePolicy,
    parser_mode: BuiltinParserMode,
    semantic: BuiltinSemanticMetadata,
}

impl BuiltinExecutableMetadata {
    pub const fn new(
        declared_length: u16,
        constructor_kind: BuiltinConstructorKind,
        construct_ability: BuiltinConstructAbility,
        inline_policy: BuiltinInlinePolicy,
    ) -> Self {
        Self {
            code_index: None,
            declared_length,
            constructor_kind,
            construct_ability,
            inline_policy,
            parser_mode: BuiltinParserMode::Function,
            semantic: BuiltinSemanticMetadata::strict_function(),
        }
    }

    pub const fn with_code_index(
        code_index: BuiltinCodeIndex,
        declared_length: u16,
        constructor_kind: BuiltinConstructorKind,
        construct_ability: BuiltinConstructAbility,
        inline_policy: BuiltinInlinePolicy,
        parser_mode: BuiltinParserMode,
    ) -> Self {
        Self {
            code_index: Some(code_index),
            declared_length,
            constructor_kind,
            construct_ability,
            inline_policy,
            parser_mode,
            semantic: BuiltinSemanticMetadata::strict_function(),
        }
    }

    pub const fn with_semantic_metadata(mut self, semantic: BuiltinSemanticMetadata) -> Self {
        self.semantic = semantic;
        self
    }

    pub const fn code_index(self) -> Option<BuiltinCodeIndex> {
        self.code_index
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

    pub const fn parser_mode(self) -> BuiltinParserMode {
        self.parser_mode
    }

    pub const fn semantic(self) -> BuiltinSemanticMetadata {
        self.semantic
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinSemanticMetadata {
    pub strict: bool,
    pub effects: BuiltinSemanticEffects,
    pub exception: BuiltinExceptionContract,
    pub parse_source: BuiltinSemanticParseSource,
}

impl BuiltinSemanticMetadata {
    pub const fn strict_function() -> Self {
        Self {
            strict: true,
            effects: BuiltinSemanticEffects::none(),
            exception: BuiltinExceptionContract::CannotThrow,
            parse_source: BuiltinSemanticParseSource::GeneratedBuiltin,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinSemanticEffects {
    pub reads_heap: bool,
    pub writes_heap: bool,
    pub allocates: bool,
    pub calls_host: bool,
    pub calls_user_code: bool,
    pub uses_private_names: bool,
}

impl BuiltinSemanticEffects {
    pub const fn none() -> Self {
        Self {
            reads_heap: false,
            writes_heap: false,
            allocates: false,
            calls_host: false,
            calls_user_code: false,
            uses_private_names: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinExceptionContract {
    CannotThrow,
    MayThrow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinSemanticParseSource {
    GeneratedBuiltin,
    DefaultClassConstructor,
    IntrinsicOnly,
    MetadataOnly,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinParserMode {
    Function,
    Constructor,
    Getter,
    Setter,
    DefaultClassConstructor,
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
/// module only names the builtin cache slot. A linked executable cell must use
/// runtime `ExecutableId` once VM materialization has happened.
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
    ParseRequested,
    UnlinkedExecutableCached,
    LinkedExecutableInstalled,
    InvalidatedForVmTeardown,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BuiltinExecutableCreationAuthority {
    #[default]
    Vm,
    GlobalObject,
    BuiltinExecutableCache,
}

/// Cache entry metadata without owning bytecode or GC cells.
///
/// The cache owns slot state and revision matching. It borrows builtin IDs and
/// generated code indexes, but mutation that creates parser products,
/// executables, or linked code blocks belongs to VM/bytecode APIs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinExecutableCacheEntry {
    id: BuiltinId,
    code_index: Option<BuiltinCodeIndex>,
    handle: BuiltinExecutableHandle,
    revision: BuiltinTableRevision,
    state: BuiltinExecutableCacheState,
    authority: BuiltinExecutableCreationAuthority,
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
            code_index: None,
            handle,
            revision,
            state,
            authority: BuiltinExecutableCreationAuthority::BuiltinExecutableCache,
        }
    }

    pub const fn with_code_index(
        id: BuiltinId,
        code_index: BuiltinCodeIndex,
        handle: BuiltinExecutableHandle,
        revision: BuiltinTableRevision,
        state: BuiltinExecutableCacheState,
        authority: BuiltinExecutableCreationAuthority,
    ) -> Self {
        Self {
            id,
            code_index: Some(code_index),
            handle,
            revision,
            state,
            authority,
        }
    }

    pub const fn state(self) -> BuiltinExecutableCacheState {
        self.state
    }

    pub const fn code_index(self) -> Option<BuiltinCodeIndex> {
        self.code_index
    }

    pub const fn authority(self) -> BuiltinExecutableCreationAuthority {
        self.authority
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

/// Component that owns a published builtin descriptor registry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BuiltinRegistryOwner {
    #[default]
    BuiltinGenerator,
    VmInitialization,
    TestFixture,
}

/// Authority allowed to mutate or replace builtin registry contents.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BuiltinRegistryMutationAuthority {
    #[default]
    GeneratedDataRefresh,
    VmStartup,
    ExternalNameRegistration,
}

/// Provenance for generated builtin descriptor tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinRegistryProvenance {
    pub generator: &'static str,
    pub source_root: &'static str,
    pub revision: BuiltinTableRevision,
}

impl BuiltinRegistryProvenance {
    pub const fn new(
        generator: &'static str,
        source_root: &'static str,
        revision: BuiltinTableRevision,
    ) -> Self {
        Self {
            generator,
            source_root,
            revision,
        }
    }
}

/// Immutable generated source-provider descriptor for builtin code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinSourceDescriptor {
    pub id: BuiltinId,
    pub source: &'static BuiltinSource,
    pub revision: BuiltinTableRevision,
}

/// Immutable generated descriptor registry for builtins.
///
/// The VM owns installation into runtime objects and executable caches. This
/// table only exposes generated metadata that already exists in static storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticBuiltinRegistry {
    pub state: BuiltinRegistryState,
    pub owner: BuiltinRegistryOwner,
    pub mutation_authority: BuiltinRegistryMutationAuthority,
    pub provenance: BuiltinRegistryProvenance,
    pub descriptors: &'static [BuiltinDescriptor],
    pub sources: &'static [BuiltinSourceDescriptor],
    pub pipeline: &'static [BuiltinGeneratedPipelineStep],
    pub intrinsic_hooks: &'static [HostIntrinsicHook],
    pub executable_cache: &'static [BuiltinExecutableCacheEntry],
}

impl StaticBuiltinRegistry {
    pub const fn descriptors(self) -> &'static [BuiltinDescriptor] {
        self.descriptors
    }

    pub const fn sources(self) -> &'static [BuiltinSourceDescriptor] {
        self.sources
    }

    pub const fn pipeline(self) -> &'static [BuiltinGeneratedPipelineStep] {
        self.pipeline
    }

    pub const fn intrinsic_hooks(self) -> &'static [HostIntrinsicHook] {
        self.intrinsic_hooks
    }

    pub const fn executable_cache(self) -> &'static [BuiltinExecutableCacheEntry] {
        self.executable_cache
    }

    pub fn descriptor_for_id(self, id: BuiltinId) -> Option<&'static BuiltinDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.id() == id)
    }

    pub fn executable_cache_entry_for_id(
        self,
        id: BuiltinId,
    ) -> Option<&'static BuiltinExecutableCacheEntry> {
        self.executable_cache.iter().find(|entry| entry.id == id)
    }

    pub fn execution_metadata_for_id(
        self,
        id: BuiltinId,
    ) -> Option<BuiltinExecutionMetadataDescriptor> {
        let descriptor = self.descriptor_for_id(id)?;
        let intrinsic_hook = descriptor.intrinsic().and_then(|intrinsic| {
            self.intrinsic_hooks
                .iter()
                .find(|hook| hook.descriptor().intrinsic() == intrinsic)
                .copied()
        });
        let cache_entry = self.executable_cache_entry_for_id(id).copied();
        Some(BuiltinExecutionMetadataDescriptor {
            id,
            source_kind: descriptor.source().kind(),
            source_range: descriptor.source().range(),
            visibility: descriptor.visibility(),
            intrinsic: descriptor.intrinsic(),
            executable: descriptor.executable(),
            intrinsic_hook,
            cache_entry,
            registry_state: self.state,
            revision: self.provenance.revision,
        })
    }

    pub fn source_for_id(self, id: BuiltinId) -> Option<&'static BuiltinSourceDescriptor> {
        self.sources.iter().find(|source| source.id == id)
    }

    pub fn validate(self) -> BuiltinRegistryValidationReport {
        let mut findings = Vec::new();
        for (index, descriptor) in self.descriptors.iter().enumerate() {
            if self.descriptors[..index]
                .iter()
                .any(|candidate| candidate.id() == descriptor.id())
            {
                findings.push(BuiltinRegistryValidationFinding::DuplicateBuiltinId {
                    id: descriptor.id(),
                });
            }
            if self.source_for_id(descriptor.id()).is_none() {
                findings.push(BuiltinRegistryValidationFinding::MissingSourceDescriptor {
                    id: descriptor.id(),
                });
            }
            validate_builtin_source(descriptor.id(), descriptor.source(), &mut findings);
            validate_init_step(self, descriptor, &mut findings);
            validate_executable_descriptor(self, descriptor, &mut findings);
        }

        for (index, source) in self.sources.iter().enumerate() {
            if self.sources[..index]
                .iter()
                .any(|candidate| candidate.id == source.id)
            {
                findings.push(
                    BuiltinRegistryValidationFinding::DuplicateSourceDescriptor { id: source.id },
                );
            }
            if self.descriptor_for_id(source.id).is_none() {
                findings.push(BuiltinRegistryValidationFinding::SourceWithoutDescriptor {
                    id: source.id,
                });
            }
            validate_builtin_source(source.id, source.source, &mut findings);
        }

        for (index, entry) in self.executable_cache.iter().enumerate() {
            if self.executable_cache[..index]
                .iter()
                .any(|candidate| candidate.handle == entry.handle)
            {
                findings.push(
                    BuiltinRegistryValidationFinding::DuplicateExecutableCacheHandle {
                        handle: entry.handle,
                    },
                );
            }
            if self.descriptor_for_id(entry.id).is_none() {
                findings.push(
                    BuiltinRegistryValidationFinding::ExecutableCacheWithoutDescriptor {
                        id: entry.id,
                    },
                );
            }
            if entry.revision != self.provenance.revision {
                findings.push(
                    BuiltinRegistryValidationFinding::ExecutableCacheRevisionMismatch {
                        id: entry.id,
                        expected: self.provenance.revision,
                        actual: entry.revision,
                    },
                );
            }
        }

        for hook in self.intrinsic_hooks {
            let intrinsic = hook.descriptor().intrinsic();
            if !self
                .descriptors
                .iter()
                .any(|descriptor| descriptor.intrinsic() == Some(intrinsic))
            {
                findings.push(
                    BuiltinRegistryValidationFinding::IntrinsicHookWithoutDescriptor { intrinsic },
                );
            } else if let Some(descriptor) = self
                .descriptors
                .iter()
                .find(|descriptor| descriptor.intrinsic() == Some(intrinsic))
            {
                validate_intrinsic_semantic_hook(descriptor, hook, &mut findings);
            }
            if builtin_stage_rank(hook.required_stage())
                < builtin_stage_rank(BuiltinInitializationStage::IntrinsicsBound)
            {
                findings.push(
                    BuiltinRegistryValidationFinding::IntrinsicHookStageTooEarly {
                        intrinsic,
                        stage: hook.required_stage(),
                    },
                );
            }
        }
        validate_pipeline(self, &mut findings);

        BuiltinRegistryValidationReport { findings }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinExecutionMetadataDescriptor {
    pub id: BuiltinId,
    pub source_kind: BuiltinSourceKind,
    pub source_range: Option<BuiltinSourceRange>,
    pub visibility: BuiltinVisibility,
    pub intrinsic: Option<BuiltinIntrinsic>,
    pub executable: Option<BuiltinExecutableMetadata>,
    pub intrinsic_hook: Option<HostIntrinsicHook>,
    pub cache_entry: Option<BuiltinExecutableCacheEntry>,
    pub registry_state: BuiltinRegistryState,
    pub revision: BuiltinTableRevision,
}

impl BuiltinExecutionMetadataDescriptor {
    pub fn is_ready_for_interpreter_entry(self) -> bool {
        self.executable.is_some()
            && self.cache_entry.is_some_and(|entry| {
                matches!(
                    entry.state(),
                    BuiltinExecutableCacheState::UnlinkedExecutableCached
                        | BuiltinExecutableCacheState::LinkedExecutableInstalled
                )
            })
            && matches!(
                self.registry_state,
                BuiltinRegistryState::ExecutablesReady | BuiltinRegistryState::Sealed
            )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinExecutionTierKind {
    MetadataOnly,
    HostIntrinsic,
    Interpreter,
    LinkedExecutable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuiltinTierExecutionMetadata {
    pub metadata: BuiltinExecutionMetadataDescriptor,
    pub tier: BuiltinExecutionTierKind,
    pub can_enter_interpreter: bool,
    pub can_call_host_intrinsic: bool,
    pub needs_lazy_parse: bool,
    pub diagnostics: Vec<BuiltinIntegrationDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuiltinRegistryIntegrationSummary {
    pub descriptor_count: u32,
    pub executable_count: u32,
    pub interpreter_ready_count: u32,
    pub intrinsic_ready_count: u32,
    pub metadata_only_count: u32,
    pub diagnostics: Vec<BuiltinIntegrationDiagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinIntegrationDiagnostic {
    pub id: Option<BuiltinId>,
    pub kind: BuiltinIntegrationDiagnosticKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinIntegrationDiagnosticKind {
    RegistryValidation(BuiltinRegistryValidationFinding),
    ExecutableMissingCacheEntry,
    ExecutableCacheNotReady(BuiltinExecutableCacheState),
    ExecutableRegistryNotReady(BuiltinRegistryState),
    IntrinsicHookMissing,
    MetadataOnlyBuiltin,
}

pub fn describe_builtin_tier_execution_metadata(
    registry: StaticBuiltinRegistry,
    id: BuiltinId,
) -> Option<BuiltinTierExecutionMetadata> {
    let metadata = registry.execution_metadata_for_id(id)?;
    let mut diagnostics = Vec::new();
    let can_enter_interpreter = metadata.is_ready_for_interpreter_entry();
    let can_call_host_intrinsic = metadata.intrinsic_hook.is_some();
    let needs_lazy_parse = metadata.executable.is_some()
        && metadata.cache_entry.is_some_and(|entry| {
            matches!(
                entry.state(),
                BuiltinExecutableCacheState::Empty
                    | BuiltinExecutableCacheState::SourceMaterialized
                    | BuiltinExecutableCacheState::MetadataValidated
                    | BuiltinExecutableCacheState::ParseRequested
            )
        });

    if metadata.source_kind == BuiltinSourceKind::GeneratedMetadataOnly {
        diagnostics.push(BuiltinIntegrationDiagnostic {
            id: Some(id),
            kind: BuiltinIntegrationDiagnosticKind::MetadataOnlyBuiltin,
        });
    }
    if metadata.executable.is_some() && metadata.cache_entry.is_none() {
        diagnostics.push(BuiltinIntegrationDiagnostic {
            id: Some(id),
            kind: BuiltinIntegrationDiagnosticKind::ExecutableMissingCacheEntry,
        });
    }
    if let Some(entry) = metadata.cache_entry {
        if metadata.executable.is_some()
            && !matches!(
                entry.state(),
                BuiltinExecutableCacheState::UnlinkedExecutableCached
                    | BuiltinExecutableCacheState::LinkedExecutableInstalled
            )
        {
            diagnostics.push(BuiltinIntegrationDiagnostic {
                id: Some(id),
                kind: BuiltinIntegrationDiagnosticKind::ExecutableCacheNotReady(entry.state()),
            });
        }
    }
    if metadata.executable.is_some()
        && !matches!(
            metadata.registry_state,
            BuiltinRegistryState::ExecutablesReady | BuiltinRegistryState::Sealed
        )
    {
        diagnostics.push(BuiltinIntegrationDiagnostic {
            id: Some(id),
            kind: BuiltinIntegrationDiagnosticKind::ExecutableRegistryNotReady(
                metadata.registry_state,
            ),
        });
    }
    if metadata.intrinsic.is_some() && metadata.intrinsic_hook.is_none() {
        diagnostics.push(BuiltinIntegrationDiagnostic {
            id: Some(id),
            kind: BuiltinIntegrationDiagnosticKind::IntrinsicHookMissing,
        });
    }

    let tier = if metadata.source_kind == BuiltinSourceKind::GeneratedMetadataOnly {
        BuiltinExecutionTierKind::MetadataOnly
    } else if metadata.cache_entry.is_some_and(|entry| {
        entry.state() == BuiltinExecutableCacheState::LinkedExecutableInstalled
    }) {
        BuiltinExecutionTierKind::LinkedExecutable
    } else if can_enter_interpreter {
        BuiltinExecutionTierKind::Interpreter
    } else if can_call_host_intrinsic {
        BuiltinExecutionTierKind::HostIntrinsic
    } else {
        BuiltinExecutionTierKind::MetadataOnly
    };

    Some(BuiltinTierExecutionMetadata {
        metadata,
        tier,
        can_enter_interpreter,
        can_call_host_intrinsic,
        needs_lazy_parse,
        diagnostics,
    })
}

pub fn summarize_builtin_registry_integration(
    registry: StaticBuiltinRegistry,
) -> BuiltinRegistryIntegrationSummary {
    let mut diagnostics: Vec<BuiltinIntegrationDiagnostic> = registry
        .validate()
        .findings
        .into_iter()
        .map(|finding| BuiltinIntegrationDiagnostic {
            id: builtin_id_for_validation_finding(finding),
            kind: BuiltinIntegrationDiagnosticKind::RegistryValidation(finding),
        })
        .collect();
    let mut executable_count = 0;
    let mut interpreter_ready_count = 0;
    let mut intrinsic_ready_count = 0;
    let mut metadata_only_count = 0;

    for descriptor in registry.descriptors {
        if descriptor.executable().is_some() {
            executable_count += 1;
        }
        if let Some(tier) = describe_builtin_tier_execution_metadata(registry, descriptor.id()) {
            if tier.can_enter_interpreter {
                interpreter_ready_count += 1;
            }
            if tier.can_call_host_intrinsic {
                intrinsic_ready_count += 1;
            }
            if tier.tier == BuiltinExecutionTierKind::MetadataOnly {
                metadata_only_count += 1;
            }
            diagnostics.extend(tier.diagnostics);
        }
    }

    BuiltinRegistryIntegrationSummary {
        descriptor_count: registry.descriptors.len() as u32,
        executable_count,
        interpreter_ready_count,
        intrinsic_ready_count,
        metadata_only_count,
        diagnostics,
    }
}

fn builtin_id_for_validation_finding(
    finding: BuiltinRegistryValidationFinding,
) -> Option<BuiltinId> {
    match finding {
        BuiltinRegistryValidationFinding::DuplicateBuiltinId { id }
        | BuiltinRegistryValidationFinding::MissingSourceDescriptor { id }
        | BuiltinRegistryValidationFinding::DuplicateSourceDescriptor { id }
        | BuiltinRegistryValidationFinding::SourceWithoutDescriptor { id }
        | BuiltinRegistryValidationFinding::UnorderedSourceRange { id, .. }
        | BuiltinRegistryValidationFinding::MetadataOnlySourceHasRange { id }
        | BuiltinRegistryValidationFinding::MissingInitDependency { id, .. }
        | BuiltinRegistryValidationFinding::InitDependencyStageInversion { id, .. }
        | BuiltinRegistryValidationFinding::MetadataOnlySourceHasExecutable { id }
        | BuiltinRegistryValidationFinding::ConstructorKindWithoutConstructAbility { id }
        | BuiltinRegistryValidationFinding::MissingExecutableCacheEntry { id, .. }
        | BuiltinRegistryValidationFinding::BuiltinSemanticEffectRequiresExceptionContract { id }
        | BuiltinRegistryValidationFinding::MetadataOnlySourceSemanticMismatch { id }
        | BuiltinRegistryValidationFinding::ExecutableCacheWithoutDescriptor { id }
        | BuiltinRegistryValidationFinding::ExecutableCacheRevisionMismatch { id, .. }
        | BuiltinRegistryValidationFinding::IntrinsicHookSemanticMismatch { id, .. } => Some(id),
        BuiltinRegistryValidationFinding::InitStepIdMismatch { descriptor, .. } => Some(descriptor),
        BuiltinRegistryValidationFinding::SelfReferentialPipelineEdge { id, .. }
        | BuiltinRegistryValidationFinding::MissingPipelineEdgeBuiltin { id, .. } => Some(id),
        BuiltinRegistryValidationFinding::IntrinsicHookWithoutDescriptor { .. }
        | BuiltinRegistryValidationFinding::IntrinsicHookStageTooEarly { .. }
        | BuiltinRegistryValidationFinding::PipelineRevisionMismatch { .. }
        | BuiltinRegistryValidationFinding::DuplicatePipelineStep { .. }
        | BuiltinRegistryValidationFinding::PipelineEdgeStageInversion { .. }
        | BuiltinRegistryValidationFinding::DuplicateExecutableCacheHandle { .. } => None,
    }
}

fn validate_pipeline(
    registry: StaticBuiltinRegistry,
    findings: &mut Vec<BuiltinRegistryValidationFinding>,
) {
    for (index, step) in registry.pipeline.iter().enumerate() {
        if step.revision != registry.provenance.revision {
            findings.push(BuiltinRegistryValidationFinding::PipelineRevisionMismatch {
                artifact: step.artifact,
                expected: registry.provenance.revision,
                actual: step.revision,
            });
        }
        if registry.pipeline[..index]
            .iter()
            .any(|candidate| candidate.artifact == step.artifact && candidate.phase == step.phase)
        {
            findings.push(BuiltinRegistryValidationFinding::DuplicatePipelineStep {
                artifact: step.artifact,
                phase: step.phase,
            });
        }
        if let Some(edge) = step.init_edge {
            if edge.before().id() == edge.after().id() {
                findings.push(
                    BuiltinRegistryValidationFinding::SelfReferentialPipelineEdge {
                        artifact: step.artifact,
                        id: edge.before().id(),
                    },
                );
            }
            if builtin_stage_rank(edge.before().stage()) > builtin_stage_rank(edge.after().stage())
            {
                findings.push(
                    BuiltinRegistryValidationFinding::PipelineEdgeStageInversion {
                        artifact: step.artifact,
                        before: edge.before().id(),
                        after: edge.after().id(),
                    },
                );
            }
            if registry.descriptor_for_id(edge.before().id()).is_none() {
                findings.push(
                    BuiltinRegistryValidationFinding::MissingPipelineEdgeBuiltin {
                        artifact: step.artifact,
                        id: edge.before().id(),
                    },
                );
            }
            if registry.descriptor_for_id(edge.after().id()).is_none() {
                findings.push(
                    BuiltinRegistryValidationFinding::MissingPipelineEdgeBuiltin {
                        artifact: step.artifact,
                        id: edge.after().id(),
                    },
                );
            }
        }
    }
}

fn validate_builtin_source(
    id: BuiltinId,
    source: &BuiltinSource,
    findings: &mut Vec<BuiltinRegistryValidationFinding>,
) {
    if let Some(range) = source.range() {
        if range.start_offset() > range.end_offset() {
            findings.push(BuiltinRegistryValidationFinding::UnorderedSourceRange { id, range });
        }
    }
    if source.kind() == BuiltinSourceKind::GeneratedMetadataOnly && source.range().is_some() {
        findings.push(BuiltinRegistryValidationFinding::MetadataOnlySourceHasRange { id });
    }
}

fn validate_init_step(
    registry: StaticBuiltinRegistry,
    descriptor: &BuiltinDescriptor,
    findings: &mut Vec<BuiltinRegistryValidationFinding>,
) {
    let step = descriptor.init_step();
    if step.id() != descriptor.id() {
        findings.push(BuiltinRegistryValidationFinding::InitStepIdMismatch {
            descriptor: descriptor.id(),
            step: step.id(),
        });
    }
    if let Some(depends_on) = step.depends_on() {
        match registry.descriptor_for_id(depends_on) {
            Some(dependency) => {
                if builtin_stage_rank(dependency.init_step().stage())
                    > builtin_stage_rank(step.stage())
                {
                    findings.push(
                        BuiltinRegistryValidationFinding::InitDependencyStageInversion {
                            id: descriptor.id(),
                            depends_on,
                        },
                    );
                }
            }
            None => findings.push(BuiltinRegistryValidationFinding::MissingInitDependency {
                id: descriptor.id(),
                depends_on,
            }),
        }
    }
}

fn validate_executable_descriptor(
    registry: StaticBuiltinRegistry,
    descriptor: &BuiltinDescriptor,
    findings: &mut Vec<BuiltinRegistryValidationFinding>,
) {
    let Some(executable) = descriptor.executable() else {
        return;
    };
    if descriptor.source().kind() == BuiltinSourceKind::GeneratedMetadataOnly {
        findings.push(
            BuiltinRegistryValidationFinding::MetadataOnlySourceHasExecutable {
                id: descriptor.id(),
            },
        );
    }
    if executable.construct_ability() == BuiltinConstructAbility::CannotConstruct
        && executable.constructor_kind() != BuiltinConstructorKind::None
    {
        findings.push(
            BuiltinRegistryValidationFinding::ConstructorKindWithoutConstructAbility {
                id: descriptor.id(),
            },
        );
    }
    if let Some(code_index) = executable.code_index() {
        let has_cache_entry = registry
            .executable_cache
            .iter()
            .any(|entry| entry.id == descriptor.id() && entry.code_index() == Some(code_index));
        if !has_cache_entry {
            findings.push(
                BuiltinRegistryValidationFinding::MissingExecutableCacheEntry {
                    id: descriptor.id(),
                    code_index,
                },
            );
        }
    }
    let semantic = executable.semantic();
    if (semantic.effects.allocates
        || semantic.effects.calls_host
        || semantic.effects.calls_user_code)
        && semantic.exception == BuiltinExceptionContract::CannotThrow
    {
        findings.push(
            BuiltinRegistryValidationFinding::BuiltinSemanticEffectRequiresExceptionContract {
                id: descriptor.id(),
            },
        );
    }
    if descriptor.source().kind() == BuiltinSourceKind::GeneratedMetadataOnly
        && semantic.parse_source != BuiltinSemanticParseSource::MetadataOnly
    {
        findings.push(
            BuiltinRegistryValidationFinding::MetadataOnlySourceSemanticMismatch {
                id: descriptor.id(),
            },
        );
    }
}

fn validate_intrinsic_semantic_hook(
    descriptor: &BuiltinDescriptor,
    hook: &HostIntrinsicHook,
    findings: &mut Vec<BuiltinRegistryValidationFinding>,
) {
    let Some(executable) = descriptor.executable() else {
        return;
    };
    let semantic = executable.semantic();
    let intrinsic = hook.descriptor().intrinsic();
    let mismatch = match hook.descriptor().safety() {
        crate::builtins::intrinsics::IntrinsicSafety::PureMetadata => {
            semantic.effects.allocates
                || semantic.effects.calls_host
                || semantic.effects.calls_user_code
                || semantic.exception != BuiltinExceptionContract::CannotThrow
        }
        crate::builtins::intrinsics::IntrinsicSafety::MayAllocate => !semantic.effects.allocates,
        crate::builtins::intrinsics::IntrinsicSafety::MayCallHost => !semantic.effects.calls_host,
        crate::builtins::intrinsics::IntrinsicSafety::MayThrow => {
            semantic.exception == BuiltinExceptionContract::CannotThrow
        }
    };
    if mismatch {
        findings.push(
            BuiltinRegistryValidationFinding::IntrinsicHookSemanticMismatch {
                id: descriptor.id(),
                intrinsic,
            },
        );
    }
}

fn builtin_stage_rank(stage: BuiltinInitializationStage) -> u8 {
    match stage {
        BuiltinInitializationStage::StaticSymbolsReserved => 0,
        BuiltinInitializationStage::NamesInterned => 1,
        BuiltinInitializationStage::MetadataRegistered => 2,
        BuiltinInitializationStage::IntrinsicsBound => 3,
        BuiltinInitializationStage::ExecutablesAvailable => 4,
        BuiltinInitializationStage::GlobalPropertiesInstalled => 5,
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BuiltinRegistryValidationReport {
    pub findings: Vec<BuiltinRegistryValidationFinding>,
}

impl BuiltinRegistryValidationReport {
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinRegistryValidationFinding {
    DuplicateBuiltinId {
        id: BuiltinId,
    },
    MissingSourceDescriptor {
        id: BuiltinId,
    },
    DuplicateSourceDescriptor {
        id: BuiltinId,
    },
    SourceWithoutDescriptor {
        id: BuiltinId,
    },
    UnorderedSourceRange {
        id: BuiltinId,
        range: BuiltinSourceRange,
    },
    MetadataOnlySourceHasRange {
        id: BuiltinId,
    },
    InitStepIdMismatch {
        descriptor: BuiltinId,
        step: BuiltinId,
    },
    MissingInitDependency {
        id: BuiltinId,
        depends_on: BuiltinId,
    },
    InitDependencyStageInversion {
        id: BuiltinId,
        depends_on: BuiltinId,
    },
    MetadataOnlySourceHasExecutable {
        id: BuiltinId,
    },
    ConstructorKindWithoutConstructAbility {
        id: BuiltinId,
    },
    MissingExecutableCacheEntry {
        id: BuiltinId,
        code_index: BuiltinCodeIndex,
    },
    BuiltinSemanticEffectRequiresExceptionContract {
        id: BuiltinId,
    },
    MetadataOnlySourceSemanticMismatch {
        id: BuiltinId,
    },
    DuplicateExecutableCacheHandle {
        handle: BuiltinExecutableHandle,
    },
    ExecutableCacheWithoutDescriptor {
        id: BuiltinId,
    },
    ExecutableCacheRevisionMismatch {
        id: BuiltinId,
        expected: BuiltinTableRevision,
        actual: BuiltinTableRevision,
    },
    IntrinsicHookWithoutDescriptor {
        intrinsic: BuiltinIntrinsic,
    },
    IntrinsicHookStageTooEarly {
        intrinsic: BuiltinIntrinsic,
        stage: BuiltinInitializationStage,
    },
    IntrinsicHookSemanticMismatch {
        id: BuiltinId,
        intrinsic: BuiltinIntrinsic,
    },
    PipelineRevisionMismatch {
        artifact: BuiltinGeneratedArtifact,
        expected: BuiltinTableRevision,
        actual: BuiltinTableRevision,
    },
    DuplicatePipelineStep {
        artifact: BuiltinGeneratedArtifact,
        phase: BuiltinGenerationPhase,
    },
    SelfReferentialPipelineEdge {
        artifact: BuiltinGeneratedArtifact,
        id: BuiltinId,
    },
    PipelineEdgeStageInversion {
        artifact: BuiltinGeneratedArtifact,
        before: BuiltinId,
        after: BuiltinId,
    },
    MissingPipelineEdgeBuiltin {
        artifact: BuiltinGeneratedArtifact,
        id: BuiltinId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: BuiltinId = BuiltinId::from_generated_index(0);
    const REVISION: BuiltinTableRevision = BuiltinTableRevision::from_generator_revision(1);
    const SOURCE: BuiltinSource = BuiltinSource::with_generated_range(
        BuiltinSourceKind::JavaScriptSource,
        WellKnownBuiltinName::PublicIdentifier(crate::strings::Identifier::from_atom(
            crate::strings::AtomId::from_table_slot(1),
        )),
        BuiltinSourceRange::new(0, 4),
    );
    const SOURCE_DESCRIPTOR: BuiltinSourceDescriptor = BuiltinSourceDescriptor {
        id: ID,
        source: &SOURCE,
        revision: REVISION,
    };
    const CODE_INDEX: BuiltinCodeIndex = BuiltinCodeIndex::from_generated_index(3);
    const EXECUTABLE: BuiltinExecutableMetadata = BuiltinExecutableMetadata::with_code_index(
        CODE_INDEX,
        0,
        BuiltinConstructorKind::None,
        BuiltinConstructAbility::CannotConstruct,
        BuiltinInlinePolicy::Never,
        BuiltinParserMode::Function,
    );
    const STEP: BuiltinInitializationStep =
        BuiltinInitializationStep::new(ID, BuiltinInitializationStage::MetadataRegistered, None);
    const DESCRIPTOR: BuiltinDescriptor = BuiltinDescriptor::with_executable_metadata(
        ID,
        SOURCE,
        BuiltinVisibility::Private,
        None,
        EXECUTABLE,
        STEP,
    );
    const CACHE_ENTRY: BuiltinExecutableCacheEntry = BuiltinExecutableCacheEntry::with_code_index(
        ID,
        CODE_INDEX,
        BuiltinExecutableHandle::from_cache_slot(0),
        REVISION,
        BuiltinExecutableCacheState::MetadataValidated,
        BuiltinExecutableCreationAuthority::BuiltinExecutableCache,
    );
    const PIPELINE_STEP: BuiltinGeneratedPipelineStep = BuiltinGeneratedPipelineStep::new(
        BuiltinGenerationPhase::EmitMetadata,
        BuiltinGeneratedArtifact::ExecutableMetadata,
        REVISION,
    );

    fn registry(
        descriptors: &'static [BuiltinDescriptor],
        sources: &'static [BuiltinSourceDescriptor],
        cache: &'static [BuiltinExecutableCacheEntry],
        pipeline: &'static [BuiltinGeneratedPipelineStep],
    ) -> StaticBuiltinRegistry {
        StaticBuiltinRegistry {
            state: BuiltinRegistryState::DescriptorsReady,
            owner: BuiltinRegistryOwner::TestFixture,
            mutation_authority: BuiltinRegistryMutationAuthority::GeneratedDataRefresh,
            provenance: BuiltinRegistryProvenance::new("test", "test", REVISION),
            descriptors,
            sources,
            pipeline,
            intrinsic_hooks: &[],
            executable_cache: cache,
        }
    }

    #[test]
    fn builtin_registry_validation_accepts_consistent_registry() {
        assert!(registry(
            &[DESCRIPTOR],
            &[SOURCE_DESCRIPTOR],
            &[CACHE_ENTRY],
            &[PIPELINE_STEP]
        )
        .validate()
        .is_valid());
    }

    #[test]
    fn builtin_registry_validation_reports_missing_edges() {
        let report = registry(&[DESCRIPTOR, DESCRIPTOR], &[], &[], &[]).validate();

        assert!(report
            .findings
            .contains(&BuiltinRegistryValidationFinding::DuplicateBuiltinId { id: ID }));
        assert!(report
            .findings
            .contains(&BuiltinRegistryValidationFinding::MissingSourceDescriptor { id: ID }));
        assert!(report.findings.contains(
            &BuiltinRegistryValidationFinding::MissingExecutableCacheEntry {
                id: ID,
                code_index: CODE_INDEX,
            }
        ));
    }

    #[test]
    fn builtin_registry_validation_reports_pipeline_mismatches() {
        const BAD_REVISION: BuiltinTableRevision = BuiltinTableRevision::from_generator_revision(2);
        const BAD_STEP: BuiltinGeneratedPipelineStep = BuiltinGeneratedPipelineStep::new(
            BuiltinGenerationPhase::EmitMetadata,
            BuiltinGeneratedArtifact::ExecutableMetadata,
            BAD_REVISION,
        );

        let report = registry(
            &[DESCRIPTOR],
            &[SOURCE_DESCRIPTOR],
            &[CACHE_ENTRY],
            &[PIPELINE_STEP, BAD_STEP],
        )
        .validate();

        assert!(report.findings.contains(
            &BuiltinRegistryValidationFinding::PipelineRevisionMismatch {
                artifact: BuiltinGeneratedArtifact::ExecutableMetadata,
                expected: REVISION,
                actual: BAD_REVISION,
            }
        ));
        assert!(report.findings.contains(
            &BuiltinRegistryValidationFinding::DuplicatePipelineStep {
                artifact: BuiltinGeneratedArtifact::ExecutableMetadata,
                phase: BuiltinGenerationPhase::EmitMetadata,
            }
        ));
    }

    #[test]
    fn builtin_registry_validation_reports_semantic_contract_mismatches() {
        const THROWING_EXECUTABLE: BuiltinExecutableMetadata =
            EXECUTABLE.with_semantic_metadata(BuiltinSemanticMetadata {
                strict: true,
                effects: BuiltinSemanticEffects {
                    calls_host: true,
                    ..BuiltinSemanticEffects::none()
                },
                exception: BuiltinExceptionContract::CannotThrow,
                parse_source: BuiltinSemanticParseSource::GeneratedBuiltin,
            });
        const THROWING_DESCRIPTOR: BuiltinDescriptor = BuiltinDescriptor::with_executable_metadata(
            ID,
            SOURCE,
            BuiltinVisibility::Private,
            Some(BuiltinIntrinsic::from_generated_index(1)),
            THROWING_EXECUTABLE,
            STEP,
        );
        const HOOK: HostIntrinsicHook = HostIntrinsicHook::new(
            BuiltinIntrinsicDescriptor::new(
                BuiltinIntrinsic::from_generated_index(1),
                crate::builtins::intrinsics::IntrinsicArity::Fixed(0),
                crate::builtins::intrinsics::IntrinsicSafety::MayThrow,
                crate::builtins::intrinsics::IntrinsicHostOwner::Vm,
                crate::builtins::intrinsics::IntrinsicBindingPhase::Bound,
            ),
            BuiltinInitializationStage::IntrinsicsBound,
        );

        let mut registry = registry(
            &[THROWING_DESCRIPTOR],
            &[SOURCE_DESCRIPTOR],
            &[CACHE_ENTRY],
            &[PIPELINE_STEP],
        );
        registry.intrinsic_hooks = &[HOOK];
        let findings = registry.validate().findings;

        assert!(findings.contains(
            &BuiltinRegistryValidationFinding::BuiltinSemanticEffectRequiresExceptionContract {
                id: ID,
            }
        ));
        assert!(findings.contains(
            &BuiltinRegistryValidationFinding::IntrinsicHookSemanticMismatch {
                id: ID,
                intrinsic: BuiltinIntrinsic::from_generated_index(1),
            }
        ));
    }

    #[test]
    fn builtin_execution_metadata_reports_interpreter_readiness() {
        const READY_CACHE_ENTRY: BuiltinExecutableCacheEntry =
            BuiltinExecutableCacheEntry::with_code_index(
                ID,
                CODE_INDEX,
                BuiltinExecutableHandle::from_cache_slot(0),
                REVISION,
                BuiltinExecutableCacheState::UnlinkedExecutableCached,
                BuiltinExecutableCreationAuthority::BuiltinExecutableCache,
            );
        let mut registry = registry(
            &[DESCRIPTOR],
            &[SOURCE_DESCRIPTOR],
            &[READY_CACHE_ENTRY],
            &[PIPELINE_STEP],
        );
        registry.state = BuiltinRegistryState::ExecutablesReady;

        let metadata = registry
            .execution_metadata_for_id(ID)
            .expect("execution metadata");

        assert_eq!(metadata.executable, Some(EXECUTABLE));
        assert_eq!(metadata.cache_entry, Some(READY_CACHE_ENTRY));
        assert!(metadata.is_ready_for_interpreter_entry());
    }

    #[test]
    fn builtin_tier_metadata_reports_lazy_parse_and_registry_gate() {
        let metadata = describe_builtin_tier_execution_metadata(
            registry(
                &[DESCRIPTOR],
                &[SOURCE_DESCRIPTOR],
                &[CACHE_ENTRY],
                &[PIPELINE_STEP],
            ),
            ID,
        )
        .expect("tier metadata");

        assert_eq!(metadata.tier, BuiltinExecutionTierKind::MetadataOnly);
        assert!(metadata.needs_lazy_parse);
        assert!(!metadata.can_enter_interpreter);
        assert!(metadata
            .diagnostics
            .contains(&BuiltinIntegrationDiagnostic {
                id: Some(ID),
                kind: BuiltinIntegrationDiagnosticKind::ExecutableCacheNotReady(
                    BuiltinExecutableCacheState::MetadataValidated,
                ),
            }));
        assert!(metadata
            .diagnostics
            .contains(&BuiltinIntegrationDiagnostic {
                id: Some(ID),
                kind: BuiltinIntegrationDiagnosticKind::ExecutableRegistryNotReady(
                    BuiltinRegistryState::DescriptorsReady,
                ),
            }));
    }

    #[test]
    fn builtin_registry_integration_counts_ready_entries() {
        const READY_CACHE_ENTRY: BuiltinExecutableCacheEntry =
            BuiltinExecutableCacheEntry::with_code_index(
                ID,
                CODE_INDEX,
                BuiltinExecutableHandle::from_cache_slot(0),
                REVISION,
                BuiltinExecutableCacheState::UnlinkedExecutableCached,
                BuiltinExecutableCreationAuthority::BuiltinExecutableCache,
            );
        let mut registry = registry(
            &[DESCRIPTOR],
            &[SOURCE_DESCRIPTOR],
            &[READY_CACHE_ENTRY],
            &[PIPELINE_STEP],
        );
        registry.state = BuiltinRegistryState::ExecutablesReady;

        let summary = summarize_builtin_registry_integration(registry);

        assert_eq!(summary.descriptor_count, 1);
        assert_eq!(summary.executable_count, 1);
        assert_eq!(summary.interpreter_ready_count, 1);
        assert!(summary.diagnostics.is_empty());
    }
}
