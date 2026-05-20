use crate::gc::{CellId, HeapId, RootId, RootKind, RootRecord, RootSet, RootSetSemanticError};
use crate::modules::key::{ModuleKey, ModuleType};
use crate::modules::registry::{ModuleErrorSlot, ModulePromiseSlot};
use crate::modules::request::ModuleRequest;
use crate::strings::Identifier;
use std::collections::HashSet;

/// Canonical stable module-record identity within a realm's loader.
///
/// The module subsystem owns this identity. Other runtime areas should import
/// or re-export `modules::ModuleRecordId`; they must not define parallel
/// module-record handles.
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

/// ECMA module phase attached to imports, exports, and namespace creation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ModulePhase {
    #[default]
    Evaluation,
    Defer,
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
    pub promise_capability_slot: Option<ModulePromiseSlot>,
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
        phase: ModulePhase,
        module_request: Identifier,
        import_name: Identifier,
        local_name: Identifier,
    },
    Namespace {
        phase: ModulePhase,
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

/// Loaded module request edge retained by the referring module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedModuleRequest {
    pub request: ModuleRequest,
    pub loaded_module: ModuleRecordId,
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

/// Cached successful resolution for a requested export name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleResolutionCacheEntry {
    pub export_name: Identifier,
    pub resolution: ModuleBindingResolution,
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
    cycle_root: Option<ModuleRecordId>,
    top_level_capability: Option<ModulePromiseSlot>,
    async_capability: Option<ModulePromiseSlot>,
    evaluation_error: Option<ModuleErrorSlot>,
    dfs_ancestor_index: u32,
    initialized: bool,
    has_top_level_await: bool,
}

impl ModuleRecordRuntimeState {
    pub const fn empty() -> Self {
        Self {
            environment: None,
            namespace: None,
            deferred_namespace: None,
            executable: None,
            cycle_root: None,
            top_level_capability: None,
            async_capability: None,
            evaluation_error: None,
            dfs_ancestor_index: 0,
            initialized: false,
            has_top_level_await: false,
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
            cycle_root: None,
            top_level_capability: None,
            async_capability: None,
            evaluation_error: None,
            dfs_ancestor_index: 0,
            initialized: false,
            has_top_level_await: false,
        }
    }

    pub const fn environment(self) -> Option<ModuleEnvironmentId> {
        self.environment
    }

    pub const fn namespace(self) -> Option<ModuleNamespaceObjectId> {
        self.namespace
    }

    pub const fn deferred_namespace(self) -> Option<ModuleNamespaceObjectId> {
        self.deferred_namespace
    }

    pub const fn executable(self) -> Option<ModuleProgramExecutableId> {
        self.executable
    }

    pub const fn cycle_root(self) -> Option<ModuleRecordId> {
        self.cycle_root
    }

    pub const fn top_level_capability(self) -> Option<ModulePromiseSlot> {
        self.top_level_capability
    }

    pub const fn async_capability(self) -> Option<ModulePromiseSlot> {
        self.async_capability
    }

    pub const fn evaluation_error(self) -> Option<ModuleErrorSlot> {
        self.evaluation_error
    }

    pub const fn has_top_level_await(self) -> bool {
        self.has_top_level_await
    }

    pub const fn initialized(self) -> bool {
        self.initialized
    }

    pub const fn with_cycle_root(mut self, cycle_root: ModuleRecordId) -> Self {
        self.cycle_root = Some(cycle_root);
        self
    }

    pub const fn with_top_level_await(mut self, capability: ModulePromiseSlot) -> Self {
        self.top_level_capability = Some(capability);
        self.has_top_level_await = true;
        self
    }

    pub const fn with_evaluation_error(mut self, error: ModuleErrorSlot) -> Self {
        self.evaluation_error = Some(error);
        self
    }

    pub const fn mark_initialized(mut self) -> Self {
        self.initialized = true;
        self
    }
}

/// Module runtime edge retained as a GC root by a loader or realm registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleRuntimeRootKind {
    Environment,
    Namespace,
    DeferredNamespace,
    Executable,
    TopLevelCapability,
    AsyncCapability,
    EvaluationError,
}

/// Root descriptor for heap-owned module runtime state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleRuntimeRootRecord {
    pub module: ModuleRecordId,
    pub kind: ModuleRuntimeRootKind,
    pub root: RootRecord,
    pub target: CellId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleRuntimeRootPlan {
    pub heap: HeapId,
    pub module: ModuleRecordId,
    pub roots: Vec<ModuleRuntimeRootRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleRuntimeRootError {
    RootSet(RootSetSemanticError),
    RootIdOverflow,
}

impl From<RootSetSemanticError> for ModuleRuntimeRootError {
    fn from(error: RootSetSemanticError) -> Self {
        Self::RootSet(error)
    }
}

pub fn plan_module_runtime_roots(
    module: ModuleRecordId,
    state: ModuleRecordRuntimeState,
    heap: HeapId,
    first_root: RootId,
) -> Result<ModuleRuntimeRootPlan, ModuleRuntimeRootError> {
    let mut roots = Vec::new();
    push_module_root(
        &mut roots,
        module,
        ModuleRuntimeRootKind::Environment,
        state
            .environment()
            .map(|environment| CellId(environment.runtime_slot())),
        heap,
        first_root,
    )?;
    push_module_root(
        &mut roots,
        module,
        ModuleRuntimeRootKind::Namespace,
        state
            .namespace()
            .map(|namespace| CellId(namespace.heap_slot())),
        heap,
        first_root,
    )?;
    push_module_root(
        &mut roots,
        module,
        ModuleRuntimeRootKind::DeferredNamespace,
        state
            .deferred_namespace()
            .map(|namespace| CellId(namespace.heap_slot())),
        heap,
        first_root,
    )?;
    push_module_root(
        &mut roots,
        module,
        ModuleRuntimeRootKind::Executable,
        state
            .executable()
            .map(|executable| CellId(executable.executable_slot())),
        heap,
        first_root,
    )?;
    push_module_root(
        &mut roots,
        module,
        ModuleRuntimeRootKind::TopLevelCapability,
        state
            .top_level_capability()
            .map(|capability| CellId(capability.runtime_slot())),
        heap,
        first_root,
    )?;
    push_module_root(
        &mut roots,
        module,
        ModuleRuntimeRootKind::AsyncCapability,
        state
            .async_capability()
            .map(|capability| CellId(capability.runtime_slot())),
        heap,
        first_root,
    )?;
    push_module_root(
        &mut roots,
        module,
        ModuleRuntimeRootKind::EvaluationError,
        state
            .evaluation_error()
            .map(|error| CellId(error.runtime_slot())),
        heap,
        first_root,
    )?;

    RootSet::from_records(roots.iter().map(|record| record.root).collect())?;
    Ok(ModuleRuntimeRootPlan {
        heap,
        module,
        roots,
    })
}

fn push_module_root(
    roots: &mut Vec<ModuleRuntimeRootRecord>,
    module: ModuleRecordId,
    kind: ModuleRuntimeRootKind,
    target: Option<CellId>,
    heap: HeapId,
    first_root: RootId,
) -> Result<(), ModuleRuntimeRootError> {
    let Some(target) = target else {
        return Ok(());
    };
    let root_id = first_root
        .0
        .checked_add(roots.len() as u64)
        .ok_or(ModuleRuntimeRootError::RootIdOverflow)?;
    roots.push(ModuleRuntimeRootRecord {
        module,
        kind,
        root: RootRecord {
            id: RootId(root_id),
            kind: RootKind::ExplicitRoot,
            heap,
        },
        target,
    });
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleSemanticOperation {
    Link,
    Evaluate,
    AsyncEvaluationFulfilled,
    AsyncEvaluationRejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleSemanticTransition {
    pub module: ModuleRecordId,
    pub operation: ModuleSemanticOperation,
    pub from: CyclicModuleStatus,
    pub to: CyclicModuleStatus,
    pub async_order: AsyncEvaluationOrder,
    pub records_error: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleSemanticTransitionError {
    InvalidStatus,
    MissingError,
    MissingAsyncOrder,
}

pub fn plan_module_semantic_transition(
    module: ModuleRecordId,
    from: CyclicModuleStatus,
    operation: ModuleSemanticOperation,
    async_state: ModuleAsyncEvaluationState,
    error: Option<ModuleErrorSlot>,
) -> Result<ModuleSemanticTransition, ModuleSemanticTransitionError> {
    let (to, records_error, async_order) = match operation {
        ModuleSemanticOperation::Link => match from {
            CyclicModuleStatus::New | CyclicModuleStatus::Unlinked => {
                (CyclicModuleStatus::Linked, false, async_state.order)
            }
            _ => return Err(ModuleSemanticTransitionError::InvalidStatus),
        },
        ModuleSemanticOperation::Evaluate => match from {
            CyclicModuleStatus::Linked => {
                if async_state.has_top_level_await
                    || async_state.pending_dependencies.unwrap_or(0) > 0
                {
                    (
                        CyclicModuleStatus::EvaluatingAsync,
                        false,
                        require_pending_order(async_state.order)?,
                    )
                } else {
                    (
                        CyclicModuleStatus::Evaluated,
                        false,
                        AsyncEvaluationOrder::Done,
                    )
                }
            }
            CyclicModuleStatus::EvaluatingAsync | CyclicModuleStatus::Evaluated => {
                (from, false, async_state.order)
            }
            _ => return Err(ModuleSemanticTransitionError::InvalidStatus),
        },
        ModuleSemanticOperation::AsyncEvaluationFulfilled => match from {
            CyclicModuleStatus::EvaluatingAsync => (
                CyclicModuleStatus::Evaluated,
                false,
                AsyncEvaluationOrder::Done,
            ),
            _ => return Err(ModuleSemanticTransitionError::InvalidStatus),
        },
        ModuleSemanticOperation::AsyncEvaluationRejected => match from {
            CyclicModuleStatus::EvaluatingAsync | CyclicModuleStatus::Evaluating => {
                if error.is_none() {
                    return Err(ModuleSemanticTransitionError::MissingError);
                }
                (CyclicModuleStatus::Failed, true, AsyncEvaluationOrder::Done)
            }
            _ => return Err(ModuleSemanticTransitionError::InvalidStatus),
        },
    };

    Ok(ModuleSemanticTransition {
        module,
        operation,
        from,
        to,
        async_order,
        records_error,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleEvaluationEntryRecord {
    pub module: ModuleRecordId,
    pub status: CyclicModuleStatus,
    pub runtime_state: ModuleRecordRuntimeState,
    pub async_state: ModuleAsyncEvaluationState,
    pub operation: ModuleSemanticOperation,
}

impl ModuleEvaluationEntryRecord {
    pub fn evaluate(
        module: ModuleRecordId,
        status: CyclicModuleStatus,
        runtime_state: ModuleRecordRuntimeState,
        async_state: ModuleAsyncEvaluationState,
    ) -> Self {
        Self {
            module,
            status,
            runtime_state,
            async_state,
            operation: ModuleSemanticOperation::Evaluate,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleEvaluationResultRecord {
    pub module: ModuleRecordId,
    pub transition: ModuleSemanticTransition,
    pub completion: ModuleEvaluationCompletion,
    pub executable: Option<ModuleProgramExecutableId>,
    pub environment: Option<ModuleEnvironmentId>,
    pub promise: Option<ModulePromiseSlot>,
    pub error: Option<ModuleErrorSlot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleEvaluationCompletion {
    Synchronous,
    AsyncPending,
    AlreadyEvaluated,
    Failed,
}

pub fn plan_module_evaluation_entry(
    entry: ModuleEvaluationEntryRecord,
    error: Option<ModuleErrorSlot>,
) -> Result<ModuleEvaluationResultRecord, ModuleSemanticTransitionError> {
    let transition = plan_module_semantic_transition(
        entry.module,
        entry.status,
        entry.operation,
        entry.async_state,
        error,
    )?;
    let completion = match transition.to {
        CyclicModuleStatus::Evaluated if transition.from == CyclicModuleStatus::Evaluated => {
            ModuleEvaluationCompletion::AlreadyEvaluated
        }
        CyclicModuleStatus::Evaluated => ModuleEvaluationCompletion::Synchronous,
        CyclicModuleStatus::EvaluatingAsync => ModuleEvaluationCompletion::AsyncPending,
        CyclicModuleStatus::Failed => ModuleEvaluationCompletion::Failed,
        _ => ModuleEvaluationCompletion::AsyncPending,
    };
    let promise = entry
        .runtime_state
        .async_capability()
        .or(entry.runtime_state.top_level_capability());

    Ok(ModuleEvaluationResultRecord {
        module: entry.module,
        transition,
        completion,
        executable: entry.runtime_state.executable(),
        environment: entry.runtime_state.environment(),
        promise,
        error,
    })
}

fn require_pending_order(
    order: AsyncEvaluationOrder,
) -> Result<AsyncEvaluationOrder, ModuleSemanticTransitionError> {
    match order {
        AsyncEvaluationOrder::Pending(_) | AsyncEvaluationOrder::WaitingOnAsyncDependency(_) => {
            Ok(order)
        }
        _ => Err(ModuleSemanticTransitionError::MissingAsyncOrder),
    }
}

/// Async module evaluation bookkeeping owned by a cyclic module record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleAsyncEvaluationState {
    pub order: AsyncEvaluationOrder,
    pub pending_dependencies: Option<u32>,
    pub top_level_capability: Option<ModulePromiseSlot>,
    pub cycle_root: Option<ModuleRecordId>,
    pub has_top_level_await: bool,
}

/// Referrer family accepted by host module loading.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleReferrer {
    Script,
    Module(ModuleRecordId),
    Realm,
}

/// Failure metadata attached to duplicated module loader errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleFailure {
    pub source: Option<ModuleRecordId>,
    pub key: Option<ModuleKey>,
    pub module_type: ModuleType,
    pub kind: ModuleFailureKind,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ModuleFailureKind {
    #[default]
    Unknown,
    Instantiation,
    Evaluation,
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

/// Owner of immutable module-record schema metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleRecordSchemaOwner {
    ParserModuleAnalysis,
    RealmModuleLoader,
    HostSyntheticModule,
    GeneratedStaticData,
}

/// Provenance for module-record descriptors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleRecordSchemaProvenance {
    ParsedSourceText,
    HostSyntheticModule,
    GeneratedFromModuleAnalysis,
    GeneratedFromEngineMetadata,
}

/// Immutable module-record descriptor.
///
/// Import/export/request tables are static metadata. Environment creation,
/// namespace creation, linking, and evaluation belong to module algorithms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleRecordDescriptor {
    pub name: &'static str,
    pub id: ModuleRecordId,
    pub kind: ModuleRecordKind,
    pub owner: ModuleRecordSchemaOwner,
    pub provenance: ModuleRecordSchemaProvenance,
    pub runtime_state: ModuleRecordRuntimeState,
    imports: &'static [ImportEntry],
    exports: &'static [ExportEntry],
    requested_modules: &'static [ModuleRequest],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModuleRecordValidationError {
    EmptyName,
    DuplicateRequestedModule(Identifier),
    MissingRequestedModule(Identifier),
    DuplicateExportName(Identifier),
    SyntheticRecordHasSourceImports,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleRecordDescriptorBuilder {
    header: ModuleRecordDescriptorHeader,
    tables: ModuleRecordDescriptorTables,
}

impl ModuleRecordDescriptorBuilder {
    pub const fn new(header: ModuleRecordDescriptorHeader) -> Self {
        Self {
            header,
            tables: ModuleRecordDescriptorTables {
                imports: &[],
                exports: &[],
                requested_modules: &[],
            },
        }
    }

    pub const fn tables(mut self, tables: ModuleRecordDescriptorTables) -> Self {
        self.tables = tables;
        self
    }

    pub fn build(self) -> Result<ModuleRecordDescriptor, ModuleRecordValidationError> {
        let descriptor = ModuleRecordDescriptor::from_parts(self.header, self.tables);
        descriptor.validate()?;
        Ok(descriptor)
    }
}

/// Header metadata for a module-record descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleRecordDescriptorHeader {
    pub name: &'static str,
    pub id: ModuleRecordId,
    pub kind: ModuleRecordKind,
    pub owner: ModuleRecordSchemaOwner,
    pub provenance: ModuleRecordSchemaProvenance,
    pub runtime_state: ModuleRecordRuntimeState,
}

/// Static import/export/request tables borrowed by a module-record descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleRecordDescriptorTables {
    pub imports: &'static [ImportEntry],
    pub exports: &'static [ExportEntry],
    pub requested_modules: &'static [ModuleRequest],
}

impl ModuleRecordDescriptor {
    pub const fn from_parts(
        header: ModuleRecordDescriptorHeader,
        tables: ModuleRecordDescriptorTables,
    ) -> Self {
        Self {
            name: header.name,
            id: header.id,
            kind: header.kind,
            owner: header.owner,
            provenance: header.provenance,
            runtime_state: header.runtime_state,
            imports: tables.imports,
            exports: tables.exports,
            requested_modules: tables.requested_modules,
        }
    }

    /// Returns immutable import-entry metadata.
    pub const fn imports(&self) -> &'static [ImportEntry] {
        self.imports
    }

    /// Returns immutable export-entry metadata.
    pub const fn exports(&self) -> &'static [ExportEntry] {
        self.exports
    }

    /// Returns immutable requested-module metadata.
    pub const fn requested_modules(&self) -> &'static [ModuleRequest] {
        self.requested_modules
    }

    pub fn validate(&self) -> Result<(), ModuleRecordValidationError> {
        validate_module_record_descriptor(self)
    }
}

pub fn validate_module_record_descriptor(
    descriptor: &ModuleRecordDescriptor,
) -> Result<(), ModuleRecordValidationError> {
    if descriptor.name.is_empty() {
        return Err(ModuleRecordValidationError::EmptyName);
    }

    if descriptor.kind == ModuleRecordKind::Synthetic && !descriptor.imports.is_empty() {
        return Err(ModuleRecordValidationError::SyntheticRecordHasSourceImports);
    }

    let mut requested = HashSet::new();
    let mut request_names = HashSet::new();
    for request in descriptor.requested_modules {
        let name = request.specifier().identifier();
        let key = (
            name,
            request.module_type(),
            request.attributes().list_id(),
            request.attributes().validation(),
        );
        if !requested.insert(key) {
            return Err(ModuleRecordValidationError::DuplicateRequestedModule(name));
        }
        request_names.insert(name);
    }

    for import in descriptor.imports {
        let module_request = match import {
            ImportEntry::Single { module_request, .. }
            | ImportEntry::Namespace { module_request, .. } => *module_request,
        };
        if !request_names.contains(&module_request) {
            return Err(ModuleRecordValidationError::MissingRequestedModule(
                module_request,
            ));
        }
    }

    let mut export_names = HashSet::new();
    for export in descriptor.exports {
        let (export_name, module_request) = match export {
            ExportEntry::Local { export_name, .. } => (*export_name, None),
            ExportEntry::Indirect {
                export_name,
                module_request,
                ..
            }
            | ExportEntry::Namespace {
                export_name,
                module_request,
            } => (*export_name, Some(*module_request)),
        };

        if !export_names.insert(export_name) {
            return Err(ModuleRecordValidationError::DuplicateExportName(
                export_name,
            ));
        }

        if let Some(module_request) = module_request {
            if !request_names.contains(&module_request) {
                return Err(ModuleRecordValidationError::MissingRequestedModule(
                    module_request,
                ));
            }
        }
    }

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_evaluate_with_tla_enters_async_evaluating() {
        let state = ModuleAsyncEvaluationState {
            order: AsyncEvaluationOrder::Pending(1),
            pending_dependencies: Some(0),
            top_level_capability: None,
            cycle_root: None,
            has_top_level_await: true,
        };

        let transition = plan_module_semantic_transition(
            ModuleRecordId::from_loader_slot(1),
            CyclicModuleStatus::Linked,
            ModuleSemanticOperation::Evaluate,
            state,
            None,
        )
        .unwrap();

        assert_eq!(transition.to, CyclicModuleStatus::EvaluatingAsync);
        assert_eq!(transition.async_order, AsyncEvaluationOrder::Pending(1));
    }

    #[test]
    fn async_module_rejection_requires_error_slot() {
        let error = plan_module_semantic_transition(
            ModuleRecordId::from_loader_slot(1),
            CyclicModuleStatus::EvaluatingAsync,
            ModuleSemanticOperation::AsyncEvaluationRejected,
            ModuleAsyncEvaluationState {
                order: AsyncEvaluationOrder::Pending(1),
                pending_dependencies: Some(0),
                top_level_capability: None,
                cycle_root: None,
                has_top_level_await: true,
            },
            None,
        )
        .unwrap_err();

        assert_eq!(error, ModuleSemanticTransitionError::MissingError);
    }

    #[test]
    fn module_evaluation_entry_records_executable_without_dispatching() {
        let runtime_state = ModuleRecordRuntimeState::new(
            Some(ModuleEnvironmentId::from_runtime_slot(3)),
            None,
            None,
            Some(ModuleProgramExecutableId::from_executable_slot(9)),
        );
        let async_state = ModuleAsyncEvaluationState {
            order: AsyncEvaluationOrder::Done,
            pending_dependencies: Some(0),
            top_level_capability: None,
            cycle_root: None,
            has_top_level_await: false,
        };

        let result = plan_module_evaluation_entry(
            ModuleEvaluationEntryRecord::evaluate(
                ModuleRecordId::from_loader_slot(1),
                CyclicModuleStatus::Linked,
                runtime_state,
                async_state,
            ),
            None,
        )
        .unwrap();

        assert_eq!(result.completion, ModuleEvaluationCompletion::Synchronous);
        assert_eq!(
            result.executable,
            Some(ModuleProgramExecutableId::from_executable_slot(9))
        );
    }

    #[test]
    fn module_runtime_roots_cover_heap_owned_runtime_edges() {
        let runtime_state = ModuleRecordRuntimeState::new(
            Some(ModuleEnvironmentId::from_runtime_slot(3)),
            Some(ModuleNamespaceObjectId::from_heap_slot(4)),
            None,
            Some(ModuleProgramExecutableId::from_executable_slot(9)),
        )
        .with_top_level_await(ModulePromiseSlot::from_runtime_slot(11))
        .with_evaluation_error(ModuleErrorSlot::from_runtime_slot(12));

        let plan = plan_module_runtime_roots(
            ModuleRecordId::from_loader_slot(1),
            runtime_state,
            HeapId(7),
            RootId(20),
        )
        .unwrap();

        assert_eq!(plan.roots.len(), 5);
        assert_eq!(plan.roots[0].kind, ModuleRuntimeRootKind::Environment);
        assert_eq!(
            plan.roots[0].root,
            RootRecord {
                id: RootId(20),
                kind: RootKind::ExplicitRoot,
                heap: HeapId(7)
            }
        );
        assert_eq!(plan.roots[4].kind, ModuleRuntimeRootKind::EvaluationError);
    }
}
