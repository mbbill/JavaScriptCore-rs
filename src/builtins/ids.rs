/// Generated builtin table index.
///
/// The index is stable only within the generated builtin metadata set that
/// created it. Public APIs should refer to names or descriptors instead.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BuiltinId(u32);

impl BuiltinId {
    pub const fn from_generated_index(index: u32) -> Self {
        Self(index)
    }

    pub const fn generated_index(self) -> u32 {
        self.0
    }
}

/// Stable identity for the generated builtin metadata set.
///
/// C++ JSC derives builtin code indexes, source offsets, visibility flags, and
/// parser metadata from generated tables. Rust keeps the revision explicit so a
/// cached executable handle can prove it came from the same metadata snapshot as
/// the descriptor that requested it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BuiltinTableRevision(u32);

impl BuiltinTableRevision {
    pub const fn from_generator_revision(revision: u32) -> Self {
        Self(revision)
    }

    pub const fn generator_revision(self) -> u32 {
        self.0
    }
}

/// Phase in VM/global initialization where a builtin descriptor becomes usable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinInitializationStage {
    /// Static symbol and private-name storage has process lifetime.
    StaticSymbolsReserved,
    /// VM common identifiers and builtin names are installed.
    NamesInterned,
    /// Generated descriptor metadata has been registered.
    MetadataRegistered,
    /// Intrinsic hooks have been attached to runtime entry points.
    IntrinsicsBound,
    /// Lazy executable cache may create unlinked builtin executables.
    ExecutablesAvailable,
    /// Global-object installation may expose public builtin properties.
    GlobalPropertiesInstalled,
}

/// Artifact emitted by the builtin generator pipeline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinGeneratedArtifact {
    SourceSpanTable,
    NameTable,
    PrivateNameTable,
    ExecutableMetadata,
    IntrinsicTable,
    DependencyOrder,
}

/// Generator phase that produced an artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinGenerationPhase {
    ScanJavaScriptSources,
    ParseAnnotations,
    ReservePrivateNames,
    EmitMetadata,
    EmitRustBindings,
    VerifyInitOrder,
}

/// Dependency edge in generated builtin initialization order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinInitializationEdge {
    before: BuiltinInitializationStep,
    after: BuiltinInitializationStep,
    artifact: BuiltinGeneratedArtifact,
}

impl BuiltinInitializationEdge {
    pub const fn new(
        before: BuiltinInitializationStep,
        after: BuiltinInitializationStep,
        artifact: BuiltinGeneratedArtifact,
    ) -> Self {
        Self {
            before,
            after,
            artifact,
        }
    }

    pub const fn before(self) -> BuiltinInitializationStep {
        self.before
    }

    pub const fn after(self) -> BuiltinInitializationStep {
        self.after
    }
}

/// Ordered initialization dependency for one builtin entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinInitializationStep {
    id: BuiltinId,
    stage: BuiltinInitializationStage,
    depends_on: Option<BuiltinId>,
}

impl BuiltinInitializationStep {
    pub const fn new(
        id: BuiltinId,
        stage: BuiltinInitializationStage,
        depends_on: Option<BuiltinId>,
    ) -> Self {
        Self {
            id,
            stage,
            depends_on,
        }
    }

    pub const fn id(self) -> BuiltinId {
        self.id
    }

    pub const fn stage(self) -> BuiltinInitializationStage {
        self.stage
    }

    pub const fn depends_on(self) -> Option<BuiltinId> {
        self.depends_on
    }
}

/// Visibility of a builtin entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinVisibility {
    Public,
    Private,
    ImplementationOnly,
    InlineOnly,
}
