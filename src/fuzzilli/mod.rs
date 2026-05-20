//! Fuzzilli instrumentation contracts.
//!
//! Fuzzilli is a testing and coverage boundary. It must never become part of
//! normal execution ownership; this module only names instrumentation hooks and
//! coverage records.

use crate::bytecode::BytecodeIndex;
use crate::gc::{CollectionKind, GcPhase, HeapId};
use crate::runtime::CodeBlockId;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct FuzzilliCoverageSiteId(pub u32);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct FuzzilliGcEventId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FuzzilliEventKind {
    BasicBlock,
    Edge,
    Compare,
    BuiltinCall,
    WasmOperation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FuzzilliGcEventResultKind {
    HarnessRequested,
    Completed,
    Rejected,
    Timeout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FuzzilliGcEventResultRecord {
    pub id: FuzzilliGcEventId,
    pub kind: FuzzilliGcEventResultKind,
    pub heap: Option<HeapId>,
    pub collection: Option<CollectionKind>,
    pub phase: Option<GcPhase>,
    pub coverage_site: Option<FuzzilliCoverageSiteId>,
    pub reprl_state: FuzzilliReprlState,
}

/// Owner of immutable Fuzzilli coverage and event schemas.
///
/// REPRL sessions and coverage maps own live instrumentation state. Static
/// schemas only describe event and coverage shapes consumed by instrumentation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum FuzzilliSchemaOwner {
    #[default]
    FuzzilliInstrumentation,
    ReprlHarness,
    GeneratedCoverageMetadata,
    TestFixture,
}

/// Authority allowed to replace Fuzzilli schema registries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum FuzzilliRegistryMutationAuthority {
    #[default]
    CrateInitialization,
    GeneratedDataRefresh,
    ReprlBootstrap,
}

/// Provenance for Fuzzilli descriptor metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct FuzzilliSchemaProvenance {
    pub generator: &'static str,
    pub source: &'static str,
    pub revision: u64,
}

impl FuzzilliSchemaProvenance {
    pub const fn new(generator: &'static str, source: &'static str, revision: u64) -> Self {
        Self {
            generator,
            source,
            revision,
        }
    }
}

/// Static coverage field family.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FuzzilliCoverageFieldKind {
    SiteId,
    EdgeIndex,
    BytecodeIndex,
    CodeBlock,
    GuardState,
    Counter,
}

/// Immutable metadata for a coverage record field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FuzzilliCoverageFieldSchema {
    pub name: &'static str,
    pub kind: FuzzilliCoverageFieldKind,
    pub required: bool,
}

impl FuzzilliCoverageFieldSchema {
    pub fn validate(self) -> Result<(), FuzzilliValidationError> {
        if self.name.is_empty() {
            Err(FuzzilliValidationError::EmptyCoverageFieldName)
        } else {
            Ok(())
        }
    }
}

/// Immutable coverage schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FuzzilliCoverageSchema {
    pub name: &'static str,
    pub fields: &'static [FuzzilliCoverageFieldSchema],
    pub storage: FuzzilliCoverageStorage,
    pub owner: FuzzilliSchemaOwner,
    pub mutation_authority: FuzzilliRegistryMutationAuthority,
    pub provenance: FuzzilliSchemaProvenance,
}

impl FuzzilliCoverageSchema {
    pub const fn fields(self) -> &'static [FuzzilliCoverageFieldSchema] {
        self.fields
    }

    pub fn validate(self) -> Result<(), FuzzilliValidationError> {
        if self.name.is_empty() {
            return Err(FuzzilliValidationError::EmptyCoverageSchemaName);
        }
        validate_unique_coverage_fields(self.fields)?;
        for field in self.fields {
            field.validate()?;
        }
        if !self
            .fields
            .iter()
            .any(|field| field.kind == FuzzilliCoverageFieldKind::SiteId && field.required)
        {
            return Err(FuzzilliValidationError::CoverageMissingRequiredSiteId);
        }
        if !self
            .fields
            .iter()
            .any(|field| field.kind == FuzzilliCoverageFieldKind::GuardState && field.required)
        {
            return Err(FuzzilliValidationError::CoverageMissingRequiredGuardState);
        }
        if self.provenance.generator.is_empty() || self.provenance.source.is_empty() {
            return Err(FuzzilliValidationError::EmptyProvenanceField);
        }
        Ok(())
    }
}

/// Immutable event descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FuzzilliEventSchema {
    pub kind: FuzzilliEventKind,
    pub name: &'static str,
    pub coverage_fields: &'static [FuzzilliCoverageFieldKind],
    pub may_reference_bytecode: bool,
    pub may_reference_wasm: bool,
    pub owner: FuzzilliSchemaOwner,
    pub mutation_authority: FuzzilliRegistryMutationAuthority,
    pub provenance: FuzzilliSchemaProvenance,
}

impl FuzzilliEventSchema {
    pub const fn coverage_fields(self) -> &'static [FuzzilliCoverageFieldKind] {
        self.coverage_fields
    }

    pub fn validate(self) -> Result<(), FuzzilliValidationError> {
        if self.name.is_empty() {
            return Err(FuzzilliValidationError::EmptyEventName);
        }
        validate_unique_event_fields(self.coverage_fields)?;
        if self.coverage_fields.is_empty() {
            return Err(FuzzilliValidationError::EventMissingCoverageFields(
                self.kind,
            ));
        }
        if self.may_reference_bytecode && self.may_reference_wasm {
            return Err(FuzzilliValidationError::EventReferencesBytecodeAndWasm(
                self.kind,
            ));
        }
        if self.provenance.generator.is_empty() || self.provenance.source.is_empty() {
            return Err(FuzzilliValidationError::EmptyProvenanceField);
        }
        Ok(())
    }
}

/// Structural Fuzzilli descriptor validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FuzzilliValidationError {
    EmptyCoverageFieldName,
    EmptyCoverageSchemaName,
    EmptyEventName,
    EmptyProvenanceField,
    DuplicateCoverageSchemaName(&'static str),
    DuplicateCoverageFieldName(&'static str),
    DuplicateCoverageFieldKind(FuzzilliCoverageFieldKind),
    DuplicateEventKind(FuzzilliEventKind),
    DuplicateEventName(&'static str),
    DuplicateEventField(FuzzilliCoverageFieldKind),
    EventMissingCoverageFields(FuzzilliEventKind),
    EventReferencesMissingCoverageField {
        event: FuzzilliEventKind,
        field: FuzzilliCoverageFieldKind,
    },
    EventReferencesBytecodeAndWasm(FuzzilliEventKind),
    CoverageMissingRequiredSiteId,
    CoverageMissingRequiredGuardState,
    CoverageMapEdgeCountExceedsMaximum,
    CoverageMapDuplicateSite(FuzzilliCoverageSiteId),
    CoverageSiteMissingEventSchema(FuzzilliEventKind),
    ReprlInputMappingMismatch,
    InstrumentationHooksWithoutReprl,
    InstrumentationPlanDuplicateSite(FuzzilliCoverageSiteId),
    CoverageSiteBytecodeRejected(FuzzilliCoverageSiteId),
    CoverageOutcomeSiteMissingFromMap(FuzzilliCoverageSiteId),
    ReprlOutcomeInputNotMapped,
    ExecutionOutcomeMissingCoverageSite(FuzzilliCoverageSiteId),
}

/// REPRL command channel state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FuzzilliReprlState {
    #[default]
    Disabled,
    HandshakePending,
    WaitingForCommand,
    InputMapped,
    Executing,
    Flushing,
}

/// Coverage backing store ownership.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FuzzilliCoverageStorage {
    #[default]
    Uninitialized,
    SharedMemory,
    LocalMallocFallback,
}

/// REPRL file-descriptor role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FuzzilliReprlChannel {
    ControlRead,
    ControlWrite,
    DataRead,
    DataWrite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FuzzilliCoverageSite {
    pub id: FuzzilliCoverageSiteId,
    pub owner: Option<CodeBlockId>,
    pub bytecode_index: Option<BytecodeIndex>,
    pub kind: FuzzilliEventKind,
    pub guard_is_armed: bool,
}

impl FuzzilliCoverageSite {
    pub fn validate(self, registry: FuzzilliSchemaRegistry) -> Result<(), FuzzilliValidationError> {
        if registry.event(self.kind).is_none() {
            Err(FuzzilliValidationError::CoverageSiteMissingEventSchema(
                self.kind,
            ))
        } else {
            Ok(())
        }
    }
}

/// Shared coverage map contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FuzzilliCoverageMap {
    pub storage: FuzzilliCoverageStorage,
    pub edge_count: u32,
    pub max_edges: u32,
    pub sites: Vec<FuzzilliCoverageSite>,
}

impl FuzzilliCoverageMap {
    pub fn validate(
        &self,
        registry: FuzzilliSchemaRegistry,
    ) -> Result<(), FuzzilliValidationError> {
        if self.edge_count > self.max_edges {
            return Err(FuzzilliValidationError::CoverageMapEdgeCountExceedsMaximum);
        }
        validate_unique_sites(&self.sites)?;
        for site in &self.sites {
            site.validate(registry)?;
        }
        Ok(())
    }
}

/// Registry of immutable Fuzzilli coverage and event schemas.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FuzzilliSchemaRegistry {
    pub coverage: &'static [FuzzilliCoverageSchema],
    pub events: &'static [FuzzilliEventSchema],
}

impl FuzzilliSchemaRegistry {
    pub const fn new(
        coverage: &'static [FuzzilliCoverageSchema],
        events: &'static [FuzzilliEventSchema],
    ) -> Self {
        Self { coverage, events }
    }

    pub const fn coverage(self) -> &'static [FuzzilliCoverageSchema] {
        self.coverage
    }

    pub const fn events(self) -> &'static [FuzzilliEventSchema] {
        self.events
    }

    pub fn coverage_named(self, name: &str) -> Option<&'static FuzzilliCoverageSchema> {
        self.coverage
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn event(self, kind: FuzzilliEventKind) -> Option<&'static FuzzilliEventSchema> {
        self.events
            .iter()
            .find(|descriptor| descriptor.kind == kind)
    }

    pub fn validate(self) -> Result<(), FuzzilliValidationError> {
        validate_unique_coverage_schemas(self.coverage)?;
        validate_unique_event_schemas(self.events)?;
        for coverage in self.coverage {
            coverage.validate()?;
        }
        for event in self.events {
            event.validate()?;
            for field in event.coverage_fields {
                if !self
                    .coverage
                    .iter()
                    .any(|coverage| coverage.fields.iter().any(|schema| schema.kind == *field))
                {
                    return Err(
                        FuzzilliValidationError::EventReferencesMissingCoverageField {
                            event: event.kind,
                            field: *field,
                        },
                    );
                }
            }
        }
        Ok(())
    }
}

/// REPRL execution contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FuzzilliReprlSession {
    pub state: FuzzilliReprlState,
    pub max_input_size: usize,
    pub has_input_mapping: bool,
    pub pending_rejected_promise_count: usize,
}

impl FuzzilliReprlSession {
    pub fn validate(self) -> Result<(), FuzzilliValidationError> {
        if self.state == FuzzilliReprlState::InputMapped && !self.has_input_mapping {
            Err(FuzzilliValidationError::ReprlInputMappingMismatch)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FuzzilliInstrumentationPlan {
    pub sites: Vec<FuzzilliCoverageSite>,
    pub coverage_storage: FuzzilliCoverageStorage,
    pub reprl_state: FuzzilliReprlState,
    pub expose_reprl_hooks: bool,
    pub deterministic_gc: bool,
    pub reject_unhandled_promises_as_failure: bool,
}

impl FuzzilliInstrumentationPlan {
    pub fn validate(
        &self,
        registry: FuzzilliSchemaRegistry,
    ) -> Result<(), FuzzilliValidationError> {
        if self.expose_reprl_hooks && self.reprl_state == FuzzilliReprlState::Disabled {
            return Err(FuzzilliValidationError::InstrumentationHooksWithoutReprl);
        }
        validate_unique_plan_sites(&self.sites)?;
        for site in &self.sites {
            site.validate(registry)?;
        }
        Ok(())
    }

    pub fn map_coverage_events(
        &self,
        registry: FuzzilliSchemaRegistry,
    ) -> Result<Vec<FuzzilliCoverageEventMapping>, FuzzilliValidationError> {
        registry.validate()?;
        self.validate(registry)?;

        let mut mappings = Vec::with_capacity(self.sites.len());
        for site in &self.sites {
            let schema = registry.event(site.kind).ok_or(
                FuzzilliValidationError::CoverageSiteMissingEventSchema(site.kind),
            )?;
            if site.bytecode_index.is_some() && !schema.may_reference_bytecode {
                return Err(FuzzilliValidationError::CoverageSiteBytecodeRejected(
                    site.id,
                ));
            }

            mappings.push(FuzzilliCoverageEventMapping {
                site: site.id,
                owner: site.owner,
                event_kind: site.kind,
                coverage_field_count: schema.coverage_fields.len(),
                records_bytecode: site.bytecode_index.is_some(),
                records_wasm: schema.may_reference_wasm,
                guard_allows_recording: site.guard_is_armed,
            });
        }
        Ok(mappings)
    }

    pub fn semantic_outcomes(
        &self,
        coverage_map: &FuzzilliCoverageMap,
        registry: FuzzilliSchemaRegistry,
    ) -> Result<Vec<FuzzilliCoverageSemanticOutcome>, FuzzilliValidationError> {
        let mappings = self.map_coverage_events(registry)?;
        coverage_map.validate(registry)?;

        let mut outcomes = Vec::with_capacity(mappings.len());
        for mapping in mappings {
            let map_site = coverage_map
                .sites
                .iter()
                .find(|site| site.id == mapping.site)
                .ok_or(FuzzilliValidationError::CoverageOutcomeSiteMissingFromMap(
                    mapping.site,
                ))?;
            let reason = if coverage_map.storage == FuzzilliCoverageStorage::Uninitialized {
                FuzzilliCoverageRecordingReason::StorageUnavailable
            } else if coverage_map.edge_count >= coverage_map.max_edges {
                FuzzilliCoverageRecordingReason::EdgeLimitReached
            } else if !mapping.guard_allows_recording || !map_site.guard_is_armed {
                FuzzilliCoverageRecordingReason::GuardDisarmed
            } else {
                FuzzilliCoverageRecordingReason::Recorded
            };

            outcomes.push(FuzzilliCoverageSemanticOutcome {
                site: mapping.site,
                event_kind: mapping.event_kind,
                recorded: reason == FuzzilliCoverageRecordingReason::Recorded,
                reason,
                records_bytecode: mapping.records_bytecode,
                records_wasm: mapping.records_wasm,
            });
        }
        Ok(outcomes)
    }

    pub fn classify_session_outcome(
        &self,
        session: FuzzilliReprlSession,
    ) -> Result<FuzzilliSessionSemanticOutcome, FuzzilliValidationError> {
        session.validate()?;
        if matches!(
            session.state,
            FuzzilliReprlState::InputMapped
                | FuzzilliReprlState::Executing
                | FuzzilliReprlState::Flushing
        ) && !session.has_input_mapping
        {
            return Err(FuzzilliValidationError::ReprlOutcomeInputNotMapped);
        }

        let status = if self.reject_unhandled_promises_as_failure
            && session.pending_rejected_promise_count != 0
        {
            FuzzilliSessionStatus::FailedRejectedPromise
        } else {
            match session.state {
                FuzzilliReprlState::Disabled => FuzzilliSessionStatus::Disabled,
                FuzzilliReprlState::HandshakePending | FuzzilliReprlState::WaitingForCommand => {
                    FuzzilliSessionStatus::Waiting
                }
                FuzzilliReprlState::InputMapped
                | FuzzilliReprlState::Executing
                | FuzzilliReprlState::Flushing => FuzzilliSessionStatus::AcceptedInput,
            }
        };

        Ok(FuzzilliSessionSemanticOutcome {
            status,
            pending_rejected_promise_count: session.pending_rejected_promise_count,
            deterministic_gc: self.deterministic_gc,
            exposes_reprl_hooks: self.expose_reprl_hooks,
        })
    }

    pub fn classify_execution_outcome(
        &self,
        session: FuzzilliReprlSession,
        coverage_map: &FuzzilliCoverageMap,
        process_status: FuzzilliObservedProcessStatus,
        registry: FuzzilliSchemaRegistry,
    ) -> Result<FuzzilliExecutionOutcomeRecord, FuzzilliValidationError> {
        let session_outcome = self.classify_session_outcome(session)?;
        let coverage_outcomes = self.semantic_outcomes(coverage_map, registry)?;
        let mut coverage_records = Vec::with_capacity(coverage_outcomes.len());

        for outcome in coverage_outcomes {
            let site = coverage_map
                .sites
                .iter()
                .find(|site| site.id == outcome.site)
                .ok_or(
                    FuzzilliValidationError::ExecutionOutcomeMissingCoverageSite(outcome.site),
                )?;
            coverage_records.push(FuzzilliCoverageRecord {
                site: outcome.site,
                event_kind: outcome.event_kind,
                owner: site.owner,
                bytecode_index: site.bytecode_index,
                recorded: outcome.recorded,
                reason: outcome.reason,
                counter_delta: if outcome.recorded { 1 } else { 0 },
            });
        }

        let kind = match (session_outcome.status, process_status) {
            (FuzzilliSessionStatus::Disabled, _) => FuzzilliExecutionOutcomeKind::Disabled,
            (FuzzilliSessionStatus::FailedRejectedPromise, _) => {
                FuzzilliExecutionOutcomeKind::RejectedPromiseFailure
            }
            (_, FuzzilliObservedProcessStatus::TimedOut) => FuzzilliExecutionOutcomeKind::TimedOut,
            (_, FuzzilliObservedProcessStatus::Crashed) => FuzzilliExecutionOutcomeKind::Crashed,
            (_, FuzzilliObservedProcessStatus::Failed) => FuzzilliExecutionOutcomeKind::Failed,
            (_, FuzzilliObservedProcessStatus::Succeeded) => {
                FuzzilliExecutionOutcomeKind::Succeeded
            }
        };

        Ok(FuzzilliExecutionOutcomeRecord {
            kind,
            session: session_outcome,
            process_status,
            coverage_records,
        })
    }
}

/// Pure mapping from an instrumentation site to its event schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FuzzilliCoverageEventMapping {
    pub site: FuzzilliCoverageSiteId,
    pub owner: Option<CodeBlockId>,
    pub event_kind: FuzzilliEventKind,
    pub coverage_field_count: usize,
    pub records_bytecode: bool,
    pub records_wasm: bool,
    pub guard_allows_recording: bool,
}

/// Reason a coverage site would or would not record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FuzzilliCoverageRecordingReason {
    Recorded,
    GuardDisarmed,
    StorageUnavailable,
    EdgeLimitReached,
}

/// Semantic coverage outcome for one instrumentation site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FuzzilliCoverageSemanticOutcome {
    pub site: FuzzilliCoverageSiteId,
    pub event_kind: FuzzilliEventKind,
    pub recorded: bool,
    pub reason: FuzzilliCoverageRecordingReason,
    pub records_bytecode: bool,
    pub records_wasm: bool,
}

/// Pure REPRL session classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FuzzilliSessionStatus {
    Disabled,
    Waiting,
    AcceptedInput,
    FailedRejectedPromise,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FuzzilliSessionSemanticOutcome {
    pub status: FuzzilliSessionStatus,
    pub pending_rejected_promise_count: usize,
    pub deterministic_gc: bool,
    pub exposes_reprl_hooks: bool,
}

/// Process-level completion observed by a REPRL harness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FuzzilliObservedProcessStatus {
    Succeeded,
    Failed,
    TimedOut,
    Crashed,
}

/// Final execution outcome class reported to Fuzzilli.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FuzzilliExecutionOutcomeKind {
    Succeeded,
    Failed,
    TimedOut,
    Crashed,
    RejectedPromiseFailure,
    Disabled,
}

/// Coverage record emitted for one observed execution outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FuzzilliCoverageRecord {
    pub site: FuzzilliCoverageSiteId,
    pub event_kind: FuzzilliEventKind,
    pub owner: Option<CodeBlockId>,
    pub bytecode_index: Option<BytecodeIndex>,
    pub recorded: bool,
    pub reason: FuzzilliCoverageRecordingReason,
    pub counter_delta: u32,
}

/// Fuzzilli execution outcome plus per-site coverage observations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FuzzilliExecutionOutcomeRecord {
    pub kind: FuzzilliExecutionOutcomeKind,
    pub session: FuzzilliSessionSemanticOutcome,
    pub process_status: FuzzilliObservedProcessStatus,
    pub coverage_records: Vec<FuzzilliCoverageRecord>,
}

/// Coverage summary for one Fuzzilli execution outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FuzzilliCoverageReportSummary {
    pub site_count: usize,
    pub recorded_count: usize,
    pub suppressed_count: usize,
    pub bytecode_record_count: usize,
    pub counter_delta_total: u32,
}

impl FuzzilliCoverageReportSummary {
    pub fn from_records(records: &[FuzzilliCoverageRecord]) -> Self {
        let recorded_count = records.iter().filter(|record| record.recorded).count();
        Self {
            site_count: records.len(),
            recorded_count,
            suppressed_count: records.len().saturating_sub(recorded_count),
            bytecode_record_count: records
                .iter()
                .filter(|record| record.bytecode_index.is_some())
                .count(),
            counter_delta_total: records.iter().map(|record| record.counter_delta).sum(),
        }
    }
}

/// Fuzzilli execution report assembled without executing a fuzzer process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FuzzilliExecutionReport {
    pub outcome: FuzzilliExecutionOutcomeRecord,
    pub coverage: FuzzilliCoverageReportSummary,
    pub gc_events: Vec<FuzzilliGcEventResultRecord>,
    pub gc_completed_count: usize,
}

impl FuzzilliExecutionReport {
    pub fn from_outcome(
        outcome: FuzzilliExecutionOutcomeRecord,
        gc_events: Vec<FuzzilliGcEventResultRecord>,
    ) -> Self {
        let coverage = FuzzilliCoverageReportSummary::from_records(&outcome.coverage_records);
        let gc_completed_count = gc_events
            .iter()
            .filter(|event| event.kind == FuzzilliGcEventResultKind::Completed)
            .count();
        Self {
            outcome,
            coverage,
            gc_events,
            gc_completed_count,
        }
    }
}

/// Builder for Fuzzilli instrumentation plans.
#[derive(Clone, Debug, Default)]
pub struct FuzzilliInstrumentationPlanBuilder {
    plan: FuzzilliInstrumentationPlan,
}

impl FuzzilliInstrumentationPlanBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn site(mut self, site: FuzzilliCoverageSite) -> Self {
        self.plan.sites.push(site);
        self
    }

    pub fn coverage_storage(mut self, storage: FuzzilliCoverageStorage) -> Self {
        self.plan.coverage_storage = storage;
        self
    }

    pub fn reprl_state(mut self, state: FuzzilliReprlState) -> Self {
        self.plan.reprl_state = state;
        self
    }

    pub fn expose_reprl_hooks(mut self, expose: bool) -> Self {
        self.plan.expose_reprl_hooks = expose;
        self
    }

    pub fn deterministic_gc(mut self, enabled: bool) -> Self {
        self.plan.deterministic_gc = enabled;
        self
    }

    pub fn reject_unhandled_promises_as_failure(mut self, reject: bool) -> Self {
        self.plan.reject_unhandled_promises_as_failure = reject;
        self
    }

    pub fn build(
        self,
        registry: FuzzilliSchemaRegistry,
    ) -> Result<FuzzilliInstrumentationPlan, FuzzilliValidationError> {
        self.plan.validate(registry)?;
        Ok(self.plan)
    }
}

const FUZZILLI_SCHEMA_PROVENANCE: FuzzilliSchemaProvenance = FuzzilliSchemaProvenance {
    generator: "hand-authored",
    source: "Source/JavaScriptCore/rust/src/fuzzilli/mod.rs",
    revision: 1,
};

pub const FUZZILLI_COVERAGE_FIELDS: &[FuzzilliCoverageFieldSchema] = &[
    FuzzilliCoverageFieldSchema {
        name: "site-id",
        kind: FuzzilliCoverageFieldKind::SiteId,
        required: true,
    },
    FuzzilliCoverageFieldSchema {
        name: "edge-index",
        kind: FuzzilliCoverageFieldKind::EdgeIndex,
        required: false,
    },
    FuzzilliCoverageFieldSchema {
        name: "bytecode-index",
        kind: FuzzilliCoverageFieldKind::BytecodeIndex,
        required: false,
    },
    FuzzilliCoverageFieldSchema {
        name: "code-block",
        kind: FuzzilliCoverageFieldKind::CodeBlock,
        required: false,
    },
    FuzzilliCoverageFieldSchema {
        name: "guard-state",
        kind: FuzzilliCoverageFieldKind::GuardState,
        required: true,
    },
];

pub const FUZZILLI_COVERAGE_SCHEMAS: &[FuzzilliCoverageSchema] = &[FuzzilliCoverageSchema {
    name: "coverage-map",
    fields: FUZZILLI_COVERAGE_FIELDS,
    storage: FuzzilliCoverageStorage::SharedMemory,
    owner: FuzzilliSchemaOwner::FuzzilliInstrumentation,
    mutation_authority: FuzzilliRegistryMutationAuthority::CrateInitialization,
    provenance: FUZZILLI_SCHEMA_PROVENANCE,
}];

const FUZZILLI_BASIC_BLOCK_FIELDS: &[FuzzilliCoverageFieldKind] = &[
    FuzzilliCoverageFieldKind::SiteId,
    FuzzilliCoverageFieldKind::BytecodeIndex,
    FuzzilliCoverageFieldKind::GuardState,
];

const FUZZILLI_EDGE_FIELDS: &[FuzzilliCoverageFieldKind] = &[
    FuzzilliCoverageFieldKind::SiteId,
    FuzzilliCoverageFieldKind::EdgeIndex,
    FuzzilliCoverageFieldKind::GuardState,
];

pub const FUZZILLI_EVENT_SCHEMAS: &[FuzzilliEventSchema] = &[
    FuzzilliEventSchema {
        kind: FuzzilliEventKind::BasicBlock,
        name: "basic-block",
        coverage_fields: FUZZILLI_BASIC_BLOCK_FIELDS,
        may_reference_bytecode: true,
        may_reference_wasm: false,
        owner: FuzzilliSchemaOwner::FuzzilliInstrumentation,
        mutation_authority: FuzzilliRegistryMutationAuthority::CrateInitialization,
        provenance: FUZZILLI_SCHEMA_PROVENANCE,
    },
    FuzzilliEventSchema {
        kind: FuzzilliEventKind::Edge,
        name: "edge",
        coverage_fields: FUZZILLI_EDGE_FIELDS,
        may_reference_bytecode: true,
        may_reference_wasm: false,
        owner: FuzzilliSchemaOwner::FuzzilliInstrumentation,
        mutation_authority: FuzzilliRegistryMutationAuthority::CrateInitialization,
        provenance: FUZZILLI_SCHEMA_PROVENANCE,
    },
    FuzzilliEventSchema {
        kind: FuzzilliEventKind::Compare,
        name: "compare",
        coverage_fields: FUZZILLI_BASIC_BLOCK_FIELDS,
        may_reference_bytecode: true,
        may_reference_wasm: false,
        owner: FuzzilliSchemaOwner::FuzzilliInstrumentation,
        mutation_authority: FuzzilliRegistryMutationAuthority::CrateInitialization,
        provenance: FUZZILLI_SCHEMA_PROVENANCE,
    },
    FuzzilliEventSchema {
        kind: FuzzilliEventKind::WasmOperation,
        name: "wasm-operation",
        coverage_fields: FUZZILLI_EDGE_FIELDS,
        may_reference_bytecode: false,
        may_reference_wasm: true,
        owner: FuzzilliSchemaOwner::FuzzilliInstrumentation,
        mutation_authority: FuzzilliRegistryMutationAuthority::CrateInitialization,
        provenance: FUZZILLI_SCHEMA_PROVENANCE,
    },
];

pub const FUZZILLI_SCHEMA_REGISTRY: FuzzilliSchemaRegistry = FuzzilliSchemaRegistry {
    coverage: FUZZILLI_COVERAGE_SCHEMAS,
    events: FUZZILLI_EVENT_SCHEMAS,
};

fn validate_unique_coverage_fields(
    fields: &[FuzzilliCoverageFieldSchema],
) -> Result<(), FuzzilliValidationError> {
    for (index, field) in fields.iter().enumerate() {
        for other in fields.iter().skip(index + 1) {
            if field.name == other.name {
                return Err(FuzzilliValidationError::DuplicateCoverageFieldName(
                    field.name,
                ));
            }
            if field.kind == other.kind {
                return Err(FuzzilliValidationError::DuplicateCoverageFieldKind(
                    field.kind,
                ));
            }
        }
    }
    Ok(())
}

fn validate_unique_event_fields(
    fields: &[FuzzilliCoverageFieldKind],
) -> Result<(), FuzzilliValidationError> {
    for (index, field) in fields.iter().enumerate() {
        for other in fields.iter().skip(index + 1) {
            if field == other {
                return Err(FuzzilliValidationError::DuplicateEventField(*field));
            }
        }
    }
    Ok(())
}

fn validate_unique_coverage_schemas(
    coverage: &[FuzzilliCoverageSchema],
) -> Result<(), FuzzilliValidationError> {
    for (index, schema) in coverage.iter().enumerate() {
        for other in coverage.iter().skip(index + 1) {
            if schema.name == other.name {
                return Err(FuzzilliValidationError::DuplicateCoverageSchemaName(
                    schema.name,
                ));
            }
        }
    }
    Ok(())
}

fn validate_unique_event_schemas(
    events: &[FuzzilliEventSchema],
) -> Result<(), FuzzilliValidationError> {
    for (index, event) in events.iter().enumerate() {
        for other in events.iter().skip(index + 1) {
            if event.kind == other.kind {
                return Err(FuzzilliValidationError::DuplicateEventKind(event.kind));
            }
            if event.name == other.name {
                return Err(FuzzilliValidationError::DuplicateEventName(event.name));
            }
        }
    }
    Ok(())
}

fn validate_unique_sites(sites: &[FuzzilliCoverageSite]) -> Result<(), FuzzilliValidationError> {
    for (index, site) in sites.iter().enumerate() {
        for other in sites.iter().skip(index + 1) {
            if site.id == other.id {
                return Err(FuzzilliValidationError::CoverageMapDuplicateSite(site.id));
            }
        }
    }
    Ok(())
}

fn validate_unique_plan_sites(
    sites: &[FuzzilliCoverageSite],
) -> Result<(), FuzzilliValidationError> {
    for (index, site) in sites.iter().enumerate() {
        for other in sites.iter().skip(index + 1) {
            if site.id == other.id {
                return Err(FuzzilliValidationError::InstrumentationPlanDuplicateSite(
                    site.id,
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(id: u32) -> FuzzilliCoverageSite {
        FuzzilliCoverageSite {
            id: FuzzilliCoverageSiteId(id),
            owner: None,
            bytecode_index: None,
            kind: FuzzilliEventKind::BasicBlock,
            guard_is_armed: true,
        }
    }

    #[test]
    fn validates_builtin_fuzzilli_registry() {
        assert_eq!(FUZZILLI_SCHEMA_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn rejects_duplicate_coverage_map_sites() {
        let map = FuzzilliCoverageMap {
            storage: FuzzilliCoverageStorage::SharedMemory,
            edge_count: 1,
            max_edges: 8,
            sites: vec![site(1), site(1)],
        };

        assert_eq!(
            map.validate(FUZZILLI_SCHEMA_REGISTRY),
            Err(FuzzilliValidationError::CoverageMapDuplicateSite(
                FuzzilliCoverageSiteId(1)
            ))
        );
    }

    #[test]
    fn instrumentation_builder_requires_reprl_for_hooks() {
        let result = FuzzilliInstrumentationPlanBuilder::new()
            .expose_reprl_hooks(true)
            .build(FUZZILLI_SCHEMA_REGISTRY);

        assert_eq!(
            result,
            Err(FuzzilliValidationError::InstrumentationHooksWithoutReprl)
        );
    }

    #[test]
    fn maps_coverage_sites_to_event_schemas() {
        let plan = FuzzilliInstrumentationPlanBuilder::new()
            .site(FuzzilliCoverageSite {
                id: FuzzilliCoverageSiteId(2),
                owner: None,
                bytecode_index: Some(BytecodeIndex::from_offset(4)),
                kind: FuzzilliEventKind::BasicBlock,
                guard_is_armed: true,
            })
            .build(FUZZILLI_SCHEMA_REGISTRY)
            .expect("plan");

        let mapping = plan
            .map_coverage_events(FUZZILLI_SCHEMA_REGISTRY)
            .expect("mapping");

        assert_eq!(mapping.len(), 1);
        assert_eq!(mapping[0].event_kind, FuzzilliEventKind::BasicBlock);
        assert!(mapping[0].records_bytecode);
        assert!(!mapping[0].records_wasm);
    }

    #[test]
    fn rejects_bytecode_mapping_for_wasm_event() {
        let plan = FuzzilliInstrumentationPlanBuilder::new()
            .site(FuzzilliCoverageSite {
                id: FuzzilliCoverageSiteId(3),
                owner: None,
                bytecode_index: Some(BytecodeIndex::from_offset(8)),
                kind: FuzzilliEventKind::WasmOperation,
                guard_is_armed: true,
            })
            .build(FUZZILLI_SCHEMA_REGISTRY)
            .expect("plan");

        assert_eq!(
            plan.map_coverage_events(FUZZILLI_SCHEMA_REGISTRY),
            Err(FuzzilliValidationError::CoverageSiteBytecodeRejected(
                FuzzilliCoverageSiteId(3)
            ))
        );
    }

    #[test]
    fn coverage_semantics_record_when_storage_and_guard_allow() {
        let coverage_site = FuzzilliCoverageSite {
            id: FuzzilliCoverageSiteId(4),
            owner: None,
            bytecode_index: Some(BytecodeIndex::from_offset(4)),
            kind: FuzzilliEventKind::BasicBlock,
            guard_is_armed: true,
        };
        let plan = FuzzilliInstrumentationPlanBuilder::new()
            .site(coverage_site)
            .build(FUZZILLI_SCHEMA_REGISTRY)
            .expect("plan");
        let map = FuzzilliCoverageMap {
            storage: FuzzilliCoverageStorage::SharedMemory,
            edge_count: 1,
            max_edges: 8,
            sites: vec![coverage_site],
        };

        let outcomes = plan
            .semantic_outcomes(&map, FUZZILLI_SCHEMA_REGISTRY)
            .expect("outcomes");

        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].recorded);
        assert_eq!(
            outcomes[0].reason,
            FuzzilliCoverageRecordingReason::Recorded
        );
    }

    #[test]
    fn coverage_semantics_report_disarmed_guard_without_recording() {
        let coverage_site = FuzzilliCoverageSite {
            id: FuzzilliCoverageSiteId(5),
            owner: None,
            bytecode_index: None,
            kind: FuzzilliEventKind::BasicBlock,
            guard_is_armed: false,
        };
        let plan = FuzzilliInstrumentationPlanBuilder::new()
            .site(coverage_site)
            .build(FUZZILLI_SCHEMA_REGISTRY)
            .expect("plan");
        let map = FuzzilliCoverageMap {
            storage: FuzzilliCoverageStorage::SharedMemory,
            edge_count: 1,
            max_edges: 8,
            sites: vec![coverage_site],
        };

        let outcomes = plan
            .semantic_outcomes(&map, FUZZILLI_SCHEMA_REGISTRY)
            .expect("outcomes");

        assert!(!outcomes[0].recorded);
        assert_eq!(
            outcomes[0].reason,
            FuzzilliCoverageRecordingReason::GuardDisarmed
        );
    }

    #[test]
    fn session_semantics_classify_rejected_promises_as_failure() {
        let plan = FuzzilliInstrumentationPlanBuilder::new()
            .reprl_state(FuzzilliReprlState::InputMapped)
            .reject_unhandled_promises_as_failure(true)
            .build(FUZZILLI_SCHEMA_REGISTRY)
            .expect("plan");

        let outcome = plan
            .classify_session_outcome(FuzzilliReprlSession {
                state: FuzzilliReprlState::InputMapped,
                max_input_size: 1024,
                has_input_mapping: true,
                pending_rejected_promise_count: 1,
            })
            .expect("session outcome");

        assert_eq!(outcome.status, FuzzilliSessionStatus::FailedRejectedPromise);
        assert_eq!(outcome.pending_rejected_promise_count, 1);
    }

    #[test]
    fn execution_outcome_records_coverage() {
        let coverage_site = FuzzilliCoverageSite {
            id: FuzzilliCoverageSiteId(6),
            owner: None,
            bytecode_index: Some(BytecodeIndex::from_offset(12)),
            kind: FuzzilliEventKind::BasicBlock,
            guard_is_armed: true,
        };
        let plan = FuzzilliInstrumentationPlanBuilder::new()
            .site(coverage_site)
            .reprl_state(FuzzilliReprlState::InputMapped)
            .build(FUZZILLI_SCHEMA_REGISTRY)
            .expect("plan");
        let map = FuzzilliCoverageMap {
            storage: FuzzilliCoverageStorage::SharedMemory,
            edge_count: 1,
            max_edges: 8,
            sites: vec![coverage_site],
        };

        let outcome = plan
            .classify_execution_outcome(
                FuzzilliReprlSession {
                    state: FuzzilliReprlState::InputMapped,
                    max_input_size: 1024,
                    has_input_mapping: true,
                    pending_rejected_promise_count: 0,
                },
                &map,
                FuzzilliObservedProcessStatus::Succeeded,
                FUZZILLI_SCHEMA_REGISTRY,
            )
            .expect("execution outcome");

        assert_eq!(outcome.kind, FuzzilliExecutionOutcomeKind::Succeeded);
        assert_eq!(outcome.coverage_records.len(), 1);
        assert_eq!(outcome.coverage_records[0].counter_delta, 1);
    }

    #[test]
    fn execution_report_summarizes_coverage_and_gc() {
        let outcome = FuzzilliExecutionOutcomeRecord {
            kind: FuzzilliExecutionOutcomeKind::Succeeded,
            session: FuzzilliSessionSemanticOutcome {
                status: FuzzilliSessionStatus::AcceptedInput,
                pending_rejected_promise_count: 0,
                deterministic_gc: true,
                exposes_reprl_hooks: true,
            },
            process_status: FuzzilliObservedProcessStatus::Succeeded,
            coverage_records: vec![
                FuzzilliCoverageRecord {
                    site: FuzzilliCoverageSiteId(8),
                    event_kind: FuzzilliEventKind::BasicBlock,
                    owner: None,
                    bytecode_index: Some(BytecodeIndex::from_offset(1)),
                    recorded: true,
                    reason: FuzzilliCoverageRecordingReason::Recorded,
                    counter_delta: 1,
                },
                FuzzilliCoverageRecord {
                    site: FuzzilliCoverageSiteId(9),
                    event_kind: FuzzilliEventKind::BasicBlock,
                    owner: None,
                    bytecode_index: None,
                    recorded: false,
                    reason: FuzzilliCoverageRecordingReason::GuardDisarmed,
                    counter_delta: 0,
                },
            ],
        };
        let report = FuzzilliExecutionReport::from_outcome(
            outcome,
            vec![FuzzilliGcEventResultRecord {
                id: FuzzilliGcEventId(1),
                kind: FuzzilliGcEventResultKind::Completed,
                heap: Some(HeapId(1)),
                collection: Some(CollectionKind::Full),
                phase: Some(GcPhase::End),
                coverage_site: Some(FuzzilliCoverageSiteId(8)),
                reprl_state: FuzzilliReprlState::Flushing,
            }],
        );

        assert_eq!(report.coverage.recorded_count, 1);
        assert_eq!(report.coverage.suppressed_count, 1);
        assert_eq!(report.gc_completed_count, 1);
    }

    #[test]
    fn execution_outcome_prefers_rejected_promise_failure() {
        let coverage_site = FuzzilliCoverageSite {
            id: FuzzilliCoverageSiteId(7),
            owner: None,
            bytecode_index: None,
            kind: FuzzilliEventKind::BasicBlock,
            guard_is_armed: true,
        };
        let plan = FuzzilliInstrumentationPlanBuilder::new()
            .site(coverage_site)
            .reprl_state(FuzzilliReprlState::InputMapped)
            .reject_unhandled_promises_as_failure(true)
            .build(FUZZILLI_SCHEMA_REGISTRY)
            .expect("plan");
        let map = FuzzilliCoverageMap {
            storage: FuzzilliCoverageStorage::SharedMemory,
            edge_count: 1,
            max_edges: 8,
            sites: vec![coverage_site],
        };

        let outcome = plan
            .classify_execution_outcome(
                FuzzilliReprlSession {
                    state: FuzzilliReprlState::InputMapped,
                    max_input_size: 1024,
                    has_input_mapping: true,
                    pending_rejected_promise_count: 1,
                },
                &map,
                FuzzilliObservedProcessStatus::Succeeded,
                FUZZILLI_SCHEMA_REGISTRY,
            )
            .expect("execution outcome");

        assert_eq!(
            outcome.kind,
            FuzzilliExecutionOutcomeKind::RejectedPromiseFailure
        );
    }
}
