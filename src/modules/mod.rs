//! Module loading and module-record contracts.
//!
//! Modules are a host-integrated state machine. This module names the registry,
//! module keys, records, graph loading, and host hooks without implementing
//! fetch, link, evaluate, promises, or JavaScript execution.

mod graph;
mod host;
mod key;
mod loader;
mod record;
mod registry;
mod request;

pub use graph::{
    GraphLoadErrorKind, GraphLoadPayloadId, GraphLoadPayloadKind, GraphLoadPhase, ModuleGraphLoad,
    ModuleLoadCompletion, VisitedModule,
};
pub use host::{
    HostEvaluateRequest, HostHookResult, HostLoadCompletion, HostModuleError, HostModuleLoader,
    HostModulePayload, HostModuleReferrer, ImportMetaObjectId,
};
pub use key::{
    ImportAttributeListId, ImportAttributePair, ImportAttributeValidation, ImportAttributes,
    ImportMapId, ImportMapMergePolicy, ImportMapResolution, ModuleKey, ModuleMapSlot,
    ModuleSpecifierResolution, ModuleType, ResolvedSpecifier, ResolvedSpecifierKind,
};
pub use loader::{
    DynamicImportPayload, ModuleLoader, ModuleLoaderOperation, ModuleLoaderPolicy,
    ModuleLoaderState, ResolutionFailureCacheEntry, TopLevelAwaitDependency, TopLevelAwaitState,
};
pub use record::{
    AsyncEvaluationOrder, CyclicModuleRecord, CyclicModuleStatus, ExportEntry, ImportEntry,
    ModuleBindingResolution, ModuleEnvironmentId, ModuleNamespaceObjectId,
    ModuleProgramExecutableId, ModuleRecord, ModuleRecordId, ModuleRecordKind,
    ModuleRecordRuntimeState, SourceTextModuleRecord, SyntheticModuleRecord,
    TopLevelAwaitContinuation,
};
pub use registry::{
    ModuleErrorSlot, ModulePromiseSlot, ModuleRegistry, ModuleRegistryEntry, ModuleRegistryError,
    ModuleResolutionFailure, ModuleStatus, ModuleStatusTransition,
};
pub use request::{
    ModuleRequest, ModuleRequestFailure, ModuleRequestFailureKind, ModuleRequestKind,
    ModuleRequestPhase, ModuleRequestResolution,
};
