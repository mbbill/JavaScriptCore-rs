use crate::modules::request::ModuleRequest;
use crate::strings::Identifier;

/// Stable module-record identity within a realm's loader.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ModuleRecordId(u32);

impl ModuleRecordId {
    pub const fn from_loader_slot(slot: u32) -> Self {
        Self(slot)
    }

    pub const fn loader_slot(self) -> u32 {
        self.0
    }
}

/// Concrete module-record family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleRecordKind {
    SourceText,
    Cyclic,
    Synthetic,
}

/// ECMA cyclic-module status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CyclicModuleStatus {
    New,
    Unlinked,
    Linking,
    Linked,
    Evaluating,
    EvaluatingAsync,
    Evaluated,
    Failed,
}

/// Async evaluation order marker for top-level await.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AsyncEvaluationOrder {
    Unset,
    Pending(i64),
    WaitingOnAsyncDependency(i64),
    Done,
}

/// Top-level await continuation owned by module evaluation state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TopLevelAwaitContinuation {
    pub module: ModuleRecordId,
    pub async_order: AsyncEvaluationOrder,
    pub pending_dependency_count: u32,
    pub promise_capability_slot: Option<crate::modules::registry::ModulePromiseSlot>,
}

/// Namespace object identity owned by the runtime heap.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ModuleNamespaceObjectId(u32);

impl ModuleNamespaceObjectId {
    pub const fn from_heap_slot(slot: u32) -> Self {
        Self(slot)
    }

    pub const fn heap_slot(self) -> u32 {
        self.0
    }
}

/// Module environment identity owned by runtime scope/environment storage.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ModuleEnvironmentId(u32);

impl ModuleEnvironmentId {
    pub const fn from_runtime_slot(slot: u32) -> Self {
        Self(slot)
    }

    pub const fn runtime_slot(self) -> u32 {
        self.0
    }
}

/// Module executable identity without owning bytecode.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ModuleProgramExecutableId(u32);

impl ModuleProgramExecutableId {
    pub const fn from_executable_slot(slot: u32) -> Self {
        Self(slot)
    }

    pub const fn executable_slot(self) -> u32 {
        self.0
    }
}

/// Import entry recorded by module analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportEntry {
    Single {
        module_request: Identifier,
        import_name: Identifier,
        local_name: Identifier,
    },
    Namespace {
        module_request: Identifier,
        local_name: Identifier,
    },
}

/// Export entry recorded by module analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportEntry {
    Local {
        export_name: Identifier,
        local_name: Identifier,
    },
    Indirect {
        export_name: Identifier,
        import_name: Identifier,
        module_request: Identifier,
    },
    Namespace {
        export_name: Identifier,
        module_request: Identifier,
    },
}

/// Result of resolving an import or export binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleBindingResolution {
    Resolved {
        module: ModuleRecordId,
        local_name: Identifier,
    },
    NotFound,
    Ambiguous,
    Error,
}

/// Heap-owned data associated with a module record.
///
/// The record points at environment, namespace, and executable identities rather
/// than Rust-owning them. That matches JSC's GC ownership: records are retained
/// by the loader registry, while related cells are traced through write barriers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleRecordRuntimeState {
    environment: Option<ModuleEnvironmentId>,
    namespace: Option<ModuleNamespaceObjectId>,
    deferred_namespace: Option<ModuleNamespaceObjectId>,
    executable: Option<ModuleProgramExecutableId>,
}

impl ModuleRecordRuntimeState {
    pub const fn empty() -> Self {
        Self {
            environment: None,
            namespace: None,
            deferred_namespace: None,
            executable: None,
        }
    }

    pub const fn new(
        environment: Option<ModuleEnvironmentId>,
        namespace: Option<ModuleNamespaceObjectId>,
        deferred_namespace: Option<ModuleNamespaceObjectId>,
        executable: Option<ModuleProgramExecutableId>,
    ) -> Self {
        Self {
            environment,
            namespace,
            deferred_namespace,
            executable,
        }
    }
}

/// Common module-record contract.
///
/// Implementations own import/export tables, requested modules, environments,
/// namespace object identity, and async/TLA state. Graph algorithms should use
/// iterative state in `ModuleGraphLoad` rather than recursive calls.
pub trait ModuleRecord {
    fn id(&self) -> ModuleRecordId;
    fn kind(&self) -> ModuleRecordKind;
    fn requested_modules(&self) -> &[ModuleRequest];
}

/// JavaScript source-text module record.
#[derive(Debug)]
pub struct SourceTextModuleRecord {
    id: ModuleRecordId,
    requested_modules: &'static [ModuleRequest],
    runtime_state: ModuleRecordRuntimeState,
}

impl SourceTextModuleRecord {
    pub const fn new_unlinked(id: ModuleRecordId) -> Self {
        Self {
            id,
            requested_modules: &[],
            runtime_state: ModuleRecordRuntimeState::empty(),
        }
    }

    pub const fn with_static_requests(
        id: ModuleRecordId,
        requested_modules: &'static [ModuleRequest],
        runtime_state: ModuleRecordRuntimeState,
    ) -> Self {
        Self {
            id,
            requested_modules,
            runtime_state,
        }
    }

    pub const fn runtime_state(&self) -> ModuleRecordRuntimeState {
        self.runtime_state
    }
}

impl ModuleRecord for SourceTextModuleRecord {
    fn id(&self) -> ModuleRecordId {
        self.id
    }

    fn kind(&self) -> ModuleRecordKind {
        ModuleRecordKind::SourceText
    }

    fn requested_modules(&self) -> &[ModuleRequest] {
        self.requested_modules
    }
}

/// Cyclic module status-machine record used for source modules and TLA.
#[derive(Debug)]
pub struct CyclicModuleRecord {
    id: ModuleRecordId,
    status: CyclicModuleStatus,
    async_order: AsyncEvaluationOrder,
    requested_modules: &'static [ModuleRequest],
    runtime_state: ModuleRecordRuntimeState,
}

impl CyclicModuleRecord {
    pub const fn new_unlinked(id: ModuleRecordId) -> Self {
        Self {
            id,
            status: CyclicModuleStatus::New,
            async_order: AsyncEvaluationOrder::Unset,
            requested_modules: &[],
            runtime_state: ModuleRecordRuntimeState::empty(),
        }
    }

    pub const fn with_static_requests(
        id: ModuleRecordId,
        status: CyclicModuleStatus,
        async_order: AsyncEvaluationOrder,
        requested_modules: &'static [ModuleRequest],
        runtime_state: ModuleRecordRuntimeState,
    ) -> Self {
        Self {
            id,
            status,
            async_order,
            requested_modules,
            runtime_state,
        }
    }

    pub const fn status(&self) -> CyclicModuleStatus {
        self.status
    }

    pub const fn async_order(&self) -> AsyncEvaluationOrder {
        self.async_order
    }

    pub const fn runtime_state(&self) -> ModuleRecordRuntimeState {
        self.runtime_state
    }
}

impl ModuleRecord for CyclicModuleRecord {
    fn id(&self) -> ModuleRecordId {
        self.id
    }

    fn kind(&self) -> ModuleRecordKind {
        ModuleRecordKind::Cyclic
    }

    fn requested_modules(&self) -> &[ModuleRequest] {
        self.requested_modules
    }
}

/// Host-created module record.
#[derive(Debug)]
pub struct SyntheticModuleRecord {
    id: ModuleRecordId,
    requested_modules: &'static [ModuleRequest],
    runtime_state: ModuleRecordRuntimeState,
}

impl SyntheticModuleRecord {
    pub const fn new_unlinked(id: ModuleRecordId) -> Self {
        Self {
            id,
            requested_modules: &[],
            runtime_state: ModuleRecordRuntimeState::empty(),
        }
    }

    pub const fn with_static_requests(
        id: ModuleRecordId,
        requested_modules: &'static [ModuleRequest],
        runtime_state: ModuleRecordRuntimeState,
    ) -> Self {
        Self {
            id,
            requested_modules,
            runtime_state,
        }
    }

    pub const fn runtime_state(&self) -> ModuleRecordRuntimeState {
        self.runtime_state
    }
}

impl ModuleRecord for SyntheticModuleRecord {
    fn id(&self) -> ModuleRecordId {
        self.id
    }

    fn kind(&self) -> ModuleRecordKind {
        ModuleRecordKind::Synthetic
    }

    fn requested_modules(&self) -> &[ModuleRequest] {
        self.requested_modules
    }
}
