use crate::modules::graph::ModuleGraphLoad;
use crate::modules::key::{ImportMapId, ImportMapMergePolicy, ModuleKey};
use crate::modules::registry::{ModuleRegistry, ModuleResolutionFailure};

/// Realm-owned module loading coordinator.
///
/// The loader owns registry state for a realm/global object. It coordinates
/// host loading, registry failure caching, graph linking, dynamic import, and
/// top-level await scheduling through runtime/promise integration points.
#[derive(Debug, Default)]
pub struct ModuleLoader {
    registry: ModuleRegistry,
}

/// Public loader entry point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleLoaderOperation {
    LoadModule,
    LinkAndEvaluate,
    DynamicImport,
    RequestImportModule,
    ProvideFetch,
    ResolveWithImportMap,
    FinishTopLevelAwait,
}

/// Coarse state for the realm-owned loader.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleLoaderState {
    Uninitialized,
    Ready,
    Loading,
    Evaluating,
    DrainingAsyncJobs,
    Failed,
}

/// Policy for a single loader operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleLoaderPolicy {
    operation: ModuleLoaderOperation,
    evaluate_after_load: bool,
    use_import_map: bool,
    dynamic_import: bool,
    import_map: Option<ImportMapId>,
    import_map_merge_policy: ImportMapMergePolicy,
}

impl ModuleLoaderPolicy {
    pub const fn new(
        operation: ModuleLoaderOperation,
        evaluate_after_load: bool,
        use_import_map: bool,
        dynamic_import: bool,
    ) -> Self {
        Self {
            operation,
            evaluate_after_load,
            use_import_map,
            dynamic_import,
            import_map: None,
            import_map_merge_policy: ImportMapMergePolicy::InitialMap,
        }
    }

    pub const fn with_import_map(
        operation: ModuleLoaderOperation,
        evaluate_after_load: bool,
        dynamic_import: bool,
        import_map: ImportMapId,
        import_map_merge_policy: ImportMapMergePolicy,
    ) -> Self {
        Self {
            operation,
            evaluate_after_load,
            use_import_map: true,
            dynamic_import,
            import_map: Some(import_map),
            import_map_merge_policy,
        }
    }

    pub const fn operation(self) -> ModuleLoaderOperation {
        self.operation
    }

    pub const fn use_import_map(self) -> bool {
        self.use_import_map
    }

    pub const fn dynamic_import(self) -> bool {
        self.dynamic_import
    }
}

/// Dynamic import payload carried until promise resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DynamicImportPayload {
    pub root: ModuleKey,
    pub promise_slot: crate::modules::registry::ModulePromiseSlot,
    pub referrer: Option<crate::modules::record::ModuleRecordId>,
    pub use_import_map: bool,
}

/// Top-level await scheduling state for a module graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TopLevelAwaitState {
    NotAsync,
    PendingEvaluation,
    WaitingOnDependency,
    Fulfilled,
    Rejected,
}

/// Dependency edge used by async module evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TopLevelAwaitDependency {
    pub parent: crate::modules::record::ModuleRecordId,
    pub child: crate::modules::record::ModuleRecordId,
    pub state: TopLevelAwaitState,
}

/// Loader-owned cache for resolution failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolutionFailureCacheEntry {
    failure: ModuleResolutionFailure,
}

impl ResolutionFailureCacheEntry {
    pub const fn new(failure: ModuleResolutionFailure) -> Self {
        Self { failure }
    }

    pub const fn failure(self) -> ModuleResolutionFailure {
        self.failure
    }
}

impl ModuleLoader {
    pub const fn new_uninitialized() -> Self {
        Self {
            registry: ModuleRegistry::new_uninitialized(),
        }
    }

    pub const fn registry(&self) -> &ModuleRegistry {
        &self.registry
    }

    pub const fn begin_graph_load(&self, root: ModuleKey) -> ModuleGraphLoad {
        ModuleGraphLoad::new(root)
    }
}
