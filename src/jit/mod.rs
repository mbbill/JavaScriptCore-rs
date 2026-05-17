//! Deferred JIT integration contracts for the Rust JavaScriptCore skeleton.
//!
//! This module intentionally reserves the shape of JIT-visible execution state
//! without generating code, interpreting bytecode, patching entrypoints, or
//! defining a JavaScript execution path.

#![forbid(unsafe_code)]

pub(crate) mod abi;
pub(crate) mod code;
pub(crate) mod disassembly;
pub(crate) mod ic;
pub(crate) mod machine;
pub(crate) mod plan;
pub(crate) mod tiering;
pub(crate) mod watchpoint;

pub use abi::{
    AbiValue, CallBoundaryId, CallBoundaryMetadata, EntryAbi, Entrypoint, EntrypointKind,
    FrameSlot, FrameSlotRole, PatchpointDescriptor, PatchpointKind, RegisterBinding, RegisterRole,
};
pub use code::{
    CodeBlockJitSlots, CodeInstallBarrier, CodeInvalidationReason, CodeInvalidationState,
    CodeLiveness, CodeOrigin, CodeOriginKind, CodeOwnership, CodeReplacement, JitCodeArtifact,
    JitCodeId, JitCodeRef, JitType,
};
pub use disassembly::{
    DisassemblyAnnotation, DisassemblyFormat, DisassemblyInstruction, DisassemblyMetadata,
    DisassemblySection, DisassemblySource,
};
pub use ic::{
    AccessCaseDescriptor, AccessCaseKind, CacheKey, CallLinkInfoDescriptor, CallLinkMode,
    InlineCacheDispatch, InlineCacheInvalidation, InlineCacheInvalidationReason, InlineCacheKind,
    InlineCacheSlot, InlineCacheSlotId, InlineCacheState, InlineCacheStub, InlineCacheStubId,
    InlineCacheStubKind, LinkedCallKind,
};
pub use machine::{
    CodePatchPlan, CodePatchRecord, CodePatchState, ExecutableAllocationId,
    ExecutableMemoryProtection, MachineCodeHandle, MachineCodeOwnership, MachineCodeRange,
    PatchWriteBarrier, RelocationKind,
};
pub use plan::{
    CompilationCancellation, CompilationMode, CompilationOutcome, CompilationPriority,
    CompilationProduct, CompilationRequest, CompilationRequestKind,
};
pub use plan::{CompilationPlan, CompilationPlanId, CompilationPlanState, JitPlanHost};
pub use tiering::{
    BaselineTierPlan, OptimizingTierPlan, OsrState, TierCounters, TierPlanDescriptor, TierPlanKind,
    TierPlanPriorityHint, TierPlanProfile, TierThresholds, TierTransition, TieringPolicy,
    TieringSnapshot, TieringState, TieringTrigger,
};
pub use watchpoint::{
    DependencyStrength, WatchpointDependency, WatchpointDependencyId, WatchpointFireEvent,
    WatchpointFirePolicy, WatchpointOwner, WatchpointSetDescriptor, WatchpointSetId,
    WatchpointSetState, WatchpointTarget,
};
