//! Module loading and module-record contracts.
//!
//! Modules are a host-integrated state machine. This module names the registry,
//! module keys, records, graph loading, and host hooks without implementing
//! fetch, link, evaluate, promises, or JavaScript execution.
//!
//! `ModuleRecordId` is the canonical module-record identity. Sibling modules
//! import it through this module root so downstream runtime code sees one owner
//! for module-record handles.

mod graph;
mod host;
mod key;
mod loader;
mod record;
mod registry;
mod request;

pub use graph::{
    resolve_module_graph_descriptor, validate_module_graph_descriptor, GraphLoadErrorKind,
    GraphLoadJoinState, GraphLoadPayloadId, GraphLoadPayloadKind, GraphLoadPhase,
    GraphVisitedOwner, ModuleGraphDescriptor, ModuleGraphDescriptorBuilder,
    ModuleGraphDescriptorOwner, ModuleGraphDescriptorProvenance, ModuleGraphEdgeDescriptor,
    ModuleGraphLoad, ModuleGraphNodeDescriptor, ModuleGraphResolutionError,
    ModuleGraphValidationError, ModuleLoadCompletion, ResolvedModuleGraphEdge, VisitedModule,
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
    plan_module_evaluation_entry, plan_module_runtime_roots, plan_module_semantic_transition,
    validate_module_record_descriptor, AsyncEvaluationOrder, CyclicModuleRecord,
    CyclicModuleStatus, ExportEntry, ImportEntry, LoadedModuleRequest, ModuleAsyncEvaluationState,
    ModuleBindingResolution, ModuleEnvironmentId, ModuleEvaluationCompletion,
    ModuleEvaluationEntryRecord, ModuleEvaluationResultRecord, ModuleFailure, ModuleFailureKind,
    ModuleNamespaceObjectId, ModulePhase, ModuleProgramExecutableId, ModuleRecord,
    ModuleRecordDescriptor, ModuleRecordDescriptorBuilder, ModuleRecordDescriptorHeader,
    ModuleRecordDescriptorTables, ModuleRecordId, ModuleRecordKind, ModuleRecordRuntimeState,
    ModuleRecordSchemaOwner, ModuleRecordSchemaProvenance, ModuleRecordValidationError,
    ModuleReferrer, ModuleResolutionCacheEntry, ModuleRuntimeRootError, ModuleRuntimeRootKind,
    ModuleRuntimeRootPlan, ModuleRuntimeRootRecord, ModuleSemanticOperation,
    ModuleSemanticTransition, ModuleSemanticTransitionError, SourceTextModuleRecord,
    SyntheticModuleRecord, TopLevelAwaitContinuation,
};
pub use registry::{
    module_registry_transition_requirement, plan_module_registry_transition,
    validate_module_registry_descriptor, validate_module_registry_entry,
    validate_module_status_transition, ModuleErrorSlot, ModulePromiseSlot, ModuleRegistry,
    ModuleRegistryDescriptor, ModuleRegistryDescriptorBuilder, ModuleRegistryEntry,
    ModuleRegistryError, ModuleRegistryErrorSlots, ModuleRegistryOwner, ModuleRegistryProvenance,
    ModuleRegistryTransitionPlan, ModuleRegistryTransitionRequirement,
    ModuleRegistryValidationError, ModuleResolutionFailure, ModuleStatus, ModuleStatusTransition,
    ScriptFetcherSlot,
};
pub use request::{
    resolve_module_request_descriptor, ModuleRequest, ModuleRequestFailure,
    ModuleRequestFailureKind, ModuleRequestKind, ModuleRequestPhase, ModuleRequestResolution,
};
