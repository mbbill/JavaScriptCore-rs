//! Code-block side-data reserved for future JIT tiers.
//!
//! The owning code-block-equivalent module should store this as optional side
//! data. Interpreter semantics must not require any field here to be populated.
//! This module describes ownership, liveness, and invalidation boundaries only;
//! executable allocation, patching, and deallocation remain deferred.

use crate::jit::{
    DisassemblyMetadata, Entrypoint, InlineCacheSlot, MachineCodeHandle, PatchpointDescriptor,
    TieringState, WatchpointDependency, WatchpointSetId,
};
use crate::runtime::{CodeBlockId, ExecutableId, NativeCodeId};

/// Execution tier represented by a code-block-equivalent object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JitType {
    None,
    InterpreterThunk,
    Baseline,
    Dfg,
    Ftl,
    WasmIpInt,
    WasmBbq,
    WasmOmg,
}

/// Stable identity for future compiled code storage.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JitCodeId(pub u64);

/// Opaque reference to future compiled code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JitCodeRef {
    pub id: JitCodeId,
    pub tier: JitType,
}

/// GC and invalidation status for generated-code side data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeLiveness {
    Unallocated,
    Compiling,
    Live,
    PendingInvalidation,
    PendingJettison,
    Invalidated,
    Finalized,
}

/// JIT-visible side-data slots reserved on linked code state.
#[derive(Clone, Debug)]
pub struct CodeBlockJitSlots {
    pub owner: Option<CodeBlockId>,
    pub tier: JitType,
    pub entrypoint: Entrypoint,
    pub code: Option<JitCodeRef>,
    pub tiering: TieringState,
    pub liveness: CodeLiveness,
    pub inline_caches: Vec<InlineCacheSlot>,
    pub watchpoints: Vec<WatchpointDependency>,
    pub invalidation: CodeInvalidationState,
}

/// Provenance for a future compiled artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeOriginKind {
    BaselineCodeBlock,
    DfgReplacement,
    FtlReplacement,
    OsrEntry,
    InlineCacheStub,
    HostThunk,
    WasmFunction,
    WasmBridge,
}

/// Origin metadata used for ownership and diagnostic reporting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeOrigin {
    pub kind: CodeOriginKind,
    pub owner: Option<CodeBlockId>,
    pub executable: Option<ExecutableId>,
    pub bytecode_index: Option<u32>,
}

/// Ownership mode for generated code storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeOwnership {
    CodeBlockOwned,
    SharedStubSet,
    WasmCalleeGroup,
    HostRegistry,
    External,
}

/// Reserved compiled-code artifact descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JitCodeArtifact {
    pub id: JitCodeId,
    pub tier: JitType,
    pub origin: CodeOrigin,
    pub ownership: CodeOwnership,
    pub native_code: Option<NativeCodeId>,
    pub machine_code: Option<MachineCodeHandle>,
    pub entrypoint: Entrypoint,
    pub patchpoints: Vec<PatchpointDescriptor>,
    pub dependencies: Vec<WatchpointDependency>,
    pub disassembly: Option<DisassemblyMetadata>,
    pub liveness: CodeLiveness,
}

/// Boundary that must be crossed before code can be installed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeInstallBarrier {
    OwnerStillLive,
    WatchpointsStillValid,
    StructureEpochUnchanged,
    ExecutableStillMatches,
    WasmInstanceStillLive,
    MainThreadFinalization,
}

/// Invalidation state carried by linked code and stubs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodeInvalidationState {
    pub epoch: u64,
    pub reason: Option<CodeInvalidationReason>,
    pub watchpoint_sets: Vec<WatchpointSetId>,
    pub barriers: Vec<CodeInstallBarrier>,
}

/// Reason code is no longer installable or executable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeInvalidationReason {
    WatchpointFired,
    OwnerCodeBlockJettisoned,
    OwnerExecutableReplaced,
    WeakReferenceCleared,
    TierReplacementInstalled,
    CompilationCancelled,
    WasmMemoryModeChanged,
    WasmCalleeReplaced,
}

/// Code replacement edge between tiers or OSR entry artifacts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodeReplacement {
    pub old_code: Option<JitCodeId>,
    pub new_code: JitCodeId,
    pub owner: CodeBlockId,
    pub install_epoch: u64,
}
