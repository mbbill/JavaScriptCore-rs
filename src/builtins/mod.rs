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
    BuiltinGeneratedArtifact, BuiltinGenerationPhase, BuiltinId, BuiltinInitializationEdge,
    BuiltinInitializationStage, BuiltinInitializationStep, BuiltinTableRevision, BuiltinVisibility,
};
pub use intrinsics::{
    BuiltinIntrinsic, BuiltinIntrinsicDescriptor, IntrinsicArity, IntrinsicBindingPhase,
    IntrinsicHostOwner, IntrinsicSafety,
};
pub use names::{
    BuiltinNameLookupKind, BuiltinNameMapEntry, BuiltinNameSlot, BuiltinNameTableState,
    BuiltinNames, BuiltinPrivateName, BuiltinPrivateNameTable, ExternalBuiltinName,
    WellKnownBuiltinName,
};
pub use registry::{
    BuiltinConstructAbility, BuiltinConstructorKind, BuiltinDescriptor, BuiltinExecutableCache,
    BuiltinExecutableCacheEntry, BuiltinExecutableCacheState, BuiltinExecutableHandle,
    BuiltinExecutableMetadata, BuiltinGeneratedPipelineStep, BuiltinInlinePolicy, BuiltinRegistry,
    BuiltinRegistryState, BuiltinSource, BuiltinSourceKind, BuiltinSourceRange, HostIntrinsicHook,
};
