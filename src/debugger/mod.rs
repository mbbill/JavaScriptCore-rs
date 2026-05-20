//! Debugger-facing contracts.
//!
//! This module is intentionally a skeleton. It names the object graph for
//! breakpoints, stepping, scope inspection, async stack traces, and debugger
//! statements before any debugger behavior is implemented.

use crate::bytecode::SourceProviderId;
use crate::gc::{HeapId, HeapSnapshotId, HeapSnapshotKind};
use crate::interpreter::{InterpreterFrameKind, InterpreterFrameRecord};
use crate::jit::TierFallbackResultRecord;
use crate::runtime::{
    CallFrameId, CodeBlockId, GlobalObjectId, HostHookId, ObjectId, RuntimeValue, ScopeId,
    StackFrameId,
};

/// Debugger-local source identity.
///
/// This is not a source-provider owner. `bytecode::SourceProviderId` remains
/// the canonical identity for source storage; debugger sources are client
/// handles that borrow provider lifetime while attached.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DebuggerSourceId(pub u64);

/// Debugger breakpoint identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DebuggerBreakpointId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct DebuggerHeapObservationId(pub u64);

/// Position used by debugger and inspector breakpoints.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DebuggerPosition {
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebuggerHeapObservationKind {
    SnapshotRequested,
    SnapshotPublished,
    ObjectPreviewCaptured,
    ScopeObjectObserved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerHeapObservationRecord {
    pub id: DebuggerHeapObservationId,
    pub kind: DebuggerHeapObservationKind,
    pub heap: Option<HeapId>,
    pub snapshot: Option<HeapSnapshotId>,
    pub snapshot_kind: HeapSnapshotKind,
    pub source: Option<DebuggerSourceId>,
    pub call_frame: Option<CallFrameId>,
    pub object: Option<ObjectId>,
    pub observes_weak_edges: bool,
}

/// Owner of immutable debugger pause and breakpoint schemas.
///
/// `bytecode::SourceProviderId` and runtime stack identities remain canonical.
/// The debugger schema owner only names static metadata consumed by debugger
/// clients and inspector adapters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DebuggerSchemaOwner {
    #[default]
    DebuggerCore,
    InspectorDebuggerAgent,
    GeneratedProtocolMetadata,
    TestFixture,
}

/// Authority allowed to replace debugger schema registries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DebuggerRegistryMutationAuthority {
    #[default]
    CrateInitialization,
    GeneratedDataRefresh,
    DebuggerSessionBootstrap,
}

/// Provenance for debugger descriptor tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct DebuggerSchemaProvenance {
    pub generator: &'static str,
    pub source: &'static str,
    pub revision: u64,
}

impl DebuggerSchemaProvenance {
    pub const fn new(generator: &'static str, source: &'static str, revision: u64) -> Self {
        Self {
            generator,
            source,
            revision,
        }
    }
}

/// Attach state for one debugger and its inspected globals.
///
/// The C++ debugger is VM-owned and may attach to multiple global objects.
/// Detaching can be requested by session teardown or by global destruction; Rust
/// contracts should keep those reasons visible instead of treating detach as a
/// generic drop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebuggerAttachmentState {
    Detached,
    Attaching(GlobalObjectId),
    Attached(GlobalObjectId),
    Detaching(DebuggerDetachReason),
}

/// Source-driven reason for `Debugger::detach`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebuggerDetachReason {
    TerminatingDebuggingSession,
    GlobalObjectIsDestructing,
}

/// Breakpoint action category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BreakpointActionKind {
    Log,
    Evaluate,
    Sound,
    Probe,
}

/// Breakpoint action metadata without expression execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BreakpointAction {
    pub kind: BreakpointActionKind,
    pub action_id: u64,
    pub emulate_user_gesture: bool,
}

/// Breakpoint resolution state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BreakpointResolutionState {
    Unlinked,
    Linked,
    Resolved,
    Disabled,
    Removed,
}

/// Debugger-observed watchpoint family.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DebuggerWatchpointKind {
    Property,
    Structure,
    Variable,
    PromiseRejection,
}

/// Immutable watchpoint schema consumed by debugger clients.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerWatchpointSchema {
    pub name: &'static str,
    pub kind: DebuggerWatchpointKind,
    pub may_invalidate_code: bool,
    pub may_pause: bool,
    pub owner: DebuggerSchemaOwner,
    pub mutation_authority: DebuggerRegistryMutationAuthority,
    pub provenance: DebuggerSchemaProvenance,
}

/// Pause-position kind gathered by debugger parse data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebuggerPausePositionKind {
    Invalid,
    Enter,
    Pause,
    Leave,
}

/// Parser-discovered pause opportunity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerPausePosition {
    pub kind: DebuggerPausePositionKind,
    pub position: DebuggerPosition,
}

/// Source breakpoint descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BreakpointDescriptor {
    pub id: DebuggerBreakpointId,
    pub source: Option<DebuggerSourceId>,
    pub requested_position: DebuggerPosition,
    pub resolved_position: Option<DebuggerPosition>,
    pub condition_source: Option<SourceProviderId>,
    pub actions: Vec<BreakpointAction>,
    pub auto_continue: bool,
    pub ignore_count: usize,
    pub hit_count: usize,
    pub state: BreakpointResolutionState,
}

impl BreakpointDescriptor {
    pub fn validate(&self) -> Result<(), DebuggerValidationError> {
        if self.id.0 == 0 {
            return Err(DebuggerValidationError::ZeroBreakpointId);
        }
        validate_actions(&self.actions)?;

        match self.state {
            BreakpointResolutionState::Resolved
                if self.source.is_none() || self.resolved_position.is_none() =>
            {
                return Err(DebuggerValidationError::ResolvedBreakpointMissingLocation);
            }
            BreakpointResolutionState::Removed if self.breakpoints_active_fields_present() => {
                return Err(DebuggerValidationError::RemovedBreakpointKeepsRuntimeState);
            }
            _ => {}
        }

        Ok(())
    }

    fn breakpoints_active_fields_present(&self) -> bool {
        self.resolved_position.is_some() || self.hit_count != 0
    }

    pub fn matches_position(&self, context: DebuggerBreakpointMatchContext) -> bool {
        self.state == BreakpointResolutionState::Resolved
            && context.breakpoints_active
            && self.source == Some(context.source)
            && self.resolved_position == Some(context.position)
    }

    pub fn match_outcome(
        &self,
        context: DebuggerBreakpointMatchContext,
    ) -> Result<Option<DebuggerBreakpointMatch>, DebuggerValidationError> {
        self.validate()?;
        if !self.matches_position(context) {
            return Ok(None);
        }

        let next_hit_count = self.hit_count.saturating_add(1);
        let ignored_by_hit_count = next_hit_count <= self.ignore_count;
        Ok(Some(DebuggerBreakpointMatch {
            breakpoint: self.id,
            source: context.source,
            position: context.position,
            next_hit_count,
            ignored_by_hit_count,
            should_pause: !ignored_by_hit_count && !self.auto_continue,
            action_count: self.actions.len(),
            has_condition: self.condition_source.is_some(),
            auto_continue: self.auto_continue,
        }))
    }
}

/// Immutable breakpoint hit site used for descriptor matching.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerBreakpointMatchContext {
    pub source: DebuggerSourceId,
    pub position: DebuggerPosition,
    pub breakpoints_active: bool,
}

/// Pure breakpoint matching decision. This does not run conditions or actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerBreakpointMatch {
    pub breakpoint: DebuggerBreakpointId,
    pub source: DebuggerSourceId,
    pub position: DebuggerPosition,
    pub next_hit_count: usize,
    pub ignored_by_hit_count: bool,
    pub should_pause: bool,
    pub action_count: usize,
    pub has_condition: bool,
    pub auto_continue: bool,
}

pub fn match_breakpoints(
    breakpoints: &[BreakpointDescriptor],
    context: DebuggerBreakpointMatchContext,
) -> Result<Vec<DebuggerBreakpointMatch>, DebuggerValidationError> {
    let mut matches = Vec::new();
    for breakpoint in breakpoints {
        if let Some(outcome) = breakpoint.match_outcome(context)? {
            matches.push(outcome);
        }
    }
    Ok(matches)
}

/// Builder for source breakpoint descriptors.
#[derive(Clone, Debug)]
pub struct BreakpointDescriptorBuilder {
    descriptor: BreakpointDescriptor,
}

impl BreakpointDescriptorBuilder {
    pub fn new(id: DebuggerBreakpointId, requested_position: DebuggerPosition) -> Self {
        Self {
            descriptor: BreakpointDescriptor {
                id,
                source: None,
                requested_position,
                resolved_position: None,
                condition_source: None,
                actions: Vec::new(),
                auto_continue: false,
                ignore_count: 0,
                hit_count: 0,
                state: BreakpointResolutionState::Unlinked,
            },
        }
    }

    pub fn source(mut self, source: DebuggerSourceId) -> Self {
        self.descriptor.source = Some(source);
        self
    }

    pub fn resolved_position(mut self, position: DebuggerPosition) -> Self {
        self.descriptor.resolved_position = Some(position);
        self
    }

    pub fn condition_source(mut self, source: SourceProviderId) -> Self {
        self.descriptor.condition_source = Some(source);
        self
    }

    pub fn action(mut self, action: BreakpointAction) -> Self {
        self.descriptor.actions.push(action);
        self
    }

    pub fn auto_continue(mut self, auto_continue: bool) -> Self {
        self.descriptor.auto_continue = auto_continue;
        self
    }

    pub fn ignore_count(mut self, ignore_count: usize) -> Self {
        self.descriptor.ignore_count = ignore_count;
        self
    }

    pub fn hit_count(mut self, hit_count: usize) -> Self {
        self.descriptor.hit_count = hit_count;
        self
    }

    pub fn state(mut self, state: BreakpointResolutionState) -> Self {
        self.descriptor.state = state;
        self
    }

    pub fn build(self) -> Result<BreakpointDescriptor, DebuggerValidationError> {
        self.descriptor.validate()?;
        Ok(self.descriptor)
    }
}

/// Pause reason visible to debugger clients.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebuggerPauseReason {
    Breakpoint(DebuggerBreakpointId),
    DebuggerStatement,
    Exception,
    Assertion,
    Microtask,
    ExplicitPause,
    Step,
    WasmTrap,
    Await,
    BlackboxedScript,
    EndOfProgram,
}

/// Payload-free pause reason identity for static schemas.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DebuggerPauseReasonKind {
    Breakpoint,
    DebuggerStatement,
    Exception,
    Assertion,
    Microtask,
    ExplicitPause,
    Step,
    WasmTrap,
    Await,
    BlackboxedScript,
    EndOfProgram,
}

impl DebuggerPauseReason {
    pub const fn kind(self) -> DebuggerPauseReasonKind {
        match self {
            Self::Breakpoint(_) => DebuggerPauseReasonKind::Breakpoint,
            Self::DebuggerStatement => DebuggerPauseReasonKind::DebuggerStatement,
            Self::Exception => DebuggerPauseReasonKind::Exception,
            Self::Assertion => DebuggerPauseReasonKind::Assertion,
            Self::Microtask => DebuggerPauseReasonKind::Microtask,
            Self::ExplicitPause => DebuggerPauseReasonKind::ExplicitPause,
            Self::Step => DebuggerPauseReasonKind::Step,
            Self::WasmTrap => DebuggerPauseReasonKind::WasmTrap,
            Self::Await => DebuggerPauseReasonKind::Await,
            Self::BlackboxedScript => DebuggerPauseReasonKind::BlackboxedScript,
            Self::EndOfProgram => DebuggerPauseReasonKind::EndOfProgram,
        }
    }
}

/// Immutable metadata for one pause reason exposed to clients.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerPauseReasonDescriptor {
    pub kind: DebuggerPauseReasonKind,
    pub protocol_name: &'static str,
    pub carries_breakpoint_id: bool,
    pub may_expose_call_frames: bool,
}

/// Stepping mode requested by a debugger client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepMode {
    None,
    Next,
    Over,
    Into,
    Out,
    ContinueToLocation(DebuggerSourceId, DebuggerPosition),
    ContinueUntilNextRunLoop,
}

/// RAII-style pause scope state.
///
/// While paused, the debugger owns invalidation authority for borrowed
/// call-frame and scope wrappers. Inspector clients may inspect through those
/// wrappers but must not keep them valid after the paused scope ends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebuggerPausedScopeState {
    NotPaused,
    EnteringPause,
    DispatchingObservers,
    RunningNestedEventLoop,
    InvalidatingFrames,
    Continuing,
}

/// Debugger pause state for one VM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerPauseState {
    pub reason: Option<DebuggerPauseReason>,
    pub step_mode: StepMode,
    pub breakpoints_active: bool,
    pub suppress_all_pauses: bool,
    pub pause_on_all_exceptions: bool,
    pub pause_on_uncaught_exceptions: bool,
}

impl DebuggerPauseState {
    pub const fn new() -> Self {
        Self {
            reason: None,
            step_mode: StepMode::None,
            breakpoints_active: true,
            suppress_all_pauses: false,
            pause_on_all_exceptions: false,
            pause_on_uncaught_exceptions: false,
        }
    }

    pub fn validate(self) -> Result<(), DebuggerValidationError> {
        if self.suppress_all_pauses && self.reason.is_some() {
            return Err(DebuggerValidationError::SuppressedPauseHasReason);
        }
        if !self.breakpoints_active
            && matches!(self.reason, Some(DebuggerPauseReason::Breakpoint(_)))
        {
            return Err(DebuggerValidationError::InactiveBreakpointsHavePauseReason);
        }
        if let Some(DebuggerPauseReason::Breakpoint(id)) = self.reason {
            if id.0 == 0 {
                return Err(DebuggerValidationError::ZeroBreakpointId);
            }
        }
        Ok(())
    }

    pub fn apply_step_command(
        self,
        command: DebuggerStepCommand,
    ) -> Result<DebuggerStepTransition, DebuggerValidationError> {
        self.validate()?;
        let next_step_mode = match command {
            DebuggerStepCommand::Resume => StepMode::None,
            DebuggerStepCommand::StepNext => StepMode::Next,
            DebuggerStepCommand::StepInto => StepMode::Into,
            DebuggerStepCommand::StepOver => StepMode::Over,
            DebuggerStepCommand::StepOut => StepMode::Out,
            DebuggerStepCommand::ContinueToLocation(source, position) => {
                StepMode::ContinueToLocation(source, position)
            }
            DebuggerStepCommand::ContinueUntilNextRunLoop => StepMode::ContinueUntilNextRunLoop,
        };

        Ok(DebuggerStepTransition {
            previous_step_mode: self.step_mode,
            next_step_mode,
            clears_pause_reason: true,
            keeps_breakpoints_active: self.breakpoints_active,
        })
    }

    pub fn semantic_pause_outcome(
        self,
        observation: DebuggerPauseObservation,
        registry: DebuggerSchemaRegistry,
    ) -> Result<DebuggerPauseSemanticOutcome, DebuggerValidationError> {
        registry.validate()?;
        self.validate()?;

        if self.suppress_all_pauses {
            return Ok(DebuggerPauseSemanticOutcome {
                reason: observation.reason,
                should_pause: false,
                should_notify_clients: false,
                exposes_call_frames: false,
                consumes_step: false,
                breakpoint: None,
            });
        }

        let pause_schema = registry
            .pauses
            .iter()
            .find(|schema| schema.reason(observation.reason.kind()).is_some())
            .ok_or(DebuggerValidationError::PauseReasonMissingFromRegistry(
                observation.reason.kind(),
            ))?;
        let reason_descriptor = pause_schema.reason(observation.reason.kind()).ok_or(
            DebuggerValidationError::PauseReasonMissingFromRegistry(observation.reason.kind()),
        )?;

        if reason_descriptor.may_expose_call_frames && !observation.has_call_frames {
            return Err(DebuggerValidationError::PauseRequiresCallFrames(
                observation.reason.kind(),
            ));
        }

        let (should_pause, consumes_step, breakpoint) = match observation.reason {
            DebuggerPauseReason::Breakpoint(id) => {
                if !self.breakpoints_active {
                    (false, false, None)
                } else {
                    let matched = observation
                        .breakpoint_match
                        .ok_or(DebuggerValidationError::BreakpointPauseMissingMatch(id))?;
                    if matched.breakpoint != id {
                        return Err(DebuggerValidationError::BreakpointPauseIdMismatch {
                            expected: id,
                            actual: matched.breakpoint,
                        });
                    }
                    (matched.should_pause, false, Some(id))
                }
            }
            DebuggerPauseReason::Step => {
                if self.step_mode == StepMode::None {
                    return Err(DebuggerValidationError::StepPauseWithoutStepMode);
                }
                (true, true, None)
            }
            DebuggerPauseReason::Exception => (
                self.pause_on_all_exceptions
                    || (self.pause_on_uncaught_exceptions && observation.exception_is_uncaught),
                false,
                None,
            ),
            DebuggerPauseReason::BlackboxedScript => (false, false, None),
            DebuggerPauseReason::DebuggerStatement
            | DebuggerPauseReason::Assertion
            | DebuggerPauseReason::Microtask
            | DebuggerPauseReason::ExplicitPause
            | DebuggerPauseReason::WasmTrap
            | DebuggerPauseReason::Await
            | DebuggerPauseReason::EndOfProgram => (true, false, None),
        };

        Ok(DebuggerPauseSemanticOutcome {
            reason: observation.reason,
            should_pause,
            should_notify_clients: should_pause
                && observation.scope_state != DebuggerPausedScopeState::InvalidatingFrames,
            exposes_call_frames: should_pause && reason_descriptor.may_expose_call_frames,
            consumes_step,
            breakpoint,
        })
    }
}

impl Default for DebuggerPauseState {
    fn default() -> Self {
        Self::new()
    }
}

/// Immutable breakpoint schema consumed by debugger clients.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerBreakpointSchema {
    pub name: &'static str,
    pub allowed_actions: &'static [BreakpointActionKind],
    pub resolution_states: &'static [BreakpointResolutionState],
    pub supports_condition_source: bool,
    pub supports_ignore_count: bool,
    pub owner: DebuggerSchemaOwner,
    pub mutation_authority: DebuggerRegistryMutationAuthority,
    pub provenance: DebuggerSchemaProvenance,
}

/// Client stepping command reduced to debugger state mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebuggerStepCommand {
    Resume,
    StepNext,
    StepInto,
    StepOver,
    StepOut,
    ContinueToLocation(DebuggerSourceId, DebuggerPosition),
    ContinueUntilNextRunLoop,
}

/// Pure stepping transition. It does not resume or execute code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerStepTransition {
    pub previous_step_mode: StepMode,
    pub next_step_mode: StepMode,
    pub clears_pause_reason: bool,
    pub keeps_breakpoints_active: bool,
}

/// Observed pause opportunity before debugger clients are notified.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerPauseObservation {
    pub reason: DebuggerPauseReason,
    pub breakpoint_match: Option<DebuggerBreakpointMatch>,
    pub has_call_frames: bool,
    pub exception_is_uncaught: bool,
    pub scope_state: DebuggerPausedScopeState,
}

/// Semantic pause decision derived from debugger state and descriptors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerPauseSemanticOutcome {
    pub reason: DebuggerPauseReason,
    pub should_pause: bool,
    pub should_notify_clients: bool,
    pub exposes_call_frames: bool,
    pub consumes_step: bool,
    pub breakpoint: Option<DebuggerBreakpointId>,
}

/// Debugger-visible tier fallback diagnostic attached to pause reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerTierFallbackDiagnostic {
    pub fallback: TierFallbackResultRecord,
    pub resumes_in_interpreter: bool,
    pub clears_active_request: bool,
    pub bytecode_resume_visible: bool,
}

impl DebuggerTierFallbackDiagnostic {
    pub const fn from_record(fallback: TierFallbackResultRecord) -> Self {
        Self {
            fallback,
            resumes_in_interpreter: matches!(
                fallback.resume,
                crate::jit::TierFallbackResumeKind::ContinueInInterpreter
            ),
            clears_active_request: fallback.clears_active_request,
            bytecode_resume_visible: fallback.bytecode_index.is_some(),
        }
    }
}

/// Pause diagnostics assembled from interpreter observations and tier records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebuggerDiagnosticReport {
    pub pause: DebuggerPauseSemanticOutcome,
    pub call_frames: Vec<DebuggerCallFrameDescriptor>,
    pub tier_fallback: Option<DebuggerTierFallbackDiagnostic>,
    pub invalid_frame_count: usize,
}

impl DebuggerDiagnosticReport {
    pub fn from_interpreter_pause(
        state: DebuggerPauseState,
        observation: &DebuggerInterpreterPauseObservation,
        tier_fallback: Option<TierFallbackResultRecord>,
        registry: DebuggerSchemaRegistry,
    ) -> Result<Self, DebuggerValidationError> {
        let pause_observation = observation.to_pause_observation()?;
        let pause = state.semantic_pause_outcome(pause_observation, registry)?;
        let mut call_frames = Vec::with_capacity(observation.frames.len());
        let mut invalid_frame_count = 0;
        for frame in &observation.frames {
            let descriptor = frame.to_call_frame_descriptor()?;
            if descriptor.is_valid {
                call_frames.push(descriptor);
            } else {
                invalid_frame_count += 1;
            }
        }

        Ok(Self {
            pause,
            call_frames,
            tier_fallback: tier_fallback.map(DebuggerTierFallbackDiagnostic::from_record),
            invalid_frame_count,
        })
    }
}

/// Debugger observation of one interpreter frame during pause or stack walking.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebuggerInterpreterFrameObservation {
    pub frame: InterpreterFrameRecord,
    pub stack_frame: Option<StackFrameId>,
    pub caller: Option<StackFrameId>,
    pub source: Option<DebuggerSourceId>,
    pub position: DebuggerPosition,
    pub this_object: Option<ObjectId>,
    pub is_tail_deleted: bool,
}

impl DebuggerInterpreterFrameObservation {
    pub fn to_call_frame_descriptor(
        &self,
    ) -> Result<DebuggerCallFrameDescriptor, DebuggerValidationError> {
        let has_identity = self.frame.frame.is_some() || self.stack_frame.is_some();
        let descriptor = DebuggerCallFrameDescriptor {
            frame: self.frame.frame,
            stack_frame: self.stack_frame,
            caller: if has_identity { self.caller } else { None },
            kind: debugger_call_frame_kind(self.frame.kind),
            source: if has_identity { self.source } else { None },
            position: self.position,
            lexical_scope: if has_identity {
                self.frame.lexical_scope
            } else {
                None
            },
            this_object: if has_identity { self.this_object } else { None },
            is_tail_deleted: self.is_tail_deleted,
            is_valid: has_identity,
        };
        descriptor.validate()?;
        Ok(descriptor)
    }
}

/// Pause observation assembled from interpreter frames without resuming or dispatching.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebuggerInterpreterPauseObservation {
    pub reason: DebuggerPauseReason,
    pub breakpoint_match: Option<DebuggerBreakpointMatch>,
    pub frames: Vec<DebuggerInterpreterFrameObservation>,
    pub exception_is_uncaught: bool,
    pub scope_state: DebuggerPausedScopeState,
}

impl DebuggerInterpreterPauseObservation {
    pub fn to_pause_observation(
        &self,
    ) -> Result<DebuggerPauseObservation, DebuggerValidationError> {
        let mut has_call_frames = false;
        for frame in &self.frames {
            if frame.to_call_frame_descriptor()?.is_valid {
                has_call_frames = true;
            }
        }

        Ok(DebuggerPauseObservation {
            reason: self.reason,
            breakpoint_match: self.breakpoint_match,
            has_call_frames,
            exception_is_uncaught: self.exception_is_uncaught,
            scope_state: self.scope_state,
        })
    }
}

fn debugger_call_frame_kind(kind: Option<InterpreterFrameKind>) -> DebuggerCallFrameKind {
    match kind {
        Some(InterpreterFrameKind::Entry) => DebuggerCallFrameKind::Program,
        Some(InterpreterFrameKind::JavaScript) => DebuggerCallFrameKind::Function,
        Some(InterpreterFrameKind::WasmBridge) => DebuggerCallFrameKind::Wasm,
        Some(
            InterpreterFrameKind::Native
            | InterpreterFrameKind::HostCallback
            | InterpreterFrameKind::Microtask,
        )
        | None => DebuggerCallFrameKind::Native,
    }
}

impl DebuggerBreakpointSchema {
    pub const fn allowed_actions(self) -> &'static [BreakpointActionKind] {
        self.allowed_actions
    }

    pub const fn resolution_states(self) -> &'static [BreakpointResolutionState] {
        self.resolution_states
    }

    pub fn validate(self) -> Result<(), DebuggerValidationError> {
        validate_non_empty(
            self.name,
            DebuggerValidationError::EmptyBreakpointSchemaName,
        )?;
        if self.allowed_actions.is_empty() {
            return Err(DebuggerValidationError::EmptyBreakpointActions);
        }
        if self.resolution_states.is_empty() {
            return Err(DebuggerValidationError::EmptyBreakpointResolutionStates);
        }
        validate_unique_actions(self.allowed_actions)?;
        validate_unique_resolution_states(self.resolution_states)?;
        validate_non_empty(
            self.provenance.generator,
            DebuggerValidationError::EmptyProvenanceField,
        )?;
        validate_non_empty(
            self.provenance.source,
            DebuggerValidationError::EmptyProvenanceField,
        )
    }
}

/// Immutable pause-state schema consumed by debugger clients.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerPauseSchema {
    pub name: &'static str,
    pub reasons: &'static [DebuggerPauseReasonDescriptor],
    pub call_frame_kinds: &'static [DebuggerCallFrameKind],
    pub scope_kinds: &'static [DebuggerScopeKind],
    pub owner: DebuggerSchemaOwner,
    pub mutation_authority: DebuggerRegistryMutationAuthority,
    pub provenance: DebuggerSchemaProvenance,
}

impl DebuggerPauseSchema {
    pub const fn reasons(self) -> &'static [DebuggerPauseReasonDescriptor] {
        self.reasons
    }

    pub fn reason(
        self,
        kind: DebuggerPauseReasonKind,
    ) -> Option<&'static DebuggerPauseReasonDescriptor> {
        self.reasons
            .iter()
            .find(|descriptor| descriptor.kind == kind)
    }

    pub fn validate(self) -> Result<(), DebuggerValidationError> {
        validate_non_empty(self.name, DebuggerValidationError::EmptyPauseSchemaName)?;
        if self.reasons.is_empty() {
            return Err(DebuggerValidationError::EmptyPauseReasons);
        }
        if self.call_frame_kinds.is_empty() {
            return Err(DebuggerValidationError::EmptyCallFrameKinds);
        }
        if self.scope_kinds.is_empty() {
            return Err(DebuggerValidationError::EmptyScopeKinds);
        }
        validate_pause_reasons(self.reasons)?;
        validate_unique_call_frame_kinds(self.call_frame_kinds)?;
        validate_unique_scope_kinds(self.scope_kinds)?;
        validate_non_empty(
            self.provenance.generator,
            DebuggerValidationError::EmptyProvenanceField,
        )?;
        validate_non_empty(
            self.provenance.source,
            DebuggerValidationError::EmptyProvenanceField,
        )
    }
}

impl DebuggerWatchpointSchema {
    pub fn validate(self) -> Result<(), DebuggerValidationError> {
        validate_non_empty(
            self.name,
            DebuggerValidationError::EmptyWatchpointSchemaName,
        )?;
        if !self.may_invalidate_code && !self.may_pause {
            return Err(DebuggerValidationError::WatchpointHasNoEffect(self.name));
        }
        validate_non_empty(
            self.provenance.generator,
            DebuggerValidationError::EmptyProvenanceField,
        )?;
        validate_non_empty(
            self.provenance.source,
            DebuggerValidationError::EmptyProvenanceField,
        )
    }

    pub fn match_observation(
        self,
        observation: DebuggerWatchpointObservation,
    ) -> Result<Option<DebuggerWatchpointMatch>, DebuggerValidationError> {
        self.validate()?;
        if self.kind != observation.kind {
            return Ok(None);
        }

        Ok(Some(DebuggerWatchpointMatch {
            schema_name: self.name,
            kind: self.kind,
            should_invalidate_code: self.may_invalidate_code && observation.can_invalidate_code,
            should_pause: self.may_pause && observation.can_pause,
        }))
    }
}

/// Watchpoint event metadata observed without touching runtime watchpoint sets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerWatchpointObservation {
    pub kind: DebuggerWatchpointKind,
    pub can_invalidate_code: bool,
    pub can_pause: bool,
}

/// Pure watchpoint routing decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerWatchpointMatch {
    pub schema_name: &'static str,
    pub kind: DebuggerWatchpointKind,
    pub should_invalidate_code: bool,
    pub should_pause: bool,
}

/// Structural debugger descriptor validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DebuggerValidationError {
    EmptyBreakpointSchemaName,
    EmptyPauseSchemaName,
    EmptyWatchpointSchemaName,
    EmptyBreakpointActions,
    EmptyBreakpointResolutionStates,
    EmptyPauseReasons,
    EmptyCallFrameKinds,
    EmptyScopeKinds,
    EmptyProtocolName,
    EmptyProvenanceField,
    DuplicateBreakpointSchemaName(&'static str),
    DuplicatePauseSchemaName(&'static str),
    DuplicateWatchpointSchemaName(&'static str),
    DuplicateBreakpointAction(BreakpointActionKind),
    DuplicateBreakpointActionId(u64),
    DuplicateResolutionState(BreakpointResolutionState),
    DuplicatePauseReason(DebuggerPauseReasonKind),
    DuplicatePauseProtocolName(&'static str),
    DuplicateCallFrameKind(DebuggerCallFrameKind),
    DuplicateScopeKind(DebuggerScopeKind),
    ZeroBreakpointId,
    ZeroBreakpointActionId,
    ResolvedBreakpointMissingLocation,
    RemovedBreakpointKeepsRuntimeState,
    SuppressedPauseHasReason,
    InactiveBreakpointsHavePauseReason,
    BreakpointReasonMustCarryId,
    NonBreakpointReasonCarriesId(DebuggerPauseReasonKind),
    WatchpointHasNoEffect(&'static str),
    PauseReasonMissingFromRegistry(DebuggerPauseReasonKind),
    PauseRequiresCallFrames(DebuggerPauseReasonKind),
    BreakpointPauseMissingMatch(DebuggerBreakpointId),
    BreakpointPauseIdMismatch {
        expected: DebuggerBreakpointId,
        actual: DebuggerBreakpointId,
    },
    StepPauseWithoutStepMode,
    ValidCallFrameMissingFrameIdentity,
    InvalidCallFrameKeepsBorrowedState,
    ValidScopeMissingIdentity,
    InvalidScopeKeepsBorrowedState,
}

/// Registry for debugger breakpoint and pause schemas.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DebuggerSchemaRegistry {
    pub breakpoints: &'static [DebuggerBreakpointSchema],
    pub pauses: &'static [DebuggerPauseSchema],
    pub watchpoints: &'static [DebuggerWatchpointSchema],
}

impl DebuggerSchemaRegistry {
    pub const fn new(
        breakpoints: &'static [DebuggerBreakpointSchema],
        pauses: &'static [DebuggerPauseSchema],
        watchpoints: &'static [DebuggerWatchpointSchema],
    ) -> Self {
        Self {
            breakpoints,
            pauses,
            watchpoints,
        }
    }

    pub const fn breakpoints(self) -> &'static [DebuggerBreakpointSchema] {
        self.breakpoints
    }

    pub const fn pauses(self) -> &'static [DebuggerPauseSchema] {
        self.pauses
    }

    pub const fn watchpoints(self) -> &'static [DebuggerWatchpointSchema] {
        self.watchpoints
    }

    pub fn breakpoint_named(self, name: &str) -> Option<&'static DebuggerBreakpointSchema> {
        self.breakpoints
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn pause_named(self, name: &str) -> Option<&'static DebuggerPauseSchema> {
        self.pauses
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn watchpoint_named(self, name: &str) -> Option<&'static DebuggerWatchpointSchema> {
        self.watchpoints
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn match_watchpoint(
        self,
        observation: DebuggerWatchpointObservation,
    ) -> Result<Option<DebuggerWatchpointMatch>, DebuggerValidationError> {
        self.validate()?;
        for schema in self.watchpoints {
            if let Some(outcome) = schema.match_observation(observation)? {
                return Ok(Some(outcome));
            }
        }
        Ok(None)
    }

    pub fn validate(self) -> Result<(), DebuggerValidationError> {
        validate_unique_names(
            self.breakpoints.iter().map(|schema| schema.name),
            DebuggerValidationError::DuplicateBreakpointSchemaName,
        )?;
        validate_unique_names(
            self.pauses.iter().map(|schema| schema.name),
            DebuggerValidationError::DuplicatePauseSchemaName,
        )?;
        validate_unique_names(
            self.watchpoints.iter().map(|schema| schema.name),
            DebuggerValidationError::DuplicateWatchpointSchemaName,
        )?;

        for schema in self.breakpoints {
            schema.validate()?;
        }
        for schema in self.pauses {
            schema.validate()?;
        }
        for schema in self.watchpoints {
            schema.validate()?;
        }

        Ok(())
    }
}

/// Temporarily disabled exception-breakpoint state.
///
/// This mirrors `TemporarilyDisableExceptionBreakpoints`: the debugger, not the
/// caller, owns replacement and restoration of the all/uncaught breakpoints.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerExceptionBreakpointMask {
    pub all_exceptions: Option<DebuggerBreakpointId>,
    pub uncaught_exceptions: Option<DebuggerBreakpointId>,
    pub replacement_installed: bool,
}

/// Debugger-visible call-frame kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebuggerCallFrameKind {
    Program,
    Function,
    Eval,
    Module,
    Native,
    Wasm,
}

/// Borrowed debugger call-frame descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerCallFrameDescriptor {
    pub frame: Option<CallFrameId>,
    pub stack_frame: Option<StackFrameId>,
    pub caller: Option<StackFrameId>,
    pub kind: DebuggerCallFrameKind,
    pub source: Option<DebuggerSourceId>,
    pub position: DebuggerPosition,
    pub lexical_scope: Option<ScopeId>,
    pub this_object: Option<ObjectId>,
    pub is_tail_deleted: bool,
    pub is_valid: bool,
}

impl DebuggerCallFrameDescriptor {
    pub fn validate(self) -> Result<(), DebuggerValidationError> {
        if self.is_valid {
            if self.frame.is_none() && self.stack_frame.is_none() {
                return Err(DebuggerValidationError::ValidCallFrameMissingFrameIdentity);
            }
            return Ok(());
        }

        if self.frame.is_some()
            || self.stack_frame.is_some()
            || self.caller.is_some()
            || self.lexical_scope.is_some()
            || self.this_object.is_some()
        {
            return Err(DebuggerValidationError::InvalidCallFrameKeepsBorrowedState);
        }

        Ok(())
    }
}

/// Scope family exposed during debugger inspection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebuggerScopeKind {
    Global,
    Local,
    Closure,
    Catch,
    With,
    Module,
    PrivateName,
    WasmLocals,
}

/// Scope object snapshot visible to injected-script and inspector code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerScopeDescriptor {
    pub scope: Option<ScopeId>,
    pub object: Option<ObjectId>,
    pub kind: DebuggerScopeKind,
    pub depth: u32,
    pub can_evaluate: bool,
    pub is_valid: bool,
}

impl DebuggerScopeDescriptor {
    pub fn validate(self) -> Result<(), DebuggerValidationError> {
        if self.is_valid {
            if self.scope.is_none() && self.object.is_none() {
                return Err(DebuggerValidationError::ValidScopeMissingIdentity);
            }
            return Ok(());
        }

        if self.scope.is_some() || self.object.is_some() || self.can_evaluate {
            return Err(DebuggerValidationError::InvalidScopeKeepsBorrowedState);
        }

        Ok(())
    }
}

/// Authority granted while debugger evaluation temporarily enables eval.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebuggerEvalMode {
    EvalOnCurrentCallFrame,
    EvalOnGlobalObjectAtDebuggerEntry,
}

/// Snapshot of the global flags temporarily changed for debugger eval.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerEvalAuthority {
    pub global_object: Option<GlobalObjectId>,
    pub mode: DebuggerEvalMode,
    pub eval_was_disabled: bool,
    pub trusted_types_were_disabled: bool,
}

/// Evaluation request scoped to a paused call frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebuggerEvaluationRequest {
    pub frame: DebuggerCallFrameDescriptor,
    pub expression_source: SourceProviderId,
    pub scope_extension: Option<ObjectId>,
    pub emulate_user_gesture: bool,
}

/// Evaluation outcome placeholder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebuggerEvaluationOutcome {
    Value(RuntimeValue),
    Threw(RuntimeValue),
    Terminated,
    NotEvaluated,
}

/// Debugger script metadata used by parse notifications.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerScript {
    pub source: DebuggerSourceId,
    pub provider: SourceProviderId,
    pub start_position: DebuggerPosition,
    pub end_position: DebuggerPosition,
    pub is_module: bool,
    pub is_internal: bool,
    pub is_content_script: bool,
}

/// Observer/client capability installed on a debugger.
///
/// Observers receive parse, pause, continue, microtask, native-executable, and
/// breakpoint-action notifications. A client may additionally provide debugger
/// scope extension objects and evaluation hooks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerClientCapability {
    pub hook: Option<HostHookId>,
    pub is_inspector_debugger_agent: bool,
    pub may_extend_scope: bool,
    pub may_wrap_breakpoint_evaluation: bool,
}

/// Blackbox rule associated with a source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebuggerBlackboxRule {
    pub source: DebuggerSourceId,
    pub ranges: Vec<(DebuggerPosition, DebuggerPosition)>,
    pub ignore_pauses: bool,
    pub defer_pauses: bool,
    pub affects_breakpoint_evaluations: bool,
}

/// Request to apply debugger state to compiled code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebuggerCodeInstrumentation {
    pub code_block: CodeBlockId,
    pub source: Option<DebuggerSourceId>,
    pub breakpoints_active: bool,
    pub needs_debugger_statement_hooks: bool,
}

const DEBUGGER_SCHEMA_PROVENANCE: DebuggerSchemaProvenance = DebuggerSchemaProvenance {
    generator: "hand-authored",
    source: "Source/JavaScriptCore/rust/src/debugger/mod.rs",
    revision: 1,
};

pub const DEBUGGER_BREAKPOINT_ACTIONS: &[BreakpointActionKind] = &[
    BreakpointActionKind::Log,
    BreakpointActionKind::Evaluate,
    BreakpointActionKind::Sound,
    BreakpointActionKind::Probe,
];

pub const DEBUGGER_BREAKPOINT_RESOLUTION_STATES: &[BreakpointResolutionState] = &[
    BreakpointResolutionState::Unlinked,
    BreakpointResolutionState::Linked,
    BreakpointResolutionState::Resolved,
    BreakpointResolutionState::Disabled,
    BreakpointResolutionState::Removed,
];

pub const DEBUGGER_PAUSE_REASON_DESCRIPTORS: &[DebuggerPauseReasonDescriptor] = &[
    DebuggerPauseReasonDescriptor {
        kind: DebuggerPauseReasonKind::Breakpoint,
        protocol_name: "breakpoint",
        carries_breakpoint_id: true,
        may_expose_call_frames: true,
    },
    DebuggerPauseReasonDescriptor {
        kind: DebuggerPauseReasonKind::DebuggerStatement,
        protocol_name: "debuggerStatement",
        carries_breakpoint_id: false,
        may_expose_call_frames: true,
    },
    DebuggerPauseReasonDescriptor {
        kind: DebuggerPauseReasonKind::Exception,
        protocol_name: "exception",
        carries_breakpoint_id: false,
        may_expose_call_frames: true,
    },
    DebuggerPauseReasonDescriptor {
        kind: DebuggerPauseReasonKind::Assertion,
        protocol_name: "assert",
        carries_breakpoint_id: false,
        may_expose_call_frames: true,
    },
    DebuggerPauseReasonDescriptor {
        kind: DebuggerPauseReasonKind::Microtask,
        protocol_name: "microtask",
        carries_breakpoint_id: false,
        may_expose_call_frames: true,
    },
    DebuggerPauseReasonDescriptor {
        kind: DebuggerPauseReasonKind::ExplicitPause,
        protocol_name: "pause",
        carries_breakpoint_id: false,
        may_expose_call_frames: true,
    },
    DebuggerPauseReasonDescriptor {
        kind: DebuggerPauseReasonKind::Step,
        protocol_name: "step",
        carries_breakpoint_id: false,
        may_expose_call_frames: true,
    },
    DebuggerPauseReasonDescriptor {
        kind: DebuggerPauseReasonKind::WasmTrap,
        protocol_name: "wasmTrap",
        carries_breakpoint_id: false,
        may_expose_call_frames: true,
    },
    DebuggerPauseReasonDescriptor {
        kind: DebuggerPauseReasonKind::Await,
        protocol_name: "await",
        carries_breakpoint_id: false,
        may_expose_call_frames: true,
    },
    DebuggerPauseReasonDescriptor {
        kind: DebuggerPauseReasonKind::BlackboxedScript,
        protocol_name: "blackboxedScript",
        carries_breakpoint_id: false,
        may_expose_call_frames: false,
    },
    DebuggerPauseReasonDescriptor {
        kind: DebuggerPauseReasonKind::EndOfProgram,
        protocol_name: "endOfProgram",
        carries_breakpoint_id: false,
        may_expose_call_frames: false,
    },
];

pub const DEBUGGER_CALL_FRAME_KINDS: &[DebuggerCallFrameKind] = &[
    DebuggerCallFrameKind::Program,
    DebuggerCallFrameKind::Function,
    DebuggerCallFrameKind::Eval,
    DebuggerCallFrameKind::Module,
    DebuggerCallFrameKind::Native,
    DebuggerCallFrameKind::Wasm,
];

pub const DEBUGGER_SCOPE_KINDS: &[DebuggerScopeKind] = &[
    DebuggerScopeKind::Global,
    DebuggerScopeKind::Local,
    DebuggerScopeKind::Closure,
    DebuggerScopeKind::Catch,
    DebuggerScopeKind::With,
    DebuggerScopeKind::Module,
    DebuggerScopeKind::PrivateName,
    DebuggerScopeKind::WasmLocals,
];

pub const DEBUGGER_BREAKPOINT_SCHEMAS: &[DebuggerBreakpointSchema] = &[DebuggerBreakpointSchema {
    name: "source-breakpoint",
    allowed_actions: DEBUGGER_BREAKPOINT_ACTIONS,
    resolution_states: DEBUGGER_BREAKPOINT_RESOLUTION_STATES,
    supports_condition_source: true,
    supports_ignore_count: true,
    owner: DebuggerSchemaOwner::DebuggerCore,
    mutation_authority: DebuggerRegistryMutationAuthority::CrateInitialization,
    provenance: DEBUGGER_SCHEMA_PROVENANCE,
}];

pub const DEBUGGER_PAUSE_SCHEMAS: &[DebuggerPauseSchema] = &[DebuggerPauseSchema {
    name: "debugger-pause",
    reasons: DEBUGGER_PAUSE_REASON_DESCRIPTORS,
    call_frame_kinds: DEBUGGER_CALL_FRAME_KINDS,
    scope_kinds: DEBUGGER_SCOPE_KINDS,
    owner: DebuggerSchemaOwner::DebuggerCore,
    mutation_authority: DebuggerRegistryMutationAuthority::CrateInitialization,
    provenance: DEBUGGER_SCHEMA_PROVENANCE,
}];

pub const DEBUGGER_WATCHPOINT_SCHEMAS: &[DebuggerWatchpointSchema] = &[
    DebuggerWatchpointSchema {
        name: "property-watchpoint",
        kind: DebuggerWatchpointKind::Property,
        may_invalidate_code: true,
        may_pause: true,
        owner: DebuggerSchemaOwner::DebuggerCore,
        mutation_authority: DebuggerRegistryMutationAuthority::CrateInitialization,
        provenance: DEBUGGER_SCHEMA_PROVENANCE,
    },
    DebuggerWatchpointSchema {
        name: "structure-watchpoint",
        kind: DebuggerWatchpointKind::Structure,
        may_invalidate_code: true,
        may_pause: false,
        owner: DebuggerSchemaOwner::DebuggerCore,
        mutation_authority: DebuggerRegistryMutationAuthority::CrateInitialization,
        provenance: DEBUGGER_SCHEMA_PROVENANCE,
    },
];

pub const DEBUGGER_SCHEMA_REGISTRY: DebuggerSchemaRegistry = DebuggerSchemaRegistry {
    breakpoints: DEBUGGER_BREAKPOINT_SCHEMAS,
    pauses: DEBUGGER_PAUSE_SCHEMAS,
    watchpoints: DEBUGGER_WATCHPOINT_SCHEMAS,
};

fn validate_non_empty(
    value: &'static str,
    error: DebuggerValidationError,
) -> Result<(), DebuggerValidationError> {
    if value.is_empty() {
        Err(error)
    } else {
        Ok(())
    }
}

fn validate_unique_names<I, F>(names: I, duplicate: F) -> Result<(), DebuggerValidationError>
where
    I: Clone + Iterator<Item = &'static str>,
    F: Fn(&'static str) -> DebuggerValidationError,
{
    for (index, name) in names.clone().enumerate() {
        for other in names.clone().skip(index + 1) {
            if name == other {
                return Err(duplicate(name));
            }
        }
    }
    Ok(())
}

fn validate_actions(actions: &[BreakpointAction]) -> Result<(), DebuggerValidationError> {
    for (index, action) in actions.iter().enumerate() {
        if action.action_id == 0 {
            return Err(DebuggerValidationError::ZeroBreakpointActionId);
        }
        for other in actions.iter().skip(index + 1) {
            if action.action_id == other.action_id {
                return Err(DebuggerValidationError::DuplicateBreakpointActionId(
                    action.action_id,
                ));
            }
        }
    }
    Ok(())
}

fn validate_unique_actions(
    actions: &[BreakpointActionKind],
) -> Result<(), DebuggerValidationError> {
    for (index, action) in actions.iter().enumerate() {
        for other in actions.iter().skip(index + 1) {
            if action == other {
                return Err(DebuggerValidationError::DuplicateBreakpointAction(*action));
            }
        }
    }
    Ok(())
}

fn validate_unique_resolution_states(
    states: &[BreakpointResolutionState],
) -> Result<(), DebuggerValidationError> {
    for (index, state) in states.iter().enumerate() {
        for other in states.iter().skip(index + 1) {
            if state == other {
                return Err(DebuggerValidationError::DuplicateResolutionState(*state));
            }
        }
    }
    Ok(())
}

fn validate_pause_reasons(
    reasons: &[DebuggerPauseReasonDescriptor],
) -> Result<(), DebuggerValidationError> {
    for (index, reason) in reasons.iter().enumerate() {
        validate_non_empty(
            reason.protocol_name,
            DebuggerValidationError::EmptyProtocolName,
        )?;
        match reason.kind {
            DebuggerPauseReasonKind::Breakpoint if !reason.carries_breakpoint_id => {
                return Err(DebuggerValidationError::BreakpointReasonMustCarryId);
            }
            DebuggerPauseReasonKind::Breakpoint => {}
            _ if reason.carries_breakpoint_id => {
                return Err(DebuggerValidationError::NonBreakpointReasonCarriesId(
                    reason.kind,
                ));
            }
            _ => {}
        }

        for other in reasons.iter().skip(index + 1) {
            if reason.kind == other.kind {
                return Err(DebuggerValidationError::DuplicatePauseReason(reason.kind));
            }
            if reason.protocol_name == other.protocol_name {
                return Err(DebuggerValidationError::DuplicatePauseProtocolName(
                    reason.protocol_name,
                ));
            }
        }
    }
    Ok(())
}

fn validate_unique_call_frame_kinds(
    kinds: &[DebuggerCallFrameKind],
) -> Result<(), DebuggerValidationError> {
    for (index, kind) in kinds.iter().enumerate() {
        for other in kinds.iter().skip(index + 1) {
            if kind == other {
                return Err(DebuggerValidationError::DuplicateCallFrameKind(*kind));
            }
        }
    }
    Ok(())
}

fn validate_unique_scope_kinds(kinds: &[DebuggerScopeKind]) -> Result<(), DebuggerValidationError> {
    for (index, kind) in kinds.iter().enumerate() {
        for other in kinds.iter().skip(index + 1) {
            if kind == other {
                return Err(DebuggerValidationError::DuplicateScopeKind(*kind));
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
    fn validates_builtin_debugger_registry() {
        assert_eq!(DEBUGGER_SCHEMA_REGISTRY.validate(), Ok(()));
    }

    #[test]
    fn rejects_zero_breakpoint_id() {
        let result = BreakpointDescriptorBuilder::new(
            DebuggerBreakpointId(0),
            DebuggerPosition { line: 1, column: 0 },
        )
        .build();

        assert_eq!(result, Err(DebuggerValidationError::ZeroBreakpointId));
    }

    #[test]
    fn rejects_valid_frame_without_identity() {
        let frame = DebuggerCallFrameDescriptor {
            frame: None,
            stack_frame: None,
            caller: None,
            kind: DebuggerCallFrameKind::Function,
            source: None,
            position: DebuggerPosition { line: 0, column: 0 },
            lexical_scope: None,
            this_object: None,
            is_tail_deleted: false,
            is_valid: true,
        };

        assert_eq!(
            frame.validate(),
            Err(DebuggerValidationError::ValidCallFrameMissingFrameIdentity)
        );
    }

    #[test]
    fn matches_resolved_breakpoint_at_source_position() {
        let breakpoint = BreakpointDescriptorBuilder::new(
            DebuggerBreakpointId(7),
            DebuggerPosition {
                line: 10,
                column: 0,
            },
        )
        .source(DebuggerSourceId(3))
        .resolved_position(DebuggerPosition {
            line: 12,
            column: 4,
        })
        .state(BreakpointResolutionState::Resolved)
        .ignore_count(1)
        .hit_count(1)
        .build()
        .expect("breakpoint");

        let matches = match_breakpoints(
            &[breakpoint],
            DebuggerBreakpointMatchContext {
                source: DebuggerSourceId(3),
                position: DebuggerPosition {
                    line: 12,
                    column: 4,
                },
                breakpoints_active: true,
            },
        )
        .expect("matches");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].breakpoint, DebuggerBreakpointId(7));
        assert_eq!(matches[0].next_hit_count, 2);
        assert!(matches[0].should_pause);
    }

    #[test]
    fn inactive_breakpoints_do_not_match() {
        let breakpoint = BreakpointDescriptorBuilder::new(
            DebuggerBreakpointId(8),
            DebuggerPosition { line: 1, column: 0 },
        )
        .source(DebuggerSourceId(4))
        .resolved_position(DebuggerPosition { line: 1, column: 0 })
        .state(BreakpointResolutionState::Resolved)
        .build()
        .expect("breakpoint");

        let matches = match_breakpoints(
            &[breakpoint],
            DebuggerBreakpointMatchContext {
                source: DebuggerSourceId(4),
                position: DebuggerPosition { line: 1, column: 0 },
                breakpoints_active: false,
            },
        )
        .expect("matches");

        assert!(matches.is_empty());
    }

    #[test]
    fn routes_watchpoint_observation_through_schema() {
        let matched = DEBUGGER_SCHEMA_REGISTRY
            .match_watchpoint(DebuggerWatchpointObservation {
                kind: DebuggerWatchpointKind::Property,
                can_invalidate_code: true,
                can_pause: true,
            })
            .expect("watchpoint")
            .expect("match");

        assert_eq!(matched.schema_name, "property-watchpoint");
        assert!(matched.should_invalidate_code);
        assert!(matched.should_pause);
    }

    #[test]
    fn pause_semantics_accept_matching_breakpoint_pause() {
        let matched = DebuggerBreakpointMatch {
            breakpoint: DebuggerBreakpointId(10),
            source: DebuggerSourceId(2),
            position: DebuggerPosition { line: 4, column: 2 },
            next_hit_count: 1,
            ignored_by_hit_count: false,
            should_pause: true,
            action_count: 0,
            has_condition: false,
            auto_continue: false,
        };

        let outcome = DebuggerPauseState::new()
            .semantic_pause_outcome(
                DebuggerPauseObservation {
                    reason: DebuggerPauseReason::Breakpoint(DebuggerBreakpointId(10)),
                    breakpoint_match: Some(matched),
                    has_call_frames: true,
                    exception_is_uncaught: false,
                    scope_state: DebuggerPausedScopeState::DispatchingObservers,
                },
                DEBUGGER_SCHEMA_REGISTRY,
            )
            .expect("pause outcome");

        assert!(outcome.should_pause);
        assert!(outcome.should_notify_clients);
        assert_eq!(outcome.breakpoint, Some(DebuggerBreakpointId(10)));
    }

    #[test]
    fn pause_semantics_reject_step_pause_without_step_mode() {
        assert_eq!(
            DebuggerPauseState::new().semantic_pause_outcome(
                DebuggerPauseObservation {
                    reason: DebuggerPauseReason::Step,
                    breakpoint_match: None,
                    has_call_frames: true,
                    exception_is_uncaught: false,
                    scope_state: DebuggerPausedScopeState::DispatchingObservers,
                },
                DEBUGGER_SCHEMA_REGISTRY,
            ),
            Err(DebuggerValidationError::StepPauseWithoutStepMode)
        );
    }

    #[test]
    fn observes_interpreter_frame_as_debugger_call_frame() {
        let observation = DebuggerInterpreterFrameObservation {
            frame: InterpreterFrameRecord {
                frame: Some(CallFrameId(3)),
                entry_frame: None,
                kind: Some(InterpreterFrameKind::JavaScript),
                code_block: Some(CodeBlockId(CellId(7))),
                bytecode_index: None,
                callee: None,
                lexical_scope: Some(ScopeId(11)),
            },
            stack_frame: Some(StackFrameId(5)),
            caller: None,
            source: Some(DebuggerSourceId(2)),
            position: DebuggerPosition {
                line: 10,
                column: 4,
            },
            this_object: Some(ObjectId(CellId(13))),
            is_tail_deleted: false,
        };

        let descriptor = observation
            .to_call_frame_descriptor()
            .expect("call frame descriptor");

        assert!(descriptor.is_valid);
        assert_eq!(descriptor.kind, DebuggerCallFrameKind::Function);
        assert_eq!(descriptor.lexical_scope, Some(ScopeId(11)));
    }

    #[test]
    fn pause_observation_records_whether_interpreter_frames_are_available() {
        let observation = DebuggerInterpreterPauseObservation {
            reason: DebuggerPauseReason::DebuggerStatement,
            breakpoint_match: None,
            frames: vec![DebuggerInterpreterFrameObservation {
                frame: InterpreterFrameRecord {
                    frame: Some(CallFrameId(4)),
                    entry_frame: None,
                    kind: Some(InterpreterFrameKind::JavaScript),
                    code_block: None,
                    bytecode_index: None,
                    callee: None,
                    lexical_scope: None,
                },
                stack_frame: None,
                caller: None,
                source: None,
                position: DebuggerPosition { line: 1, column: 0 },
                this_object: None,
                is_tail_deleted: false,
            }],
            exception_is_uncaught: false,
            scope_state: DebuggerPausedScopeState::DispatchingObservers,
        };

        let pause = observation
            .to_pause_observation()
            .expect("pause observation");

        assert!(pause.has_call_frames);
        assert_eq!(pause.reason, DebuggerPauseReason::DebuggerStatement);
    }

    #[test]
    fn diagnostic_report_integrates_pause_frames_and_tier_fallback() {
        let observation = DebuggerInterpreterPauseObservation {
            reason: DebuggerPauseReason::DebuggerStatement,
            breakpoint_match: None,
            frames: vec![DebuggerInterpreterFrameObservation {
                frame: InterpreterFrameRecord {
                    frame: Some(CallFrameId(9)),
                    entry_frame: None,
                    kind: Some(InterpreterFrameKind::JavaScript),
                    code_block: Some(CodeBlockId(CellId(17))),
                    bytecode_index: None,
                    callee: None,
                    lexical_scope: None,
                },
                stack_frame: Some(StackFrameId(8)),
                caller: None,
                source: None,
                position: DebuggerPosition { line: 2, column: 0 },
                this_object: None,
                is_tail_deleted: false,
            }],
            exception_is_uncaught: false,
            scope_state: DebuggerPausedScopeState::DispatchingObservers,
        };
        let fallback = TierFallbackResultRecord {
            owner: CodeBlockId(CellId(17)),
            from_tier: crate::jit::JitType::Baseline,
            attempted_tier: crate::jit::JitType::Dfg,
            reason: crate::jit::TierFallbackReason::UnsupportedTier,
            target: crate::jit::TierFallbackTarget::ReturnToInterpreter,
            bytecode_index: Some(crate::bytecode::BytecodeIndex::from_offset(4)),
            resume: crate::jit::TierFallbackResumeKind::ContinueInInterpreter,
            preserves_profile: true,
            should_count_invalidation: true,
            clears_active_request: true,
        };

        let report = DebuggerDiagnosticReport::from_interpreter_pause(
            DebuggerPauseState::new(),
            &observation,
            Some(fallback),
            DEBUGGER_SCHEMA_REGISTRY,
        )
        .expect("diagnostic report");

        assert!(report.pause.should_pause);
        assert_eq!(report.call_frames.len(), 1);
        assert!(report.tier_fallback.is_some_and(
            |fallback| fallback.resumes_in_interpreter && fallback.bytecode_resume_visible
        ));
    }

    #[test]
    fn step_command_records_transition_without_resuming_execution() {
        let transition = DebuggerPauseState {
            reason: Some(DebuggerPauseReason::ExplicitPause),
            step_mode: StepMode::None,
            breakpoints_active: true,
            suppress_all_pauses: false,
            pause_on_all_exceptions: false,
            pause_on_uncaught_exceptions: false,
        }
        .apply_step_command(DebuggerStepCommand::StepInto)
        .expect("step transition");

        assert_eq!(transition.previous_step_mode, StepMode::None);
        assert_eq!(transition.next_step_mode, StepMode::Into);
        assert!(transition.clears_pause_reason);
    }
}
