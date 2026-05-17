//! Future concurrent compilation plan contracts.
//!
//! Plans may eventually hold weak or externally traced references to code
//! blocks, runtime objects, profiles, and watchpoints. This skeleton keeps those
//! references opaque so neighboring modules can define ownership first.

use crate::jit::{
    CodeInstallBarrier, CodeInvalidationReason, JitCodeArtifact, JitCodeId, JitType,
    TieringSnapshot, TieringTrigger, WatchpointDependency,
};
use crate::runtime::{CodeBlockId, ExecutableId};

/// Stable identity for a deferred compilation plan.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompilationPlanId(pub u64);

/// Lifecycle state for future JIT compilation jobs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilationPlanState {
    Queued,
    Preparing,
    Compiling,
    Finalizing,
    ReadyToInstall,
    Installed,
    Cancelled,
    Failed,
    Invalidated,
}

/// Result category from a future compilation attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilationOutcome {
    Deferred,
    Successful,
    InvalidatedBeforeInstall,
    Failed,
    Cancelled,
}

/// Compilation mode requested from the future worklist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilationMode {
    Baseline,
    Dfg,
    Ftl,
    OsrEntry,
    InlineCacheStub,
    WasmFunction,
    WasmBridge,
}

/// Why a compilation request exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilationRequestKind {
    TierUp(TieringTrigger),
    OsrEntry,
    RecompileAfterInvalidation,
    InlineCacheRegeneration,
    WasmCompilation,
    HostBridge,
}

/// Cancellation reason for queued or in-flight compilation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilationCancellation {
    OwnerDied,
    WatchpointInvalidated,
    SupersededByNewerPlan,
    RuntimeShuttingDown,
    PolicyDisabled,
    ExplicitRequest,
}

/// Request payload submitted to a future worklist.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilationRequest {
    pub id: CompilationPlanId,
    pub owner: Option<CodeBlockId>,
    pub executable: Option<ExecutableId>,
    pub mode: CompilationMode,
    pub requested_tier: JitType,
    pub kind: CompilationRequestKind,
    pub tiering_snapshot: Option<TieringSnapshot>,
    pub install_barriers: Vec<CodeInstallBarrier>,
    pub dependencies: Vec<WatchpointDependency>,
    pub priority: CompilationPriority,
}

/// Scheduling priority without selecting a concrete queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilationPriority {
    Interactive,
    HotLoop,
    Background,
    WasmStreaming,
    Maintenance,
}

/// Finalization artifact returned to the main thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilationProduct {
    pub plan: CompilationPlanId,
    pub outcome: CompilationOutcome,
    pub code: Option<JitCodeArtifact>,
    pub replacement_for: Option<JitCodeId>,
    pub invalidation: Option<CodeInvalidationReason>,
}

/// Host callbacks required by future JIT plan execution and installation.
pub trait JitPlanHost {
    fn can_compile_concurrently(&self) -> bool;
    fn trace_plan_edges(&mut self, plan: CompilationPlanId);
    fn invalidate_plan(&mut self, plan: CompilationPlanId);
    fn plan_owner_is_live(&self, owner: CodeBlockId) -> bool;
    fn install_compilation_product(&mut self, product: CompilationProduct) -> CompilationOutcome;
}

/// Abstract compilation job without implementation strategy.
pub trait CompilationPlan {
    fn id(&self) -> CompilationPlanId;
    fn request(&self) -> &CompilationRequest;
    fn requested_tier(&self) -> JitType;
    fn state(&self) -> CompilationPlanState;
    fn watchpoint_dependencies(&self) -> &[WatchpointDependency];
    fn product(&self) -> Option<&CompilationProduct>;
    fn cancel(&mut self, host: &mut dyn JitPlanHost);
}
