//! Tier-up, OSR, and profiling attachment state.
//!
//! The counters and policies here reserve fields for later JIT work. They do
//! not select tiers, inspect bytecode cost, or trigger compilation. Runtime code
//! owns counter mutation; compilation plans may only snapshot and validate this
//! data before installation.

use crate::bytecode::BytecodeIndex;
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
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TierFallbackReason {
    PolicyDisabled,
    ActiveRequest,
    UnsupportedTier,
    NativeEntryDisabled,
    OsrNotReady,
    FrequentExit,
    WatchpointInvalidated,
    CompilationCancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TierFallbackTarget {
    StayInCurrentTier,
    ReturnToBaseline,
    ReturnToInterpreter,
    RecompileSameTier,
    CancelPlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TierFallbackSemantics {
    pub owner: CodeBlockId,
    pub from_tier: JitType,
    pub attempted_tier: JitType,
    pub reason: TierFallbackReason,
    pub target: TierFallbackTarget,
    pub preserves_profile: bool,
    pub should_count_invalidation: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TierFallbackResumeKind {
    ContinueInInterpreter,
    ContinueInCurrentTier,
    ReturnToBaseline,
    RecompileBeforeResume,
    CancelWithoutResume,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TierFallbackResultRecord {
    pub owner: CodeBlockId,
    pub from_tier: JitType,
    pub attempted_tier: JitType,
    pub reason: TierFallbackReason,
    pub target: TierFallbackTarget,
    pub bytecode_index: Option<BytecodeIndex>,
    pub resume: TierFallbackResumeKind,
    pub preserves_profile: bool,
    pub should_count_invalidation: bool,
    pub clears_active_request: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TieringValidationError {
    EmptyName,
    EmptyProvenance(&'static str),
    EmptyTriggers(&'static str),
    DuplicateDescriptorKind(TierPlanKind),
    InvalidTierTransition(TierPlanKind),
    TriggerNotAllowed(TierPlanKind),
    ThresholdOrder(TierPlanKind),
    DescriptorPlanMismatch,
    OsrEntryMissingBytecodeIndex,
    FallbackResumeMismatch,
    FallbackMissingBytecodeIndex,
}

impl TierPlanDescriptor {
    pub fn from_static_descriptor(
        descriptor: &StaticTierDescriptor,
        owner: CodeBlockId,
        plan: CompilationPlanId,
        trigger: TieringTrigger,
        profile: TierPlanProfile,
    ) -> Result<Self, TieringValidationError> {
        let plan_descriptor = Self {
            kind: descriptor.kind,
            owner,
            plan,
            trigger,
            priority: descriptor.priority,
            from_tier: descriptor.from_tier,
            to_tier: descriptor.to_tier,
            profile,
        };
        plan_descriptor.validate_against(descriptor)?;
        Ok(plan_descriptor)
    }

    pub fn validate_against(
        &self,
        descriptor: &StaticTierDescriptor,
    ) -> Result<(), TieringValidationError> {
        if self.kind != descriptor.kind
            || self.from_tier != descriptor.from_tier
            || self.to_tier != descriptor.to_tier
            || self.priority != descriptor.priority
        {
            return Err(TieringValidationError::DescriptorPlanMismatch);
        }

        if !descriptor.triggers.contains(&self.trigger) {
            return Err(TieringValidationError::TriggerNotAllowed(self.kind));
        }

        if self.kind == TierPlanKind::OsrEntry && self.profile.osr_entry_bytecode_index.is_none() {
            return Err(TieringValidationError::OsrEntryMissingBytecodeIndex);
        }

        Ok(())
    }

    pub fn fallback_semantics(&self, reason: TierFallbackReason) -> TierFallbackSemantics {
        TierFallbackSemantics {
            owner: self.owner,
            from_tier: self.from_tier,
            attempted_tier: self.to_tier,
            reason,
            target: fallback_target(self.kind, self.from_tier, reason),
            preserves_profile: !matches!(
                reason,
                TierFallbackReason::PolicyDisabled | TierFallbackReason::CompilationCancelled
            ),
            should_count_invalidation: matches!(
                reason,
                TierFallbackReason::WatchpointInvalidated | TierFallbackReason::FrequentExit
            ),
        }
    }
}

impl TierFallbackResultRecord {
    pub fn from_semantics(
        semantics: TierFallbackSemantics,
        bytecode_index: Option<BytecodeIndex>,
    ) -> Result<Self, TieringValidationError> {
        let record = Self {
            owner: semantics.owner,
            from_tier: semantics.from_tier,
            attempted_tier: semantics.attempted_tier,
            reason: semantics.reason,
            target: semantics.target,
            bytecode_index,
            resume: resume_kind_for_fallback_target(semantics.target),
            preserves_profile: semantics.preserves_profile,
            should_count_invalidation: semantics.should_count_invalidation,
            clears_active_request: matches!(
                semantics.target,
                TierFallbackTarget::CancelPlan
                    | TierFallbackTarget::ReturnToInterpreter
                    | TierFallbackTarget::ReturnToBaseline
            ),
        };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), TieringValidationError> {
        if self.resume != resume_kind_for_fallback_target(self.target) {
            return Err(TieringValidationError::FallbackResumeMismatch);
        }
        if matches!(self.resume, TierFallbackResumeKind::ContinueInInterpreter)
            && self.bytecode_index.is_none()
        {
            return Err(TieringValidationError::FallbackMissingBytecodeIndex);
        }
        Ok(())
    }
}

const fn resume_kind_for_fallback_target(target: TierFallbackTarget) -> TierFallbackResumeKind {
    match target {
        TierFallbackTarget::StayInCurrentTier => TierFallbackResumeKind::ContinueInCurrentTier,
        TierFallbackTarget::ReturnToBaseline => TierFallbackResumeKind::ReturnToBaseline,
        TierFallbackTarget::ReturnToInterpreter => TierFallbackResumeKind::ContinueInInterpreter,
        TierFallbackTarget::RecompileSameTier => TierFallbackResumeKind::RecompileBeforeResume,
        TierFallbackTarget::CancelPlan => TierFallbackResumeKind::CancelWithoutResume,
    }
}

fn fallback_target(
    kind: TierPlanKind,
    from_tier: JitType,
    reason: TierFallbackReason,
) -> TierFallbackTarget {
    match reason {
        TierFallbackReason::CompilationCancelled => TierFallbackTarget::CancelPlan,
        TierFallbackReason::WatchpointInvalidated
        | TierFallbackReason::FrequentExit
        | TierFallbackReason::UnsupportedTier
        | TierFallbackReason::NativeEntryDisabled => match from_tier {
            JitType::Ftl => TierFallbackTarget::ReturnToBaseline,
            JitType::Dfg if kind == TierPlanKind::RecompileAfterInvalidation => {
                TierFallbackTarget::RecompileSameTier
            }
            JitType::Dfg => TierFallbackTarget::ReturnToBaseline,
            JitType::Baseline => TierFallbackTarget::ReturnToInterpreter,
            _ => TierFallbackTarget::StayInCurrentTier,
        },
        TierFallbackReason::OsrNotReady
        | TierFallbackReason::ActiveRequest
        | TierFallbackReason::PolicyDisabled => TierFallbackTarget::StayInCurrentTier,
    }
}

/// Selects the next data-only tier plan from a tiering snapshot.
///
/// This does not enqueue compilation, mutate counters, or install code. It
/// mirrors the policy gates used by JSC tiering: explicit requests win, active
/// requests suppress duplicate planning, and hotness counters only select the
/// next legal descriptor in the static table.
pub fn select_tier_plan(
    state: &TieringState,
    owner: CodeBlockId,
    plan: CompilationPlanId,
    bytecode_size: Option<u32>,
    inline_cache_count: u32,
) -> Result<Option<TierPlanDescriptor>, TieringValidationError> {
    if state.active_request.is_some() || !tier_policy_allows_planning(state.policy) {
        return Ok(None);
    }

    let selection = requested_tier_selection(state)
        .or_else(|| invalidation_selection(state))
        .or_else(|| hotness_selection(state));

    let Some((kind, trigger)) = selection else {
        return Ok(None);
    };

    if !tier_policy_allows_descriptor(state.policy, kind) {
        return Ok(None);
    }

    let descriptor = TIER_DESCRIPTOR_TABLE
        .descriptor_for_kind(kind)
        .ok_or(TieringValidationError::InvalidTierTransition(kind))?;
    let profile = TierPlanProfile {
        counters: state.counters,
        thresholds: state.thresholds,
        osr_entry_bytecode_index: None,
        bytecode_size,
        inline_cache_count,
        exit_count: state.counters.osr_exit_count,
    };

    TierPlanDescriptor::from_static_descriptor(descriptor, owner, plan, trigger, profile).map(Some)
}

const fn tier_policy_allows_planning(policy: TieringPolicy) -> bool {
    matches!(
        policy,
        TieringPolicy::BaselineAllowed | TieringPolicy::OptimizingAllowed
    )
}

const fn tier_policy_allows_descriptor(policy: TieringPolicy, kind: TierPlanKind) -> bool {
    matches!(
        (policy, kind),
        (
            TieringPolicy::BaselineAllowed,
            TierPlanKind::BaselineFromBytecode
        ) | (
            TieringPolicy::OptimizingAllowed,
            TierPlanKind::BaselineFromBytecode
        ) | (
            TieringPolicy::OptimizingAllowed,
            TierPlanKind::DfgFromBaseline
        ) | (TieringPolicy::OptimizingAllowed, TierPlanKind::FtlFromDfg)
            | (
                TieringPolicy::OptimizingAllowed,
                TierPlanKind::RecompileAfterInvalidation
            )
    )
}

const fn requested_tier_selection(state: &TieringState) -> Option<(TierPlanKind, TieringTrigger)> {
    match (state.current_tier, state.requested_tier) {
        (JitType::None, Some(JitType::Baseline | JitType::Dfg | JitType::Ftl)) => Some((
            TierPlanKind::BaselineFromBytecode,
            TieringTrigger::EntryCounter,
        )),
        (JitType::Baseline, Some(JitType::Dfg | JitType::Ftl)) => {
            Some((TierPlanKind::DfgFromBaseline, TieringTrigger::LoopCounter))
        }
        (JitType::Dfg, Some(JitType::Ftl)) => {
            Some((TierPlanKind::FtlFromDfg, TieringTrigger::LoopCounter))
        }
        _ => None,
    }
}

const fn invalidation_selection(state: &TieringState) -> Option<(TierPlanKind, TieringTrigger)> {
    if state.counters.invalidation_count == 0 {
        return None;
    }
    match state.current_tier {
        JitType::Dfg => Some((
            TierPlanKind::RecompileAfterInvalidation,
            TieringTrigger::RecompileAfterInvalidation,
        )),
        _ => None,
    }
}

const fn hotness_selection(state: &TieringState) -> Option<(TierPlanKind, TieringTrigger)> {
    match state.current_tier {
        JitType::None
            if state.counters.execution_count >= state.thresholds.baseline_warmup as u64 =>
        {
            Some((
                TierPlanKind::BaselineFromBytecode,
                TieringTrigger::EntryCounter,
            ))
        }
        JitType::Baseline
            if state.thresholds.osr_entry != 0
                && state.counters.loop_count >= state.thresholds.osr_entry as u64
                && matches!(state.osr, OsrState::Candidate | OsrState::Ready) =>
        {
            Some((TierPlanKind::DfgFromBaseline, TieringTrigger::OsrEntry))
        }
        JitType::Baseline
            if state.thresholds.dfg_entry != 0
                && state.counters.loop_count >= state.thresholds.dfg_entry as u64 =>
        {
            Some((TierPlanKind::DfgFromBaseline, TieringTrigger::LoopCounter))
        }
        JitType::Dfg
            if state.thresholds.ftl_entry != 0
                && state.counters.loop_count >= state.thresholds.ftl_entry as u64 =>
        {
            Some((TierPlanKind::FtlFromDfg, TieringTrigger::LoopCounter))
        }
        _ => None,
    }
}

/// Static owner for immutable tier descriptor tables.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum TierDescriptorOwner {
    #[default]
    RuntimeTieringPolicy,
    BaselineJit,
    DfgJit,
    FtlJit,
}

/// Authority allowed to replace a published tier descriptor table.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum TierDescriptorMutationAuthority {
    #[default]
    GeneratedStaticDataRefresh,
    RuntimePolicyInitialization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticTierDescriptor {
    pub name: &'static str,
    pub kind: TierPlanKind,
    pub from_tier: JitType,
    pub to_tier: JitType,
    pub policy: TieringPolicy,
    pub triggers: &'static [TieringTrigger],
    pub priority: TierPlanPriorityHint,
    pub default_thresholds: TierThresholds,
    pub owner: TierDescriptorOwner,
    pub mutation_authority: TierDescriptorMutationAuthority,
    pub provenance: &'static str,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TierDescriptorTable {
    pub descriptors: &'static [StaticTierDescriptor],
}

impl TierDescriptorTable {
    pub const fn new(descriptors: &'static [StaticTierDescriptor]) -> Self {
        Self { descriptors }
    }

    pub const fn descriptors(self) -> &'static [StaticTierDescriptor] {
        self.descriptors
    }

    pub fn descriptor_for_kind(self, kind: TierPlanKind) -> Option<&'static StaticTierDescriptor> {
        self.descriptors
            .iter()
            .find(|descriptor| descriptor.kind == kind)
    }

    pub fn validate(self) -> Result<(), TieringValidationError> {
        for (index, descriptor) in self.descriptors.iter().enumerate() {
            descriptor.validate()?;

            if self.descriptors[index + 1..]
                .iter()
                .any(|other| other.kind == descriptor.kind)
            {
                return Err(TieringValidationError::DuplicateDescriptorKind(
                    descriptor.kind,
                ));
            }
        }

        Ok(())
    }
}

impl StaticTierDescriptor {
    pub fn validate(&self) -> Result<(), TieringValidationError> {
        if self.name.is_empty() {
            return Err(TieringValidationError::EmptyName);
        }
        if self.provenance.is_empty() {
            return Err(TieringValidationError::EmptyProvenance(self.name));
        }
        if self.triggers.is_empty() {
            return Err(TieringValidationError::EmptyTriggers(self.name));
        }
        if !valid_tier_transition(self.kind, self.from_tier, self.to_tier) {
            return Err(TieringValidationError::InvalidTierTransition(self.kind));
        }
        if self.default_thresholds.baseline_warmup == 0
            || (self.default_thresholds.ftl_entry != 0
                && self.default_thresholds.ftl_entry < self.default_thresholds.dfg_entry)
        {
            return Err(TieringValidationError::ThresholdOrder(self.kind));
        }

        Ok(())
    }
}

const fn valid_tier_transition(kind: TierPlanKind, from_tier: JitType, to_tier: JitType) -> bool {
    matches!(
        (kind, from_tier, to_tier),
        (
            TierPlanKind::BaselineFromBytecode,
            JitType::None,
            JitType::Baseline
        ) | (
            TierPlanKind::DfgFromBaseline,
            JitType::Baseline,
            JitType::Dfg
        ) | (TierPlanKind::FtlFromDfg, JitType::Dfg, JitType::Ftl)
            | (TierPlanKind::OsrEntry, JitType::Baseline, JitType::Dfg)
            | (TierPlanKind::OsrEntry, JitType::Dfg, JitType::Ftl)
            | (
                TierPlanKind::RecompileAfterInvalidation,
                JitType::Dfg,
                JitType::Dfg
            )
            | (
                TierPlanKind::RecompileAfterInvalidation,
                JitType::Ftl,
                JitType::Ftl
            )
    )
}

const BASELINE_TIER_TRIGGERS: &[TieringTrigger] = &[TieringTrigger::EntryCounter];
const OPTIMIZING_TIER_TRIGGERS: &[TieringTrigger] =
    &[TieringTrigger::LoopCounter, TieringTrigger::OsrEntry];
const RECOMPILE_TIER_TRIGGERS: &[TieringTrigger] = &[TieringTrigger::RecompileAfterInvalidation];

const BASELINE_DEFAULT_THRESHOLDS: TierThresholds = TierThresholds {
    baseline_warmup: 1,
    dfg_entry: 0,
    ftl_entry: 0,
    osr_entry: 0,
};
const DFG_DEFAULT_THRESHOLDS: TierThresholds = TierThresholds {
    baseline_warmup: 1,
    dfg_entry: 100,
    ftl_entry: 0,
    osr_entry: 10,
};
const FTL_DEFAULT_THRESHOLDS: TierThresholds = TierThresholds {
    baseline_warmup: 1,
    dfg_entry: 100,
    ftl_entry: 1000,
    osr_entry: 100,
};

pub const STATIC_TIER_DESCRIPTORS: &[StaticTierDescriptor] = &[
    StaticTierDescriptor {
        name: "baseline-from-bytecode",
        kind: TierPlanKind::BaselineFromBytecode,
        from_tier: JitType::None,
        to_tier: JitType::Baseline,
        policy: TieringPolicy::BaselineAllowed,
        triggers: BASELINE_TIER_TRIGGERS,
        priority: TierPlanPriorityHint::EntryWarmup,
        default_thresholds: BASELINE_DEFAULT_THRESHOLDS,
        owner: TierDescriptorOwner::BaselineJit,
        mutation_authority: TierDescriptorMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust tier descriptor table",
    },
    StaticTierDescriptor {
        name: "dfg-from-baseline",
        kind: TierPlanKind::DfgFromBaseline,
        from_tier: JitType::Baseline,
        to_tier: JitType::Dfg,
        policy: TieringPolicy::OptimizingAllowed,
        triggers: OPTIMIZING_TIER_TRIGGERS,
        priority: TierPlanPriorityHint::HotLoop,
        default_thresholds: DFG_DEFAULT_THRESHOLDS,
        owner: TierDescriptorOwner::DfgJit,
        mutation_authority: TierDescriptorMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust tier descriptor table",
    },
    StaticTierDescriptor {
        name: "ftl-from-dfg",
        kind: TierPlanKind::FtlFromDfg,
        from_tier: JitType::Dfg,
        to_tier: JitType::Ftl,
        policy: TieringPolicy::OptimizingAllowed,
        triggers: OPTIMIZING_TIER_TRIGGERS,
        priority: TierPlanPriorityHint::FrequentExit,
        default_thresholds: FTL_DEFAULT_THRESHOLDS,
        owner: TierDescriptorOwner::FtlJit,
        mutation_authority: TierDescriptorMutationAuthority::GeneratedStaticDataRefresh,
        provenance: "static Rust tier descriptor table",
    },
    StaticTierDescriptor {
        name: "recompile-after-invalidation",
        kind: TierPlanKind::RecompileAfterInvalidation,
        from_tier: JitType::Dfg,
        to_tier: JitType::Dfg,
        policy: TieringPolicy::OptimizingAllowed,
        triggers: RECOMPILE_TIER_TRIGGERS,
        priority: TierPlanPriorityHint::Background,
        default_thresholds: DFG_DEFAULT_THRESHOLDS,
        owner: TierDescriptorOwner::RuntimeTieringPolicy,
        mutation_authority: TierDescriptorMutationAuthority::RuntimePolicyInitialization,
        provenance: "static Rust tier descriptor table",
    },
];

pub const TIER_DESCRIPTOR_TABLE: TierDescriptorTable =
    TierDescriptorTable::new(STATIC_TIER_DESCRIPTORS);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::CellId;

    #[test]
    fn static_tier_table_validates_descriptor_shape() {
        assert_eq!(TIER_DESCRIPTOR_TABLE.validate(), Ok(()));
    }

    #[test]
    fn tier_plan_rejects_unlisted_trigger() {
        let descriptor = TIER_DESCRIPTOR_TABLE
            .descriptor_for_kind(TierPlanKind::DfgFromBaseline)
            .expect("static DFG tier descriptor");
        let profile = TierPlanProfile {
            counters: TierCounters {
                execution_count: 1,
                loop_count: 1,
                slow_path_count: 0,
                osr_exit_count: 0,
                invalidation_count: 0,
            },
            thresholds: descriptor.default_thresholds,
            osr_entry_bytecode_index: None,
            bytecode_size: Some(1),
            inline_cache_count: 0,
            exit_count: 0,
        };

        let plan = TierPlanDescriptor::from_static_descriptor(
            descriptor,
            CodeBlockId(CellId(1)),
            CompilationPlanId(1),
            TieringTrigger::EntryCounter,
            profile,
        );

        assert_eq!(
            plan,
            Err(TieringValidationError::TriggerNotAllowed(
                TierPlanKind::DfgFromBaseline
            ))
        );
    }

    #[test]
    fn tier_selection_picks_dfg_for_hot_baseline_loop() {
        let owner = CodeBlockId(CellId(7));
        let state = TieringState {
            owner: Some(owner),
            current_tier: JitType::Baseline,
            requested_tier: None,
            policy: TieringPolicy::OptimizingAllowed,
            osr: OsrState::Unavailable,
            execution_counter_slot: None,
            active_request: None,
            thresholds: DFG_DEFAULT_THRESHOLDS,
            counters: TierCounters {
                execution_count: 10,
                loop_count: 100,
                slow_path_count: 0,
                osr_exit_count: 0,
                invalidation_count: 0,
            },
        };

        let plan = select_tier_plan(&state, owner, CompilationPlanId(2), Some(12), 3)
            .expect("selection should validate");

        assert_eq!(
            plan.map(|descriptor| (descriptor.kind, descriptor.trigger, descriptor.to_tier)),
            Some((
                TierPlanKind::DfgFromBaseline,
                TieringTrigger::LoopCounter,
                JitType::Dfg
            ))
        );
    }

    #[test]
    fn tier_selection_suppresses_when_policy_disabled() {
        let owner = CodeBlockId(CellId(8));
        let state = TieringState {
            owner: Some(owner),
            current_tier: JitType::None,
            requested_tier: None,
            policy: TieringPolicy::Disabled,
            osr: OsrState::Unavailable,
            execution_counter_slot: None,
            active_request: None,
            thresholds: BASELINE_DEFAULT_THRESHOLDS,
            counters: TierCounters {
                execution_count: 100,
                loop_count: 0,
                slow_path_count: 0,
                osr_exit_count: 0,
                invalidation_count: 0,
            },
        };

        assert_eq!(
            select_tier_plan(&state, owner, CompilationPlanId(3), None, 0),
            Ok(None)
        );
    }

    #[test]
    fn tier_fallback_for_invalidated_dfg_recompile_preserves_profile() {
        let descriptor = TIER_DESCRIPTOR_TABLE
            .descriptor_for_kind(TierPlanKind::RecompileAfterInvalidation)
            .expect("static recompile descriptor");
        let profile = TierPlanProfile {
            counters: TierCounters {
                execution_count: 10,
                loop_count: 20,
                slow_path_count: 1,
                osr_exit_count: 2,
                invalidation_count: 1,
            },
            thresholds: descriptor.default_thresholds,
            osr_entry_bytecode_index: None,
            bytecode_size: Some(8),
            inline_cache_count: 2,
            exit_count: 2,
        };
        let plan = TierPlanDescriptor::from_static_descriptor(
            descriptor,
            CodeBlockId(CellId(9)),
            CompilationPlanId(9),
            TieringTrigger::RecompileAfterInvalidation,
            profile,
        )
        .unwrap();

        let fallback = plan.fallback_semantics(TierFallbackReason::WatchpointInvalidated);

        assert_eq!(fallback.target, TierFallbackTarget::RecompileSameTier);
        assert!(fallback.preserves_profile);
        assert!(fallback.should_count_invalidation);
    }

    #[test]
    fn fallback_result_to_interpreter_requires_bytecode_resume() {
        let semantics = TierFallbackSemantics {
            owner: CodeBlockId(CellId(10)),
            from_tier: JitType::Baseline,
            attempted_tier: JitType::Dfg,
            reason: TierFallbackReason::UnsupportedTier,
            target: TierFallbackTarget::ReturnToInterpreter,
            preserves_profile: true,
            should_count_invalidation: true,
        };

        assert_eq!(
            TierFallbackResultRecord::from_semantics(semantics, None),
            Err(TieringValidationError::FallbackMissingBytecodeIndex)
        );

        let record = TierFallbackResultRecord::from_semantics(
            semantics,
            Some(BytecodeIndex::from_offset(6)),
        )
        .unwrap();

        assert_eq!(record.resume, TierFallbackResumeKind::ContinueInInterpreter);
        assert!(record.clears_active_request);
    }
}
