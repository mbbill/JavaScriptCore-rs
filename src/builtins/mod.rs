//! Builtin-code contracts.
//!
//! Builtins are privileged engine code. This module names the registry, builtin
//! names, private names, intrinsic surface, and lazy executable cache without
//! parsing, compiling, or executing builtin JavaScript.

mod ids;
mod intrinsics;
mod names;
mod registry;

pub use ids::{
    BuiltinCodeIndex, BuiltinGeneratedArtifact, BuiltinGenerationPhase, BuiltinId,
    BuiltinInitializationEdge, BuiltinInitializationStage, BuiltinInitializationStep,
    BuiltinTableRevision, BuiltinVisibility,
};
pub use intrinsics::{
    BuiltinIntrinsic, BuiltinIntrinsicDescriptor, IntrinsicArity, IntrinsicBindingPhase,
    IntrinsicHostOwner, IntrinsicRegistryMutationAuthority, IntrinsicRegistryOwner,
    IntrinsicSafety, IntrinsicValidationFinding, IntrinsicValidationReport,
    StaticIntrinsicRegistry,
};
pub use names::{
    BuiltinNameLookupKind, BuiltinNameMapEntry, BuiltinNameMutationAuthority,
    BuiltinNameRegistryOwner, BuiltinNameSlot, BuiltinNameTableState, BuiltinNameValidationFinding,
    BuiltinNameValidationReport, BuiltinNames, BuiltinPrivateName, BuiltinPrivateNameTable,
    ExternalBuiltinName, StaticBuiltinNameRegistry, WellKnownBuiltinName,
};
pub use registry::{
    describe_builtin_tier_execution_metadata, summarize_builtin_registry_integration,
    BuiltinConstructAbility, BuiltinConstructorKind, BuiltinDescriptor, BuiltinExceptionContract,
    BuiltinExecutableCache, BuiltinExecutableCacheEntry, BuiltinExecutableCacheState,
    BuiltinExecutableCreationAuthority, BuiltinExecutableHandle, BuiltinExecutableMetadata,
    BuiltinExecutionMetadataDescriptor, BuiltinExecutionTierKind, BuiltinGeneratedPipelineStep,
    BuiltinInlinePolicy, BuiltinIntegrationDiagnostic, BuiltinIntegrationDiagnosticKind,
    BuiltinParserMode, BuiltinRegistry, BuiltinRegistryIntegrationSummary,
    BuiltinRegistryMutationAuthority, BuiltinRegistryOwner, BuiltinRegistryProvenance,
    BuiltinRegistryState, BuiltinRegistryValidationFinding, BuiltinRegistryValidationReport,
    BuiltinSemanticEffects, BuiltinSemanticMetadata, BuiltinSemanticParseSource, BuiltinSource,
    BuiltinSourceDescriptor, BuiltinSourceKind, BuiltinSourceRange, BuiltinTierExecutionMetadata,
    HostIntrinsicHook, StaticBuiltinRegistry,
};
