//! JetStream 3 Octane manifest and scoring contracts for the shell.
//!
//! This module intentionally does not execute JavaScript. It mirrors the
//! driver-owned Octane plan metadata, loads source text into shell provenance
//! records, prepares runner-owned sources, and keeps the synchronous
//! `DefaultBenchmark` scoring math used by JetStream 3.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use super::{
    ShellMode, ShellSourceAppendRequest, ShellSourceKind, ShellSourceLoadError, ShellSourceLoader,
};
use crate::bytecode::{SourceOriginId, SourceProviderId};
use crate::interpreter::{
    CoreHostOutputRecord, CoreHostOutputSink, CoreHostResultRecord, DispatchConfig,
    ExecutionCompletion, ExecutionError,
};
use crate::syntax::source::SourceText;
use crate::vm::{
    BaselineGeneratedExecutionPolicy, GeneratedDirectCallGeneratedEntryPolicy,
    SourceExecutionError, SourceSessionHandle, SourceSessionHostGlobalConfig, SourceSessionSource,
    Vm, VmBaselineGeneratedDispatchedOpcodeCount, VmBaselineGeneratedDispatchedSiteOpcodeCount,
    VmBaselineGeneratedExecutionSummary, VmBaselineGeneratedInvalidationSummary, VmConfig,
    VmGeneratedDirectCallCalleeFallbackSummary,
    VmGeneratedDirectCallRootlessPreferredNativeEntryCounts,
    VmGeneratedDirectCallRootlessRejectionCounts,
    VmGeneratedDirectCallRootlessRetainedSideExitCount,
    VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount,
    VmGeneratedDirectCallRouteOpportunitySummary, VmGeneratedDirectCallTransactionSummary,
};

pub const OCTANE_DEFAULT_ITERATION_COUNT: usize = 120;
pub const OCTANE_DEFAULT_WORST_CASE_COUNT: usize = 4;
const OCTANE_DEFAULT_BENCHMARK_TELEMETRY_PREFIX: &str = "__JSC_RUST_OCTANE_RESULTS__:";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctaneBenchmarkClass {
    DefaultBenchmark,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OctaneBenchmarkPlan {
    pub name: &'static str,
    pub files: &'static [&'static str],
    pub deterministic_random: bool,
    pub iterations: Option<usize>,
    pub worst_case_count: Option<usize>,
    pub benchmark_class: OctaneBenchmarkClass,
}

impl OctaneBenchmarkPlan {
    pub fn run_config(
        self,
        overrides: OctaneBenchmarkRunOverrides,
    ) -> Result<OctaneDefaultBenchmarkRunConfig, OctaneScoringError> {
        OctaneDefaultBenchmarkRunConfig::new(
            overrides
                .iterations
                .or(self.iterations)
                .unwrap_or(OCTANE_DEFAULT_ITERATION_COUNT),
            overrides
                .worst_case_count
                .or(self.worst_case_count)
                .unwrap_or(OCTANE_DEFAULT_WORST_CASE_COUNT),
        )
    }

    pub fn default_run_config(self) -> Result<OctaneDefaultBenchmarkRunConfig, OctaneScoringError> {
        self.run_config(OctaneBenchmarkRunOverrides::none())
    }

    pub fn score_default_results(
        self,
        overrides: OctaneBenchmarkRunOverrides,
        result_times_ms: &[f64],
    ) -> Result<OctaneDefaultBenchmarkScores, OctaneScoringError> {
        self.run_config(overrides)?.score_results(result_times_ms)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OctaneBenchmarkRunOverrides {
    pub iterations: Option<usize>,
    pub worst_case_count: Option<usize>,
}

impl OctaneBenchmarkRunOverrides {
    pub const fn none() -> Self {
        Self {
            iterations: None,
            worst_case_count: None,
        }
    }

    pub const fn new(iterations: Option<usize>, worst_case_count: Option<usize>) -> Self {
        Self {
            iterations,
            worst_case_count,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OctaneDefaultBenchmarkRunConfig {
    pub iterations: usize,
    pub worst_case_count: usize,
}

impl OctaneDefaultBenchmarkRunConfig {
    pub fn new(iterations: usize, worst_case_count: usize) -> Result<Self, OctaneScoringError> {
        if iterations <= worst_case_count {
            return Err(OctaneScoringError::IterationsMustExceedWorstCase {
                iterations,
                worst_case_count,
            });
        }

        Ok(Self {
            iterations,
            worst_case_count,
        })
    }

    pub fn score_results(
        self,
        result_times_ms: &[f64],
    ) -> Result<OctaneDefaultBenchmarkScores, OctaneScoringError> {
        if result_times_ms.len() < self.iterations {
            return Err(OctaneScoringError::TooFewResults {
                expected: self.iterations,
                actual: result_times_ms.len(),
            });
        }
        if result_times_ms.len() > self.iterations {
            return Err(OctaneScoringError::TooManyResults {
                expected: self.iterations,
                actual: result_times_ms.len(),
            });
        }

        let (first_time, remaining_times) =
            result_times_ms
                .split_first()
                .ok_or(OctaneScoringError::TooFewResults {
                    expected: self.iterations,
                    actual: 0,
                })?;

        if remaining_times.len() < self.worst_case_count {
            return Err(OctaneScoringError::TooFewResultsForWorstCase {
                expected: self.worst_case_count,
                actual: remaining_times.len(),
            });
        }

        let first_iteration = octane_default_to_score(*first_time);
        let average = octane_default_to_score(arithmetic_mean(remaining_times));

        let mut slowest_times = remaining_times.to_vec();
        slowest_times.sort_by(|left, right| right.partial_cmp(left).unwrap_or(Ordering::Equal));
        let worst_case =
            octane_default_to_score(arithmetic_mean(&slowest_times[..self.worst_case_count]));

        let score = geometric_mean(&[first_iteration, worst_case, average]);
        Ok(OctaneDefaultBenchmarkScores {
            first_iteration,
            worst_case,
            average,
            score,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OctaneDefaultBenchmarkScores {
    pub first_iteration: f64,
    pub worst_case: f64,
    pub average: f64,
    pub score: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctaneScoringError {
    IterationsMustExceedWorstCase {
        iterations: usize,
        worst_case_count: usize,
    },
    TooFewResults {
        expected: usize,
        actual: usize,
    },
    TooManyResults {
        expected: usize,
        actual: usize,
    },
    TooFewResultsForWorstCase {
        expected: usize,
        actual: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OctaneBenchmarkSelection {
    pub name: &'static str,
    pub benchmark_names: &'static [&'static str],
}

impl OctaneBenchmarkSelection {
    pub fn resolved_plans(self) -> Result<Vec<&'static OctaneBenchmarkPlan>, OctaneManifestError> {
        let mut plans = Vec::with_capacity(self.benchmark_names.len());
        for name in self.benchmark_names {
            plans.push(
                octane_plan_by_name(name).ok_or(OctaneManifestError::BenchmarkNotFound(name))?,
            );
        }
        Ok(plans)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctaneManifestError {
    BenchmarkNotFound(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctaneSuite {
    Core,
    Full,
}

impl OctaneSuite {
    pub fn name(self) -> &'static str {
        match self {
            Self::Core => OCTANE_CORE_SELECTION.name,
            Self::Full => "Octane-full",
        }
    }

    pub fn resolved_plans(self) -> Result<Vec<&'static OctaneBenchmarkPlan>, OctaneManifestError> {
        match self {
            Self::Core => OCTANE_CORE_SELECTION.resolved_plans(),
            Self::Full => Ok(OCTANE_DRIVER_PLANS.iter().collect()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OctaneRunConfig {
    pub suite: OctaneSuite,
    pub overrides: Option<OctaneBenchmarkRunOverrides>,
}

impl OctaneRunConfig {
    pub const fn new(suite: OctaneSuite) -> Self {
        Self {
            suite,
            overrides: None,
        }
    }

    pub const fn with_overrides(
        suite: OctaneSuite,
        overrides: OctaneBenchmarkRunOverrides,
    ) -> Self {
        Self {
            suite,
            overrides: Some(overrides),
        }
    }

    pub const fn effective_overrides(self) -> OctaneBenchmarkRunOverrides {
        match self.overrides {
            Some(overrides) => overrides,
            None => OctaneBenchmarkRunOverrides::none(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OctanePreparationConfig {
    pub jetstream_root: PathBuf,
    pub run: OctaneRunConfig,
}

impl OctanePreparationConfig {
    pub fn new(jetstream_root: impl Into<PathBuf>, run: OctaneRunConfig) -> Self {
        Self {
            jetstream_root: jetstream_root.into(),
            run,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OctaneExecutionMode {
    #[default]
    InterpreterOnly,
    BaselineAllowed,
}

impl OctaneExecutionMode {
    pub fn vm_config(self) -> VmConfig {
        match self {
            Self::InterpreterOnly => VmConfig::interpreter_only(),
            Self::BaselineAllowed => VmConfig::baseline_allowed(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OctaneSuiteFailurePolicy {
    #[default]
    FailFast,
    CollectAll,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OctaneExecutionConfig {
    pub mode: OctaneExecutionMode,
    pub failure_policy: OctaneSuiteFailurePolicy,
    pub dispatch_config: DispatchConfig,
    pub baseline_generated_execution_policy: BaselineGeneratedExecutionPolicy,
    pub generated_direct_call_generated_entry_policy: GeneratedDirectCallGeneratedEntryPolicy,
}

impl OctaneExecutionConfig {
    pub const fn new(mode: OctaneExecutionMode, failure_policy: OctaneSuiteFailurePolicy) -> Self {
        Self {
            mode,
            failure_policy,
            dispatch_config: DispatchConfig::unbounded(),
            baseline_generated_execution_policy: BaselineGeneratedExecutionPolicy::Enabled,
            generated_direct_call_generated_entry_policy:
                GeneratedDirectCallGeneratedEntryPolicy::Enabled,
        }
    }

    pub const fn with_dispatch_config(mut self, dispatch_config: DispatchConfig) -> Self {
        self.dispatch_config = dispatch_config;
        self
    }

    pub const fn with_generated_direct_call_generated_entry_policy(
        mut self,
        policy: GeneratedDirectCallGeneratedEntryPolicy,
    ) -> Self {
        self.generated_direct_call_generated_entry_policy = policy;
        self
    }

    pub const fn with_baseline_generated_execution_policy(
        mut self,
        policy: BaselineGeneratedExecutionPolicy,
    ) -> Self {
        self.baseline_generated_execution_policy = policy;
        self
    }

    pub fn vm_config(self) -> VmConfig {
        // Oversized-file exception: Octane execution config already owns the
        // probe dispatch plumbing in this file; keep this diagnostic VM policy
        // crossing here until the Octane shell config is extracted.
        self.mode
            .vm_config()
            .with_baseline_generated_execution_policy(self.baseline_generated_execution_policy)
            .with_generated_direct_call_generated_entry_policy(
                self.generated_direct_call_generated_entry_policy,
            )
    }
}

impl Default for OctaneExecutionConfig {
    fn default() -> Self {
        Self::new(
            OctaneExecutionMode::default(),
            OctaneSuiteFailurePolicy::default(),
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OctanePreparedSuiteExecutionReport {
    pub suite: OctaneSuite,
    pub mode: OctaneExecutionMode,
    pub failure_policy: OctaneSuiteFailurePolicy,
    pub benchmarks: Vec<OctaneBenchmarkExecutionReport>,
    pub suite_score: Option<OctaneSuiteScoreRecord>,
    pub stopped_early: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OctaneBenchmarkExecutionReport {
    pub benchmark: &'static str,
    pub mode: OctaneExecutionMode,
    pub run_config: OctaneDefaultBenchmarkRunConfig,
    pub source_records: Vec<OctaneSourceExecutionRecord>,
    pub host_output_records: Vec<CoreHostOutputRecord>,
    /// Per-benchmark VM tiering activity. Suite execution reuses one VM, so
    /// these counts are deltas from the start of this benchmark.
    pub tiering_delta: OctaneTieringSummary,
    pub outcome: OctaneExecutionOutcome,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OctaneTieringSummary {
    pub entry_decisions: usize,
    pub fallback_records: usize,
    pub diagnostics: usize,
    pub baseline_installs: usize,
    pub baseline_entry_artifacts: usize,
    pub baseline_materializations: usize,
    pub baseline_generated_code_artifacts: usize,
    pub baseline_generated_code_invalidations: usize,
    pub baseline_generated_code_invalidation_summary: VmBaselineGeneratedInvalidationSummary,
    pub baseline_generated_executions: usize,
    pub baseline_generated_executed_bytecodes: u64,
    pub baseline_entry_auto_materializations: usize,
    pub baseline_native_lowering_failures: usize,
    pub baseline_native_semantic_byte_emission_failures: usize,
    pub baseline_native_entry_readiness: usize,
    pub baseline_generated_execution_summaries: Vec<VmBaselineGeneratedExecutionSummary>,
    pub baseline_generated_dispatched_opcode_counts: Vec<VmBaselineGeneratedDispatchedOpcodeCount>,
    pub baseline_generated_dispatched_site_opcode_counts:
        Vec<VmBaselineGeneratedDispatchedSiteOpcodeCount>,
    pub generated_direct_call_transactions: usize,
    pub generated_direct_call_generated_entries: usize,
    pub generated_direct_call_native_entries: usize,
    pub generated_direct_call_native_interpreter_fallbacks: usize,
    pub generated_direct_call_nested_interpreter_fallbacks: usize,
    pub generated_direct_call_hot_slot_hits: usize,
    pub generated_direct_call_sidecar_hot_slot_hits: usize,
    pub generated_direct_call_preferred_route_hits: usize,
    pub generated_direct_call_rootless_generated_entries: usize,
    pub generated_direct_call_rootless_generated_entry_proof_cache_hits: usize,
    pub generated_direct_call_rootless_native_entries: usize,
    pub generated_direct_call_rootless_rejections: VmGeneratedDirectCallRootlessRejectionCounts,
    pub generated_direct_call_rootless_native_entry_rejections:
        VmGeneratedDirectCallRootlessRejectionCounts,
    pub generated_direct_call_rootless_unsupported_body_opcode_counts:
        Vec<VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount>,
    pub generated_direct_call_rootless_native_entry_unsupported_body_opcode_counts:
        Vec<VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount>,
    pub generated_direct_call_rootless_native_entry_retained_side_exit_counts:
        Vec<VmGeneratedDirectCallRootlessRetainedSideExitCount>,
    pub generated_direct_call_rootless_preferred_native_entry_counts:
        VmGeneratedDirectCallRootlessPreferredNativeEntryCounts,
    pub generated_direct_call_transaction_summaries: Vec<VmGeneratedDirectCallTransactionSummary>,
    pub generated_direct_call_callee_fallback_summaries:
        Vec<VmGeneratedDirectCallCalleeFallbackSummary>,
    pub generated_direct_call_route_opportunity_summaries:
        Vec<VmGeneratedDirectCallRouteOpportunitySummary>,
    pub launch_descriptors: usize,
    pub call_observations: usize,
    pub call_link_boundary_validations: usize,
    pub call_link_inline_cache_attachments: usize,
    pub property_load_observations: usize,
    pub property_store_observations: usize,
    pub property_inline_cache_evolution_records: usize,
    pub property_inline_cache_evolution_admitted: usize,
    pub property_inline_cache_evolution_buffered: usize,
    pub property_inline_cache_evolution_buffered_duplicates: usize,
    pub property_inline_cache_evolution_cooldowns: usize,
    pub property_inline_cache_evolution_final_gave_up: usize,
    pub property_inline_cache_evolution_gave_up_skips: usize,
    pub property_inline_cache_evolution_generated_megamorphic_load: usize,
    pub property_inline_cache_evolution_megamorphic_load_skips: usize,
    pub property_inline_cache_evolution_generated_megamorphic_store: usize,
    pub property_inline_cache_evolution_megamorphic_store_skips: usize,
    pub property_inline_cache_evolution_generated_megamorphic_has: usize,
    pub property_inline_cache_evolution_megamorphic_has_skips: usize,
    pub property_load_megamorphic_cache_records: usize,
    pub property_store_megamorphic_cache_records: usize,
    pub property_has_megamorphic_cache_records: usize,
    pub property_inline_cache_attachments: usize,
}

impl OctaneTieringSummary {
    fn from_vm(vm: &Vm) -> Self {
        let tiering = vm.tiering_integration();
        // Redesign-audit telemetry Unit 3: these used to `.iter().filter(..).count()`
        // over the deleted `property_inline_cache_evolution_records` log (an
        // unbounded per-property-access vec); they now read write-time
        // cumulative counters (`VmPropertyInlineCacheEvolutionDecisionCounts`,
        // the `VmGeneratedDirectCallRootlessPreferredNativeEntryCounts`
        // scoreboard pattern) instead.
        let evolution_counts = tiering.property_inline_cache_evolution_decision_counts();
        let property_inline_cache_evolution_records = evolution_counts.total as usize;
        let property_inline_cache_evolution_admitted = evolution_counts.admitted as usize;
        let property_inline_cache_evolution_buffered = evolution_counts.buffered as usize;
        let property_inline_cache_evolution_buffered_duplicates =
            evolution_counts.buffered_duplicates as usize;
        let property_inline_cache_evolution_cooldowns = evolution_counts.cooldowns as usize;
        let property_inline_cache_evolution_final_gave_up = evolution_counts.final_gave_up as usize;
        let property_inline_cache_evolution_gave_up_skips = evolution_counts.gave_up_skips as usize;
        let property_inline_cache_evolution_generated_megamorphic_load =
            evolution_counts.generated_megamorphic_load as usize;
        let property_inline_cache_evolution_megamorphic_load_skips =
            evolution_counts.megamorphic_load_skips as usize;
        let property_inline_cache_evolution_generated_megamorphic_store =
            evolution_counts.generated_megamorphic_store as usize;
        let property_inline_cache_evolution_megamorphic_store_skips =
            evolution_counts.megamorphic_store_skips as usize;
        let property_inline_cache_evolution_generated_megamorphic_has =
            evolution_counts.generated_megamorphic_has as usize;
        let property_inline_cache_evolution_megamorphic_has_skips =
            evolution_counts.megamorphic_has_skips as usize;
        Self {
            entry_decisions: tiering.entry_decision_count() as usize,
            fallback_records: tiering.fallback_records().len(),
            diagnostics: tiering.diagnostics().len(),
            baseline_installs: tiering.baseline_install_records().len(),
            baseline_entry_artifacts: tiering.baseline_entry_artifacts().len(),
            baseline_materializations: tiering.baseline_executable_materializations().len(),
            baseline_generated_code_artifacts: tiering.baseline_generated_code_artifacts().len(),
            baseline_generated_code_invalidations: tiering
                .baseline_generated_code_invalidations()
                .len(),
            baseline_generated_code_invalidation_summary: tiering
                .baseline_generated_code_invalidation_summary(),
            baseline_generated_executions: tiering.baseline_generated_execution_count(),
            baseline_generated_executed_bytecodes: tiering
                .baseline_generated_executed_bytecode_count(),
            // Redesign-audit telemetry Unit 1: no longer clones the entire
            // (now owner-keyed-bounded) `baseline_entry_auto_materializations`
            // vec into the report per summary -- that still accumulates
            // without bound across a whole benchmark-suite run's worth of
            // reports even though the source state itself is now bounded.
            // The count below is the load-bearing signal for telemetry.
            baseline_entry_auto_materializations: tiering
                .baseline_entry_auto_materializations()
                .len(),
            baseline_native_lowering_failures: tiering.baseline_native_lowering_failure_count(),
            baseline_native_semantic_byte_emission_failures: tiering
                .baseline_native_semantic_byte_emission_failure_count(),
            baseline_native_entry_readiness: tiering
                .baseline_native_entry_readiness_records()
                .len(),
            baseline_generated_execution_summaries: tiering
                .baseline_generated_execution_summaries()
                .to_vec(),
            baseline_generated_dispatched_opcode_counts: tiering
                .baseline_generated_dispatched_opcode_counts()
                .to_vec(),
            baseline_generated_dispatched_site_opcode_counts: tiering
                .baseline_generated_dispatched_site_opcode_counts()
                .to_vec(),
            generated_direct_call_transactions: tiering.generated_direct_call_transaction_count(),
            generated_direct_call_generated_entries: tiering
                .generated_direct_call_generated_entry_count(),
            generated_direct_call_native_entries: tiering
                .generated_direct_call_native_entry_count(),
            generated_direct_call_native_interpreter_fallbacks: tiering
                .generated_direct_call_native_interpreter_fallback_count(),
            generated_direct_call_nested_interpreter_fallbacks: tiering
                .generated_direct_call_nested_interpreter_fallback_count(),
            generated_direct_call_hot_slot_hits: tiering.generated_direct_call_hot_slot_hit_count(),
            generated_direct_call_sidecar_hot_slot_hits: tiering
                .generated_direct_call_sidecar_hot_slot_hit_count(),
            generated_direct_call_preferred_route_hits: tiering
                .generated_direct_call_preferred_route_hit_count(),
            generated_direct_call_rootless_generated_entries: tiering
                .generated_direct_call_rootless_generated_entry_count(),
            generated_direct_call_rootless_generated_entry_proof_cache_hits: tiering
                .generated_direct_call_rootless_generated_entry_proof_cache_hit_count(),
            generated_direct_call_rootless_native_entries: tiering
                .generated_direct_call_rootless_native_entry_count(),
            generated_direct_call_rootless_rejections: tiering
                .generated_direct_call_rootless_rejection_counts(),
            generated_direct_call_rootless_native_entry_rejections: tiering
                .generated_direct_call_rootless_native_entry_rejection_counts(),
            generated_direct_call_rootless_unsupported_body_opcode_counts: tiering
                .generated_direct_call_rootless_unsupported_body_opcode_counts()
                .to_vec(),
            generated_direct_call_rootless_native_entry_unsupported_body_opcode_counts: tiering
                .generated_direct_call_rootless_native_entry_unsupported_body_opcode_counts()
                .to_vec(),
            generated_direct_call_rootless_native_entry_retained_side_exit_counts: tiering
                .generated_direct_call_rootless_native_entry_retained_side_exit_counts()
                .to_vec(),
            generated_direct_call_rootless_preferred_native_entry_counts: tiering
                .generated_direct_call_rootless_preferred_native_entry_counts(),
            generated_direct_call_transaction_summaries: tiering
                .generated_direct_call_transaction_summaries()
                .to_vec(),
            generated_direct_call_callee_fallback_summaries: tiering
                .generated_direct_call_callee_fallback_summaries()
                .to_vec(),
            generated_direct_call_route_opportunity_summaries: tiering
                .generated_direct_call_route_opportunity_summaries()
                .to_vec(),
            launch_descriptors: vm.entry_state().launch_descriptors().len(),
            call_observations: tiering.call_observations().len(),
            call_link_boundary_validations: tiering.call_link_boundary_validation_records().len(),
            call_link_inline_cache_attachments: tiering
                .call_link_inline_cache_attachment_records()
                .len(),
            property_load_observations: tiering.property_load_observations().len(),
            property_store_observations: tiering.property_store_observations().len(),
            property_inline_cache_evolution_records,
            property_inline_cache_evolution_admitted,
            property_inline_cache_evolution_buffered,
            property_inline_cache_evolution_buffered_duplicates,
            property_inline_cache_evolution_cooldowns,
            property_inline_cache_evolution_final_gave_up,
            property_inline_cache_evolution_gave_up_skips,
            property_inline_cache_evolution_generated_megamorphic_load,
            property_inline_cache_evolution_megamorphic_load_skips,
            property_inline_cache_evolution_generated_megamorphic_store,
            property_inline_cache_evolution_megamorphic_store_skips,
            property_inline_cache_evolution_generated_megamorphic_has,
            property_inline_cache_evolution_megamorphic_has_skips,
            property_load_megamorphic_cache_records: tiering
                .property_load_megamorphic_cache_records()
                .len(),
            property_store_megamorphic_cache_records: tiering
                .property_store_megamorphic_cache_records()
                .len(),
            property_has_megamorphic_cache_records: tiering
                .property_has_megamorphic_cache_records()
                .len(),
            property_inline_cache_attachments: tiering
                .property_inline_cache_attachment_records()
                .len(),
        }
    }

    fn delta_since(self, start: Self) -> Self {
        let baseline_generated_execution_summaries =
            octane_baseline_generated_execution_summary_delta(
                &self.baseline_generated_execution_summaries,
                &start.baseline_generated_execution_summaries,
            );
        let baseline_generated_dispatched_opcode_counts =
            octane_baseline_generated_dispatched_opcode_count_delta(
                &self.baseline_generated_dispatched_opcode_counts,
                &start.baseline_generated_dispatched_opcode_counts,
            );
        let baseline_generated_dispatched_site_opcode_counts =
            octane_baseline_generated_dispatched_site_opcode_count_delta(
                &self.baseline_generated_dispatched_site_opcode_counts,
                &start.baseline_generated_dispatched_site_opcode_counts,
            );
        let generated_direct_call_rootless_unsupported_body_opcode_counts =
            octane_rootless_unsupported_body_opcode_count_delta(
                &self.generated_direct_call_rootless_unsupported_body_opcode_counts,
                &start.generated_direct_call_rootless_unsupported_body_opcode_counts,
            );
        let generated_direct_call_rootless_native_entry_unsupported_body_opcode_counts =
            octane_rootless_unsupported_body_opcode_count_delta(
                &self.generated_direct_call_rootless_native_entry_unsupported_body_opcode_counts,
                &start.generated_direct_call_rootless_native_entry_unsupported_body_opcode_counts,
            );
        let generated_direct_call_rootless_native_entry_retained_side_exit_counts =
            octane_rootless_retained_side_exit_count_delta(
                &self.generated_direct_call_rootless_native_entry_retained_side_exit_counts,
                &start.generated_direct_call_rootless_native_entry_retained_side_exit_counts,
            );
        let generated_direct_call_rootless_preferred_native_entry_counts =
            octane_rootless_preferred_native_entry_counts_delta(
                self.generated_direct_call_rootless_preferred_native_entry_counts,
                start.generated_direct_call_rootless_preferred_native_entry_counts,
            );
        let generated_direct_call_transaction_summaries =
            octane_generated_direct_call_transaction_summary_delta(
                &self.generated_direct_call_transaction_summaries,
                &start.generated_direct_call_transaction_summaries,
            );
        let generated_direct_call_callee_fallback_summaries =
            octane_generated_direct_call_callee_fallback_summary_delta(
                &self.generated_direct_call_callee_fallback_summaries,
                &start.generated_direct_call_callee_fallback_summaries,
            );
        let generated_direct_call_route_opportunity_summaries =
            octane_generated_direct_call_route_opportunity_summary_delta(
                &self.generated_direct_call_route_opportunity_summaries,
                &start.generated_direct_call_route_opportunity_summaries,
            );
        Self {
            entry_decisions: self.entry_decisions.saturating_sub(start.entry_decisions),
            fallback_records: self.fallback_records.saturating_sub(start.fallback_records),
            diagnostics: self.diagnostics.saturating_sub(start.diagnostics),
            baseline_installs: self
                .baseline_installs
                .saturating_sub(start.baseline_installs),
            baseline_entry_artifacts: self
                .baseline_entry_artifacts
                .saturating_sub(start.baseline_entry_artifacts),
            baseline_materializations: self
                .baseline_materializations
                .saturating_sub(start.baseline_materializations),
            baseline_generated_code_artifacts: self
                .baseline_generated_code_artifacts
                .saturating_sub(start.baseline_generated_code_artifacts),
            baseline_generated_code_invalidations: self
                .baseline_generated_code_invalidations
                .saturating_sub(start.baseline_generated_code_invalidations),
            baseline_generated_code_invalidation_summary: self
                .baseline_generated_code_invalidation_summary
                .saturating_sub(start.baseline_generated_code_invalidation_summary),
            baseline_generated_executions: self
                .baseline_generated_executions
                .saturating_sub(start.baseline_generated_executions),
            baseline_generated_executed_bytecodes: self
                .baseline_generated_executed_bytecodes
                .saturating_sub(start.baseline_generated_executed_bytecodes),
            baseline_entry_auto_materializations: self
                .baseline_entry_auto_materializations
                .saturating_sub(start.baseline_entry_auto_materializations),
            baseline_native_lowering_failures: self
                .baseline_native_lowering_failures
                .saturating_sub(start.baseline_native_lowering_failures),
            baseline_native_semantic_byte_emission_failures: self
                .baseline_native_semantic_byte_emission_failures
                .saturating_sub(start.baseline_native_semantic_byte_emission_failures),
            baseline_native_entry_readiness: self
                .baseline_native_entry_readiness
                .saturating_sub(start.baseline_native_entry_readiness),
            baseline_generated_execution_summaries,
            baseline_generated_dispatched_opcode_counts,
            baseline_generated_dispatched_site_opcode_counts,
            generated_direct_call_transactions: self
                .generated_direct_call_transactions
                .saturating_sub(start.generated_direct_call_transactions),
            generated_direct_call_generated_entries: self
                .generated_direct_call_generated_entries
                .saturating_sub(start.generated_direct_call_generated_entries),
            generated_direct_call_native_entries: self
                .generated_direct_call_native_entries
                .saturating_sub(start.generated_direct_call_native_entries),
            generated_direct_call_native_interpreter_fallbacks: self
                .generated_direct_call_native_interpreter_fallbacks
                .saturating_sub(start.generated_direct_call_native_interpreter_fallbacks),
            generated_direct_call_nested_interpreter_fallbacks: self
                .generated_direct_call_nested_interpreter_fallbacks
                .saturating_sub(start.generated_direct_call_nested_interpreter_fallbacks),
            generated_direct_call_hot_slot_hits: self
                .generated_direct_call_hot_slot_hits
                .saturating_sub(start.generated_direct_call_hot_slot_hits),
            generated_direct_call_sidecar_hot_slot_hits: self
                .generated_direct_call_sidecar_hot_slot_hits
                .saturating_sub(start.generated_direct_call_sidecar_hot_slot_hits),
            generated_direct_call_preferred_route_hits: self
                .generated_direct_call_preferred_route_hits
                .saturating_sub(start.generated_direct_call_preferred_route_hits),
            generated_direct_call_rootless_generated_entries: self
                .generated_direct_call_rootless_generated_entries
                .saturating_sub(start.generated_direct_call_rootless_generated_entries),
            generated_direct_call_rootless_generated_entry_proof_cache_hits: self
                .generated_direct_call_rootless_generated_entry_proof_cache_hits
                .saturating_sub(
                    start.generated_direct_call_rootless_generated_entry_proof_cache_hits,
                ),
            generated_direct_call_rootless_native_entries: self
                .generated_direct_call_rootless_native_entries
                .saturating_sub(start.generated_direct_call_rootless_native_entries),
            generated_direct_call_rootless_rejections: self
                .generated_direct_call_rootless_rejections
                .saturating_sub(start.generated_direct_call_rootless_rejections),
            generated_direct_call_rootless_native_entry_rejections: self
                .generated_direct_call_rootless_native_entry_rejections
                .saturating_sub(start.generated_direct_call_rootless_native_entry_rejections),
            generated_direct_call_rootless_unsupported_body_opcode_counts,
            generated_direct_call_rootless_native_entry_unsupported_body_opcode_counts,
            generated_direct_call_rootless_native_entry_retained_side_exit_counts,
            generated_direct_call_rootless_preferred_native_entry_counts,
            generated_direct_call_transaction_summaries,
            generated_direct_call_callee_fallback_summaries,
            generated_direct_call_route_opportunity_summaries,
            launch_descriptors: self
                .launch_descriptors
                .saturating_sub(start.launch_descriptors),
            call_observations: self
                .call_observations
                .saturating_sub(start.call_observations),
            call_link_boundary_validations: self
                .call_link_boundary_validations
                .saturating_sub(start.call_link_boundary_validations),
            call_link_inline_cache_attachments: self
                .call_link_inline_cache_attachments
                .saturating_sub(start.call_link_inline_cache_attachments),
            property_load_observations: self
                .property_load_observations
                .saturating_sub(start.property_load_observations),
            property_store_observations: self
                .property_store_observations
                .saturating_sub(start.property_store_observations),
            property_inline_cache_evolution_records: self
                .property_inline_cache_evolution_records
                .saturating_sub(start.property_inline_cache_evolution_records),
            property_inline_cache_evolution_admitted: self
                .property_inline_cache_evolution_admitted
                .saturating_sub(start.property_inline_cache_evolution_admitted),
            property_inline_cache_evolution_buffered: self
                .property_inline_cache_evolution_buffered
                .saturating_sub(start.property_inline_cache_evolution_buffered),
            property_inline_cache_evolution_buffered_duplicates: self
                .property_inline_cache_evolution_buffered_duplicates
                .saturating_sub(start.property_inline_cache_evolution_buffered_duplicates),
            property_inline_cache_evolution_cooldowns: self
                .property_inline_cache_evolution_cooldowns
                .saturating_sub(start.property_inline_cache_evolution_cooldowns),
            property_inline_cache_evolution_final_gave_up: self
                .property_inline_cache_evolution_final_gave_up
                .saturating_sub(start.property_inline_cache_evolution_final_gave_up),
            property_inline_cache_evolution_gave_up_skips: self
                .property_inline_cache_evolution_gave_up_skips
                .saturating_sub(start.property_inline_cache_evolution_gave_up_skips),
            property_inline_cache_evolution_generated_megamorphic_load: self
                .property_inline_cache_evolution_generated_megamorphic_load
                .saturating_sub(start.property_inline_cache_evolution_generated_megamorphic_load),
            property_inline_cache_evolution_megamorphic_load_skips: self
                .property_inline_cache_evolution_megamorphic_load_skips
                .saturating_sub(start.property_inline_cache_evolution_megamorphic_load_skips),
            property_inline_cache_evolution_generated_megamorphic_store: self
                .property_inline_cache_evolution_generated_megamorphic_store
                .saturating_sub(start.property_inline_cache_evolution_generated_megamorphic_store),
            property_inline_cache_evolution_megamorphic_store_skips: self
                .property_inline_cache_evolution_megamorphic_store_skips
                .saturating_sub(start.property_inline_cache_evolution_megamorphic_store_skips),
            property_inline_cache_evolution_generated_megamorphic_has: self
                .property_inline_cache_evolution_generated_megamorphic_has
                .saturating_sub(start.property_inline_cache_evolution_generated_megamorphic_has),
            property_inline_cache_evolution_megamorphic_has_skips: self
                .property_inline_cache_evolution_megamorphic_has_skips
                .saturating_sub(start.property_inline_cache_evolution_megamorphic_has_skips),
            property_load_megamorphic_cache_records: self
                .property_load_megamorphic_cache_records
                .saturating_sub(start.property_load_megamorphic_cache_records),
            property_store_megamorphic_cache_records: self
                .property_store_megamorphic_cache_records
                .saturating_sub(start.property_store_megamorphic_cache_records),
            property_has_megamorphic_cache_records: self
                .property_has_megamorphic_cache_records
                .saturating_sub(start.property_has_megamorphic_cache_records),
            property_inline_cache_attachments: self
                .property_inline_cache_attachments
                .saturating_sub(start.property_inline_cache_attachments),
        }
    }
}

fn octane_rootless_unsupported_body_opcode_count_delta(
    current: &[VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount],
    start: &[VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount],
) -> Vec<VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount> {
    current
        .iter()
        .filter_map(|current_count| {
            let start_count = start
                .iter()
                .find(|start_count| start_count.opcode == current_count.opcode)
                .map(|start_count| start_count.count)
                .unwrap_or(0);
            let count = current_count.count.saturating_sub(start_count);
            (count > 0).then_some(VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount {
                opcode: current_count.opcode,
                count,
            })
        })
        .collect()
}

fn octane_rootless_retained_side_exit_count_delta(
    current: &[VmGeneratedDirectCallRootlessRetainedSideExitCount],
    start: &[VmGeneratedDirectCallRootlessRetainedSideExitCount],
) -> Vec<VmGeneratedDirectCallRootlessRetainedSideExitCount> {
    current
        .iter()
        .filter_map(|current_count| {
            let start_count = start
                .iter()
                .find(|start_count| {
                    start_count.target_code_block == current_count.target_code_block
                        && start_count.bytecode_index == current_count.bytecode_index
                        && start_count.opcode == current_count.opcode
                        && start_count.reason == current_count.reason
                })
                .map(|start_count| start_count.count)
                .unwrap_or(0);
            let count = current_count.count.saturating_sub(start_count);
            (count > 0).then_some(VmGeneratedDirectCallRootlessRetainedSideExitCount {
                target_code_block: current_count.target_code_block,
                bytecode_index: current_count.bytecode_index,
                opcode: current_count.opcode,
                reason: current_count.reason,
                count,
            })
        })
        .collect()
}

fn octane_rootless_preferred_native_entry_counts_delta(
    current: VmGeneratedDirectCallRootlessPreferredNativeEntryCounts,
    start: VmGeneratedDirectCallRootlessPreferredNativeEntryCounts,
) -> VmGeneratedDirectCallRootlessPreferredNativeEntryCounts {
    VmGeneratedDirectCallRootlessPreferredNativeEntryCounts {
        pure_baseline_shim: current
            .pure_baseline_shim
            .saturating_sub(start.pure_baseline_shim),
        emitted_semantic_c_abi_entry: current
            .emitted_semantic_c_abi_entry
            .saturating_sub(start.emitted_semantic_c_abi_entry),
        unknown: current.unknown.saturating_sub(start.unknown),
    }
}

fn octane_baseline_generated_execution_summary_delta(
    current: &[VmBaselineGeneratedExecutionSummary],
    start: &[VmBaselineGeneratedExecutionSummary],
) -> Vec<VmBaselineGeneratedExecutionSummary> {
    current
        .iter()
        .filter_map(|current_summary| {
            let start_summary = start
                .iter()
                .find(|start_summary| start_summary.owner == current_summary.owner);
            let delta = VmBaselineGeneratedExecutionSummary {
                owner: current_summary.owner,
                execution_count: current_summary.execution_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.execution_count)
                        .unwrap_or(0),
                ),
                executed_bytecode_count: current_summary.executed_bytecode_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.executed_bytecode_count)
                        .unwrap_or(0),
                ),
                returned_count: current_summary.returned_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.returned_count)
                        .unwrap_or(0),
                ),
                threw_count: current_summary.threw_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.threw_count)
                        .unwrap_or(0),
                ),
                ordinary_bytecode_call_count: current_summary
                    .ordinary_bytecode_call_count
                    .saturating_sub(
                        start_summary
                            .map(|start_summary| start_summary.ordinary_bytecode_call_count)
                            .unwrap_or(0),
                    ),
                ordinary_bytecode_construct_count: current_summary
                    .ordinary_bytecode_construct_count
                    .saturating_sub(
                        start_summary
                            .map(|start_summary| start_summary.ordinary_bytecode_construct_count)
                            .unwrap_or(0),
                    ),
                function_value_call_count: current_summary
                    .function_value_call_count
                    .saturating_sub(
                        start_summary
                            .map(|start_summary| start_summary.function_value_call_count)
                            .unwrap_or(0),
                    ),
                terminated_count: current_summary.terminated_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.terminated_count)
                        .unwrap_or(0),
                ),
                suspended_count: current_summary.suspended_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.suspended_count)
                        .unwrap_or(0),
                ),
                failed_count: current_summary.failed_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.failed_count)
                        .unwrap_or(0),
                ),
                fallback_count: current_summary.fallback_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.fallback_count)
                        .unwrap_or(0),
                ),
                js_call_count: current_summary.js_call_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.js_call_count)
                        .unwrap_or(0),
                ),
                property_count: current_summary.property_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.property_count)
                        .unwrap_or(0),
                ),
                runtime_helper_count: current_summary.runtime_helper_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.runtime_helper_count)
                        .unwrap_or(0),
                ),
                rejected_count: current_summary.rejected_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.rejected_count)
                        .unwrap_or(0),
                ),
            };
            octane_baseline_generated_execution_summary_has_activity(delta).then_some(delta)
        })
        .collect()
}

fn octane_baseline_generated_execution_summary_has_activity(
    summary: VmBaselineGeneratedExecutionSummary,
) -> bool {
    summary.execution_count > 0
        || summary.executed_bytecode_count > 0
        || summary.returned_count > 0
        || summary.threw_count > 0
        || summary.ordinary_bytecode_call_count > 0
        || summary.ordinary_bytecode_construct_count > 0
        || summary.function_value_call_count > 0
        || summary.terminated_count > 0
        || summary.suspended_count > 0
        || summary.failed_count > 0
        || summary.fallback_count > 0
        || summary.js_call_count > 0
        || summary.property_count > 0
        || summary.runtime_helper_count > 0
        || summary.rejected_count > 0
}

fn octane_baseline_generated_dispatched_opcode_count_delta(
    current: &[VmBaselineGeneratedDispatchedOpcodeCount],
    start: &[VmBaselineGeneratedDispatchedOpcodeCount],
) -> Vec<VmBaselineGeneratedDispatchedOpcodeCount> {
    current
        .iter()
        .filter_map(|current_count| {
            let start_count = start
                .iter()
                .find(|start_count| {
                    start_count.owner == current_count.owner
                        && start_count.opcode == current_count.opcode
                })
                .map(|start_count| start_count.count)
                .unwrap_or(0);
            let count = current_count.count.saturating_sub(start_count);
            (count > 0).then_some(VmBaselineGeneratedDispatchedOpcodeCount {
                owner: current_count.owner,
                opcode: current_count.opcode,
                count,
            })
        })
        .collect()
}

fn octane_baseline_generated_dispatched_site_opcode_count_delta(
    current: &[VmBaselineGeneratedDispatchedSiteOpcodeCount],
    start: &[VmBaselineGeneratedDispatchedSiteOpcodeCount],
) -> Vec<VmBaselineGeneratedDispatchedSiteOpcodeCount> {
    current
        .iter()
        .filter_map(|current_count| {
            let start_count = start
                .iter()
                .find(|start_count| {
                    start_count.owner == current_count.owner
                        && start_count.bytecode_index == current_count.bytecode_index
                        && start_count.opcode == current_count.opcode
                        && start_count.property_load_sidecar_readiness
                            == current_count.property_load_sidecar_readiness
                })
                .map(|start_count| start_count.count)
                .unwrap_or(0);
            let count = current_count.count.saturating_sub(start_count);
            (count > 0).then_some(VmBaselineGeneratedDispatchedSiteOpcodeCount {
                owner: current_count.owner,
                bytecode_index: current_count.bytecode_index,
                opcode: current_count.opcode,
                property_load_sidecar_readiness: current_count.property_load_sidecar_readiness,
                count,
            })
        })
        .collect()
}

fn octane_generated_direct_call_transaction_summary_delta(
    current: &[VmGeneratedDirectCallTransactionSummary],
    start: &[VmGeneratedDirectCallTransactionSummary],
) -> Vec<VmGeneratedDirectCallTransactionSummary> {
    current
        .iter()
        .filter_map(|current_summary| {
            let start_summary = start.iter().find(|start_summary| {
                start_summary.caller == current_summary.caller
                    && start_summary.call_bytecode_index == current_summary.call_bytecode_index
                    && start_summary.target_code_block == current_summary.target_code_block
                    && start_summary.argument_count_including_this
                        == current_summary.argument_count_including_this
                    && start_summary.route == current_summary.route
            });
            let delta = VmGeneratedDirectCallTransactionSummary {
                caller: current_summary.caller,
                call_bytecode_index: current_summary.call_bytecode_index,
                target_code_block: current_summary.target_code_block,
                argument_count_including_this: current_summary.argument_count_including_this,
                route: current_summary.route,
                transaction_count: current_summary.transaction_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.transaction_count)
                        .unwrap_or(0),
                ),
                continue_count: current_summary.continue_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.continue_count)
                        .unwrap_or(0),
                ),
                jump_count: current_summary.jump_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.jump_count)
                        .unwrap_or(0),
                ),
                return_count: current_summary.return_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.return_count)
                        .unwrap_or(0),
                ),
                threw_count: current_summary.threw_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.threw_count)
                        .unwrap_or(0),
                ),
                ordinary_bytecode_call_count: current_summary
                    .ordinary_bytecode_call_count
                    .saturating_sub(
                        start_summary
                            .map(|start_summary| start_summary.ordinary_bytecode_call_count)
                            .unwrap_or(0),
                    ),
                ordinary_bytecode_construct_count: current_summary
                    .ordinary_bytecode_construct_count
                    .saturating_sub(
                        start_summary
                            .map(|start_summary| start_summary.ordinary_bytecode_construct_count)
                            .unwrap_or(0),
                    ),
                function_value_call_count: current_summary
                    .function_value_call_count
                    .saturating_sub(
                        start_summary
                            .map(|start_summary| start_summary.function_value_call_count)
                            .unwrap_or(0),
                    ),
                suspended_count: current_summary.suspended_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.suspended_count)
                        .unwrap_or(0),
                ),
                failed_count: current_summary.failed_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.failed_count)
                        .unwrap_or(0),
                ),
            };
            octane_generated_direct_call_transaction_summary_has_activity(delta).then_some(delta)
        })
        .collect()
}

fn octane_generated_direct_call_transaction_summary_has_activity(
    summary: VmGeneratedDirectCallTransactionSummary,
) -> bool {
    summary.transaction_count > 0
        || summary.continue_count > 0
        || summary.jump_count > 0
        || summary.return_count > 0
        || summary.threw_count > 0
        || summary.ordinary_bytecode_call_count > 0
        || summary.ordinary_bytecode_construct_count > 0
        || summary.function_value_call_count > 0
        || summary.suspended_count > 0
        || summary.failed_count > 0
}

fn octane_generated_direct_call_callee_fallback_summary_delta(
    current: &[VmGeneratedDirectCallCalleeFallbackSummary],
    start: &[VmGeneratedDirectCallCalleeFallbackSummary],
) -> Vec<VmGeneratedDirectCallCalleeFallbackSummary> {
    current
        .iter()
        .filter_map(|current_summary| {
            let start_summary = start.iter().find(|start_summary| {
                start_summary.caller == current_summary.caller
                    && start_summary.call_bytecode_index == current_summary.call_bytecode_index
                    && start_summary.target_code_block == current_summary.target_code_block
                    && start_summary.argument_count_including_this
                        == current_summary.argument_count_including_this
                    && start_summary.preferred_route == current_summary.preferred_route
                    && start_summary.generated_entry_miss == current_summary.generated_entry_miss
                    && start_summary.native_entry_miss == current_summary.native_entry_miss
            });
            let delta = VmGeneratedDirectCallCalleeFallbackSummary {
                caller: current_summary.caller,
                call_bytecode_index: current_summary.call_bytecode_index,
                target_code_block: current_summary.target_code_block,
                argument_count_including_this: current_summary.argument_count_including_this,
                preferred_route: current_summary.preferred_route,
                generated_entry_miss: current_summary.generated_entry_miss,
                native_entry_miss: current_summary.native_entry_miss,
                fallback_count: current_summary.fallback_count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.fallback_count)
                        .unwrap_or(0),
                ),
            };
            (delta.fallback_count > 0).then_some(delta)
        })
        .collect()
}

fn octane_generated_direct_call_route_opportunity_summary_delta(
    current: &[VmGeneratedDirectCallRouteOpportunitySummary],
    start: &[VmGeneratedDirectCallRouteOpportunitySummary],
) -> Vec<VmGeneratedDirectCallRouteOpportunitySummary> {
    current
        .iter()
        .filter_map(|current_summary| {
            let start_summary = start.iter().find(|start_summary| {
                start_summary.caller == current_summary.caller
                    && start_summary.call_bytecode_index == current_summary.call_bytecode_index
                    && start_summary.target_code_block == current_summary.target_code_block
                    && start_summary.argument_count_including_this
                        == current_summary.argument_count_including_this
                    && start_summary.selected_route == current_summary.selected_route
                    && start_summary.preferred_route == current_summary.preferred_route
                    && start_summary.native_entry_miss == current_summary.native_entry_miss
            });
            let delta = VmGeneratedDirectCallRouteOpportunitySummary {
                caller: current_summary.caller,
                call_bytecode_index: current_summary.call_bytecode_index,
                target_code_block: current_summary.target_code_block,
                argument_count_including_this: current_summary.argument_count_including_this,
                selected_route: current_summary.selected_route,
                preferred_route: current_summary.preferred_route,
                native_entry_miss: current_summary.native_entry_miss,
                count: current_summary.count.saturating_sub(
                    start_summary
                        .map(|start_summary| start_summary.count)
                        .unwrap_or(0),
                ),
            };
            (delta.count > 0).then_some(delta)
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OctaneExecutionProgress {
    BenchmarkStarted {
        benchmark: &'static str,
        mode: OctaneExecutionMode,
    },
    SourceSessionStarted {
        benchmark: &'static str,
        mode: OctaneExecutionMode,
    },
    SourceSessionOpened {
        benchmark: &'static str,
        mode: OctaneExecutionMode,
    },
    SourceSessionErrored {
        benchmark: &'static str,
        mode: OctaneExecutionMode,
        error: SourceExecutionError,
    },
    SourceStarted {
        benchmark: &'static str,
        mode: OctaneExecutionMode,
        order_index: usize,
        order_entry: OctanePreparedSourceOrderEntry,
        label: String,
    },
    SourceCompleted {
        benchmark: &'static str,
        mode: OctaneExecutionMode,
        order_index: usize,
        order_entry: OctanePreparedSourceOrderEntry,
        label: String,
        completion: ExecutionCompletion,
    },
    SourceErrored {
        benchmark: &'static str,
        mode: OctaneExecutionMode,
        order_index: usize,
        order_entry: OctanePreparedSourceOrderEntry,
        label: String,
        error: SourceExecutionError,
    },
    ScoreTelemetryStarted {
        benchmark: &'static str,
        mode: OctaneExecutionMode,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OctaneSourceExecutionRecord {
    pub order_index: usize,
    pub order_entry: OctanePreparedSourceOrderEntry,
    pub label: String,
    pub completion: Option<ExecutionCompletion>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OctaneSuiteScoreRecord {
    pub suite: OctaneSuite,
    pub mode: OctaneExecutionMode,
    pub benchmark_scores: Vec<OctaneSuiteBenchmarkScoreRecord>,
    pub score: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OctaneSuiteBenchmarkScoreRecord {
    pub benchmark: &'static str,
    pub score: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OctanePreparedSuiteModeComparisonReport {
    pub suite: OctaneSuite,
    pub failure_policy: OctaneSuiteFailurePolicy,
    pub benchmarks: Vec<OctaneBenchmarkModeComparisonRecord>,
    pub stopped_early: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OctaneBenchmarkModeComparisonRecord {
    pub benchmark: &'static str,
    pub interpreter_only: OctaneBenchmarkExecutionReport,
    pub baseline_allowed: OctaneBenchmarkExecutionReport,
}

#[derive(Debug)]
struct OctaneBenchmarkExecutionRun {
    report: OctaneBenchmarkExecutionReport,
    retained_session: Option<SourceSessionHandle>,
}

#[derive(Debug)]
struct OctaneBenchmarkModeComparisonRun {
    record: OctaneBenchmarkModeComparisonRecord,
    interpreter_session: Option<SourceSessionHandle>,
    baseline_session: Option<SourceSessionHandle>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctaneExecutionPhase {
    Parse,
    BytecodeEmit,
    SessionLink,
    ExecuteRuntime,
    ThrownOrOracle,
    ScoreTelemetry,
    BaselineOnly,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OctaneExecutionOutcome {
    Succeeded(OctaneBenchmarkExecutionSuccess),
    Failed(OctaneExecutionFailure),
}

impl OctaneExecutionOutcome {
    pub const fn phase(&self) -> OctaneExecutionPhase {
        match self {
            Self::Succeeded(_) => OctaneExecutionPhase::ScoreTelemetry,
            Self::Failed(failure) => failure.phase,
        }
    }

    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Succeeded(_))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OctaneBenchmarkExecutionSuccess {
    pub telemetry: OctaneDefaultBenchmarkTelemetry,
    pub scores: OctaneDefaultBenchmarkScores,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OctaneDefaultBenchmarkTelemetry {
    pub elapsed_times_ms: Vec<f64>,
    pub validation_state: OctaneBenchmarkValidationState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctaneBenchmarkValidationState {
    NotRun,
    Passed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OctaneExecutionFailure {
    pub phase: OctaneExecutionPhase,
    pub order_index: Option<usize>,
    pub order_entry: Option<OctanePreparedSourceOrderEntry>,
    pub label: Option<String>,
    pub detail: OctaneExecutionFailureDetail,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OctaneExecutionFailureDetail {
    SourceExecutionError(SourceExecutionError),
    Completion(ExecutionCompletion),
    MissingPreparedSource {
        entry: OctanePreparedSourceOrderEntry,
    },
    OracleAlert(CoreHostOutputRecord),
    ScoreTelemetry(OctaneScoreTelemetryError),
}

#[derive(Clone, Debug, PartialEq)]
pub enum OctaneScoreTelemetryError {
    Missing,
    Duplicate {
        count: usize,
    },
    Malformed {
        text: String,
    },
    UnexpectedBenchmark {
        expected: &'static str,
        actual: String,
    },
    InvalidIterationCount {
        value: String,
    },
    UnexpectedIterationCount {
        expected: usize,
        actual: usize,
    },
    InvalidValidationState {
        value: String,
    },
    InvalidElapsedTime {
        index: usize,
        value: String,
    },
    NonFiniteElapsedTime {
        index: usize,
        value: String,
    },
    ElapsedTimeOutOfRange {
        index: usize,
        value: String,
    },
    Scoring(OctaneScoringError),
    NonFiniteScore {
        component: OctaneScoreComponent,
        value: String,
    },
    RejectedResult {
        reason: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctaneScoreComponent {
    FirstIteration,
    WorstCase,
    Average,
    Score,
}

#[derive(Clone, Debug)]
pub struct OctanePreparedSuite {
    pub config: OctanePreparationConfig,
    pub benchmarks: Vec<OctanePreparedBenchmark>,
}

#[derive(Clone, Debug)]
pub struct OctanePreparedBenchmark {
    pub plan: &'static OctaneBenchmarkPlan,
    pub run_config: OctaneDefaultBenchmarkRunConfig,
    pub benchmark_sources: Vec<OctanePreparedBenchmarkSource>,
    pub generated_sources: Vec<OctanePreparedGeneratedSource>,
    pub source_order: Vec<OctanePreparedSourceOrderEntry>,
}

impl OctanePreparedBenchmark {
    pub fn generated_source(
        &self,
        kind: OctanePreparedGeneratedSourceKind,
    ) -> Option<&OctanePreparedGeneratedSource> {
        self.generated_sources
            .iter()
            .find(|source| source.kind == kind)
    }
}

#[derive(Clone, Debug)]
pub struct OctanePreparedBenchmarkSource {
    pub manifest_path: &'static str,
    pub resolved_path: PathBuf,
    pub canonical_path: PathBuf,
    pub label: String,
    pub text: String,
    pub source: SourceSessionSource,
    pub provider_id: SourceProviderId,
    pub origin_id: SourceOriginId,
}

#[derive(Clone, Debug)]
pub struct OctanePreparedGeneratedSource {
    pub kind: OctanePreparedGeneratedSourceKind,
    pub label: String,
    pub text: String,
    pub source: SourceSessionSource,
    pub provider_id: SourceProviderId,
    pub origin_id: SourceOriginId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctanePreparedGeneratedSourceKind {
    Prelude,
    DeterministicRandom,
    Runner,
}

impl OctanePreparedGeneratedSourceKind {
    fn label_component(self) -> &'static str {
        match self {
            Self::Prelude => "prelude",
            Self::DeterministicRandom => "deterministic-random",
            Self::Runner => "runner",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctanePreparedSourceOrderEntry {
    Generated(OctanePreparedGeneratedSourceKind),
    BenchmarkFile(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctanePreparationPhase {
    BenchmarkLookup,
    Config,
    SourceLoad,
    SourceAppend,
    GeneratedSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OctanePreparationError {
    BenchmarkLookup {
        suite: OctaneSuite,
        error: OctaneManifestError,
    },
    Config {
        benchmark: &'static str,
        error: OctaneScoringError,
    },
    SourceLoad {
        benchmark: &'static str,
        manifest_path: &'static str,
        resolved_path: PathBuf,
        error: ShellSourceLoadError,
    },
    SourceLoadMissingCanonicalPath {
        benchmark: &'static str,
        manifest_path: &'static str,
        resolved_path: PathBuf,
    },
    SourceAppend {
        benchmark: &'static str,
        generated_kind: OctanePreparedGeneratedSourceKind,
        label: String,
        error: ShellSourceLoadError,
    },
    GeneratedSource {
        benchmark: &'static str,
        generated_kind: OctanePreparedGeneratedSourceKind,
        error: OctaneGeneratedSourceError,
    },
}

impl OctanePreparationError {
    pub const fn phase(&self) -> OctanePreparationPhase {
        match self {
            Self::BenchmarkLookup { .. } => OctanePreparationPhase::BenchmarkLookup,
            Self::Config { .. } => OctanePreparationPhase::Config,
            Self::SourceLoad { .. } | Self::SourceLoadMissingCanonicalPath { .. } => {
                OctanePreparationPhase::SourceLoad
            }
            Self::SourceAppend { .. } => OctanePreparationPhase::SourceAppend,
            Self::GeneratedSource { .. } => OctanePreparationPhase::GeneratedSource,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OctaneGeneratedSourceError {
    UnsupportedBenchmarkClass {
        benchmark_class: OctaneBenchmarkClass,
    },
}

pub fn prepare_octane_suite(
    config: OctanePreparationConfig,
) -> Result<OctanePreparedSuite, OctanePreparationError> {
    let plans = config.run.suite.resolved_plans().map_err(|error| {
        OctanePreparationError::BenchmarkLookup {
            suite: config.run.suite,
            error,
        }
    })?;
    let mut loader = ShellSourceLoader::default();
    let mut benchmarks = Vec::with_capacity(plans.len());

    for plan in plans {
        benchmarks.push(prepare_octane_benchmark_with_loader(
            &mut loader,
            &config.jetstream_root,
            plan,
            config.run.effective_overrides(),
        )?);
    }

    Ok(OctanePreparedSuite { config, benchmarks })
}

pub fn prepare_octane_benchmark(
    jetstream_root: impl AsRef<Path>,
    plan: &'static OctaneBenchmarkPlan,
    overrides: OctaneBenchmarkRunOverrides,
) -> Result<OctanePreparedBenchmark, OctanePreparationError> {
    let mut loader = ShellSourceLoader::default();
    prepare_octane_benchmark_with_loader(&mut loader, jetstream_root, plan, overrides)
}

fn prepare_octane_benchmark_with_loader(
    loader: &mut ShellSourceLoader,
    jetstream_root: impl AsRef<Path>,
    plan: &'static OctaneBenchmarkPlan,
    overrides: OctaneBenchmarkRunOverrides,
) -> Result<OctanePreparedBenchmark, OctanePreparationError> {
    let jetstream_root = jetstream_root.as_ref();
    let run_config =
        plan.run_config(overrides)
            .map_err(|error| OctanePreparationError::Config {
                benchmark: plan.name,
                error,
            })?;
    let mut benchmark_sources = Vec::with_capacity(plan.files.len());

    for manifest_path in plan.files {
        let resolved_path = jetstream_root.join(manifest_path);
        let loaded = loader
            .load_file_source(
                &resolved_path,
                ShellSourceKind::File,
                ShellMode::ScriptFile,
                false,
            )
            .map_err(|error| OctanePreparationError::SourceLoad {
                benchmark: plan.name,
                manifest_path,
                resolved_path: resolved_path.clone(),
                error,
            })?;
        let canonical_path = loaded
            .provider_record()
            .canonical_path
            .clone()
            .ok_or_else(|| OctanePreparationError::SourceLoadMissingCanonicalPath {
                benchmark: plan.name,
                manifest_path,
                resolved_path: resolved_path.clone(),
            })?;
        let text = source_text_to_string(loaded.source().provider().text());
        let provider_id = loaded.provider_id();
        let origin_id = loaded.origin_id();
        benchmark_sources.push(OctanePreparedBenchmarkSource {
            manifest_path,
            resolved_path,
            canonical_path,
            label: format!("{}:{}", plan.name, manifest_path),
            text,
            source: SourceSessionSource::with_provenance(
                loaded.source_code(),
                provider_id,
                origin_id,
            ),
            provider_id,
            origin_id,
        });
    }

    let mut generated_sources = Vec::new();
    generated_sources.push(append_generated_source(
        loader,
        plan,
        OctanePreparedGeneratedSourceKind::Prelude,
        generate_octane_prelude_source(),
    )?);
    if plan.deterministic_random {
        generated_sources.push(append_generated_source(
            loader,
            plan,
            OctanePreparedGeneratedSourceKind::DeterministicRandom,
            generate_octane_deterministic_random_source(),
        )?);
    }
    generated_sources.push(append_generated_source(
        loader,
        plan,
        OctanePreparedGeneratedSourceKind::Runner,
        generate_octane_runner_source(plan, run_config).map_err(|error| {
            OctanePreparationError::GeneratedSource {
                benchmark: plan.name,
                generated_kind: OctanePreparedGeneratedSourceKind::Runner,
                error,
            }
        })?,
    )?);

    let mut source_order = Vec::with_capacity(benchmark_sources.len() + generated_sources.len());
    source_order.push(OctanePreparedSourceOrderEntry::Generated(
        OctanePreparedGeneratedSourceKind::Prelude,
    ));
    if plan.deterministic_random {
        source_order.push(OctanePreparedSourceOrderEntry::Generated(
            OctanePreparedGeneratedSourceKind::DeterministicRandom,
        ));
    }
    source_order.extend(
        benchmark_sources
            .iter()
            .enumerate()
            .map(|(index, _)| OctanePreparedSourceOrderEntry::BenchmarkFile(index)),
    );
    source_order.push(OctanePreparedSourceOrderEntry::Generated(
        OctanePreparedGeneratedSourceKind::Runner,
    ));

    Ok(OctanePreparedBenchmark {
        plan,
        run_config,
        benchmark_sources,
        generated_sources,
        source_order,
    })
}

pub fn execute_prepared_octane_benchmark(
    prepared: &OctanePreparedBenchmark,
    config: OctaneExecutionConfig,
) -> OctaneBenchmarkExecutionReport {
    // S7 (baseline-dispatch.md): own the dispatch-site `Vm` as `Box<Vm>` so the
    // baked `jit_pending` AbsoluteAddress + the parked `*mut Vm` the baseline JIT
    // reuses stay valid at ONE heap address for an installed image's lifetime
    // (JSC's `VM` is heap-allocated). The `&mut *vm` deref hands the helpers the
    // same `&mut Vm` interface.
    let mut vm: Box<Vm> = Box::new(Vm::new(config.vm_config()));
    let mut progress = None;
    execute_prepared_octane_benchmark_run_with_vm(&mut vm, prepared, config, &mut progress).report
}

pub fn execute_prepared_octane_benchmark_with_progress(
    prepared: &OctanePreparedBenchmark,
    config: OctaneExecutionConfig,
    progress: &mut dyn FnMut(OctaneExecutionProgress),
) -> OctaneBenchmarkExecutionReport {
    // S7: pin-stable boxed `Vm` (see `execute_prepared_octane_benchmark`).
    let mut vm: Box<Vm> = Box::new(Vm::new(config.vm_config()));
    let mut progress = Some(progress);
    execute_prepared_octane_benchmark_run_with_vm(&mut vm, prepared, config, &mut progress).report
}

fn execute_prepared_octane_benchmark_run_with_vm(
    vm: &mut Vm,
    prepared: &OctanePreparedBenchmark,
    config: OctaneExecutionConfig,
    progress: &mut Option<&mut dyn FnMut(OctaneExecutionProgress)>,
) -> OctaneBenchmarkExecutionRun {
    let mut source_records = Vec::with_capacity(prepared.source_order.len());
    let tiering_start = OctaneTieringSummary::from_vm(vm);
    emit_octane_execution_progress(
        progress,
        OctaneExecutionProgress::BenchmarkStarted {
            benchmark: prepared.plan.name,
            mode: config.mode,
        },
    );
    emit_octane_execution_progress(
        progress,
        OctaneExecutionProgress::SourceSessionStarted {
            benchmark: prepared.plan.name,
            mode: config.mode,
        },
    );
    let mut session = match vm.open_source_session_with_host_globals_and_dispatch_config(
        SourceSessionHostGlobalConfig::safe_benchmark_host_globals(),
        config.dispatch_config,
    ) {
        Ok(session) => {
            emit_octane_execution_progress(
                progress,
                OctaneExecutionProgress::SourceSessionOpened {
                    benchmark: prepared.plan.name,
                    mode: config.mode,
                },
            );
            session
        }
        Err(error) => {
            emit_octane_execution_progress(
                progress,
                OctaneExecutionProgress::SourceSessionErrored {
                    benchmark: prepared.plan.name,
                    mode: config.mode,
                    error: error.clone(),
                },
            );
            return OctaneBenchmarkExecutionRun {
                report: OctaneBenchmarkExecutionReport {
                    benchmark: prepared.plan.name,
                    mode: config.mode,
                    run_config: prepared.run_config,
                    source_records,
                    host_output_records: Vec::new(),
                    tiering_delta: OctaneTieringSummary::from_vm(vm).delta_since(tiering_start),
                    outcome: OctaneExecutionOutcome::Failed(classify_source_execution_error(
                        config.mode,
                        None,
                        None,
                        None,
                        error,
                    )),
                },
                retained_session: None,
            };
        }
    };

    for (order_index, order_entry) in prepared.source_order.iter().copied().enumerate() {
        let Some(source) = prepared_source_for_order_entry(prepared, order_entry) else {
            return OctaneBenchmarkExecutionRun {
                report: OctaneBenchmarkExecutionReport {
                    benchmark: prepared.plan.name,
                    mode: config.mode,
                    run_config: prepared.run_config,
                    source_records,
                    host_output_records: session.host_output_records().to_vec(),
                    tiering_delta: OctaneTieringSummary::from_vm(vm).delta_since(tiering_start),
                    outcome: OctaneExecutionOutcome::Failed(OctaneExecutionFailure {
                        phase: OctaneExecutionPhase::SessionLink,
                        order_index: Some(order_index),
                        order_entry: Some(order_entry),
                        label: None,
                        detail: OctaneExecutionFailureDetail::MissingPreparedSource {
                            entry: order_entry,
                        },
                    }),
                },
                retained_session: Some(session),
            };
        };

        let label = source.label.to_string();
        emit_octane_execution_progress(
            progress,
            OctaneExecutionProgress::SourceStarted {
                benchmark: prepared.plan.name,
                mode: config.mode,
                order_index,
                order_entry,
                label: label.clone(),
            },
        );
        let mut source_record = OctaneSourceExecutionRecord {
            order_index,
            order_entry,
            label: label.clone(),
            completion: None,
        };

        match vm.append_source_session_source(&mut session, source.source.clone()) {
            Ok(completion) => {
                emit_octane_execution_progress(
                    progress,
                    OctaneExecutionProgress::SourceCompleted {
                        benchmark: prepared.plan.name,
                        mode: config.mode,
                        order_index,
                        order_entry,
                        label: label.clone(),
                        completion: completion.clone(),
                    },
                );
                source_record.completion = Some(completion.clone());
                source_records.push(source_record);
                if let Some(failure) =
                    classify_completion(config.mode, order_index, order_entry, label, completion)
                {
                    return OctaneBenchmarkExecutionRun {
                        report: OctaneBenchmarkExecutionReport {
                            benchmark: prepared.plan.name,
                            mode: config.mode,
                            run_config: prepared.run_config,
                            source_records,
                            host_output_records: session.host_output_records().to_vec(),
                            tiering_delta: OctaneTieringSummary::from_vm(vm)
                                .delta_since(tiering_start),
                            outcome: OctaneExecutionOutcome::Failed(failure),
                        },
                        retained_session: Some(session),
                    };
                }
            }
            Err(error) => {
                emit_octane_execution_progress(
                    progress,
                    OctaneExecutionProgress::SourceErrored {
                        benchmark: prepared.plan.name,
                        mode: config.mode,
                        order_index,
                        order_entry,
                        label: label.clone(),
                        error: error.clone(),
                    },
                );
                source_records.push(source_record);
                return OctaneBenchmarkExecutionRun {
                    report: OctaneBenchmarkExecutionReport {
                        benchmark: prepared.plan.name,
                        mode: config.mode,
                        run_config: prepared.run_config,
                        source_records,
                        host_output_records: session.host_output_records().to_vec(),
                        tiering_delta: OctaneTieringSummary::from_vm(vm).delta_since(tiering_start),
                        outcome: OctaneExecutionOutcome::Failed(classify_source_execution_error(
                            config.mode,
                            Some(order_index),
                            Some(order_entry),
                            Some(label),
                            error,
                        )),
                    },
                    retained_session: Some(session),
                };
            }
        }
    }

    emit_octane_execution_progress(
        progress,
        OctaneExecutionProgress::ScoreTelemetryStarted {
            benchmark: prepared.plan.name,
            mode: config.mode,
        },
    );
    OctaneBenchmarkExecutionRun {
        report: OctaneBenchmarkExecutionReport {
            benchmark: prepared.plan.name,
            mode: config.mode,
            run_config: prepared.run_config,
            source_records,
            host_output_records: session.host_output_records().to_vec(),
            tiering_delta: OctaneTieringSummary::from_vm(vm).delta_since(tiering_start),
            outcome: match extract_octane_benchmark_success(
                prepared,
                session.host_result_records(),
                session.host_output_records(),
            ) {
                Ok(success) => OctaneExecutionOutcome::Succeeded(success),
                Err(failure) => OctaneExecutionOutcome::Failed(failure),
            },
        },
        retained_session: Some(session),
    }
}

pub fn execute_prepared_octane_suite(
    prepared: &OctanePreparedSuite,
    config: OctaneExecutionConfig,
) -> OctanePreparedSuiteExecutionReport {
    execute_prepared_octane_suite_with_optional_progress(prepared, config, None)
}

pub fn execute_prepared_octane_suite_with_progress(
    prepared: &OctanePreparedSuite,
    config: OctaneExecutionConfig,
    progress: &mut dyn FnMut(OctaneExecutionProgress),
) -> OctanePreparedSuiteExecutionReport {
    execute_prepared_octane_suite_with_optional_progress(prepared, config, Some(progress))
}

fn execute_prepared_octane_suite_with_optional_progress(
    prepared: &OctanePreparedSuite,
    config: OctaneExecutionConfig,
    progress: Option<&mut dyn FnMut(OctaneExecutionProgress)>,
) -> OctanePreparedSuiteExecutionReport {
    let mut progress = progress;
    // S7: pin-stable boxed `Vm` (see `execute_prepared_octane_benchmark`).
    let mut vm: Box<Vm> = Box::new(Vm::new(config.vm_config()));
    let mut retained_sessions = Vec::new();
    let mut benchmarks = Vec::with_capacity(prepared.benchmarks.len());
    let mut stopped_early = false;

    for benchmark in octane_driver_benchmark_execution_order(&prepared.benchmarks) {
        let run = execute_prepared_octane_benchmark_run_with_vm(
            &mut vm,
            benchmark,
            config,
            &mut progress,
        );
        if let Some(session) = run.retained_session {
            retained_sessions.push(session);
        }
        let report = run.report;
        let should_stop = config.failure_policy == OctaneSuiteFailurePolicy::FailFast
            && !report.outcome.is_success();
        benchmarks.push(report);
        if should_stop {
            stopped_early = true;
            break;
        }
    }

    OctanePreparedSuiteExecutionReport {
        suite: prepared.config.run.suite,
        mode: config.mode,
        failure_policy: config.failure_policy,
        suite_score: octane_suite_score_record(prepared.config.run.suite, config.mode, &benchmarks),
        benchmarks,
        stopped_early,
    }
}

fn emit_octane_execution_progress(
    progress: &mut Option<&mut dyn FnMut(OctaneExecutionProgress)>,
    event: OctaneExecutionProgress,
) {
    if let Some(progress) = progress.as_deref_mut() {
        progress(event);
    }
}

pub fn execute_prepared_octane_benchmark_mode_comparison(
    prepared: &OctanePreparedBenchmark,
    failure_policy: OctaneSuiteFailurePolicy,
) -> OctaneBenchmarkModeComparisonRecord {
    // S7: both A/B harness Vms are pin-stable boxed homes — the baseline_vm runs
    // the JIT once the live native path is wired, so its parked `*mut Vm` + baked
    // `jit_pending` address must not move.
    let mut interpreter_vm: Box<Vm> =
        Box::new(Vm::new(OctaneExecutionMode::InterpreterOnly.vm_config()));
    let mut baseline_vm: Box<Vm> =
        Box::new(Vm::new(OctaneExecutionMode::BaselineAllowed.vm_config()));
    execute_prepared_octane_benchmark_mode_comparison_run_with_vms(
        &mut interpreter_vm,
        &mut baseline_vm,
        prepared,
        failure_policy,
    )
    .record
}

fn execute_prepared_octane_benchmark_mode_comparison_run_with_vms(
    interpreter_vm: &mut Vm,
    baseline_vm: &mut Vm,
    prepared: &OctanePreparedBenchmark,
    failure_policy: OctaneSuiteFailurePolicy,
) -> OctaneBenchmarkModeComparisonRun {
    let mut no_progress = None;
    let interpreter_run = execute_prepared_octane_benchmark_run_with_vm(
        interpreter_vm,
        prepared,
        OctaneExecutionConfig::new(OctaneExecutionMode::InterpreterOnly, failure_policy),
        &mut no_progress,
    );
    let baseline_run = execute_prepared_octane_benchmark_run_with_vm(
        baseline_vm,
        prepared,
        OctaneExecutionConfig::new(OctaneExecutionMode::BaselineAllowed, failure_policy),
        &mut no_progress,
    );

    OctaneBenchmarkModeComparisonRun {
        record: OctaneBenchmarkModeComparisonRecord {
            benchmark: prepared.plan.name,
            interpreter_only: interpreter_run.report,
            baseline_allowed: baseline_run.report,
        },
        interpreter_session: interpreter_run.retained_session,
        baseline_session: baseline_run.retained_session,
    }
}

pub fn execute_prepared_octane_suite_mode_comparison(
    prepared: &OctanePreparedSuite,
    failure_policy: OctaneSuiteFailurePolicy,
) -> OctanePreparedSuiteModeComparisonReport {
    // S7: pin-stable boxed A/B harness Vms (see the per-benchmark comparison).
    let mut interpreter_vm: Box<Vm> =
        Box::new(Vm::new(OctaneExecutionMode::InterpreterOnly.vm_config()));
    let mut baseline_vm: Box<Vm> =
        Box::new(Vm::new(OctaneExecutionMode::BaselineAllowed.vm_config()));
    let mut interpreter_sessions = Vec::new();
    let mut baseline_sessions = Vec::new();
    let mut benchmarks = Vec::with_capacity(prepared.benchmarks.len());
    let mut stopped_early = false;

    for benchmark in octane_driver_benchmark_execution_order(&prepared.benchmarks) {
        let run = execute_prepared_octane_benchmark_mode_comparison_run_with_vms(
            &mut interpreter_vm,
            &mut baseline_vm,
            benchmark,
            failure_policy,
        );
        if let Some(session) = run.interpreter_session {
            interpreter_sessions.push(session);
        }
        if let Some(session) = run.baseline_session {
            baseline_sessions.push(session);
        }
        let record = run.record;
        let should_stop = failure_policy == OctaneSuiteFailurePolicy::FailFast
            && (!record.interpreter_only.outcome.is_success()
                || !record.baseline_allowed.outcome.is_success());
        benchmarks.push(record);
        if should_stop {
            stopped_early = true;
            break;
        }
    }

    OctanePreparedSuiteModeComparisonReport {
        suite: prepared.config.run.suite,
        failure_policy,
        benchmarks,
        stopped_early,
    }
}

fn octane_driver_benchmark_execution_order(
    benchmarks: &[OctanePreparedBenchmark],
) -> Vec<&OctanePreparedBenchmark> {
    let mut ordered = benchmarks.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        reverse_case_insensitive_octane_name_order(left.plan.name, right.plan.name)
    });
    ordered
}

fn reverse_case_insensitive_octane_name_order(left: &str, right: &str) -> Ordering {
    let left_lower = left.to_ascii_lowercase();
    let right_lower = right.to_ascii_lowercase();
    right_lower.cmp(&left_lower).then_with(|| right.cmp(left))
}

fn octane_suite_score_record(
    suite: OctaneSuite,
    mode: OctaneExecutionMode,
    reports: &[OctaneBenchmarkExecutionReport],
) -> Option<OctaneSuiteScoreRecord> {
    if reports.is_empty() {
        return None;
    }

    let mut benchmark_scores = Vec::with_capacity(reports.len());
    for report in reports {
        let OctaneExecutionOutcome::Succeeded(success) = &report.outcome else {
            return None;
        };
        benchmark_scores.push(OctaneSuiteBenchmarkScoreRecord {
            benchmark: report.benchmark,
            score: success.scores.score,
        });
    }

    let scores = benchmark_scores
        .iter()
        .map(|record| record.score)
        .collect::<Vec<_>>();
    Some(OctaneSuiteScoreRecord {
        suite,
        mode,
        benchmark_scores,
        score: geometric_mean(&scores),
    })
}

fn extract_octane_benchmark_success(
    prepared: &OctanePreparedBenchmark,
    host_result_records: &[CoreHostResultRecord],
    host_output_records: &[CoreHostOutputRecord],
) -> Result<OctaneBenchmarkExecutionSuccess, OctaneExecutionFailure> {
    if let Some(failure) = classify_octane_oracle_alert(prepared, host_output_records) {
        return Err(failure);
    }

    match extract_octane_default_benchmark_results(
        prepared.plan,
        prepared.run_config,
        host_result_records,
        host_output_records,
    )
    .and_then(|telemetry| score_octane_default_benchmark_telemetry(prepared.run_config, telemetry))
    {
        Ok(success) => Ok(success),
        Err(error) => Err(octane_score_telemetry_failure(prepared, error)),
    }
}

fn classify_octane_oracle_alert(
    prepared: &OctanePreparedBenchmark,
    host_output_records: &[CoreHostOutputRecord],
) -> Option<OctaneExecutionFailure> {
    host_output_records
        .iter()
        .find(|record| record.sink == CoreHostOutputSink::Alert)
        .cloned()
        .map(|record| {
            let (order_index, order_entry, label) = octane_runner_execution_context(prepared);
            OctaneExecutionFailure {
                phase: OctaneExecutionPhase::ThrownOrOracle,
                order_index,
                order_entry,
                label,
                detail: OctaneExecutionFailureDetail::OracleAlert(record),
            }
        })
}

fn extract_octane_default_benchmark_results(
    plan: &'static OctaneBenchmarkPlan,
    run_config: OctaneDefaultBenchmarkRunConfig,
    host_result_records: &[CoreHostResultRecord],
    host_output_records: &[CoreHostOutputRecord],
) -> Result<OctaneDefaultBenchmarkTelemetry, OctaneScoreTelemetryError> {
    if !host_result_records.is_empty() {
        return extract_octane_default_benchmark_resolved_result(host_result_records);
    }

    extract_octane_default_benchmark_telemetry(plan, run_config, host_output_records)
}

fn extract_octane_default_benchmark_resolved_result(
    host_result_records: &[CoreHostResultRecord],
) -> Result<OctaneDefaultBenchmarkTelemetry, OctaneScoreTelemetryError> {
    let [record] = host_result_records else {
        return Err(OctaneScoreTelemetryError::Duplicate {
            count: host_result_records.len(),
        });
    };

    match record {
        CoreHostResultRecord::Resolved { elapsed_times_ms } => {
            Ok(OctaneDefaultBenchmarkTelemetry {
                elapsed_times_ms: elapsed_times_ms.clone(),
                validation_state: OctaneBenchmarkValidationState::Passed,
            })
        }
        CoreHostResultRecord::Rejected { reason } => {
            Err(OctaneScoreTelemetryError::RejectedResult {
                reason: reason.clone(),
            })
        }
    }
}

fn extract_octane_default_benchmark_telemetry(
    plan: &'static OctaneBenchmarkPlan,
    run_config: OctaneDefaultBenchmarkRunConfig,
    host_output_records: &[CoreHostOutputRecord],
) -> Result<OctaneDefaultBenchmarkTelemetry, OctaneScoreTelemetryError> {
    let telemetry_records = host_output_records
        .iter()
        .filter(|record| {
            record.sink == CoreHostOutputSink::Print
                && octane_telemetry_text_matches_prefix(&record.text)
        })
        .collect::<Vec<_>>();

    let [record] = telemetry_records.as_slice() else {
        return if telemetry_records.is_empty() {
            Err(OctaneScoreTelemetryError::Missing)
        } else {
            Err(OctaneScoreTelemetryError::Duplicate {
                count: telemetry_records.len(),
            })
        };
    };

    parse_octane_default_benchmark_telemetry_text(plan, run_config, &record.text)
}

fn parse_octane_default_benchmark_telemetry_text(
    plan: &'static OctaneBenchmarkPlan,
    run_config: OctaneDefaultBenchmarkRunConfig,
    text: &str,
) -> Result<OctaneDefaultBenchmarkTelemetry, OctaneScoreTelemetryError> {
    let Some(telemetry) = text.strip_prefix(OCTANE_DEFAULT_BENCHMARK_TELEMETRY_PREFIX) else {
        return Err(OctaneScoreTelemetryError::Malformed {
            text: text.to_string(),
        });
    };

    let mut fields = telemetry.split('|');
    let Some(benchmark) = fields.next() else {
        return Err(OctaneScoreTelemetryError::Malformed {
            text: text.to_string(),
        });
    };
    let Some(iterations) = fields.next() else {
        return Err(OctaneScoreTelemetryError::Malformed {
            text: text.to_string(),
        });
    };
    let Some(validation_state) = fields.next() else {
        return Err(OctaneScoreTelemetryError::Malformed {
            text: text.to_string(),
        });
    };
    let Some(elapsed_times) = fields.next() else {
        return Err(OctaneScoreTelemetryError::Malformed {
            text: text.to_string(),
        });
    };
    if fields.next().is_some() {
        return Err(OctaneScoreTelemetryError::Malformed {
            text: text.to_string(),
        });
    }

    if benchmark != plan.name {
        return Err(OctaneScoreTelemetryError::UnexpectedBenchmark {
            expected: plan.name,
            actual: benchmark.to_string(),
        });
    }

    let iterations = iterations.parse::<usize>().map_err(|_| {
        OctaneScoreTelemetryError::InvalidIterationCount {
            value: iterations.to_string(),
        }
    })?;
    if iterations != run_config.iterations {
        return Err(OctaneScoreTelemetryError::UnexpectedIterationCount {
            expected: run_config.iterations,
            actual: iterations,
        });
    }

    let validation_state = parse_octane_validation_state(validation_state)?;
    let elapsed_times_ms = if elapsed_times.is_empty() {
        Vec::new()
    } else {
        elapsed_times
            .split(',')
            .enumerate()
            .map(|(index, value)| parse_octane_elapsed_time(index, value))
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(OctaneDefaultBenchmarkTelemetry {
        elapsed_times_ms,
        validation_state,
    })
}

fn parse_octane_validation_state(
    value: &str,
) -> Result<OctaneBenchmarkValidationState, OctaneScoreTelemetryError> {
    match value {
        "not-run" => Ok(OctaneBenchmarkValidationState::NotRun),
        "passed" => Ok(OctaneBenchmarkValidationState::Passed),
        _ => Err(OctaneScoreTelemetryError::InvalidValidationState {
            value: value.to_string(),
        }),
    }
}

fn parse_octane_elapsed_time(index: usize, value: &str) -> Result<f64, OctaneScoreTelemetryError> {
    let elapsed_time =
        value
            .parse::<f64>()
            .map_err(|_| OctaneScoreTelemetryError::InvalidElapsedTime {
                index,
                value: value.to_string(),
            })?;
    if !elapsed_time.is_finite() {
        return Err(OctaneScoreTelemetryError::NonFiniteElapsedTime {
            index,
            value: value.to_string(),
        });
    }
    if elapsed_time < 1.0 {
        return Err(OctaneScoreTelemetryError::ElapsedTimeOutOfRange {
            index,
            value: value.to_string(),
        });
    }
    Ok(elapsed_time)
}

fn score_octane_default_benchmark_telemetry(
    run_config: OctaneDefaultBenchmarkRunConfig,
    telemetry: OctaneDefaultBenchmarkTelemetry,
) -> Result<OctaneBenchmarkExecutionSuccess, OctaneScoreTelemetryError> {
    let scores = run_config
        .score_results(&telemetry.elapsed_times_ms)
        .map_err(OctaneScoreTelemetryError::Scoring)?;
    validate_octane_scores(scores)?;
    Ok(OctaneBenchmarkExecutionSuccess { telemetry, scores })
}

fn validate_octane_scores(
    scores: OctaneDefaultBenchmarkScores,
) -> Result<(), OctaneScoreTelemetryError> {
    for (component, value) in [
        (OctaneScoreComponent::FirstIteration, scores.first_iteration),
        (OctaneScoreComponent::WorstCase, scores.worst_case),
        (OctaneScoreComponent::Average, scores.average),
        (OctaneScoreComponent::Score, scores.score),
    ] {
        if !value.is_finite() {
            return Err(OctaneScoreTelemetryError::NonFiniteScore {
                component,
                value: value.to_string(),
            });
        }
    }
    Ok(())
}

fn octane_score_telemetry_failure(
    prepared: &OctanePreparedBenchmark,
    error: OctaneScoreTelemetryError,
) -> OctaneExecutionFailure {
    let (order_index, order_entry, label) = octane_runner_execution_context(prepared);

    OctaneExecutionFailure {
        phase: OctaneExecutionPhase::ScoreTelemetry,
        order_index,
        order_entry,
        label,
        detail: OctaneExecutionFailureDetail::ScoreTelemetry(error),
    }
}

fn octane_runner_execution_context(
    prepared: &OctanePreparedBenchmark,
) -> (
    Option<usize>,
    Option<OctanePreparedSourceOrderEntry>,
    Option<String>,
) {
    let order_entry =
        OctanePreparedSourceOrderEntry::Generated(OctanePreparedGeneratedSourceKind::Runner);
    let order_index = prepared
        .source_order
        .iter()
        .position(|entry| *entry == order_entry);
    let label = prepared
        .generated_source(OctanePreparedGeneratedSourceKind::Runner)
        .map(|source| source.label.clone());

    (order_index, Some(order_entry), label)
}

fn octane_telemetry_text_matches_prefix(text: &str) -> bool {
    text.starts_with(OCTANE_DEFAULT_BENCHMARK_TELEMETRY_PREFIX)
}

struct OctaneOrderedPreparedSource<'a> {
    label: &'a str,
    source: &'a SourceSessionSource,
}

fn prepared_source_for_order_entry(
    prepared: &OctanePreparedBenchmark,
    order_entry: OctanePreparedSourceOrderEntry,
) -> Option<OctaneOrderedPreparedSource<'_>> {
    match order_entry {
        OctanePreparedSourceOrderEntry::Generated(kind) => {
            prepared
                .generated_source(kind)
                .map(|source| OctaneOrderedPreparedSource {
                    label: &source.label,
                    source: &source.source,
                })
        }
        OctanePreparedSourceOrderEntry::BenchmarkFile(index) => prepared
            .benchmark_sources
            .get(index)
            .map(|source| OctaneOrderedPreparedSource {
                label: &source.label,
                source: &source.source,
            }),
    }
}

fn classify_source_execution_error(
    mode: OctaneExecutionMode,
    order_index: Option<usize>,
    order_entry: Option<OctanePreparedSourceOrderEntry>,
    label: Option<String>,
    error: SourceExecutionError,
) -> OctaneExecutionFailure {
    let phase = match &error {
        SourceExecutionError::Parse(_) => OctaneExecutionPhase::Parse,
        SourceExecutionError::BytecompilerHandoff(_)
        | SourceExecutionError::BytecodeEmission(_)
        | SourceExecutionError::MissingUnlinkedCode
        | SourceExecutionError::SourceSessionInstructionDecode(_)
        | SourceExecutionError::SourceSessionMissingIdentifierText(_)
        | SourceExecutionError::SourceSessionInvalidLoadFunctionOperand { .. }
        | SourceExecutionError::SourceSessionFunctionIndexOverflow { .. }
        | SourceExecutionError::SourceSessionFunctionTableOverflow { .. }
        | SourceExecutionError::SourceSessionGlobalBindingConflict { .. } => {
            OctaneExecutionPhase::BytecodeEmit
        }
        SourceExecutionError::MissingStaticCellMetadata(_)
        | SourceExecutionError::ExecutableAllocation(_)
        | SourceExecutionError::ExecutablePublication(_)
        | SourceExecutionError::ExecutableRegistration(_)
        | SourceExecutionError::ExecutableInstall(_)
        | SourceExecutionError::CodeBlockAllocation(_)
        | SourceExecutionError::CodeBlockPublication(_)
        | SourceExecutionError::GlobalObjectAllocation(_)
        | SourceExecutionError::GlobalObjectPublication(_)
        | SourceExecutionError::GlobalObjectValue(_)
        | SourceExecutionError::SourceSessionGlobalLexicalInstall(_)
        | SourceExecutionError::GlobalRootRegistration(_) => OctaneExecutionPhase::SessionLink,
        SourceExecutionError::ExceptionRootSynchronization(_)
        | SourceExecutionError::FramePush(_)
        | SourceExecutionError::FramePop(_)
        | SourceExecutionError::EntryLeave(_) => OctaneExecutionPhase::ExecuteRuntime,
    };
    let phase = if source_execution_error_is_baseline_only(mode, &error) {
        OctaneExecutionPhase::BaselineOnly
    } else {
        phase
    };

    OctaneExecutionFailure {
        phase,
        order_index,
        order_entry,
        label,
        detail: OctaneExecutionFailureDetail::SourceExecutionError(error),
    }
}

fn classify_completion(
    mode: OctaneExecutionMode,
    order_index: usize,
    order_entry: OctanePreparedSourceOrderEntry,
    label: String,
    completion: ExecutionCompletion,
) -> Option<OctaneExecutionFailure> {
    let phase = match &completion {
        ExecutionCompletion::Returned(_) => return None,
        ExecutionCompletion::Threw(_) => OctaneExecutionPhase::ThrownOrOracle,
        ExecutionCompletion::Failed(error) if execution_error_is_baseline_only(mode, error) => {
            OctaneExecutionPhase::BaselineOnly
        }
        ExecutionCompletion::Failed(_) => OctaneExecutionPhase::ExecuteRuntime,
        ExecutionCompletion::OrdinaryBytecodeCall(_)
        | ExecutionCompletion::OrdinaryBytecodeConstruct(_)
        | ExecutionCompletion::BaselineLoopHandoff(_)
        | ExecutionCompletion::FunctionValueCall(_)
        | ExecutionCompletion::EvalRequest(_)
        | ExecutionCompletion::CompileFunctionRequest(_)
        | ExecutionCompletion::Terminated(_)
        | ExecutionCompletion::Suspended(_) => OctaneExecutionPhase::ExecuteRuntime,
    };

    Some(OctaneExecutionFailure {
        phase,
        order_index: Some(order_index),
        order_entry: Some(order_entry),
        label: Some(label),
        detail: OctaneExecutionFailureDetail::Completion(completion),
    })
}

fn source_execution_error_is_baseline_only(
    mode: OctaneExecutionMode,
    error: &SourceExecutionError,
) -> bool {
    mode == OctaneExecutionMode::BaselineAllowed
        && matches!(
            error,
            SourceExecutionError::FramePush(error)
                | SourceExecutionError::FramePop(error)
                | SourceExecutionError::EntryLeave(error)
                | SourceExecutionError::GlobalObjectValue(error)
                if execution_error_is_baseline_only(mode, error)
        )
}

fn execution_error_is_baseline_only(mode: OctaneExecutionMode, error: &ExecutionError) -> bool {
    mode == OctaneExecutionMode::BaselineAllowed
        && matches!(
            error,
            ExecutionError::BaselineGeneratedCodeUnavailable
                | ExecutionError::BaselineGeneratedExecutionRejected
        )
}

fn append_generated_source(
    loader: &mut ShellSourceLoader,
    plan: &'static OctaneBenchmarkPlan,
    kind: OctanePreparedGeneratedSourceKind,
    text: String,
) -> Result<OctanePreparedGeneratedSource, OctanePreparationError> {
    let label = format!("octane://{}/{}", plan.name, kind.label_component());
    let loaded = loader
        .append_source_text(ShellSourceAppendRequest::eval(
            text.clone(),
            ShellMode::ScriptFile,
            Some(label.clone()),
        ))
        .map_err(|error| OctanePreparationError::SourceAppend {
            benchmark: plan.name,
            generated_kind: kind,
            label: label.clone(),
            error,
        })?;
    let provider_id = loaded.provider_id();
    let origin_id = loaded.origin_id();

    Ok(OctanePreparedGeneratedSource {
        kind,
        label,
        text,
        source: SourceSessionSource::with_provenance(loaded.source_code(), provider_id, origin_id),
        provider_id,
        origin_id,
    })
}

fn source_text_to_string(source_text: &SourceText) -> String {
    match source_text {
        SourceText::Latin1(text) => String::from_utf8_lossy(text).into_owned(),
        SourceText::Utf16(text) => String::from_utf16_lossy(text),
    }
}

fn generate_octane_prelude_source() -> String {
    [
        "var isInBrowser = false;",
        "var self = this;",
        "if (typeof performance === \"undefined\")",
        "    var performance = Date;",
        "",
    ]
    .join("\n")
}

fn generate_octane_deterministic_random_source() -> String {
    [
        "(function() {",
        "    var initialSeed = 49734321;",
        "    var seed = initialSeed;",
        "",
        "    Math.random = function() {",
        "        seed = ((seed + 0x7ed55d16) + (seed << 12)) & 0xffffffff;",
        "        seed = ((seed ^ 0xc761c23c) ^ (seed >>> 19)) & 0xffffffff;",
        "        seed = ((seed + 0x165667b1) + (seed << 5)) & 0xffffffff;",
        "        seed = ((seed + 0xd3a2646c) ^ (seed << 9)) & 0xffffffff;",
        "        seed = ((seed + 0xfd7046c5) + (seed << 3)) & 0xffffffff;",
        "        seed = ((seed ^ 0xb55a4f09) ^ (seed >>> 16)) & 0xffffffff;",
        "        return (seed >>> 0) / 0x100000000;",
        "    };",
        "",
        "    Math.random.__resetSeed = function() {",
        "        seed = initialSeed;",
        "    };",
        "})();",
        "",
    ]
    .join("\n")
}

fn generate_octane_runner_source(
    plan: &'static OctaneBenchmarkPlan,
    run_config: OctaneDefaultBenchmarkRunConfig,
) -> Result<String, OctaneGeneratedSourceError> {
    match plan.benchmark_class {
        OctaneBenchmarkClass::DefaultBenchmark => {
            let random_reset = if plan.deterministic_random {
                "        Math.random.__resetSeed();\n"
            } else {
                ""
            };
            Ok(format!(
                "\
let __octaneBenchmark = new Benchmark({iterations});
let __octaneResults = [];
for (let i = 0; i < {iterations}; i++) {{
    if (typeof __octaneBenchmark.prepareForNextIteration === \"function\")
        __octaneBenchmark.prepareForNextIteration();
{random_reset}    let start = performance.now();
    __octaneBenchmark.runIteration();
    let end = performance.now();
    let __octaneElapsed = Math.max(1, end - start);
    __octaneResults.push(__octaneElapsed);
}}
if (typeof __octaneBenchmark.validate === \"function\") {{
    let __octaneValidationResult = __octaneBenchmark.validate();
    if (__octaneValidationResult === false)
        alert(\"Octane validation failed\");
}}
top.currentResolve(__octaneResults);
__octaneResults;
",
                iterations = run_config.iterations,
                random_reset = random_reset
            ))
        }
    }
}

pub const OCTANE_DRIVER_PLANS: &[OctaneBenchmarkPlan] = &[
    OctaneBenchmarkPlan {
        name: "Box2D",
        files: &["./Octane/box2d.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "octane-code-load",
        files: &["./Octane/code-first-load.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "crypto",
        files: &["./Octane/crypto.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "delta-blue",
        files: &["./Octane/deltablue.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "earley-boyer",
        files: &["./Octane/earley-boyer.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "gbemu",
        files: &["./Octane/gbemu-part1.js", "./Octane/gbemu-part2.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "mandreel",
        files: &["./Octane/mandreel.js"],
        deterministic_random: true,
        iterations: Some(80),
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "navier-stokes",
        files: &["./Octane/navier-stokes.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "pdfjs",
        files: &["./Octane/pdfjs.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "raytrace",
        files: &["./Octane/raytrace.js"],
        deterministic_random: false,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "regexp",
        files: &["./Octane/regexp.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "richards",
        files: &["./Octane/richards.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "splay",
        files: &["./Octane/splay.js"],
        deterministic_random: true,
        iterations: None,
        worst_case_count: None,
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "typescript",
        files: &[
            "./Octane/typescript-compiler.js",
            "./Octane/typescript-input.js",
            "./Octane/typescript.js",
        ],
        deterministic_random: true,
        iterations: Some(15),
        worst_case_count: Some(2),
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
    OctaneBenchmarkPlan {
        name: "octane-zlib",
        files: &["./Octane/zlib-data.js", "./Octane/zlib.js"],
        deterministic_random: true,
        iterations: Some(15),
        worst_case_count: Some(2),
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    },
];

pub const OCTANE_CORE_SELECTION: OctaneBenchmarkSelection = OctaneBenchmarkSelection {
    name: "Octane-core",
    benchmark_names: &[
        "richards",
        "delta-blue",
        "crypto",
        "splay",
        "navier-stokes",
        "raytrace",
    ],
};

pub fn octane_plan_by_name(name: &str) -> Option<&'static OctaneBenchmarkPlan> {
    OCTANE_DRIVER_PLANS.iter().find(|plan| plan.name == name)
}

pub fn octane_default_to_score(time_ms: f64) -> f64 {
    5000.0 / if time_ms < 1.0 { 1.0 } else { time_ms }
}

fn arithmetic_mean(values: &[f64]) -> f64 {
    values.iter().copied().sum::<f64>() / values.len() as f64
}

fn geometric_mean(values: &[f64]) -> f64 {
    values
        .iter()
        .copied()
        .product::<f64>()
        .powf(1.0 / values.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::super::ShellFilesystemOperation;
    use super::*;
    use crate::bytecode::{BytecodeIndex, CoreOpcode};
    use crate::gc::CellId;
    use crate::jit::{
        BaselineGeneratedPropertyLoadSidecarReadiness, P6X86_64BaselineSelectedSideExitReason,
    };
    use crate::runtime::CodeBlockId;
    use crate::vm::{
        VmGeneratedDirectCallGeneratedEntryMissReason, VmGeneratedDirectCallNativeEntryMissReason,
        VmGeneratedDirectCallTransactionRoute,
    };
    use std::collections::HashSet;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    static OCTANE_TEST_FUNCTION_PLAN: OctaneBenchmarkPlan = OctaneBenchmarkPlan {
        name: "test-function",
        files: &["./Octane/test-function.js"],
        deterministic_random: false,
        iterations: Some(3),
        worst_case_count: Some(1),
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    };

    static OCTANE_TEST_CLASS_PLAN: OctaneBenchmarkPlan = OctaneBenchmarkPlan {
        name: "test-class",
        files: &["./Octane/test-class.js"],
        deterministic_random: false,
        iterations: Some(3),
        worst_case_count: Some(1),
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    };

    static OCTANE_TEST_UNSUPPORTED_PLAN: OctaneBenchmarkPlan = OctaneBenchmarkPlan {
        name: "test-unsupported",
        files: &["./Octane/test-unsupported.js"],
        deterministic_random: false,
        iterations: Some(3),
        worst_case_count: Some(1),
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    };

    static OCTANE_TEST_SECOND_PLAN: OctaneBenchmarkPlan = OctaneBenchmarkPlan {
        name: "test-second",
        files: &["./Octane/test-second.js"],
        deterministic_random: false,
        iterations: Some(3),
        worst_case_count: Some(1),
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    };

    static OCTANE_TEST_ALPHA_PLAN: OctaneBenchmarkPlan = OctaneBenchmarkPlan {
        name: "alpha-test",
        files: &["./Octane/alpha-test.js"],
        deterministic_random: false,
        iterations: Some(3),
        worst_case_count: Some(1),
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    };

    static OCTANE_TEST_ZULU_PLAN: OctaneBenchmarkPlan = OctaneBenchmarkPlan {
        name: "zulu-test",
        files: &["./Octane/zulu-test.js"],
        deterministic_random: false,
        iterations: Some(3),
        worst_case_count: Some(1),
        benchmark_class: OctaneBenchmarkClass::DefaultBenchmark,
    };

    struct TempJetStreamRoot {
        path: PathBuf,
    }

    impl TempJetStreamRoot {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("octane-prep-test-{}-{unique}", std::process::id()));
            fs::create_dir_all(&path).expect("temporary root should be created");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn write_manifest_file(&self, manifest_path: &str, text: &str) {
            let relative_path = manifest_path
                .strip_prefix("./")
                .expect("test manifest path should be explicitly relative");
            let path = self.path.join(relative_path);
            fs::create_dir_all(path.parent().expect("test path should have a parent"))
                .expect("test parent directory should be created");
            fs::write(path, text).expect("test file should be written");
        }
    }

    impl Drop for TempJetStreamRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {actual} to be within 1e-9 of {expected}"
        );
    }

    fn octane_telemetry_text(
        benchmark: &str,
        iterations: usize,
        validation_state: &str,
        elapsed_times: &str,
    ) -> String {
        format!(
            "{OCTANE_DEFAULT_BENCHMARK_TELEMETRY_PREFIX}{benchmark}|{iterations}|{validation_state}|{elapsed_times}"
        )
    }

    fn octane_print_record(text: String) -> CoreHostOutputRecord {
        CoreHostOutputRecord {
            sink: CoreHostOutputSink::Print,
            text,
        }
    }

    fn octane_alert_record(text: &str) -> CoreHostOutputRecord {
        CoreHostOutputRecord {
            sink: CoreHostOutputSink::Alert,
            text: text.to_string(),
        }
    }

    fn minimal_function_benchmark_source() -> &'static str {
        "\
function Benchmark(iterations) {
}
Benchmark.prototype.runIteration = function() {
    return 1;
};
Benchmark.prototype.validate = function() {
    return 1;
};
"
    }

    fn deterministic_timing_benchmark_source() -> &'static str {
        "\
var __testNowValues = [100, 105, 200, 210, 300, 320];
var __testNowIndex = 0;
performance.now = function() {
    return __testNowValues[__testNowIndex++];
};
function Benchmark(iterations) {
    this.iterations = iterations;
}
Benchmark.prototype.runIteration = function() {
    return this.iterations;
};
Benchmark.prototype.validate = function() {
    return true;
};
"
    }

    fn prepare_test_benchmark(
        root: &TempJetStreamRoot,
        plan: &'static OctaneBenchmarkPlan,
    ) -> OctanePreparedBenchmark {
        prepare_octane_benchmark(root.path(), plan, OctaneBenchmarkRunOverrides::none())
            .expect("test benchmark should prepare")
    }

    fn octane_progress_phase_name(event: &OctaneExecutionProgress) -> &'static str {
        match event {
            OctaneExecutionProgress::BenchmarkStarted { .. } => "benchmark-start",
            OctaneExecutionProgress::SourceSessionStarted { .. } => "session-open-start",
            OctaneExecutionProgress::SourceSessionOpened { .. } => "session-open-done",
            OctaneExecutionProgress::SourceSessionErrored { .. } => "session-open-error",
            OctaneExecutionProgress::SourceStarted { .. } => "source-start",
            OctaneExecutionProgress::SourceCompleted { .. } => "source-done",
            OctaneExecutionProgress::SourceErrored { .. } => "source-error",
            OctaneExecutionProgress::ScoreTelemetryStarted { .. } => "score-telemetry-start",
        }
    }

    #[test]
    fn octane_full_manifest_matches_driver_order_and_metadata() {
        let names: Vec<_> = OCTANE_DRIVER_PLANS.iter().map(|plan| plan.name).collect();
        assert_eq!(
            names,
            vec![
                "Box2D",
                "octane-code-load",
                "crypto",
                "delta-blue",
                "earley-boyer",
                "gbemu",
                "mandreel",
                "navier-stokes",
                "pdfjs",
                "raytrace",
                "regexp",
                "richards",
                "splay",
                "typescript",
                "octane-zlib",
            ]
        );

        assert!(OCTANE_DRIVER_PLANS
            .iter()
            .all(|plan| plan.benchmark_class == OctaneBenchmarkClass::DefaultBenchmark));
        assert!(OCTANE_DRIVER_PLANS
            .iter()
            .filter(|plan| plan.name != "raytrace")
            .all(|plan| plan.deterministic_random));

        let raytrace = octane_plan_by_name("raytrace").expect("raytrace plan");
        assert_eq!(raytrace.files, &["./Octane/raytrace.js"]);
        assert!(!raytrace.deterministic_random);

        let gbemu = octane_plan_by_name("gbemu").expect("gbemu plan");
        assert_eq!(
            gbemu.files,
            &["./Octane/gbemu-part1.js", "./Octane/gbemu-part2.js"]
        );

        let mandreel = octane_plan_by_name("mandreel").expect("mandreel plan");
        assert_eq!(mandreel.iterations, Some(80));
        assert_eq!(mandreel.worst_case_count, None);

        let typescript = octane_plan_by_name("typescript").expect("typescript plan");
        assert_eq!(
            typescript.files,
            &[
                "./Octane/typescript-compiler.js",
                "./Octane/typescript-input.js",
                "./Octane/typescript.js",
            ]
        );
        assert_eq!(typescript.iterations, Some(15));
        assert_eq!(typescript.worst_case_count, Some(2));

        let zlib = octane_plan_by_name("octane-zlib").expect("octane-zlib plan");
        assert_eq!(zlib.files, &["./Octane/zlib-data.js", "./Octane/zlib.js"]);
        assert_eq!(zlib.iterations, Some(15));
        assert_eq!(zlib.worst_case_count, Some(2));
    }

    #[test]
    fn octane_core_selection_preserves_accepted_subset_order() {
        assert_eq!(OCTANE_CORE_SELECTION.name, "Octane-core");
        assert_eq!(
            OCTANE_CORE_SELECTION.benchmark_names,
            &[
                "richards",
                "delta-blue",
                "crypto",
                "splay",
                "navier-stokes",
                "raytrace",
            ]
        );

        let plans = OCTANE_CORE_SELECTION
            .resolved_plans()
            .expect("core plans should resolve");
        let names: Vec<_> = plans.iter().map(|plan| plan.name).collect();
        assert_eq!(
            names,
            vec![
                "richards",
                "delta-blue",
                "crypto",
                "splay",
                "navier-stokes",
                "raytrace",
            ]
        );

        let full_driver_names: Vec<_> = OCTANE_DRIVER_PLANS.iter().map(|plan| plan.name).collect();
        assert_ne!(&names, &full_driver_names[..names.len()]);
    }

    #[test]
    fn octane_suite_resolution_preserves_full_and_core_order() {
        assert_eq!(OctaneSuite::Full.name(), "Octane-full");
        let full = OctaneSuite::Full
            .resolved_plans()
            .expect("full suite should resolve");
        assert_eq!(full.len(), OCTANE_DRIVER_PLANS.len());
        assert_eq!(full.first().map(|plan| plan.name), Some("Box2D"));
        assert_eq!(full.last().map(|plan| plan.name), Some("octane-zlib"));

        assert_eq!(OctaneSuite::Core.name(), "Octane-core");
        let core = OctaneSuite::Core
            .resolved_plans()
            .expect("core suite should resolve");
        let names: Vec<_> = core.iter().map(|plan| plan.name).collect();
        assert_eq!(
            names,
            vec![
                "richards",
                "delta-blue",
                "crypto",
                "splay",
                "navier-stokes",
                "raytrace",
            ]
        );
    }

    #[test]
    fn octane_run_config_resolves_defaults_and_overrides() {
        let box2d = octane_plan_by_name("Box2D").expect("Box2D plan");
        assert_eq!(
            box2d.default_run_config(),
            Ok(OctaneDefaultBenchmarkRunConfig {
                iterations: OCTANE_DEFAULT_ITERATION_COUNT,
                worst_case_count: OCTANE_DEFAULT_WORST_CASE_COUNT,
            })
        );

        let mandreel = octane_plan_by_name("mandreel").expect("mandreel plan");
        assert_eq!(
            mandreel.default_run_config(),
            Ok(OctaneDefaultBenchmarkRunConfig {
                iterations: 80,
                worst_case_count: OCTANE_DEFAULT_WORST_CASE_COUNT,
            })
        );

        let typescript = octane_plan_by_name("typescript").expect("typescript plan");
        assert_eq!(
            typescript.default_run_config(),
            Ok(OctaneDefaultBenchmarkRunConfig {
                iterations: 15,
                worst_case_count: 2,
            })
        );
        assert_eq!(
            typescript.run_config(OctaneBenchmarkRunOverrides::new(Some(9), Some(3))),
            Ok(OctaneDefaultBenchmarkRunConfig {
                iterations: 9,
                worst_case_count: 3,
            })
        );
    }

    #[test]
    fn octane_default_benchmark_scoring_matches_known_times() {
        let config = OctaneDefaultBenchmarkRunConfig::new(5, 2).expect("valid config");
        let scores = config
            .score_results(&[0.0, 10.0, 40.0, 30.0, 20.0])
            .expect("scores");

        assert_close(scores.first_iteration, 5000.0);
        assert_close(scores.worst_case, 5000.0 / 35.0);
        assert_close(scores.average, 5000.0 / 25.0);
        assert_close(
            scores.score,
            (scores.first_iteration * scores.worst_case * scores.average).powf(1.0 / 3.0),
        );
    }

    #[test]
    fn octane_scoring_rejects_invalid_iteration_and_result_counts() {
        assert_eq!(
            OctaneDefaultBenchmarkRunConfig::new(4, 4),
            Err(OctaneScoringError::IterationsMustExceedWorstCase {
                iterations: 4,
                worst_case_count: 4,
            })
        );

        let config = OctaneDefaultBenchmarkRunConfig::new(5, 4).expect("valid config");
        assert_eq!(
            config.score_results(&[1.0, 2.0, 3.0, 4.0]),
            Err(OctaneScoringError::TooFewResults {
                expected: 5,
                actual: 4,
            })
        );
        assert_eq!(
            config.score_results(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
            Err(OctaneScoringError::TooManyResults {
                expected: 5,
                actual: 6,
            })
        );
    }

    #[test]
    fn octane_score_telemetry_parses_reserved_runner_record() {
        let run_config = OctaneDefaultBenchmarkRunConfig::new(3, 1).expect("valid config");
        let telemetry = parse_octane_default_benchmark_telemetry_text(
            &OCTANE_TEST_FUNCTION_PLAN,
            run_config,
            &octane_telemetry_text("test-function", 3, "not-run", "1,2,3"),
        )
        .expect("telemetry should parse");

        assert_eq!(
            telemetry,
            OctaneDefaultBenchmarkTelemetry {
                elapsed_times_ms: vec![1.0, 2.0, 3.0],
                validation_state: OctaneBenchmarkValidationState::NotRun,
            }
        );
    }

    #[test]
    fn octane_score_telemetry_classifies_missing_duplicate_and_invalid_records() {
        let run_config = OctaneDefaultBenchmarkRunConfig::new(3, 1).expect("valid config");
        assert_eq!(
            extract_octane_default_benchmark_telemetry(&OCTANE_TEST_FUNCTION_PLAN, run_config, &[],),
            Err(OctaneScoreTelemetryError::Missing)
        );

        let telemetry = octane_telemetry_text("test-function", 3, "passed", "1,2,3");
        assert_eq!(
            extract_octane_default_benchmark_telemetry(
                &OCTANE_TEST_FUNCTION_PLAN,
                run_config,
                &[
                    octane_print_record(telemetry.clone()),
                    octane_print_record(telemetry)
                ],
            ),
            Err(OctaneScoreTelemetryError::Duplicate { count: 2 })
        );

        assert_eq!(
            parse_octane_default_benchmark_telemetry_text(
                &OCTANE_TEST_FUNCTION_PLAN,
                run_config,
                &octane_telemetry_text("wrong-benchmark", 3, "passed", "1,2,3"),
            ),
            Err(OctaneScoreTelemetryError::UnexpectedBenchmark {
                expected: "test-function",
                actual: "wrong-benchmark".to_string(),
            })
        );
        assert_eq!(
            parse_octane_default_benchmark_telemetry_text(
                &OCTANE_TEST_FUNCTION_PLAN,
                run_config,
                &octane_telemetry_text("test-function", 4, "passed", "1,2,3"),
            ),
            Err(OctaneScoreTelemetryError::UnexpectedIterationCount {
                expected: 3,
                actual: 4,
            })
        );
        assert_eq!(
            parse_octane_default_benchmark_telemetry_text(
                &OCTANE_TEST_FUNCTION_PLAN,
                run_config,
                &octane_telemetry_text("test-function", 3, "unknown", "1,2,3"),
            ),
            Err(OctaneScoreTelemetryError::InvalidValidationState {
                value: "unknown".to_string(),
            })
        );
        assert_eq!(
            parse_octane_default_benchmark_telemetry_text(
                &OCTANE_TEST_FUNCTION_PLAN,
                run_config,
                &octane_telemetry_text("test-function", 3, "passed", "1,nope,3"),
            ),
            Err(OctaneScoreTelemetryError::InvalidElapsedTime {
                index: 1,
                value: "nope".to_string(),
            })
        );
    }

    #[test]
    fn octane_preparation_loads_manifest_files_in_order_with_provenance() {
        let root = TempJetStreamRoot::new();
        let plan = octane_plan_by_name("gbemu").expect("gbemu plan");
        root.write_manifest_file("./Octane/gbemu-part1.js", "var gbemuPart1 = 1;\n");
        root.write_manifest_file("./Octane/gbemu-part2.js", "var gbemuPart2 = 2;\n");

        let prepared =
            prepare_octane_benchmark(root.path(), plan, OctaneBenchmarkRunOverrides::none())
                .expect("benchmark should prepare");

        assert_eq!(prepared.plan.name, "gbemu");
        assert_eq!(prepared.benchmark_sources.len(), 2);
        assert_eq!(
            prepared
                .benchmark_sources
                .iter()
                .map(|source| source.manifest_path)
                .collect::<Vec<_>>(),
            vec!["./Octane/gbemu-part1.js", "./Octane/gbemu-part2.js"]
        );
        assert_eq!(prepared.benchmark_sources[0].text, "var gbemuPart1 = 1;\n");
        assert_eq!(prepared.benchmark_sources[1].text, "var gbemuPart2 = 2;\n");
        assert_eq!(
            prepared.benchmark_sources[0].canonical_path,
            fs::canonicalize(root.path().join("Octane/gbemu-part1.js"))
                .expect("test file should canonicalize")
        );
        assert_eq!(
            prepared.benchmark_sources[1].canonical_path,
            fs::canonicalize(root.path().join("Octane/gbemu-part2.js"))
                .expect("test file should canonicalize")
        );
        assert_eq!(
            prepared.benchmark_sources[0].source.provider_id(),
            Some(prepared.benchmark_sources[0].provider_id)
        );
        assert_eq!(
            prepared.benchmark_sources[0].source.origin_id(),
            Some(prepared.benchmark_sources[0].origin_id)
        );
        assert_eq!(
            prepared.source_order,
            vec![
                OctanePreparedSourceOrderEntry::Generated(
                    OctanePreparedGeneratedSourceKind::Prelude
                ),
                OctanePreparedSourceOrderEntry::Generated(
                    OctanePreparedGeneratedSourceKind::DeterministicRandom
                ),
                OctanePreparedSourceOrderEntry::BenchmarkFile(0),
                OctanePreparedSourceOrderEntry::BenchmarkFile(1),
                OctanePreparedSourceOrderEntry::Generated(
                    OctanePreparedGeneratedSourceKind::Runner
                ),
            ]
        );
    }

    #[test]
    fn octane_suite_preparation_uses_selected_suite_and_run_overrides() {
        let root = TempJetStreamRoot::new();
        let core_plans = OctaneSuite::Core
            .resolved_plans()
            .expect("core suite should resolve");
        for plan in &core_plans {
            for manifest_path in plan.files {
                root.write_manifest_file(manifest_path, "function Benchmark() {}\n");
            }
        }

        let config = OctanePreparationConfig::new(
            root.path(),
            OctaneRunConfig::with_overrides(
                OctaneSuite::Core,
                OctaneBenchmarkRunOverrides::new(Some(9), Some(3)),
            ),
        );
        let prepared = prepare_octane_suite(config.clone()).expect("core suite should prepare");

        assert_eq!(prepared.config, config);
        assert_eq!(prepared.benchmarks.len(), core_plans.len());
        assert_eq!(
            prepared
                .benchmarks
                .iter()
                .map(|benchmark| benchmark.plan.name)
                .collect::<Vec<_>>(),
            core_plans.iter().map(|plan| plan.name).collect::<Vec<_>>()
        );
        assert!(prepared.benchmarks.iter().all(|benchmark| {
            benchmark.run_config
                == OctaneDefaultBenchmarkRunConfig {
                    iterations: 9,
                    worst_case_count: 3,
                }
        }));

        let mut provider_ids = HashSet::new();
        for benchmark in &prepared.benchmarks {
            for source in &benchmark.benchmark_sources {
                assert!(
                    provider_ids.insert(source.provider_id),
                    "benchmark provider IDs should be unique across a prepared suite"
                );
            }
            for source in &benchmark.generated_sources {
                assert!(
                    provider_ids.insert(source.provider_id),
                    "generated provider IDs should be unique across a prepared suite"
                );
            }
        }
    }

    #[test]
    fn octane_preparation_adds_deterministic_random_only_for_marked_plans() {
        let root = TempJetStreamRoot::new();
        let crypto = octane_plan_by_name("crypto").expect("crypto plan");
        let raytrace = octane_plan_by_name("raytrace").expect("raytrace plan");
        root.write_manifest_file("./Octane/crypto.js", "function Benchmark() {}\n");
        root.write_manifest_file("./Octane/raytrace.js", "function Benchmark() {}\n");

        let prepared_crypto =
            prepare_octane_benchmark(root.path(), crypto, OctaneBenchmarkRunOverrides::none())
                .expect("crypto should prepare");
        let random_source = prepared_crypto
            .generated_source(OctanePreparedGeneratedSourceKind::DeterministicRandom)
            .expect("crypto should include deterministic random source");
        assert!(random_source.text.contains("var initialSeed = 49734321;"));
        assert!(random_source.text.contains("0x100000000"));
        assert_eq!(
            random_source.source.provider_id(),
            Some(random_source.provider_id)
        );
        assert_eq!(
            random_source.source.origin_id(),
            Some(random_source.origin_id)
        );

        let prepared_raytrace =
            prepare_octane_benchmark(root.path(), raytrace, OctaneBenchmarkRunOverrides::none())
                .expect("raytrace should prepare");
        assert!(prepared_raytrace
            .generated_source(OctanePreparedGeneratedSourceKind::DeterministicRandom)
            .is_none());
        assert_eq!(
            prepared_raytrace.source_order,
            vec![
                OctanePreparedSourceOrderEntry::Generated(
                    OctanePreparedGeneratedSourceKind::Prelude
                ),
                OctanePreparedSourceOrderEntry::BenchmarkFile(0),
                OctanePreparedSourceOrderEntry::Generated(
                    OctanePreparedGeneratedSourceKind::Runner
                ),
            ]
        );
    }

    #[test]
    fn octane_runner_resets_random_before_measured_iteration_when_deterministic() {
        let root = TempJetStreamRoot::new();
        let crypto = octane_plan_by_name("crypto").expect("crypto plan");
        let raytrace = octane_plan_by_name("raytrace").expect("raytrace plan");
        root.write_manifest_file("./Octane/crypto.js", "function Benchmark() {}\n");
        root.write_manifest_file("./Octane/raytrace.js", "function Benchmark() {}\n");

        let prepared_crypto =
            prepare_octane_benchmark(root.path(), crypto, OctaneBenchmarkRunOverrides::none())
                .expect("crypto should prepare");
        let crypto_runner = &prepared_crypto
            .generated_source(OctanePreparedGeneratedSourceKind::Runner)
            .expect("crypto runner source")
            .text;
        let reset_index = crypto_runner
            .find("Math.random.__resetSeed();")
            .expect("runner should reset random");
        let run_index = crypto_runner
            .find("__octaneBenchmark.runIteration();")
            .expect("runner should run benchmark");
        assert!(reset_index < run_index);
        assert!(crypto_runner.contains("let __octaneBenchmark = new Benchmark(120);"));
        assert!(crypto_runner.contains("top.currentResolve(__octaneResults);"));
        assert!(!crypto_runner.contains(OCTANE_DEFAULT_BENCHMARK_TELEMETRY_PREFIX));
        assert!(!crypto_runner.contains("print("));
        assert!(crypto_runner.ends_with("__octaneResults;\n"));

        let prepared_raytrace =
            prepare_octane_benchmark(root.path(), raytrace, OctaneBenchmarkRunOverrides::none())
                .expect("raytrace should prepare");
        let raytrace_runner = &prepared_raytrace
            .generated_source(OctanePreparedGeneratedSourceKind::Runner)
            .expect("raytrace runner source")
            .text;
        assert!(!raytrace_runner.contains("Math.random.__resetSeed();"));
        assert!(raytrace_runner.contains("__octaneBenchmark.runIteration();"));
    }

    #[test]
    fn octane_preparation_classifies_missing_manifest_file_as_source_load() {
        let root = TempJetStreamRoot::new();
        let plan = octane_plan_by_name("crypto").expect("crypto plan");

        let error =
            prepare_octane_benchmark(root.path(), plan, OctaneBenchmarkRunOverrides::none())
                .expect_err("missing file should fail");

        assert_eq!(error.phase(), OctanePreparationPhase::SourceLoad);
        match error {
            OctanePreparationError::SourceLoad {
                benchmark,
                manifest_path,
                resolved_path,
                error:
                    ShellSourceLoadError::Filesystem {
                        operation,
                        path,
                        message: _,
                    },
            } => {
                assert_eq!(benchmark, "crypto");
                assert_eq!(manifest_path, "./Octane/crypto.js");
                assert_eq!(resolved_path, root.path().join("./Octane/crypto.js"));
                assert_eq!(operation, ShellFilesystemOperation::Canonicalize);
                assert_eq!(path, root.path().join("./Octane/crypto.js"));
            }
            error => panic!("unexpected preparation error: {error:?}"),
        }
    }

    #[test]
    fn octane_preparation_never_adds_stale_octane_run_js_source() {
        let root = TempJetStreamRoot::new();
        let plan = octane_plan_by_name("crypto").expect("crypto plan");
        root.write_manifest_file("./Octane/crypto.js", "function Benchmark() {}\n");

        let prepared =
            prepare_octane_benchmark(root.path(), plan, OctaneBenchmarkRunOverrides::none())
                .expect("crypto should prepare");

        for source in &prepared.benchmark_sources {
            assert!(!source.manifest_path.contains("Octane/run.js"));
            assert!(!source
                .resolved_path
                .to_string_lossy()
                .contains("Octane/run.js"));
            assert!(!source.label.contains("Octane/run.js"));
            assert!(!source.text.contains("Octane/run.js"));
        }
        for source in &prepared.generated_sources {
            assert!(!source.label.contains("Octane/run.js"));
            assert!(!source.text.contains("Octane/run.js"));
        }
    }

    #[test]
    fn octane_executes_function_style_benchmark_and_scores_extracted_results() {
        let root = TempJetStreamRoot::new();
        root.write_manifest_file(
            "./Octane/test-function.js",
            deterministic_timing_benchmark_source(),
        );
        let prepared = prepare_test_benchmark(&root, &OCTANE_TEST_FUNCTION_PLAN);

        let report = execute_prepared_octane_benchmark(
            &prepared,
            OctaneExecutionConfig::new(
                OctaneExecutionMode::InterpreterOnly,
                OctaneSuiteFailurePolicy::FailFast,
            ),
        );

        assert_eq!(report.benchmark, "test-function");
        assert_eq!(report.mode, OctaneExecutionMode::InterpreterOnly);
        assert_eq!(
            report.run_config,
            OctaneDefaultBenchmarkRunConfig {
                iterations: 3,
                worst_case_count: 1,
            }
        );
        let OctaneExecutionOutcome::Succeeded(success) = &report.outcome else {
            panic!("expected success: {report:#?}");
        };
        assert_eq!(
            success.telemetry,
            OctaneDefaultBenchmarkTelemetry {
                elapsed_times_ms: vec![5.0, 10.0, 20.0],
                validation_state: OctaneBenchmarkValidationState::Passed,
            }
        );
        assert_close(success.scores.first_iteration, 1000.0);
        assert_close(success.scores.worst_case, 250.0);
        assert_close(success.scores.average, 5000.0 / 15.0);
        assert_close(
            success.scores.score,
            (1000.0_f64 * 250.0 * (5000.0 / 15.0)).powf(1.0 / 3.0),
        );
        assert_eq!(report.source_records.len(), prepared.source_order.len());
        assert!(report
            .source_records
            .iter()
            .all(|record| matches!(record.completion, Some(ExecutionCompletion::Returned(_)))));
        assert!(report.host_output_records.is_empty());
        assert!(report.tiering_delta.entry_decisions > 0);
        assert!(report.tiering_delta.diagnostics > 0);
        assert_eq!(report.tiering_delta.fallback_records, 0);
        assert_eq!(report.tiering_delta.baseline_installs, 0);
        assert_eq!(report.tiering_delta.launch_descriptors, 0);
    }

    #[test]
    fn octane_tiering_summary_delta_preserves_detailed_rootless_telemetry() {
        let owner = CodeBlockId(CellId(11));
        let second_owner = CodeBlockId(CellId(12));
        let caller = CodeBlockId(CellId(21));
        let target = CodeBlockId(CellId(22));
        let call_bytecode_index = BytecodeIndex::from_offset(16);
        let side_exit_bytecode_index = BytecodeIndex::from_offset(24);

        let start = OctaneTieringSummary {
            baseline_generated_code_invalidations: 5,
            baseline_generated_code_invalidation_summary: VmBaselineGeneratedInvalidationSummary {
                total: 5,
                accepted: 3,
                no_matching_artifact: 1,
                property_load_probe_miss: 2,
                guarded_property_load_probe_miss: 1,
                ..VmBaselineGeneratedInvalidationSummary::default()
            },
            generated_direct_call_rootless_rejections:
                VmGeneratedDirectCallRootlessRejectionCounts {
                    hot_slot_miss: 2,
                    unsupported_body_opcode: 1,
                    ..VmGeneratedDirectCallRootlessRejectionCounts::default()
                },
            generated_direct_call_rootless_native_entry_rejections:
                VmGeneratedDirectCallRootlessRejectionCounts {
                    missing_generated_artifact: 1,
                    ..VmGeneratedDirectCallRootlessRejectionCounts::default()
                },
            generated_direct_call_rootless_unsupported_body_opcode_counts: vec![
                VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount {
                    opcode: CoreOpcode::RightShiftInt32,
                    count: 2,
                },
                VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount {
                    opcode: CoreOpcode::BitAndInt32,
                    count: 4,
                },
            ],
            generated_direct_call_rootless_native_entry_unsupported_body_opcode_counts: vec![
                VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount {
                    opcode: CoreOpcode::LoadInt32,
                    count: 1,
                },
            ],
            generated_direct_call_rootless_native_entry_retained_side_exit_counts: vec![
                VmGeneratedDirectCallRootlessRetainedSideExitCount {
                    target_code_block: target,
                    bytecode_index: side_exit_bytecode_index,
                    opcode: Some(CoreOpcode::AddInt32),
                    reason: P6X86_64BaselineSelectedSideExitReason::NonInt32Operand,
                    count: 2,
                },
            ],
            generated_direct_call_rootless_preferred_native_entry_counts:
                VmGeneratedDirectCallRootlessPreferredNativeEntryCounts {
                    pure_baseline_shim: 1,
                    emitted_semantic_c_abi_entry: 3,
                    unknown: 2,
                },
            baseline_generated_execution_summaries: vec![VmBaselineGeneratedExecutionSummary {
                owner,
                execution_count: 2,
                executed_bytecode_count: 10,
                returned_count: 1,
                threw_count: 0,
                ordinary_bytecode_call_count: 0,
                ordinary_bytecode_construct_count: 0,
                function_value_call_count: 0,
                terminated_count: 0,
                suspended_count: 0,
                failed_count: 0,
                fallback_count: 0,
                js_call_count: 1,
                property_count: 0,
                runtime_helper_count: 0,
                rejected_count: 0,
            }],
            baseline_generated_dispatched_opcode_counts: vec![
                VmBaselineGeneratedDispatchedOpcodeCount {
                    owner,
                    opcode: CoreOpcode::Call,
                    count: 4,
                },
                VmBaselineGeneratedDispatchedOpcodeCount {
                    owner,
                    opcode: CoreOpcode::LoadInt32,
                    count: 2,
                },
            ],
            baseline_generated_dispatched_site_opcode_counts: vec![
                VmBaselineGeneratedDispatchedSiteOpcodeCount {
                    owner,
                    bytecode_index: BytecodeIndex::from_offset(36),
                    opcode: CoreOpcode::GetByName,
                    property_load_sidecar_readiness:
                        BaselineGeneratedPropertyLoadSidecarReadiness::OwnDataPlan,
                    count: 5,
                },
                VmBaselineGeneratedDispatchedSiteOpcodeCount {
                    owner,
                    bytecode_index: BytecodeIndex::from_offset(44),
                    opcode: CoreOpcode::GetByName,
                    property_load_sidecar_readiness:
                        BaselineGeneratedPropertyLoadSidecarReadiness::NoLoadPlan,
                    count: 1,
                },
            ],
            generated_direct_call_transaction_summaries: vec![
                VmGeneratedDirectCallTransactionSummary {
                    caller,
                    call_bytecode_index,
                    target_code_block: target,
                    argument_count_including_this: 2,
                    route: VmGeneratedDirectCallTransactionRoute::GeneratedEntry,
                    transaction_count: 2,
                    continue_count: 1,
                    jump_count: 0,
                    return_count: 1,
                    threw_count: 0,
                    ordinary_bytecode_call_count: 0,
                    ordinary_bytecode_construct_count: 0,
                    function_value_call_count: 0,
                    suspended_count: 0,
                    failed_count: 0,
                },
            ],
            generated_direct_call_callee_fallback_summaries: vec![
                VmGeneratedDirectCallCalleeFallbackSummary {
                    caller,
                    call_bytecode_index,
                    target_code_block: target,
                    argument_count_including_this: 2,
                    preferred_route: None,
                    generated_entry_miss:
                        VmGeneratedDirectCallGeneratedEntryMissReason::MissingArtifact,
                    native_entry_miss: VmGeneratedDirectCallNativeEntryMissReason::MissingGate,
                    fallback_count: 2,
                },
            ],
            generated_direct_call_route_opportunity_summaries: vec![
                VmGeneratedDirectCallRouteOpportunitySummary {
                    caller,
                    call_bytecode_index,
                    target_code_block: target,
                    argument_count_including_this: 2,
                    selected_route: VmGeneratedDirectCallTransactionRoute::GeneratedEntry,
                    preferred_route: Some(VmGeneratedDirectCallTransactionRoute::GeneratedEntry),
                    native_entry_miss:
                        VmGeneratedDirectCallNativeEntryMissReason::HostBlockedX86_64,
                    count: 2,
                },
            ],
            baseline_entry_auto_materializations: 1,
            property_inline_cache_evolution_records: 7,
            property_inline_cache_evolution_admitted: 5,
            property_inline_cache_evolution_buffered: 3,
            property_inline_cache_evolution_buffered_duplicates: 2,
            property_inline_cache_evolution_cooldowns: 1,
            property_inline_cache_evolution_final_gave_up: 1,
            property_inline_cache_evolution_gave_up_skips: 2,
            property_inline_cache_evolution_generated_megamorphic_load: 1,
            property_inline_cache_evolution_megamorphic_load_skips: 3,
            property_inline_cache_evolution_generated_megamorphic_store: 2,
            property_inline_cache_evolution_megamorphic_store_skips: 4,
            property_inline_cache_evolution_generated_megamorphic_has: 1,
            property_inline_cache_evolution_megamorphic_has_skips: 2,
            property_load_megamorphic_cache_records: 4,
            property_store_megamorphic_cache_records: 6,
            property_has_megamorphic_cache_records: 8,
            ..OctaneTieringSummary::default()
        };
        let current = OctaneTieringSummary {
            baseline_generated_code_invalidations: 13,
            baseline_generated_code_invalidation_summary: VmBaselineGeneratedInvalidationSummary {
                total: 13,
                accepted: 9,
                no_matching_artifact: 2,
                bytecode_snapshot_mismatch: 1,
                property_load_probe_miss: 7,
                guarded_property_load_probe_miss: 3,
                property_load_guard_watchpoint_invalidation: 2,
                ..VmBaselineGeneratedInvalidationSummary::default()
            },
            generated_direct_call_rootless_rejections:
                VmGeneratedDirectCallRootlessRejectionCounts {
                    hot_slot_miss: 5,
                    effect_contract: 2,
                    unsupported_body_opcode: 4,
                    ..VmGeneratedDirectCallRootlessRejectionCounts::default()
                },
            generated_direct_call_rootless_native_entry_rejections:
                VmGeneratedDirectCallRootlessRejectionCounts {
                    missing_generated_artifact: 3,
                    ..VmGeneratedDirectCallRootlessRejectionCounts::default()
                },
            generated_direct_call_rootless_unsupported_body_opcode_counts: vec![
                VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount {
                    opcode: CoreOpcode::RightShiftInt32,
                    count: 5,
                },
                VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount {
                    opcode: CoreOpcode::UnsignedRightShiftInt32,
                    count: 3,
                },
            ],
            generated_direct_call_rootless_native_entry_unsupported_body_opcode_counts: vec![
                VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount {
                    opcode: CoreOpcode::LoadInt32,
                    count: 4,
                },
            ],
            generated_direct_call_rootless_native_entry_retained_side_exit_counts: vec![
                VmGeneratedDirectCallRootlessRetainedSideExitCount {
                    target_code_block: target,
                    bytecode_index: side_exit_bytecode_index,
                    opcode: Some(CoreOpcode::AddInt32),
                    reason: P6X86_64BaselineSelectedSideExitReason::NonInt32Operand,
                    count: 5,
                },
                VmGeneratedDirectCallRootlessRetainedSideExitCount {
                    target_code_block: target,
                    bytecode_index: BytecodeIndex::from_offset(28),
                    opcode: Some(CoreOpcode::MulInt32),
                    reason: P6X86_64BaselineSelectedSideExitReason::NegativeZero,
                    count: 1,
                },
            ],
            generated_direct_call_rootless_preferred_native_entry_counts:
                VmGeneratedDirectCallRootlessPreferredNativeEntryCounts {
                    pure_baseline_shim: 4,
                    emitted_semantic_c_abi_entry: 3,
                    unknown: 5,
                },
            baseline_generated_execution_summaries: vec![
                VmBaselineGeneratedExecutionSummary {
                    owner,
                    execution_count: 5,
                    executed_bytecode_count: 30,
                    returned_count: 4,
                    threw_count: 0,
                    ordinary_bytecode_call_count: 0,
                    ordinary_bytecode_construct_count: 0,
                    function_value_call_count: 0,
                    terminated_count: 0,
                    suspended_count: 0,
                    failed_count: 0,
                    fallback_count: 0,
                    js_call_count: 2,
                    property_count: 0,
                    runtime_helper_count: 0,
                    rejected_count: 1,
                },
                VmBaselineGeneratedExecutionSummary {
                    owner: second_owner,
                    execution_count: 1,
                    executed_bytecode_count: 9,
                    returned_count: 0,
                    threw_count: 0,
                    ordinary_bytecode_call_count: 0,
                    ordinary_bytecode_construct_count: 0,
                    function_value_call_count: 0,
                    terminated_count: 0,
                    suspended_count: 0,
                    failed_count: 0,
                    fallback_count: 1,
                    js_call_count: 0,
                    property_count: 0,
                    runtime_helper_count: 0,
                    rejected_count: 0,
                },
            ],
            baseline_generated_dispatched_opcode_counts: vec![
                VmBaselineGeneratedDispatchedOpcodeCount {
                    owner,
                    opcode: CoreOpcode::Call,
                    count: 11,
                },
                VmBaselineGeneratedDispatchedOpcodeCount {
                    owner,
                    opcode: CoreOpcode::AddInt32,
                    count: 3,
                },
                VmBaselineGeneratedDispatchedOpcodeCount {
                    owner: second_owner,
                    opcode: CoreOpcode::LoopHint,
                    count: 1,
                },
            ],
            baseline_generated_dispatched_site_opcode_counts: vec![
                VmBaselineGeneratedDispatchedSiteOpcodeCount {
                    owner,
                    bytecode_index: BytecodeIndex::from_offset(36),
                    opcode: CoreOpcode::GetByName,
                    property_load_sidecar_readiness:
                        BaselineGeneratedPropertyLoadSidecarReadiness::OwnDataPlan,
                    count: 13,
                },
                VmBaselineGeneratedDispatchedSiteOpcodeCount {
                    owner,
                    bytecode_index: BytecodeIndex::from_offset(44),
                    opcode: CoreOpcode::GetByName,
                    property_load_sidecar_readiness:
                        BaselineGeneratedPropertyLoadSidecarReadiness::NoLoadPlan,
                    count: 1,
                },
                VmBaselineGeneratedDispatchedSiteOpcodeCount {
                    owner,
                    bytecode_index: BytecodeIndex::from_offset(44),
                    opcode: CoreOpcode::GetByName,
                    property_load_sidecar_readiness:
                        BaselineGeneratedPropertyLoadSidecarReadiness::GuardedPrototypeData,
                    count: 3,
                },
            ],
            generated_direct_call_transaction_summaries: vec![
                VmGeneratedDirectCallTransactionSummary {
                    caller,
                    call_bytecode_index,
                    target_code_block: target,
                    argument_count_including_this: 2,
                    route: VmGeneratedDirectCallTransactionRoute::GeneratedEntry,
                    transaction_count: 5,
                    continue_count: 2,
                    jump_count: 0,
                    return_count: 3,
                    threw_count: 0,
                    ordinary_bytecode_call_count: 0,
                    ordinary_bytecode_construct_count: 0,
                    function_value_call_count: 0,
                    suspended_count: 0,
                    failed_count: 1,
                },
                VmGeneratedDirectCallTransactionSummary {
                    caller,
                    call_bytecode_index,
                    target_code_block: target,
                    argument_count_including_this: 2,
                    route: VmGeneratedDirectCallTransactionRoute::NativeEntry,
                    transaction_count: 1,
                    continue_count: 0,
                    jump_count: 0,
                    return_count: 1,
                    threw_count: 0,
                    ordinary_bytecode_call_count: 0,
                    ordinary_bytecode_construct_count: 0,
                    function_value_call_count: 0,
                    suspended_count: 0,
                    failed_count: 0,
                },
            ],
            generated_direct_call_callee_fallback_summaries: vec![
                VmGeneratedDirectCallCalleeFallbackSummary {
                    caller,
                    call_bytecode_index,
                    target_code_block: target,
                    argument_count_including_this: 2,
                    preferred_route: None,
                    generated_entry_miss:
                        VmGeneratedDirectCallGeneratedEntryMissReason::MissingArtifact,
                    native_entry_miss: VmGeneratedDirectCallNativeEntryMissReason::MissingGate,
                    fallback_count: 5,
                },
                VmGeneratedDirectCallCalleeFallbackSummary {
                    caller,
                    call_bytecode_index,
                    target_code_block: second_owner,
                    argument_count_including_this: 1,
                    preferred_route: Some(VmGeneratedDirectCallTransactionRoute::GeneratedEntry),
                    generated_entry_miss:
                        VmGeneratedDirectCallGeneratedEntryMissReason::SnapshotMismatch,
                    native_entry_miss:
                        VmGeneratedDirectCallNativeEntryMissReason::HostBlockedX86_64,
                    fallback_count: 1,
                },
            ],
            generated_direct_call_route_opportunity_summaries: vec![
                VmGeneratedDirectCallRouteOpportunitySummary {
                    caller,
                    call_bytecode_index,
                    target_code_block: target,
                    argument_count_including_this: 2,
                    selected_route: VmGeneratedDirectCallTransactionRoute::GeneratedEntry,
                    preferred_route: Some(VmGeneratedDirectCallTransactionRoute::GeneratedEntry),
                    native_entry_miss:
                        VmGeneratedDirectCallNativeEntryMissReason::HostBlockedX86_64,
                    count: 6,
                },
                VmGeneratedDirectCallRouteOpportunitySummary {
                    caller,
                    call_bytecode_index,
                    target_code_block: target,
                    argument_count_including_this: 2,
                    selected_route: VmGeneratedDirectCallTransactionRoute::NativeEntry,
                    preferred_route: Some(VmGeneratedDirectCallTransactionRoute::NativeEntry),
                    native_entry_miss: VmGeneratedDirectCallNativeEntryMissReason::Ready,
                    count: 1,
                },
            ],
            baseline_entry_auto_materializations: 2,
            property_inline_cache_evolution_records: 12,
            property_inline_cache_evolution_admitted: 8,
            property_inline_cache_evolution_buffered: 4,
            property_inline_cache_evolution_buffered_duplicates: 5,
            property_inline_cache_evolution_cooldowns: 3,
            property_inline_cache_evolution_final_gave_up: 2,
            property_inline_cache_evolution_gave_up_skips: 6,
            property_inline_cache_evolution_generated_megamorphic_load: 4,
            property_inline_cache_evolution_megamorphic_load_skips: 8,
            property_inline_cache_evolution_generated_megamorphic_store: 6,
            property_inline_cache_evolution_megamorphic_store_skips: 9,
            property_inline_cache_evolution_generated_megamorphic_has: 4,
            property_inline_cache_evolution_megamorphic_has_skips: 6,
            property_load_megamorphic_cache_records: 9,
            property_store_megamorphic_cache_records: 11,
            property_has_megamorphic_cache_records: 14,
            ..OctaneTieringSummary::default()
        };

        let delta = current.delta_since(start);

        assert_eq!(delta.baseline_generated_code_invalidations, 8);
        assert_eq!(
            delta.baseline_generated_code_invalidation_summary,
            VmBaselineGeneratedInvalidationSummary {
                total: 8,
                accepted: 6,
                no_matching_artifact: 1,
                bytecode_snapshot_mismatch: 1,
                property_load_probe_miss: 5,
                guarded_property_load_probe_miss: 2,
                property_load_guard_watchpoint_invalidation: 2,
                ..VmBaselineGeneratedInvalidationSummary::default()
            }
        );
        assert_eq!(
            delta
                .generated_direct_call_rootless_rejections
                .hot_slot_miss,
            3
        );
        assert_eq!(
            delta
                .generated_direct_call_rootless_rejections
                .effect_contract,
            2
        );
        assert_eq!(
            delta
                .generated_direct_call_rootless_rejections
                .unsupported_body_opcode,
            3
        );
        assert_eq!(
            delta
                .generated_direct_call_rootless_native_entry_rejections
                .missing_generated_artifact,
            2
        );
        assert_eq!(
            delta.generated_direct_call_rootless_unsupported_body_opcode_counts,
            vec![
                VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount {
                    opcode: CoreOpcode::RightShiftInt32,
                    count: 3,
                },
                VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount {
                    opcode: CoreOpcode::UnsignedRightShiftInt32,
                    count: 3,
                },
            ]
        );
        assert_eq!(
            delta.generated_direct_call_rootless_native_entry_unsupported_body_opcode_counts,
            vec![VmGeneratedDirectCallRootlessUnsupportedBodyOpcodeCount {
                opcode: CoreOpcode::LoadInt32,
                count: 3,
            }]
        );
        assert_eq!(
            delta.generated_direct_call_rootless_native_entry_retained_side_exit_counts,
            vec![
                VmGeneratedDirectCallRootlessRetainedSideExitCount {
                    target_code_block: target,
                    bytecode_index: side_exit_bytecode_index,
                    opcode: Some(CoreOpcode::AddInt32),
                    reason: P6X86_64BaselineSelectedSideExitReason::NonInt32Operand,
                    count: 3,
                },
                VmGeneratedDirectCallRootlessRetainedSideExitCount {
                    target_code_block: target,
                    bytecode_index: BytecodeIndex::from_offset(28),
                    opcode: Some(CoreOpcode::MulInt32),
                    reason: P6X86_64BaselineSelectedSideExitReason::NegativeZero,
                    count: 1,
                },
            ]
        );
        assert_eq!(
            delta.generated_direct_call_rootless_preferred_native_entry_counts,
            VmGeneratedDirectCallRootlessPreferredNativeEntryCounts {
                pure_baseline_shim: 3,
                emitted_semantic_c_abi_entry: 0,
                unknown: 3,
            }
        );
        assert_eq!(delta.baseline_generated_execution_summaries.len(), 2);
        assert_eq!(
            delta.baseline_generated_execution_summaries[0],
            VmBaselineGeneratedExecutionSummary {
                owner,
                execution_count: 3,
                executed_bytecode_count: 20,
                returned_count: 3,
                threw_count: 0,
                ordinary_bytecode_call_count: 0,
                ordinary_bytecode_construct_count: 0,
                function_value_call_count: 0,
                terminated_count: 0,
                suspended_count: 0,
                failed_count: 0,
                fallback_count: 0,
                js_call_count: 1,
                property_count: 0,
                runtime_helper_count: 0,
                rejected_count: 1,
            }
        );
        assert_eq!(
            delta.baseline_generated_execution_summaries[1],
            VmBaselineGeneratedExecutionSummary {
                owner: second_owner,
                execution_count: 1,
                executed_bytecode_count: 9,
                returned_count: 0,
                threw_count: 0,
                ordinary_bytecode_call_count: 0,
                ordinary_bytecode_construct_count: 0,
                function_value_call_count: 0,
                terminated_count: 0,
                suspended_count: 0,
                failed_count: 0,
                fallback_count: 1,
                js_call_count: 0,
                property_count: 0,
                runtime_helper_count: 0,
                rejected_count: 0,
            }
        );
        assert_eq!(
            delta.baseline_generated_dispatched_opcode_counts,
            vec![
                VmBaselineGeneratedDispatchedOpcodeCount {
                    owner,
                    opcode: CoreOpcode::Call,
                    count: 7,
                },
                VmBaselineGeneratedDispatchedOpcodeCount {
                    owner,
                    opcode: CoreOpcode::AddInt32,
                    count: 3,
                },
                VmBaselineGeneratedDispatchedOpcodeCount {
                    owner: second_owner,
                    opcode: CoreOpcode::LoopHint,
                    count: 1,
                },
            ]
        );
        assert_eq!(
            delta.baseline_generated_dispatched_site_opcode_counts,
            vec![
                VmBaselineGeneratedDispatchedSiteOpcodeCount {
                    owner,
                    bytecode_index: BytecodeIndex::from_offset(36),
                    opcode: CoreOpcode::GetByName,
                    property_load_sidecar_readiness:
                        BaselineGeneratedPropertyLoadSidecarReadiness::OwnDataPlan,
                    count: 8,
                },
                VmBaselineGeneratedDispatchedSiteOpcodeCount {
                    owner,
                    bytecode_index: BytecodeIndex::from_offset(44),
                    opcode: CoreOpcode::GetByName,
                    property_load_sidecar_readiness:
                        BaselineGeneratedPropertyLoadSidecarReadiness::GuardedPrototypeData,
                    count: 3,
                },
            ]
        );
        assert_eq!(delta.generated_direct_call_transaction_summaries.len(), 2);
        assert_eq!(
            delta.generated_direct_call_transaction_summaries[0],
            VmGeneratedDirectCallTransactionSummary {
                caller,
                call_bytecode_index,
                target_code_block: target,
                argument_count_including_this: 2,
                route: VmGeneratedDirectCallTransactionRoute::GeneratedEntry,
                transaction_count: 3,
                continue_count: 1,
                jump_count: 0,
                return_count: 2,
                threw_count: 0,
                ordinary_bytecode_call_count: 0,
                ordinary_bytecode_construct_count: 0,
                function_value_call_count: 0,
                suspended_count: 0,
                failed_count: 1,
            }
        );
        assert_eq!(
            delta.generated_direct_call_transaction_summaries[1],
            VmGeneratedDirectCallTransactionSummary {
                caller,
                call_bytecode_index,
                target_code_block: target,
                argument_count_including_this: 2,
                route: VmGeneratedDirectCallTransactionRoute::NativeEntry,
                transaction_count: 1,
                continue_count: 0,
                jump_count: 0,
                return_count: 1,
                threw_count: 0,
                ordinary_bytecode_call_count: 0,
                ordinary_bytecode_construct_count: 0,
                function_value_call_count: 0,
                suspended_count: 0,
                failed_count: 0,
            }
        );
        assert_eq!(
            delta.generated_direct_call_callee_fallback_summaries,
            vec![
                VmGeneratedDirectCallCalleeFallbackSummary {
                    caller,
                    call_bytecode_index,
                    target_code_block: target,
                    argument_count_including_this: 2,
                    preferred_route: None,
                    generated_entry_miss:
                        VmGeneratedDirectCallGeneratedEntryMissReason::MissingArtifact,
                    native_entry_miss: VmGeneratedDirectCallNativeEntryMissReason::MissingGate,
                    fallback_count: 3,
                },
                VmGeneratedDirectCallCalleeFallbackSummary {
                    caller,
                    call_bytecode_index,
                    target_code_block: second_owner,
                    argument_count_including_this: 1,
                    preferred_route: Some(VmGeneratedDirectCallTransactionRoute::GeneratedEntry),
                    generated_entry_miss:
                        VmGeneratedDirectCallGeneratedEntryMissReason::SnapshotMismatch,
                    native_entry_miss:
                        VmGeneratedDirectCallNativeEntryMissReason::HostBlockedX86_64,
                    fallback_count: 1,
                },
            ]
        );
        assert_eq!(
            delta.generated_direct_call_route_opportunity_summaries,
            vec![
                VmGeneratedDirectCallRouteOpportunitySummary {
                    caller,
                    call_bytecode_index,
                    target_code_block: target,
                    argument_count_including_this: 2,
                    selected_route: VmGeneratedDirectCallTransactionRoute::GeneratedEntry,
                    preferred_route: Some(VmGeneratedDirectCallTransactionRoute::GeneratedEntry),
                    native_entry_miss:
                        VmGeneratedDirectCallNativeEntryMissReason::HostBlockedX86_64,
                    count: 4,
                },
                VmGeneratedDirectCallRouteOpportunitySummary {
                    caller,
                    call_bytecode_index,
                    target_code_block: target,
                    argument_count_including_this: 2,
                    selected_route: VmGeneratedDirectCallTransactionRoute::NativeEntry,
                    preferred_route: Some(VmGeneratedDirectCallTransactionRoute::NativeEntry),
                    native_entry_miss: VmGeneratedDirectCallNativeEntryMissReason::Ready,
                    count: 1,
                },
            ]
        );
        assert_eq!(delta.baseline_entry_auto_materializations, 1);
        assert_eq!(delta.property_inline_cache_evolution_records, 5);
        assert_eq!(delta.property_inline_cache_evolution_admitted, 3);
        assert_eq!(delta.property_inline_cache_evolution_buffered, 1);
        assert_eq!(delta.property_inline_cache_evolution_buffered_duplicates, 3);
        assert_eq!(delta.property_inline_cache_evolution_cooldowns, 2);
        assert_eq!(delta.property_inline_cache_evolution_final_gave_up, 1);
        assert_eq!(delta.property_inline_cache_evolution_gave_up_skips, 4);
        assert_eq!(
            delta.property_inline_cache_evolution_generated_megamorphic_load,
            3
        );
        assert_eq!(
            delta.property_inline_cache_evolution_megamorphic_load_skips,
            5
        );
        assert_eq!(
            delta.property_inline_cache_evolution_generated_megamorphic_store,
            4
        );
        assert_eq!(
            delta.property_inline_cache_evolution_megamorphic_store_skips,
            5
        );
        assert_eq!(
            delta.property_inline_cache_evolution_generated_megamorphic_has,
            3
        );
        assert_eq!(
            delta.property_inline_cache_evolution_megamorphic_has_skips,
            4
        );
        assert_eq!(delta.property_load_megamorphic_cache_records, 5);
        assert_eq!(delta.property_store_megamorphic_cache_records, 5);
        assert_eq!(delta.property_has_megamorphic_cache_records, 6);
    }

    #[test]
    fn octane_execution_defaults_to_unbounded_dispatch_budget() {
        let config = OctaneExecutionConfig::new(
            OctaneExecutionMode::InterpreterOnly,
            OctaneSuiteFailurePolicy::FailFast,
        );

        assert_eq!(config.dispatch_config, DispatchConfig::unbounded());
    }

    #[test]
    fn octane_execution_honors_explicit_dispatch_budget() {
        let root = TempJetStreamRoot::new();
        root.write_manifest_file(
            "./Octane/test-function.js",
            minimal_function_benchmark_source(),
        );
        let prepared = prepare_test_benchmark(&root, &OCTANE_TEST_FUNCTION_PLAN);

        let report = execute_prepared_octane_benchmark(
            &prepared,
            OctaneExecutionConfig::new(
                OctaneExecutionMode::InterpreterOnly,
                OctaneSuiteFailurePolicy::FailFast,
            )
            .with_dispatch_config(DispatchConfig::new(0)),
        );

        assert!(matches!(
            report.outcome,
            OctaneExecutionOutcome::Failed(OctaneExecutionFailure {
                detail: OctaneExecutionFailureDetail::Completion(ExecutionCompletion::Failed(
                    ExecutionError::DispatchStepLimitExceeded
                )),
                ..
            })
        ));
    }

    #[test]
    fn octane_execution_progress_reports_source_boundaries_in_order() {
        let root = TempJetStreamRoot::new();
        root.write_manifest_file(
            "./Octane/test-function.js",
            deterministic_timing_benchmark_source(),
        );
        let prepared = prepare_test_benchmark(&root, &OCTANE_TEST_FUNCTION_PLAN);
        let mut events = Vec::new();

        let report = execute_prepared_octane_benchmark_with_progress(
            &prepared,
            OctaneExecutionConfig::new(
                OctaneExecutionMode::InterpreterOnly,
                OctaneSuiteFailurePolicy::FailFast,
            ),
            &mut |event| events.push(event),
        );

        assert!(report.outcome.is_success(), "{report:#?}");
        assert_eq!(
            events
                .iter()
                .map(octane_progress_phase_name)
                .collect::<Vec<_>>(),
            vec![
                "benchmark-start",
                "session-open-start",
                "session-open-done",
                "source-start",
                "source-done",
                "source-start",
                "source-done",
                "source-start",
                "source-done",
                "score-telemetry-start",
            ]
        );

        let source_starts = events
            .iter()
            .filter_map(|event| match event {
                OctaneExecutionProgress::SourceStarted {
                    order_index,
                    order_entry,
                    label,
                    ..
                } => Some((*order_index, *order_entry, label.as_str())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            source_starts,
            vec![
                (
                    0,
                    OctanePreparedSourceOrderEntry::Generated(
                        OctanePreparedGeneratedSourceKind::Prelude
                    ),
                    "octane://test-function/prelude",
                ),
                (
                    1,
                    OctanePreparedSourceOrderEntry::BenchmarkFile(0),
                    "test-function:./Octane/test-function.js",
                ),
                (
                    2,
                    OctanePreparedSourceOrderEntry::Generated(
                        OctanePreparedGeneratedSourceKind::Runner
                    ),
                    "octane://test-function/runner",
                ),
            ]
        );
    }

    #[test]
    fn octane_executes_class_style_benchmark_and_extracts_score_telemetry() {
        let root = TempJetStreamRoot::new();
        root.write_manifest_file(
            "./Octane/test-class.js",
            "\
class Benchmark {
    constructor() {}
    runIteration() {}
    validate() {}
}
",
        );
        let prepared = prepare_test_benchmark(&root, &OCTANE_TEST_CLASS_PLAN);

        let report = execute_prepared_octane_benchmark(
            &prepared,
            OctaneExecutionConfig::new(
                OctaneExecutionMode::InterpreterOnly,
                OctaneSuiteFailurePolicy::FailFast,
            ),
        );

        let OctaneExecutionOutcome::Succeeded(success) = &report.outcome else {
            panic!("expected success: {report:#?}");
        };
        assert_eq!(
            success.telemetry.validation_state,
            OctaneBenchmarkValidationState::Passed
        );
        assert_eq!(success.telemetry.elapsed_times_ms.len(), 3);
        assert!(success
            .telemetry
            .elapsed_times_ms
            .iter()
            .all(|time| *time >= 1.0));
        assert_eq!(report.source_records.len(), prepared.source_order.len());
        assert!(report
            .source_records
            .iter()
            .all(|record| matches!(record.completion, Some(ExecutionCompletion::Returned(_)))));
    }

    #[test]
    fn octane_oracle_alert_is_reported_as_runner_execution_failure() {
        let root = TempJetStreamRoot::new();
        root.write_manifest_file(
            "./Octane/test-function.js",
            minimal_function_benchmark_source(),
        );
        let prepared = prepare_test_benchmark(&root, &OCTANE_TEST_FUNCTION_PLAN);

        let failure = classify_octane_oracle_alert(
            &prepared,
            &[octane_alert_record("Octane validation failed")],
        )
        .expect("alert should classify as oracle failure");

        assert_eq!(failure.phase, OctaneExecutionPhase::ThrownOrOracle);
        assert_eq!(
            failure.order_entry,
            Some(OctanePreparedSourceOrderEntry::Generated(
                OctanePreparedGeneratedSourceKind::Runner
            ))
        );
        assert!(failure
            .label
            .as_deref()
            .is_some_and(|label| label.contains("runner")));
        assert!(matches!(
            failure.detail,
            OctaneExecutionFailureDetail::OracleAlert(CoreHostOutputRecord {
                sink: CoreHostOutputSink::Alert,
                ..
            })
        ));
    }

    #[test]
    fn octane_mode_comparison_records_interpreter_and_baseline_outcomes() {
        let root = TempJetStreamRoot::new();
        root.write_manifest_file(
            "./Octane/test-function.js",
            minimal_function_benchmark_source(),
        );
        let prepared = prepare_test_benchmark(&root, &OCTANE_TEST_FUNCTION_PLAN);

        let comparison = execute_prepared_octane_benchmark_mode_comparison(
            &prepared,
            OctaneSuiteFailurePolicy::FailFast,
        );

        assert_eq!(comparison.benchmark, "test-function");
        assert_eq!(
            comparison.interpreter_only.mode,
            OctaneExecutionMode::InterpreterOnly
        );
        assert_eq!(
            comparison.baseline_allowed.mode,
            OctaneExecutionMode::BaselineAllowed
        );
        assert!(comparison.interpreter_only.outcome.is_success());
        assert!(
            comparison.baseline_allowed.outcome.is_success()
                || comparison.baseline_allowed.outcome.phase()
                    == OctaneExecutionPhase::BaselineOnly,
            "{comparison:#?}"
        );
    }

    #[test]
    fn octane_do_while_gets_past_parse_and_lowering() {
        let root = TempJetStreamRoot::new();
        root.write_manifest_file(
            "./Octane/test-unsupported.js",
            "\
function Benchmark(iterations) {
}
Benchmark.prototype.runIteration = function() {
    let value = 0;
    do {
        value = value + 1;
    } while (false);
    return value;
};
Benchmark.prototype.validate = function() {
    return 1;
};
",
        );
        let prepared = prepare_test_benchmark(&root, &OCTANE_TEST_UNSUPPORTED_PLAN);

        let report = execute_prepared_octane_benchmark(
            &prepared,
            OctaneExecutionConfig::new(
                OctaneExecutionMode::InterpreterOnly,
                OctaneSuiteFailurePolicy::FailFast,
            ),
        );

        assert_ne!(report.outcome.phase(), OctaneExecutionPhase::Parse);
        assert_ne!(report.outcome.phase(), OctaneExecutionPhase::BytecodeEmit);
        assert!(matches!(
            report.outcome,
            OctaneExecutionOutcome::Succeeded(_)
                | OctaneExecutionOutcome::Failed(OctaneExecutionFailure {
                    phase: OctaneExecutionPhase::ExecuteRuntime,
                    ..
                })
        ));
    }

    #[test]
    fn octane_baseline_allowed_mode_is_accepted_and_recorded() {
        let root = TempJetStreamRoot::new();
        root.write_manifest_file(
            "./Octane/test-function.js",
            minimal_function_benchmark_source(),
        );
        let prepared = prepare_test_benchmark(&root, &OCTANE_TEST_FUNCTION_PLAN);

        let report = execute_prepared_octane_benchmark(
            &prepared,
            OctaneExecutionConfig::new(
                OctaneExecutionMode::BaselineAllowed,
                OctaneSuiteFailurePolicy::FailFast,
            ),
        );

        assert_eq!(report.mode, OctaneExecutionMode::BaselineAllowed);
        assert_eq!(report.benchmark, "test-function");
        assert!(
            matches!(report.outcome, OctaneExecutionOutcome::Succeeded(_))
                || report.outcome.phase() == OctaneExecutionPhase::BaselineOnly,
            "{report:#?}"
        );
    }

    #[test]
    fn octane_suite_execution_respects_fail_fast_and_collect_all_policy() {
        let root = TempJetStreamRoot::new();
        root.write_manifest_file(
            "./Octane/test-unsupported.js",
            "\
function Benchmark() {
",
        );
        root.write_manifest_file(
            "./Octane/test-second.js",
            minimal_function_benchmark_source(),
        );
        let failing = prepare_test_benchmark(&root, &OCTANE_TEST_UNSUPPORTED_PLAN);
        let second = prepare_test_benchmark(&root, &OCTANE_TEST_SECOND_PLAN);
        let prepared_suite = OctanePreparedSuite {
            config: OctanePreparationConfig::new(
                root.path(),
                OctaneRunConfig::new(OctaneSuite::Core),
            ),
            benchmarks: vec![failing, second],
        };

        let fail_fast = execute_prepared_octane_suite(
            &prepared_suite,
            OctaneExecutionConfig::new(
                OctaneExecutionMode::InterpreterOnly,
                OctaneSuiteFailurePolicy::FailFast,
            ),
        );
        assert!(fail_fast.stopped_early);
        assert_eq!(fail_fast.failure_policy, OctaneSuiteFailurePolicy::FailFast);
        assert_eq!(fail_fast.benchmarks.len(), 1);
        assert_eq!(fail_fast.benchmarks[0].benchmark, "test-unsupported");
        assert_eq!(
            fail_fast.benchmarks[0].outcome.phase(),
            OctaneExecutionPhase::Parse
        );

        let collect_all = execute_prepared_octane_suite(
            &prepared_suite,
            OctaneExecutionConfig::new(
                OctaneExecutionMode::InterpreterOnly,
                OctaneSuiteFailurePolicy::CollectAll,
            ),
        );
        assert!(!collect_all.stopped_early);
        assert_eq!(
            collect_all.failure_policy,
            OctaneSuiteFailurePolicy::CollectAll
        );
        assert_eq!(collect_all.benchmarks.len(), 2);
        assert_eq!(collect_all.benchmarks[0].benchmark, "test-unsupported");
        assert_eq!(collect_all.benchmarks[1].benchmark, "test-second");
        assert!(collect_all.benchmarks[1].outcome.is_success());
        assert!(collect_all.suite_score.is_none());
    }

    #[test]
    fn octane_suite_execution_uses_jetstream_reverse_case_insensitive_order() {
        let root = TempJetStreamRoot::new();
        root.write_manifest_file(
            "./Octane/alpha-test.js",
            minimal_function_benchmark_source(),
        );
        root.write_manifest_file(
            "./Octane/test-second.js",
            minimal_function_benchmark_source(),
        );
        root.write_manifest_file("./Octane/zulu-test.js", minimal_function_benchmark_source());
        let alpha = prepare_test_benchmark(&root, &OCTANE_TEST_ALPHA_PLAN);
        let second = prepare_test_benchmark(&root, &OCTANE_TEST_SECOND_PLAN);
        let zulu = prepare_test_benchmark(&root, &OCTANE_TEST_ZULU_PLAN);
        let prepared_suite = OctanePreparedSuite {
            config: OctanePreparationConfig::new(
                root.path(),
                OctaneRunConfig::new(OctaneSuite::Core),
            ),
            benchmarks: vec![alpha, second, zulu],
        };

        let report = execute_prepared_octane_suite(
            &prepared_suite,
            OctaneExecutionConfig::new(
                OctaneExecutionMode::InterpreterOnly,
                OctaneSuiteFailurePolicy::CollectAll,
            ),
        );

        assert_eq!(
            report
                .benchmarks
                .iter()
                .map(|benchmark| benchmark.benchmark)
                .collect::<Vec<_>>(),
            vec!["zulu-test", "test-second", "alpha-test"]
        );
        assert!(
            report
                .benchmarks
                .iter()
                .all(|benchmark| benchmark.outcome.is_success()),
            "{report:#?}"
        );
        assert_eq!(
            report
                .suite_score
                .as_ref()
                .expect("all benchmark scores should produce a suite score")
                .benchmark_scores
                .iter()
                .map(|record| record.benchmark)
                .collect::<Vec<_>>(),
            vec!["zulu-test", "test-second", "alpha-test"]
        );
    }
}
