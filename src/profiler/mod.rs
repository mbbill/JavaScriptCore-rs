//! Profiler contracts.
//!
//! Profilers observe execution, allocation, type flow, and control flow. They
//! must not own VM state; this module records sample and report shapes.

use crate::bytecode::{BytecodeIndex, CodeOrigin, FullCodeOrigin};
use crate::gc::{HeapId, HeapSnapshotId, HeapSnapshotKind};
use crate::jit::{JitType, TierFallbackResultRecord, TieringState};
use crate::runtime::{CodeBlockId, StackFrameId};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct ProfilerRunId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct ProfilerHeapObservationId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfilerKind {
    Sampling,
    Type,
    ControlFlow,
    Heap,
    Bytecode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfilerHeapObservationKind {
    SnapshotStarted,
    SnapshotFinished,
    AllocationSample,
    RetainedSizeSample,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfilerHeapObservationRecord {
    pub id: ProfilerHeapObservationId,
    pub run: ProfilerRunId,
    pub kind: ProfilerHeapObservationKind,
    pub heap: Option<HeapId>,
    pub snapshot: Option<HeapSnapshotId>,
    pub snapshot_kind: HeapSnapshotKind,
    pub code_block: Option<CodeBlockId>,
    pub stack_frame: Option<StackFrameId>,
    pub observed_bytes: usize,
    pub dropped_samples: u32,
}

/// Owner of immutable profiler event and counter schemas.
///
/// The profiler database owns live samples and mutable counters. Static schema
/// owners only publish descriptor tables that reporting code may borrow.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ProfilerSchemaOwner {
    #[default]
    ProfilerDatabase,
    SamplingProfiler,
    BytecodeProfiler,
    GeneratedProfilerMetadata,
    TestFixture,
}

/// Authority allowed to replace profiler schema registries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum ProfilerRegistryMutationAuthority {
    #[default]
    GeneratedDataRefresh,
    CrateInitialization,
    ProfilerDatabaseInitialization,
}

/// Provenance for generated profiler descriptor metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ProfilerSchemaProvenance {
    pub generator: &'static str,
    pub source: &'static str,
    pub revision: u64,
}

impl ProfilerSchemaProvenance {
    pub const fn new(generator: &'static str, source: &'static str, revision: u64) -> Self {
        Self {
            generator,
            source,
            revision,
        }
    }
}

/// Static counter identity used by profiler reports.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ProfilerCounterKind {
    ExecutionCount,
    OsrExitCount,
    InlineCacheHit,
    InlineCacheMiss,
    JitCompilationCount,
    AllocationCount,
    DroppedSampleCount,
}

/// Immutable counter descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfilerCounterSchema {
    pub kind: ProfilerCounterKind,
    pub name: &'static str,
    pub unit: &'static str,
    pub owner: ProfilerSchemaOwner,
    pub mutation_authority: ProfilerRegistryMutationAuthority,
    pub provenance: ProfilerSchemaProvenance,
}

impl ProfilerCounterSchema {
    pub fn validate(self) -> Result<(), ProfilerValidationError> {
        validate_non_empty(self.name, ProfilerValidationError::EmptyCounterName)?;
        validate_non_empty(self.unit, ProfilerValidationError::EmptyCounterUnit)?;
        validate_non_empty(
            self.provenance.generator,
            ProfilerValidationError::EmptyProvenanceField,
        )?;
        validate_non_empty(
            self.provenance.source,
            ProfilerValidationError::EmptyProvenanceField,
        )
    }
}

/// Static profiler event identity used by reports.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ProfilerEventKind {
    CompilationStarted,
    CompilationFinished,
    CodeBlockJettisoned,
    OsrExitRecorded,
    SampleRecorded,
    TypeInformationRecorded,
    ControlFlowBlockRecorded,
    HeapSnapshotRecorded,
}

/// Immutable event descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfilerEventSchema {
    pub kind: ProfilerEventKind,
    pub name: &'static str,
    pub profiler_kind: ProfilerKind,
    pub counters: &'static [ProfilerCounterKind],
    pub owner: ProfilerSchemaOwner,
    pub mutation_authority: ProfilerRegistryMutationAuthority,
    pub provenance: ProfilerSchemaProvenance,
}

impl ProfilerEventSchema {
    pub const fn counters(self) -> &'static [ProfilerCounterKind] {
        self.counters
    }

    pub fn validate(self) -> Result<(), ProfilerValidationError> {
        validate_non_empty(self.name, ProfilerValidationError::EmptyEventName)?;
        validate_unique_counter_kinds(self.counters)?;
        validate_non_empty(
            self.provenance.generator,
            ProfilerValidationError::EmptyProvenanceField,
        )?;
        validate_non_empty(
            self.provenance.source,
            ProfilerValidationError::EmptyProvenanceField,
        )
    }
}

/// Structural profiler descriptor validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfilerValidationError {
    EmptyCounterName,
    EmptyCounterUnit,
    EmptyEventName,
    EmptyProvenanceField,
    DuplicateCounterKind(ProfilerCounterKind),
    DuplicateCounterName(&'static str),
    DuplicateEventKind(ProfilerEventKind),
    DuplicateEventName(&'static str),
    EventReferencesMissingCounter {
        event: ProfilerEventKind,
        counter: ProfilerCounterKind,
    },
    BytecodeSequenceMissingColumns,
    BytecodeSequenceDuplicateIndex(BytecodeIndex),
    OriginStackMissingFrames,
    CompilationRecordMissingProfiledBytecodes,
    OsrExitRecordMissingCount,
    EventRecordMissingSummary,
    EventRecordMissingSchema(String),
    ReportDroppedSamplesMismatch,
    BytecodeEventMissingCompilationKind(String),
    SamplingEventHasCompilationKind(String),
    ExecutionEventRecordMissingCount,
    ExecutionEventRecordInvalidOrigin(FullCodeOrigin),
}

/// Registry of immutable profiler event and counter schemas.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProfilerSchemaRegistry {
    pub events: &'static [ProfilerEventSchema],
    pub counters: &'static [ProfilerCounterSchema],
}

impl ProfilerSchemaRegistry {
    pub const fn new(
        events: &'static [ProfilerEventSchema],
        counters: &'static [ProfilerCounterSchema],
    ) -> Self {
        Self { events, counters }
    }

    pub const fn events(self) -> &'static [ProfilerEventSchema] {
        self.events
    }

    pub const fn counters(self) -> &'static [ProfilerCounterSchema] {
        self.counters
    }

    pub fn event(self, kind: ProfilerEventKind) -> Option<&'static ProfilerEventSchema> {
        self.events
            .iter()
            .find(|descriptor| descriptor.kind == kind)
    }

    pub fn event_by_name(self, name: &str) -> Option<&'static ProfilerEventSchema> {
        self.events
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn counter(self, kind: ProfilerCounterKind) -> Option<&'static ProfilerCounterSchema> {
        self.counters
            .iter()
            .find(|descriptor| descriptor.kind == kind)
    }

    pub fn validate(self) -> Result<(), ProfilerValidationError> {
        validate_unique_counter_schemas(self.counters)?;
        validate_unique_event_schemas(self.events)?;
        for counter in self.counters {
            counter.validate()?;
        }
        for event in self.events {
            event.validate()?;
            for counter in event.counters {
                if self.counter(*counter).is_none() {
                    return Err(ProfilerValidationError::EventReferencesMissingCounter {
                        event: event.kind,
                        counter: *counter,
                    });
                }
            }
        }
        Ok(())
    }
}

/// Per-bytecode profiler compilation tier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfilerCompilationKind {
    LlInt,
    Baseline,
    Dfg,
    UnlinkedDfg,
    Ftl,
    FtlForOsrEntry,
}

/// Reason compiled code was jettisoned from profiler accounting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfilerJettisonReason {
    NotJettisoned,
    WeakReference,
    DebuggerBreakpoint,
    DebuggerStepping,
    BaselineLoopReoptimizationTrigger,
    BaselineLoopReoptimizationTriggerOnOsrEntryFail,
    OsrExit,
    ProfiledWatchpoint,
    UnprofiledWatchpoint,
    OldAge,
    VmTraps,
}

/// Lifetime of the per-bytecode profiler database.
///
/// The database is VM-owned, lock-protected, and may register itself for an
/// at-exit save. CodeBlock destruction notifies it so pointer-indexed bytecode
/// and compilation maps do not imply ownership of the destroyed CodeBlock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfilerDatabaseLifecycle {
    Created,
    Recording,
    SaveRegisteredAtExit,
    Saving,
    Destroying,
}

/// Bytecode description entry in a profiler sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilerBytecodeDescriptor {
    pub bytecode_index: BytecodeIndex,
    pub opcode_ordinal: u32,
    pub description: String,
}

/// Sequence contract from `Profiler::BytecodeSequence`.
///
/// The sequence is ordered by profiler display order, not directly by bytecode
/// index. Consumers must carry both indexes when correlating samples.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilerBytecodeSequence {
    pub owner: Option<CodeBlockId>,
    pub header_columns: Vec<String>,
    pub bytecodes: Vec<ProfilerBytecodeDescriptor>,
}

impl ProfilerBytecodeSequence {
    pub fn validate(&self) -> Result<(), ProfilerValidationError> {
        if self.header_columns.is_empty() {
            return Err(ProfilerValidationError::BytecodeSequenceMissingColumns);
        }
        for (index, bytecode) in self.bytecodes.iter().enumerate() {
            for other in self.bytecodes.iter().skip(index + 1) {
                if bytecode.bytecode_index == other.bytecode_index {
                    return Err(ProfilerValidationError::BytecodeSequenceDuplicateIndex(
                        bytecode.bytecode_index,
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Origin stack identity for inlined or generated profiler records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilerOriginStackDescriptor {
    pub frames_from_bottom: Vec<StackFrameId>,
    pub may_be_non_bytecode_machine_code: bool,
}

impl ProfilerOriginStackDescriptor {
    pub fn validate(&self) -> Result<(), ProfilerValidationError> {
        if self.frames_from_bottom.is_empty() && !self.may_be_non_bytecode_machine_code {
            Err(ProfilerValidationError::OriginStackMissingFrames)
        } else {
            Ok(())
        }
    }
}

/// Mutable execution counter owned by a compilation record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilerExecutionCounterDescriptor {
    pub origin: ProfilerOriginStackDescriptor,
    pub count: u64,
}

/// OSR exit record and counter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilerOsrExitDescriptor {
    pub exit_id: u32,
    pub origin: ProfilerOriginStackDescriptor,
    pub exit_kind_ordinal: u32,
    pub is_watchpoint: bool,
    pub count: u64,
}

/// Profiler compilation record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilerCompilationRecord {
    pub run: ProfilerRunId,
    pub code_block: Option<CodeBlockId>,
    pub kind: ProfilerCompilationKind,
    pub profiled_bytecode_count: usize,
    pub inlined_get_by_id_count: u32,
    pub inlined_put_by_id_count: u32,
    pub inlined_call_count: u32,
    pub jettison_reason: ProfilerJettisonReason,
    pub counters: Vec<ProfilerExecutionCounterDescriptor>,
    pub osr_exits: Vec<ProfilerOsrExitDescriptor>,
}

impl ProfilerCompilationRecord {
    pub fn validate(&self) -> Result<(), ProfilerValidationError> {
        if self.profiled_bytecode_count == 0 {
            return Err(ProfilerValidationError::CompilationRecordMissingProfiledBytecodes);
        }
        for counter in &self.counters {
            counter.origin.validate()?;
        }
        for exit in &self.osr_exits {
            exit.origin.validate()?;
            if exit.count == 0 {
                return Err(ProfilerValidationError::OsrExitRecordMissingCount);
            }
        }
        Ok(())
    }
}

/// Event logged against bytecodes and an optional compilation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilerEventRecord {
    pub run: ProfilerRunId,
    pub code_block: Option<CodeBlockId>,
    pub compilation_kind: Option<ProfilerCompilationKind>,
    pub summary: String,
    pub detail: String,
}

impl ProfilerEventRecord {
    pub fn validate(&self) -> Result<(), ProfilerValidationError> {
        if self.summary.is_empty() {
            Err(ProfilerValidationError::EventRecordMissingSummary)
        } else {
            Ok(())
        }
    }
}

/// Execution event recorded against a bytecode origin without owning code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfilerExecutionEventRecord {
    pub run: ProfilerRunId,
    pub origin: Option<FullCodeOrigin>,
    pub kind: ProfilerEventKind,
    pub count: u64,
}

impl ProfilerExecutionEventRecord {
    pub fn validate(self) -> Result<(), ProfilerValidationError> {
        if self.count == 0 {
            return Err(ProfilerValidationError::ExecutionEventRecordMissingCount);
        }
        if let Some(origin) = self.origin {
            if !origin.origin.is_set() {
                return Err(ProfilerValidationError::ExecutionEventRecordInvalidOrigin(
                    origin,
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfilerSample {
    pub run: ProfilerRunId,
    pub frame: Option<StackFrameId>,
    pub code_block: Option<CodeBlockId>,
    pub bytecode_index: Option<BytecodeIndex>,
    pub kind: ProfilerKind,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProfilerReport {
    pub samples: Vec<ProfilerSample>,
    pub compilations: Vec<ProfilerCompilationRecord>,
    pub events: Vec<ProfilerEventRecord>,
    pub execution_events: Vec<ProfilerExecutionEventRecord>,
    pub dropped_sample_count: u64,
}

impl ProfilerReport {
    pub fn validate(&self) -> Result<(), ProfilerValidationError> {
        for compilation in &self.compilations {
            compilation.validate()?;
        }
        for event in &self.events {
            event.validate()?;
        }
        for event in &self.execution_events {
            event.validate()?;
        }
        let dropped_samples = self
            .samples
            .iter()
            .filter(|sample| sample.kind == ProfilerKind::Sampling && sample.frame.is_none())
            .count() as u64;
        if self.dropped_sample_count < dropped_samples {
            return Err(ProfilerValidationError::ReportDroppedSamplesMismatch);
        }
        Ok(())
    }

    pub fn aggregate(
        &self,
        registry: ProfilerSchemaRegistry,
    ) -> Result<ProfilerReportAggregation, ProfilerValidationError> {
        registry.validate()?;
        self.validate()?;

        let mut events = Vec::new();
        for schema in registry.events {
            let event_count = self
                .events
                .iter()
                .filter(|event| event.summary == schema.name)
                .count();
            let counters = schema
                .counters
                .iter()
                .filter_map(|kind| {
                    aggregate_counter(*kind, self)
                        .map(|total| ProfilerCounterAggregation { kind: *kind, total })
                })
                .collect();
            events.push(ProfilerEventAggregation {
                kind: schema.kind,
                profiler_kind: schema.profiler_kind,
                event_count,
                counters,
            });
        }

        for event in &self.events {
            if registry
                .events
                .iter()
                .all(|schema| schema.name != event.summary)
            {
                return Err(ProfilerValidationError::EventRecordMissingSchema(
                    event.summary.clone(),
                ));
            }
        }

        Ok(ProfilerReportAggregation {
            sample_count: self.samples.len(),
            dropped_sample_count: self.dropped_sample_count,
            compilation_count: self.compilations.len(),
            events,
        })
    }

    pub fn semantic_aggregation(
        &self,
        registry: ProfilerSchemaRegistry,
    ) -> Result<ProfilerSemanticAggregation, ProfilerValidationError> {
        let aggregation = self.aggregate(registry)?;
        for event in &self.events {
            let schema = registry.event_by_name(&event.summary).ok_or_else(|| {
                ProfilerValidationError::EventRecordMissingSchema(event.summary.clone())
            })?;
            match schema.profiler_kind {
                ProfilerKind::Bytecode if event.compilation_kind.is_none() => {
                    return Err(
                        ProfilerValidationError::BytecodeEventMissingCompilationKind(
                            event.summary.clone(),
                        ),
                    );
                }
                ProfilerKind::Sampling if event.compilation_kind.is_some() => {
                    return Err(ProfilerValidationError::SamplingEventHasCompilationKind(
                        event.summary.clone(),
                    ));
                }
                _ => {}
            }
        }

        let mut runs = Vec::new();
        for sample in &self.samples {
            push_unique_run(&mut runs, sample.run);
        }
        for compilation in &self.compilations {
            push_unique_run(&mut runs, compilation.run);
        }
        for event in &self.events {
            push_unique_run(&mut runs, event.run);
        }
        for event in &self.execution_events {
            push_unique_run(&mut runs, event.run);
        }

        Ok(ProfilerSemanticAggregation {
            aggregation,
            run_count: runs.len(),
            bytecode_event_records: self
                .events
                .iter()
                .filter(|event| {
                    registry
                        .event_by_name(&event.summary)
                        .is_some_and(|schema| schema.profiler_kind == ProfilerKind::Bytecode)
                })
                .count(),
            sampling_event_records: self
                .events
                .iter()
                .filter(|event| {
                    registry
                        .event_by_name(&event.summary)
                        .is_some_and(|schema| schema.profiler_kind == ProfilerKind::Sampling)
                })
                .count(),
        })
    }

    pub fn aggregate_execution_origins(
        &self,
    ) -> Result<ProfilerExecutionOriginAggregation, ProfilerValidationError> {
        self.validate()?;

        let mut summaries = Vec::new();
        let mut runs = Vec::new();
        let mut unqualified_sample_count = 0;
        let mut unqualified_event_count = 0;

        for sample in &self.samples {
            push_unique_run(&mut runs, sample.run);
            if let (Some(code_block), Some(bytecode_index)) =
                (sample.code_block, sample.bytecode_index)
            {
                push_execution_origin_sample(
                    &mut summaries,
                    FullCodeOrigin {
                        code_block,
                        origin: CodeOrigin::new(bytecode_index),
                    },
                );
            } else {
                unqualified_sample_count += 1;
            }
        }

        for event in &self.execution_events {
            push_unique_run(&mut runs, event.run);
            if let Some(origin) = event.origin {
                push_execution_origin_event(&mut summaries, origin, event.count);
            } else {
                unqualified_event_count += 1;
            }
        }

        Ok(ProfilerExecutionOriginAggregation {
            run_count: runs.len(),
            origins: summaries,
            unqualified_sample_count,
            unqualified_event_count,
        })
    }
}

/// Counter totals attached to a profiler event schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilerCounterAggregation {
    pub kind: ProfilerCounterKind,
    pub total: u64,
}

/// Event-row aggregate used by non-executing profiler reports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilerEventAggregation {
    pub kind: ProfilerEventKind,
    pub profiler_kind: ProfilerKind,
    pub event_count: usize,
    pub counters: Vec<ProfilerCounterAggregation>,
}

/// Report aggregate planned from immutable event/counter descriptors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilerReportAggregation {
    pub sample_count: usize,
    pub dropped_sample_count: u64,
    pub compilation_count: usize,
    pub events: Vec<ProfilerEventAggregation>,
}

/// Semantic report aggregation that classifies records by profiler family.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilerSemanticAggregation {
    pub aggregation: ProfilerReportAggregation,
    pub run_count: usize,
    pub bytecode_event_records: usize,
    pub sampling_event_records: usize,
}

/// Aggregated execution observation for one bytecode origin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilerExecutionOriginSummary {
    pub origin: FullCodeOrigin,
    pub sample_count: usize,
    pub event_count: usize,
    pub execution_count: u64,
}

/// Execution samples and events grouped by bytecode origin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilerExecutionOriginAggregation {
    pub run_count: usize,
    pub origins: Vec<ProfilerExecutionOriginSummary>,
    pub unqualified_sample_count: usize,
    pub unqualified_event_count: usize,
}

/// Profiler-visible tier diagnostic derived from tiering state or fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfilerTierDiagnosticRecord {
    pub code_block: Option<CodeBlockId>,
    pub current_tier: JitType,
    pub requested_tier: Option<JitType>,
    pub fallback: Option<TierFallbackResultRecord>,
    pub active_request_visible: bool,
    pub preserves_profile: bool,
}

impl ProfilerTierDiagnosticRecord {
    pub const fn from_tiering_state(state: &TieringState) -> Self {
        Self {
            code_block: state.owner,
            current_tier: state.current_tier,
            requested_tier: state.requested_tier,
            fallback: None,
            active_request_visible: state.active_request.is_some(),
            preserves_profile: true,
        }
    }

    pub const fn from_fallback(fallback: TierFallbackResultRecord) -> Self {
        Self {
            code_block: Some(fallback.owner),
            current_tier: fallback.from_tier,
            requested_tier: Some(fallback.attempted_tier),
            fallback: Some(fallback),
            active_request_visible: true,
            preserves_profile: fallback.preserves_profile,
        }
    }
}

/// Profiler diagnostics assembled from report, tier, and GC observations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilerDiagnosticsReport {
    pub aggregation: ProfilerReportAggregation,
    pub execution_origins: ProfilerExecutionOriginAggregation,
    pub tier_diagnostics: Vec<ProfilerTierDiagnosticRecord>,
    pub heap_observations: Vec<ProfilerHeapObservationRecord>,
}

impl ProfilerDiagnosticsReport {
    pub fn from_report(
        report: &ProfilerReport,
        registry: ProfilerSchemaRegistry,
        tier_diagnostics: Vec<ProfilerTierDiagnosticRecord>,
        heap_observations: Vec<ProfilerHeapObservationRecord>,
    ) -> Result<Self, ProfilerValidationError> {
        Ok(Self {
            aggregation: report.aggregate(registry)?,
            execution_origins: report.aggregate_execution_origins()?,
            tier_diagnostics,
            heap_observations,
        })
    }
}

fn push_unique_run(runs: &mut Vec<ProfilerRunId>, run: ProfilerRunId) {
    if !runs.contains(&run) {
        runs.push(run);
    }
}

fn push_execution_origin_sample(
    summaries: &mut Vec<ProfilerExecutionOriginSummary>,
    origin: FullCodeOrigin,
) {
    if let Some(summary) = summaries
        .iter_mut()
        .find(|summary| summary.origin == origin)
    {
        summary.sample_count += 1;
        return;
    }
    summaries.push(ProfilerExecutionOriginSummary {
        origin,
        sample_count: 1,
        event_count: 0,
        execution_count: 0,
    });
}

fn push_execution_origin_event(
    summaries: &mut Vec<ProfilerExecutionOriginSummary>,
    origin: FullCodeOrigin,
    count: u64,
) {
    if let Some(summary) = summaries
        .iter_mut()
        .find(|summary| summary.origin == origin)
    {
        summary.event_count += 1;
        summary.execution_count += count;
        return;
    }
    summaries.push(ProfilerExecutionOriginSummary {
        origin,
        sample_count: 0,
        event_count: 1,
        execution_count: count,
    });
}

fn aggregate_counter(kind: ProfilerCounterKind, report: &ProfilerReport) -> Option<u64> {
    match kind {
        ProfilerCounterKind::ExecutionCount => Some(
            report
                .compilations
                .iter()
                .flat_map(|compilation| compilation.counters.iter())
                .map(|counter| counter.count)
                .sum(),
        ),
        ProfilerCounterKind::OsrExitCount => Some(
            report
                .compilations
                .iter()
                .flat_map(|compilation| compilation.osr_exits.iter())
                .map(|exit| exit.count)
                .sum(),
        ),
        ProfilerCounterKind::DroppedSampleCount => Some(report.dropped_sample_count),
        _ => None,
    }
}

/// Builder for profiler reports assembled from non-executing records.
#[derive(Clone, Debug, Default)]
pub struct ProfilerReportBuilder {
    report: ProfilerReport,
}

impl ProfilerReportBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sample(mut self, sample: ProfilerSample) -> Self {
        self.report.samples.push(sample);
        self
    }

    pub fn compilation(mut self, compilation: ProfilerCompilationRecord) -> Self {
        self.report.compilations.push(compilation);
        self
    }

    pub fn event(mut self, event: ProfilerEventRecord) -> Self {
        self.report.events.push(event);
        self
    }

    pub fn execution_event(mut self, event: ProfilerExecutionEventRecord) -> Self {
        self.report.execution_events.push(event);
        self
    }

    pub fn dropped_sample_count(mut self, count: u64) -> Self {
        self.report.dropped_sample_count = count;
        self
    }

    pub fn build(self) -> Result<ProfilerReport, ProfilerValidationError> {
        self.report.validate()?;
        Ok(self.report)
    }
}

const PROFILER_SCHEMA_PROVENANCE: ProfilerSchemaProvenance = ProfilerSchemaProvenance {
    generator: "hand-authored",
    source: "Source/JavaScriptCore/rust/src/profiler/mod.rs",
    revision: 1,
};

pub const PROFILER_COUNTER_SCHEMAS: &[ProfilerCounterSchema] = &[
    ProfilerCounterSchema {
        kind: ProfilerCounterKind::ExecutionCount,
        name: "execution-count",
        unit: "count",
        owner: ProfilerSchemaOwner::ProfilerDatabase,
        mutation_authority: ProfilerRegistryMutationAuthority::ProfilerDatabaseInitialization,
        provenance: PROFILER_SCHEMA_PROVENANCE,
    },
    ProfilerCounterSchema {
        kind: ProfilerCounterKind::OsrExitCount,
        name: "osr-exit-count",
        unit: "count",
        owner: ProfilerSchemaOwner::ProfilerDatabase,
        mutation_authority: ProfilerRegistryMutationAuthority::ProfilerDatabaseInitialization,
        provenance: PROFILER_SCHEMA_PROVENANCE,
    },
    ProfilerCounterSchema {
        kind: ProfilerCounterKind::DroppedSampleCount,
        name: "dropped-sample-count",
        unit: "count",
        owner: ProfilerSchemaOwner::SamplingProfiler,
        mutation_authority: ProfilerRegistryMutationAuthority::ProfilerDatabaseInitialization,
        provenance: PROFILER_SCHEMA_PROVENANCE,
    },
];

const PROFILER_COMPILATION_COUNTERS: &[ProfilerCounterKind] = &[
    ProfilerCounterKind::ExecutionCount,
    ProfilerCounterKind::OsrExitCount,
];

const PROFILER_SAMPLING_COUNTERS: &[ProfilerCounterKind] =
    &[ProfilerCounterKind::DroppedSampleCount];

pub const PROFILER_EVENT_SCHEMAS: &[ProfilerEventSchema] = &[
    ProfilerEventSchema {
        kind: ProfilerEventKind::CompilationStarted,
        name: "compilation-started",
        profiler_kind: ProfilerKind::Bytecode,
        counters: PROFILER_COMPILATION_COUNTERS,
        owner: ProfilerSchemaOwner::ProfilerDatabase,
        mutation_authority: ProfilerRegistryMutationAuthority::ProfilerDatabaseInitialization,
        provenance: PROFILER_SCHEMA_PROVENANCE,
    },
    ProfilerEventSchema {
        kind: ProfilerEventKind::CompilationFinished,
        name: "compilation-finished",
        profiler_kind: ProfilerKind::Bytecode,
        counters: PROFILER_COMPILATION_COUNTERS,
        owner: ProfilerSchemaOwner::ProfilerDatabase,
        mutation_authority: ProfilerRegistryMutationAuthority::ProfilerDatabaseInitialization,
        provenance: PROFILER_SCHEMA_PROVENANCE,
    },
    ProfilerEventSchema {
        kind: ProfilerEventKind::SampleRecorded,
        name: "sample-recorded",
        profiler_kind: ProfilerKind::Sampling,
        counters: PROFILER_SAMPLING_COUNTERS,
        owner: ProfilerSchemaOwner::SamplingProfiler,
        mutation_authority: ProfilerRegistryMutationAuthority::ProfilerDatabaseInitialization,
        provenance: PROFILER_SCHEMA_PROVENANCE,
    },
];

pub const PROFILER_SCHEMA_REGISTRY: ProfilerSchemaRegistry = ProfilerSchemaRegistry {
    events: PROFILER_EVENT_SCHEMAS,
    counters: PROFILER_COUNTER_SCHEMAS,
};

fn validate_non_empty(
    value: &'static str,
    error: ProfilerValidationError,
) -> Result<(), ProfilerValidationError> {
    if value.is_empty() {
        Err(error)
    } else {
        Ok(())
    }
}

fn validate_unique_counter_kinds(
    counters: &[ProfilerCounterKind],
) -> Result<(), ProfilerValidationError> {
    for (index, counter) in counters.iter().enumerate() {
        for other in counters.iter().skip(index + 1) {
            if counter == other {
                return Err(ProfilerValidationError::DuplicateCounterKind(*counter));
            }
        }
    }
    Ok(())
}

fn validate_unique_counter_schemas(
    counters: &[ProfilerCounterSchema],
) -> Result<(), ProfilerValidationError> {
    for (index, counter) in counters.iter().enumerate() {
        for other in counters.iter().skip(index + 1) {
            if counter.kind == other.kind {
                return Err(ProfilerValidationError::DuplicateCounterKind(counter.kind));
            }
            if counter.name == other.name {
                return Err(ProfilerValidationError::DuplicateCounterName(counter.name));
            }
        }
    }
    Ok(())
}

fn validate_unique_event_schemas(
    events: &[ProfilerEventSchema],
) -> Result<(), ProfilerValidationError> {
    for (index, event) in events.iter().enumerate() {
        for other in events.iter().skip(index + 1) {
            if event.kind == other.kind {
                return Err(ProfilerValidationError::DuplicateEventKind(event.kind));
            }
            if event.name == other.name {
                return Err(ProfilerValidationError::DuplicateEventName(event.name));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::CellId;

    #[test]
    fn validates_builtin_profiler_registry() {
        assert_eq!(PROFILER_SCHEMA_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn rejects_duplicate_bytecode_indexes() {
        let sequence = ProfilerBytecodeSequence {
            owner: None,
            header_columns: vec!["opcode".to_string()],
            bytecodes: vec![
                ProfilerBytecodeDescriptor {
                    bytecode_index: BytecodeIndex::from_offset(0),
                    opcode_ordinal: 1,
                    description: "first".to_string(),
                },
                ProfilerBytecodeDescriptor {
                    bytecode_index: BytecodeIndex::from_offset(0),
                    opcode_ordinal: 2,
                    description: "second".to_string(),
                },
            ],
        };

        assert_eq!(
            sequence.validate(),
            Err(ProfilerValidationError::BytecodeSequenceDuplicateIndex(
                BytecodeIndex::from_offset(0)
            ))
        );
    }

    #[test]
    fn report_builder_rejects_empty_event_summary() {
        let result = ProfilerReportBuilder::new()
            .event(ProfilerEventRecord {
                run: ProfilerRunId(1),
                code_block: None,
                compilation_kind: None,
                summary: String::new(),
                detail: "detail".to_string(),
            })
            .build();

        assert_eq!(
            result,
            Err(ProfilerValidationError::EventRecordMissingSummary)
        );
    }

    #[test]
    fn aggregates_report_against_event_schemas() {
        let report = ProfilerReportBuilder::new()
            .compilation(ProfilerCompilationRecord {
                run: ProfilerRunId(1),
                code_block: None,
                kind: ProfilerCompilationKind::Baseline,
                profiled_bytecode_count: 1,
                inlined_get_by_id_count: 0,
                inlined_put_by_id_count: 0,
                inlined_call_count: 0,
                jettison_reason: ProfilerJettisonReason::NotJettisoned,
                counters: vec![ProfilerExecutionCounterDescriptor {
                    origin: ProfilerOriginStackDescriptor {
                        frames_from_bottom: vec![StackFrameId(1)],
                        may_be_non_bytecode_machine_code: false,
                    },
                    count: 5,
                }],
                osr_exits: vec![ProfilerOsrExitDescriptor {
                    exit_id: 1,
                    origin: ProfilerOriginStackDescriptor {
                        frames_from_bottom: vec![StackFrameId(1)],
                        may_be_non_bytecode_machine_code: false,
                    },
                    exit_kind_ordinal: 3,
                    is_watchpoint: false,
                    count: 2,
                }],
            })
            .event(ProfilerEventRecord {
                run: ProfilerRunId(1),
                code_block: None,
                compilation_kind: Some(ProfilerCompilationKind::Baseline),
                summary: "compilation-finished".to_string(),
                detail: "finished".to_string(),
            })
            .dropped_sample_count(1)
            .build()
            .expect("report");

        let aggregate = report
            .aggregate(PROFILER_SCHEMA_REGISTRY)
            .expect("aggregation");
        let finished = aggregate
            .events
            .iter()
            .find(|event| event.kind == ProfilerEventKind::CompilationFinished)
            .expect("finished event");

        assert_eq!(aggregate.compilation_count, 1);
        assert_eq!(finished.event_count, 1);
        assert!(finished.counters.iter().any(|counter| {
            counter.kind == ProfilerCounterKind::ExecutionCount && counter.total == 5
        }));
        assert!(finished.counters.iter().any(|counter| {
            counter.kind == ProfilerCounterKind::OsrExitCount && counter.total == 2
        }));
    }

    #[test]
    fn rejects_event_record_without_schema() {
        let report = ProfilerReportBuilder::new()
            .event(ProfilerEventRecord {
                run: ProfilerRunId(1),
                code_block: None,
                compilation_kind: None,
                summary: "missing-event".to_string(),
                detail: "detail".to_string(),
            })
            .build()
            .expect("report");

        assert_eq!(
            report.aggregate(PROFILER_SCHEMA_REGISTRY),
            Err(ProfilerValidationError::EventRecordMissingSchema(
                "missing-event".to_string()
            ))
        );
    }

    #[test]
    fn semantic_aggregation_classifies_profiler_families() {
        let report = ProfilerReportBuilder::new()
            .event(ProfilerEventRecord {
                run: ProfilerRunId(1),
                code_block: None,
                compilation_kind: Some(ProfilerCompilationKind::Baseline),
                summary: "compilation-started".to_string(),
                detail: "started".to_string(),
            })
            .event(ProfilerEventRecord {
                run: ProfilerRunId(1),
                code_block: None,
                compilation_kind: None,
                summary: "sample-recorded".to_string(),
                detail: "sample".to_string(),
            })
            .build()
            .expect("report");

        let semantic = report
            .semantic_aggregation(PROFILER_SCHEMA_REGISTRY)
            .expect("semantic aggregation");

        assert_eq!(semantic.run_count, 1);
        assert_eq!(semantic.bytecode_event_records, 1);
        assert_eq!(semantic.sampling_event_records, 1);
    }

    #[test]
    fn semantic_aggregation_rejects_bytecode_event_without_compilation_kind() {
        let report = ProfilerReportBuilder::new()
            .event(ProfilerEventRecord {
                run: ProfilerRunId(1),
                code_block: None,
                compilation_kind: None,
                summary: "compilation-started".to_string(),
                detail: "started".to_string(),
            })
            .build()
            .expect("report");

        assert_eq!(
            report.semantic_aggregation(PROFILER_SCHEMA_REGISTRY),
            Err(
                ProfilerValidationError::BytecodeEventMissingCompilationKind(
                    "compilation-started".to_string()
                )
            )
        );
    }

    #[test]
    fn aggregates_execution_samples_and_events_by_bytecode_origin() {
        let origin = FullCodeOrigin {
            code_block: CodeBlockId(CellId(9)),
            origin: CodeOrigin::new(BytecodeIndex::from_offset(16)),
        };
        let report = ProfilerReportBuilder::new()
            .sample(ProfilerSample {
                run: ProfilerRunId(1),
                frame: Some(StackFrameId(1)),
                code_block: Some(origin.code_block),
                bytecode_index: Some(origin.origin.bytecode_index),
                kind: ProfilerKind::Sampling,
            })
            .execution_event(ProfilerExecutionEventRecord {
                run: ProfilerRunId(1),
                origin: Some(origin),
                kind: ProfilerEventKind::SampleRecorded,
                count: 3,
            })
            .build()
            .expect("report");

        let aggregation = report
            .aggregate_execution_origins()
            .expect("execution aggregation");

        assert_eq!(aggregation.run_count, 1);
        assert_eq!(aggregation.origins.len(), 1);
        assert_eq!(aggregation.origins[0].sample_count, 1);
        assert_eq!(aggregation.origins[0].event_count, 1);
        assert_eq!(aggregation.origins[0].execution_count, 3);
    }

    #[test]
    fn diagnostics_report_exposes_tier_and_heap_observations() {
        let report = ProfilerReportBuilder::new()
            .sample(ProfilerSample {
                run: ProfilerRunId(1),
                frame: Some(StackFrameId(1)),
                code_block: Some(CodeBlockId(CellId(9))),
                bytecode_index: Some(BytecodeIndex::from_offset(1)),
                kind: ProfilerKind::Sampling,
            })
            .build()
            .expect("report");
        let tier = ProfilerTierDiagnosticRecord::from_fallback(TierFallbackResultRecord {
            owner: CodeBlockId(CellId(9)),
            from_tier: JitType::Baseline,
            attempted_tier: JitType::Dfg,
            reason: crate::jit::TierFallbackReason::UnsupportedTier,
            target: crate::jit::TierFallbackTarget::ReturnToInterpreter,
            bytecode_index: Some(BytecodeIndex::from_offset(1)),
            resume: crate::jit::TierFallbackResumeKind::ContinueInInterpreter,
            preserves_profile: true,
            should_count_invalidation: true,
            clears_active_request: true,
        });
        let heap = ProfilerHeapObservationRecord {
            id: ProfilerHeapObservationId(1),
            run: ProfilerRunId(1),
            kind: ProfilerHeapObservationKind::AllocationSample,
            heap: Some(HeapId(2)),
            snapshot: None,
            snapshot_kind: HeapSnapshotKind::Inspector,
            code_block: Some(CodeBlockId(CellId(9))),
            stack_frame: Some(StackFrameId(1)),
            observed_bytes: 64,
            dropped_samples: 0,
        };

        let diagnostics = ProfilerDiagnosticsReport::from_report(
            &report,
            PROFILER_SCHEMA_REGISTRY,
            vec![tier],
            vec![heap],
        )
        .expect("diagnostics");

        assert_eq!(diagnostics.aggregation.sample_count, 1);
        assert_eq!(diagnostics.tier_diagnostics.len(), 1);
        assert_eq!(diagnostics.heap_observations[0].observed_bytes, 64);
    }

    #[test]
    fn rejects_zero_count_execution_event() {
        let result = ProfilerReportBuilder::new()
            .execution_event(ProfilerExecutionEventRecord {
                run: ProfilerRunId(1),
                origin: None,
                kind: ProfilerEventKind::SampleRecorded,
                count: 0,
            })
            .build();

        assert_eq!(
            result,
            Err(ProfilerValidationError::ExecutionEventRecordMissingCount)
        );
    }
}
