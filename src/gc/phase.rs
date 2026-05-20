//! Collection phases and scoped proofs.

use core::marker::PhantomData;

/// Coarse collector phase.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GcPhase {
    #[default]
    NotRunning,
    Begin,
    Fixpoint,
    Concurrent,
    Reloop,
    End,
}

/// Collection strength requested by clients or heuristics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CollectionKind {
    #[default]
    Any,
    Eden,
    Full,
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
    /// The mutator is outside a heap slow path.
    #[default]
    Running,
    /// The mutator is in an allocation slow path.
    Allocating,
    /// The mutator owns sweep progress.
    Sweeping,
    /// The mutator owns collection progress.
    Collecting,
}

/// Current owner of collection progress.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GcConductor {
    #[default]
    Mutator,
    Collector,
    Helper,
}

/// Authority presented at a heap state boundary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeapMutationAuthority {
    /// Ordinary runtime code with mutator heap access.
    #[default]
    Mutator,
    /// Collector-owned marking or collection progress.
    Collector,
    /// Helper-thread collector work that cannot mutate runtime object fields.
    Helper,
    /// Sweeper-owned destruction and free-list publication.
    Sweeper,
    /// Weak processing owned by the collector end/fixpoint phases.
    WeakProcessor,
    /// Finalization owned by the collector end phase.
    Finalizer,
    /// VM or heap lifecycle code that may enqueue and configure collection.
    HeapLifecycle,
    /// Read-only diagnostics or descriptor inspection.
    Observer,
}

/// Descriptor-only heap operation whose runtime implementation is deferred.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeapSemanticOperation {
    Allocate { may_trigger_collection: bool },
    QueueCollection,
    MutatePublishedCell,
    InitializeUnpublishedCell,
    TraceRoots,
    ProcessWeak,
    RunFinalizers,
    Sweep,
    StopMutator,
    ResumeMutator,
    Observe,
}

/// Snapshot of heap phase facts used to validate semantic boundaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeapStateDescriptor {
    pub phase: GcPhase,
    pub mutator_state: MutatorState,
    pub conductor: GcConductor,
    pub no_gc_scope_depth: u32,
    pub mutator_has_heap_access: bool,
    pub mutator_should_be_fenced: bool,
}

impl Default for HeapStateDescriptor {
    fn default() -> Self {
        Self {
            phase: GcPhase::NotRunning,
            mutator_state: MutatorState::Running,
            conductor: GcConductor::Mutator,
            no_gc_scope_depth: 0,
            mutator_has_heap_access: true,
            mutator_should_be_fenced: false,
        }
    }
}

impl HeapStateDescriptor {
    pub const fn in_no_gc_scope(mut self, depth: u32) -> Self {
        self.no_gc_scope_depth = depth;
        self
    }

    pub const fn fenced(mut self, should_fence: bool) -> Self {
        self.mutator_should_be_fenced = should_fence;
        self
    }

    pub const fn without_mutator_heap_access(mut self) -> Self {
        self.mutator_has_heap_access = false;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeapSemanticGrant {
    pub operation: HeapSemanticOperation,
    pub authority: HeapMutationAuthority,
    pub phase: GcPhase,
    pub requires_world_suspension: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeapSemanticError {
    NoGcScopeActive(HeapSemanticOperation),
    MutatorLacksHeapAccess,
    MutatorFenceRequired,
    WrongAuthority {
        operation: HeapSemanticOperation,
        authority: HeapMutationAuthority,
    },
    WrongPhase {
        operation: HeapSemanticOperation,
        phase: GcPhase,
    },
    MutatorMustBeStopped {
        operation: HeapSemanticOperation,
        mutator_state: MutatorState,
    },
}

pub fn evaluate_heap_semantics(
    state: HeapStateDescriptor,
    authority: HeapMutationAuthority,
    operation: HeapSemanticOperation,
) -> Result<HeapSemanticGrant, HeapSemanticError> {
    if state.no_gc_scope_depth > 0
        && matches!(
            operation,
            HeapSemanticOperation::QueueCollection
                | HeapSemanticOperation::Allocate {
                    may_trigger_collection: true
                }
                | HeapSemanticOperation::ProcessWeak
                | HeapSemanticOperation::RunFinalizers
        )
    {
        return Err(HeapSemanticError::NoGcScopeActive(operation));
    }

    match operation {
        HeapSemanticOperation::Observe => {}
        HeapSemanticOperation::QueueCollection => {
            require_any_authority(
                operation,
                authority,
                &[
                    HeapMutationAuthority::Mutator,
                    HeapMutationAuthority::HeapLifecycle,
                ],
            )?;
        }
        HeapSemanticOperation::Allocate {
            may_trigger_collection: _,
        }
        | HeapSemanticOperation::InitializeUnpublishedCell => {
            require_authority(operation, authority, HeapMutationAuthority::Mutator)?;
            require_mutator_heap_access(state)?;
            if !matches!(
                state.phase,
                GcPhase::NotRunning | GcPhase::Concurrent | GcPhase::Reloop
            ) {
                return Err(HeapSemanticError::WrongPhase {
                    operation,
                    phase: state.phase,
                });
            }
        }
        HeapSemanticOperation::MutatePublishedCell => {
            require_authority(operation, authority, HeapMutationAuthority::Mutator)?;
            require_mutator_heap_access(state)?;
            if state.mutator_should_be_fenced {
                return Err(HeapSemanticError::MutatorFenceRequired);
            }
            if !matches!(
                state.phase,
                GcPhase::NotRunning | GcPhase::Concurrent | GcPhase::Reloop
            ) {
                return Err(HeapSemanticError::WrongPhase {
                    operation,
                    phase: state.phase,
                });
            }
        }
        HeapSemanticOperation::TraceRoots => {
            require_any_authority(
                operation,
                authority,
                &[
                    HeapMutationAuthority::Collector,
                    HeapMutationAuthority::Helper,
                ],
            )?;
            require_collection_phase(operation, state.phase)?;
        }
        HeapSemanticOperation::ProcessWeak => {
            require_any_authority(
                operation,
                authority,
                &[
                    HeapMutationAuthority::Collector,
                    HeapMutationAuthority::WeakProcessor,
                ],
            )?;
            if !matches!(state.phase, GcPhase::Fixpoint | GcPhase::End) {
                return Err(HeapSemanticError::WrongPhase {
                    operation,
                    phase: state.phase,
                });
            }
        }
        HeapSemanticOperation::RunFinalizers => {
            require_any_authority(
                operation,
                authority,
                &[
                    HeapMutationAuthority::Collector,
                    HeapMutationAuthority::Finalizer,
                ],
            )?;
            if state.phase != GcPhase::End {
                return Err(HeapSemanticError::WrongPhase {
                    operation,
                    phase: state.phase,
                });
            }
            require_stopped_mutator(operation, state.mutator_state)?;
        }
        HeapSemanticOperation::Sweep => {
            require_any_authority(
                operation,
                authority,
                &[
                    HeapMutationAuthority::Collector,
                    HeapMutationAuthority::Sweeper,
                ],
            )?;
            if !matches!(state.phase, GcPhase::End | GcPhase::NotRunning) {
                return Err(HeapSemanticError::WrongPhase {
                    operation,
                    phase: state.phase,
                });
            }
        }
        HeapSemanticOperation::StopMutator | HeapSemanticOperation::ResumeMutator => {
            require_any_authority(
                operation,
                authority,
                &[
                    HeapMutationAuthority::Collector,
                    HeapMutationAuthority::HeapLifecycle,
                ],
            )?;
        }
    }

    Ok(HeapSemanticGrant {
        operation,
        authority,
        phase: state.phase,
        requires_world_suspension: CollectionScope::new(state.phase).requires_world_suspension(),
    })
}

fn require_authority(
    operation: HeapSemanticOperation,
    actual: HeapMutationAuthority,
    expected: HeapMutationAuthority,
) -> Result<(), HeapSemanticError> {
    if actual == expected {
        Ok(())
    } else {
        Err(HeapSemanticError::WrongAuthority {
            operation,
            authority: actual,
        })
    }
}

fn require_any_authority(
    operation: HeapSemanticOperation,
    actual: HeapMutationAuthority,
    expected: &[HeapMutationAuthority],
) -> Result<(), HeapSemanticError> {
    if expected.contains(&actual) {
        Ok(())
    } else {
        Err(HeapSemanticError::WrongAuthority {
            operation,
            authority: actual,
        })
    }
}

fn require_collection_phase(
    operation: HeapSemanticOperation,
    phase: GcPhase,
) -> Result<(), HeapSemanticError> {
    if matches!(
        phase,
        GcPhase::Begin | GcPhase::Fixpoint | GcPhase::Concurrent | GcPhase::Reloop | GcPhase::End
    ) {
        Ok(())
    } else {
        Err(HeapSemanticError::WrongPhase { operation, phase })
    }
}

fn require_mutator_heap_access(state: HeapStateDescriptor) -> Result<(), HeapSemanticError> {
    if state.mutator_has_heap_access {
        Ok(())
    } else {
        Err(HeapSemanticError::MutatorLacksHeapAccess)
    }
}

fn require_stopped_mutator(
    operation: HeapSemanticOperation,
    mutator_state: MutatorState,
) -> Result<(), HeapSemanticError> {
    if mutator_state == MutatorState::Collecting {
        Ok(())
    } else {
        Err(HeapSemanticError::MutatorMustBeStopped {
            operation,
            mutator_state,
        })
    }
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

/// Optional completion hook associated with a queued GC request.
///
/// The callback body belongs to VM/embedder integration. This descriptor only
/// records that C++ `GCRequest::didFinishEndPhase` has end-phase authority.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct CollectionCompletionCallbackId(pub u64);

/// Queueable collection request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CollectionRequest {
    pub kind: CollectionKind,
    pub synchronousness: Synchronousness,
    pub trigger: CollectionTriggerKind,
    pub requested_bytes: usize,
    pub did_finish_end_phase: Option<CollectionCompletionCallbackId>,
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
/// The lifetime borrows heap collection authority; it does not own the heap or
/// any cell visited during the phase.
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
            GcPhase::Begin | GcPhase::Fixpoint | GcPhase::End
        )
    }
}

/// Proof that an operation cannot trigger collection.
///
/// This borrows the heap's no-GC precondition for a lexical lifetime. It keeps
/// allocation and collection authority out of value or runtime code.
#[derive(Debug)]
pub struct NoGcScope<'heap> {
    _heap: PhantomData<&'heap ()>,
}

impl<'heap> NoGcScope<'heap> {
    pub fn new() -> Self {
        Self { _heap: PhantomData }
    }

    pub fn contract(&self) -> NoGcScopeContract {
        NoGcScopeContract {
            depth: 1,
            collection_allowed: false,
            allocation_may_trigger_collection: false,
        }
    }
}

impl<'heap> Default for NoGcScope<'heap> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoGcScopeContract {
    pub depth: u32,
    pub collection_allowed: bool,
    pub allocation_may_trigger_collection: bool,
}

impl NoGcScopeContract {
    pub fn allows(self, operation: HeapSemanticOperation) -> bool {
        match operation {
            HeapSemanticOperation::QueueCollection => self.collection_allowed,
            HeapSemanticOperation::Allocate {
                may_trigger_collection,
            } => !may_trigger_collection || self.allocation_may_trigger_collection,
            HeapSemanticOperation::ProcessWeak | HeapSemanticOperation::RunFinalizers => false,
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_gc_scope_rejects_collection_triggering_allocation() {
        let state = HeapStateDescriptor::default().in_no_gc_scope(1);

        assert_eq!(
            evaluate_heap_semantics(
                state,
                HeapMutationAuthority::Mutator,
                HeapSemanticOperation::Allocate {
                    may_trigger_collection: true
                }
            ),
            Err(HeapSemanticError::NoGcScopeActive(
                HeapSemanticOperation::Allocate {
                    may_trigger_collection: true
                }
            ))
        );
    }

    #[test]
    fn no_gc_scope_allows_non_collecting_allocation_descriptor() {
        let state = HeapStateDescriptor::default().in_no_gc_scope(1);

        assert!(evaluate_heap_semantics(
            state,
            HeapMutationAuthority::Mutator,
            HeapSemanticOperation::Allocate {
                may_trigger_collection: false
            }
        )
        .is_ok());
    }

    #[test]
    fn no_gc_scope_rejects_weak_processing() {
        let state = HeapStateDescriptor {
            phase: GcPhase::Fixpoint,
            mutator_state: MutatorState::Collecting,
            conductor: GcConductor::Collector,
            no_gc_scope_depth: 1,
            mutator_has_heap_access: false,
            mutator_should_be_fenced: false,
        };

        assert_eq!(
            evaluate_heap_semantics(
                state,
                HeapMutationAuthority::WeakProcessor,
                HeapSemanticOperation::ProcessWeak,
            ),
            Err(HeapSemanticError::NoGcScopeActive(
                HeapSemanticOperation::ProcessWeak
            ))
        );
    }

    #[test]
    fn no_gc_scope_rejects_finalizer_processing() {
        let state = HeapStateDescriptor {
            phase: GcPhase::End,
            mutator_state: MutatorState::Collecting,
            conductor: GcConductor::Collector,
            no_gc_scope_depth: 1,
            mutator_has_heap_access: false,
            mutator_should_be_fenced: false,
        };

        assert_eq!(
            evaluate_heap_semantics(
                state,
                HeapMutationAuthority::Finalizer,
                HeapSemanticOperation::RunFinalizers,
            ),
            Err(HeapSemanticError::NoGcScopeActive(
                HeapSemanticOperation::RunFinalizers
            ))
        );
    }

    #[test]
    fn finalizers_require_end_phase_and_stopped_mutator() {
        let state = HeapStateDescriptor {
            phase: GcPhase::End,
            mutator_state: MutatorState::Collecting,
            conductor: GcConductor::Collector,
            no_gc_scope_depth: 0,
            mutator_has_heap_access: false,
            mutator_should_be_fenced: false,
        };

        assert!(evaluate_heap_semantics(
            state,
            HeapMutationAuthority::Finalizer,
            HeapSemanticOperation::RunFinalizers,
        )
        .is_ok());
    }

    #[test]
    fn fenced_mutator_cannot_mutate_published_cell() {
        let state = HeapStateDescriptor::default().fenced(true);

        assert_eq!(
            evaluate_heap_semantics(
                state,
                HeapMutationAuthority::Mutator,
                HeapSemanticOperation::MutatePublishedCell,
            ),
            Err(HeapSemanticError::MutatorFenceRequired)
        );
    }
}
