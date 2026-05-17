//! Tier-up, OSR, and profiling attachment state.
//!
//! The counters and policies here reserve fields for later JIT work. They do
//! not select tiers, inspect bytecode cost, or trigger compilation. Runtime code
//! owns counter mutation; compilation plans may only snapshot and validate this
//! data before installation.

use crate::jit::{CompilationPlanId, JitType};
use crate::runtime::CodeBlockId;

/// Reserved tiering policy selected by the runtime or embedder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TieringPolicy {
    Disabled,
    InterpreterOnly,
    BaselineAllowed,
    OptimizingAllowed,
    WasmTieringAllowed,
}

/// Reserved on-stack-replacement state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OsrState {
    Unavailable,
    Candidate,
    Ready,
    Entering,
    Exited,
    Failed,
}

/// Side-data for future tier-up and OSR decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TieringState {
    pub owner: Option<CodeBlockId>,
    pub current_tier: JitType,
    pub requested_tier: Option<JitType>,
    pub policy: TieringPolicy,
    pub osr: OsrState,
    pub execution_counter_slot: Option<u32>,
    pub active_request: Option<CompilationPlanId>,
    pub thresholds: TierThresholds,
    pub counters: TierCounters,
}

/// Tier-up threshold slots mirrored from future profiling metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TierThresholds {
    pub baseline_warmup: u32,
    pub dfg_entry: u32,
    pub ftl_entry: u32,
    pub osr_entry: u32,
}

/// Mutable tiering counters owned by the code block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TierCounters {
    pub execution_count: u64,
    pub loop_count: u64,
    pub slow_path_count: u64,
    pub osr_exit_count: u32,
    pub invalidation_count: u32,
}

/// Trigger that caused a tiering request to be created.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TieringTrigger {
    EntryCounter,
    LoopCounter,
    OsrEntry,
    WasmFunctionHot,
    ExplicitEmbedderRequest,
    RecompileAfterInvalidation,
}

/// Snapshot handed to a future compiler thread.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TieringSnapshot {
    pub owner: CodeBlockId,
    pub from_tier: JitType,
    pub to_tier: JitType,
    pub trigger: TieringTrigger,
    pub counters: TierCounters,
    pub osr_entry_bytecode_index: Option<u32>,
    pub epoch: u64,
}

/// Reserved transition edge between tiers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TierTransition {
    pub owner: CodeBlockId,
    pub from_tier: JitType,
    pub to_tier: JitType,
    pub plan: CompilationPlanId,
    pub trigger: TieringTrigger,
}

/// Tier-plan family selected before a compilation request is enqueued.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TierPlanKind {
    BaselineFromBytecode,
    DfgFromBaseline,
    FtlFromDfg,
    OsrEntry,
    RecompileAfterInvalidation,
}

/// Scheduling hint derived from hotness and user-visible execution state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TierPlanPriorityHint {
    EntryWarmup,
    HotLoop,
    FrequentExit,
    Interactive,
    Background,
}

/// Snapshot of profiling data used to justify a tier plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TierPlanProfile {
    pub counters: TierCounters,
    pub thresholds: TierThresholds,
    pub osr_entry_bytecode_index: Option<u32>,
    pub bytecode_size: Option<u32>,
    pub inline_cache_count: u32,
    pub exit_count: u32,
}

/// Baseline-specific tier plan. Baseline remains the first generated-code tier
/// and owns bytecode metadata attachment, not optimization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BaselineTierPlan {
    pub owner: CodeBlockId,
    pub plan: CompilationPlanId,
    pub profile: TierPlanProfile,
    pub install_entrypoint: bool,
}

/// Optimizing-tier plan for DFG or FTL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OptimizingTierPlan {
    pub owner: CodeBlockId,
    pub plan: CompilationPlanId,
    pub from_tier: JitType,
    pub to_tier: JitType,
    pub profile: TierPlanProfile,
    pub requires_osr_entry: bool,
    pub replaces_existing_code: bool,
}

/// Data-only tier plan selected by policy before compilation starts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TierPlanDescriptor {
    pub kind: TierPlanKind,
    pub owner: CodeBlockId,
    pub plan: CompilationPlanId,
    pub trigger: TieringTrigger,
    pub priority: TierPlanPriorityHint,
    pub from_tier: JitType,
    pub to_tier: JitType,
    pub profile: TierPlanProfile,
}
