use crate::modules::key::{ImportAttributes, ModuleKey, ModuleType, ResolvedSpecifier};

/// Syntactic or dynamic module request category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleRequestKind {
    StaticImport,
    ExportFrom,
    DynamicImport,
    WasmModuleImport,
    ImportMeta,
}

/// ECMA-262 module phase carried by a request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleRequestPhase {
    Parse,
    Resolve,
    Fetch,
    Link,
    Evaluation,
    Defer,
    AsyncEvaluation,
}

/// Resolution state for a module request as it crosses host hooks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModuleRequestResolution {
    Unresolved,
    Resolved(ModuleKey),
    Failed(ModuleRequestFailure),
}

/// Failure category attached to request resolution or loading.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleRequestFailureKind {
    Resolution,
    Fetch,
    Instantiation,
    Linking,
    Evaluation,
    UnsupportedAttributes,
    ImportMap,
    TopLevelAwait,
}

/// Cached request failure identity.
///
/// The concrete exception value belongs to runtime/GC modules. This struct only
/// records the module key and failure class so the loader can preserve priority
/// between fetch, instantiation, and evaluation errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleRequestFailure {
    kind: ModuleRequestFailureKind,
    key: Option<ModuleKey>,
}

impl ModuleRequestFailure {
    pub const fn new(kind: ModuleRequestFailureKind, key: Option<ModuleKey>) -> Self {
        Self { kind, key }
    }

    pub const fn kind(&self) -> ModuleRequestFailureKind {
        self.kind
    }

    pub const fn key(&self) -> Option<&ModuleKey> {
        self.key.as_ref()
    }
}

/// Import request before or after host resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleRequest {
    specifier: ResolvedSpecifier,
    module_type: ModuleType,
    attributes: ImportAttributes,
    kind: ModuleRequestKind,
    phase: ModuleRequestPhase,
    resolution: ModuleRequestResolution,
    is_top_level_await_dependency: bool,
}

impl ModuleRequest {
    pub const fn new(
        specifier: ResolvedSpecifier,
        module_type: ModuleType,
        attributes: ImportAttributes,
        kind: ModuleRequestKind,
    ) -> Self {
        Self {
            specifier,
            module_type,
            attributes,
            kind,
            phase: ModuleRequestPhase::Evaluation,
            resolution: ModuleRequestResolution::Unresolved,
            is_top_level_await_dependency: false,
        }
    }

    pub const fn with_phase(
        specifier: ResolvedSpecifier,
        module_type: ModuleType,
        attributes: ImportAttributes,
        kind: ModuleRequestKind,
        phase: ModuleRequestPhase,
    ) -> Self {
        Self {
            specifier,
            module_type,
            attributes,
            kind,
            phase,
            resolution: ModuleRequestResolution::Unresolved,
            is_top_level_await_dependency: matches!(phase, ModuleRequestPhase::AsyncEvaluation),
        }
    }

    pub const fn kind(&self) -> ModuleRequestKind {
        self.kind
    }

    pub const fn phase(&self) -> ModuleRequestPhase {
        self.phase
    }

    pub const fn resolution(&self) -> &ModuleRequestResolution {
        &self.resolution
    }

    pub const fn attributes(&self) -> &ImportAttributes {
        &self.attributes
    }

    pub const fn module_type(&self) -> ModuleType {
        self.module_type
    }

    pub const fn specifier(&self) -> ResolvedSpecifier {
        self.specifier
    }

    pub const fn is_top_level_await_dependency(&self) -> bool {
        self.is_top_level_await_dependency
    }
}
