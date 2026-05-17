//! Collection phases and scoped proofs.

use core::marker::PhantomData;

/// Coarse collector phase.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GcPhase {
    #[default]
    Idle,
    Begin,
    Allocating,
    Fixpoint,
    Marking,
    Concurrent,
    Reloop,
    Sweeping,
    WeakProcessing,
    Finalizing,
    End,
}

/// Collection strength requested by clients or heuristics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CollectionKind {
    Eden,
    Full,
    #[default]
    Any,
}

/// Whether a collection waits for completion.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Synchronousness {
    #[default]
    Async,
    Sync,
}

/// Mutator state relative to the collector.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MutatorState {
    #[default]
    Running,
    Stopping,
    Stopped,
    Resuming,
}

/// Current owner of collection progress.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GcConductor {
    #[default]
    Mutator,
    Collector,
    Helper,
}

/// Reason a collection was requested.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CollectionTriggerKind {
    #[default]
    Allocation,
    ExtraMemory,
    ExternalMemory,
    Opportunistic,
    Timer,
    API,
    Shutdown,
}

/// Queueable collection request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CollectionRequest {
    pub kind: CollectionKind,
    pub synchronousness: Synchronousness,
    pub trigger: CollectionTriggerKind,
    pub requested_bytes: usize,
}

impl CollectionRequest {
    pub fn subsumes(self, other: Self) -> bool {
        self.kind.subsumes(other.kind)
    }
}

impl CollectionKind {
    pub fn subsumes(self, other: Self) -> bool {
        matches!(
            (self, other),
            (CollectionKind::Full, _)
                | (CollectionKind::Any, CollectionKind::Any)
                | (CollectionKind::Any, CollectionKind::Eden)
                | (CollectionKind::Eden, CollectionKind::Eden)
        )
    }
}

/// Scheduling policy for incremental and concurrent collector progress.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MutatorSchedulerPolicy {
    #[default]
    SynchronousStopTheWorld,
    SpaceTime,
    StochasticSpaceTime,
}

/// Timer/activity callback family.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GcActivityKind {
    Eden,
    #[default]
    Full,
}

/// State tracked by Eden and full GC activity callbacks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GcActivityCallbackState {
    pub kind: GcActivityKind,
    pub synchronousness: Synchronousness,
    pub enabled: bool,
    pub did_gc_recently: bool,
    pub delay_micros: u64,
}

/// Decision returned by scheduling heuristics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GcScheduleDecision {
    pub request: Option<CollectionRequest>,
    pub should_stop_mutator: bool,
    pub byte_budget: usize,
}

/// Active collection scope.
///
/// Mutation that is only valid during collection should require this proof
/// rather than checking global phase state opportunistically.
#[derive(Debug)]
pub struct CollectionScope<'heap> {
    phase: GcPhase,
    _heap: PhantomData<&'heap mut ()>,
}

impl<'heap> CollectionScope<'heap> {
    pub fn new(phase: GcPhase) -> Self {
        Self {
            phase,
            _heap: PhantomData,
        }
    }

    pub fn phase(&self) -> GcPhase {
        self.phase
    }

    pub fn requires_world_suspension(&self) -> bool {
        matches!(
            self.phase,
            GcPhase::Begin
                | GcPhase::Fixpoint
                | GcPhase::Sweeping
                | GcPhase::WeakProcessing
                | GcPhase::Finalizing
                | GcPhase::End
        )
    }
}

/// Proof that an operation cannot trigger collection.
#[derive(Debug)]
pub struct NoGcScope<'heap> {
    _heap: PhantomData<&'heap ()>,
}

impl<'heap> NoGcScope<'heap> {
    pub fn new() -> Self {
        Self { _heap: PhantomData }
    }
}

impl<'heap> Default for NoGcScope<'heap> {
    fn default() -> Self {
        Self::new()
    }
}
