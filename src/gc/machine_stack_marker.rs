//! C++ `MachineStackMarker` and ARM64 `RegisterState` conservative-root boundary.

// This module is intentionally dormant until platform register/stack capture is
// wired in; tests exercise the C++-mapped proof boundary without production use.
#![allow(dead_code)]

use core::marker::PhantomData;

use crate::gc::{
    ConservativeRootSpan, ConservativeRoots, GcPhase, HeapEpoch, HeapId, HeapIntegrationError,
    HeapStateDescriptor, MutatorState,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub(crate) struct JscArm64RegisterState {
    pub x19: u64,
    pub x20: u64,
    pub x21: u64,
    pub x22: u64,
    pub x23: u64,
    pub x24: u64,
    pub x25: u64,
    pub x26: u64,
    pub x27: u64,
    pub x28: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::gc) struct JscCurrentThreadStateSnapshot {
    stack_origin: usize,
    stack_top: usize,
    register_state: JscArm64RegisterState,
}

impl JscCurrentThreadStateSnapshot {
    pub(in crate::gc) const fn new(
        stack_top: usize,
        stack_origin: usize,
        register_state: JscArm64RegisterState,
    ) -> Self {
        Self {
            stack_origin,
            stack_top,
            register_state,
        }
    }
}

#[derive(Debug)]
pub(in crate::gc) struct JscCurrentThreadState {
    stack_origin: usize,
    stack_top: usize,
    register_state: JscArm64RegisterState,
}

impl JscCurrentThreadState {
    pub(in crate::gc) fn stack_span(&self) -> ConservativeRootSpan {
        normalize_span(ConservativeRootSpan {
            begin: self.stack_top,
            end: self.stack_origin,
        })
    }

    pub(in crate::gc) fn register_state(&self) -> &JscArm64RegisterState {
        &self.register_state
    }

    pub(in crate::gc) fn register_state_span(&self) -> ConservativeRootSpan {
        let begin = self.register_state() as *const JscArm64RegisterState as usize;
        ConservativeRootSpan {
            begin,
            end: round_up_to_multiple(
                begin + core::mem::size_of::<JscArm64RegisterState>(),
                core::mem::size_of::<usize>(),
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JscMachineStackRootSpanKind {
    RegisterState,
    Stack,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JscMachineStackRootSpan {
    pub kind: JscMachineStackRootSpanKind,
    pub span: ConservativeRootSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JscMachineStackMarkerError {
    CollectionPhaseRequired {
        phase: GcPhase,
    },
    MutatorMustBeCollecting {
        mutator_state: MutatorState,
    },
    EmptySpan {
        kind: JscMachineStackRootSpanKind,
        span: ConservativeRootSpan,
    },
    UnalignedSpan {
        kind: JscMachineStackRootSpanKind,
        span: ConservativeRootSpan,
        pointer_width: usize,
    },
    UnexpectedCurrentThreadGatherOrder {
        observed: [Option<JscMachineStackRootSpanKind>; 2],
    },
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::gc) enum JscMachineStackRootingIngestError {
    Marker(JscMachineStackMarkerError),
    Heap(HeapIntegrationError),
}

impl From<JscMachineStackMarkerError> for JscMachineStackRootingIngestError {
    fn from(error: JscMachineStackMarkerError) -> Self {
        Self::Marker(error)
    }
}

impl From<HeapIntegrationError> for JscMachineStackRootingIngestError {
    fn from(error: HeapIntegrationError) -> Self {
        Self::Heap(error)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct JscMachineStackConservativeRootingProof<'state> {
    heap: HeapId,
    epoch: HeapEpoch,
    phase: GcPhase,
    mutator_state: MutatorState,
    roots: ConservativeRoots,
    spans: [JscMachineStackRootSpan; 2],
    _state: PhantomData<&'state JscCurrentThreadState>,
}

impl<'state> JscMachineStackConservativeRootingProof<'state> {
    pub(crate) fn heap(&self) -> HeapId {
        self.heap
    }

    pub(crate) fn epoch(&self) -> HeapEpoch {
        self.epoch
    }

    pub(crate) fn phase(&self) -> GcPhase {
        self.phase
    }

    pub(crate) fn mutator_state(&self) -> MutatorState {
        self.mutator_state
    }

    pub(crate) fn conservative_roots(&self) -> &ConservativeRoots {
        &self.roots
    }

    pub(crate) fn spans(&self) -> &[JscMachineStackRootSpan] {
        &self.spans
    }

    pub(crate) fn into_conservative_roots(self) -> ConservativeRoots {
        self.roots
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct JscMachineStackMarker {
    _private: (),
}

impl JscMachineStackMarker {
    pub(in crate::gc) const fn new() -> Self {
        Self { _private: () }
    }

    pub(in crate::gc) fn call_with_current_thread_state<R>(
        &self,
        snapshot: &JscCurrentThreadStateSnapshot,
        lambda: impl for<'state> FnOnce(&'state JscCurrentThreadState) -> R,
    ) -> R {
        // C++ `DECLARE_AND_COMPUTE_CURRENT_THREAD_STATE` captures a live stack
        // local plus real callee-save registers. This dormant Rust skeleton
        // still takes explicit test stack bounds and synthetic register values,
        // but the register span is derived from this lexical register storage,
        // not from caller-supplied addresses. Accepting boxed VM/call-frame
        // addresses here would not prove that native machine stack words are
        // conservatively visible to GC.
        let state = JscCurrentThreadState {
            stack_origin: snapshot.stack_origin,
            stack_top: snapshot.stack_top,
            register_state: snapshot.register_state,
        };
        lambda(&state)
    }

    pub(in crate::gc) fn with_current_thread_conservative_roots<R>(
        &self,
        heap: HeapId,
        epoch: HeapEpoch,
        heap_state: HeapStateDescriptor,
        snapshot: &JscCurrentThreadStateSnapshot,
        lambda: impl for<'state> FnOnce(JscMachineStackConservativeRootingProof<'state>) -> R,
    ) -> Result<R, JscMachineStackMarkerError> {
        self.call_with_current_thread_state(snapshot, |state| {
            self.gather_from_current_thread_state(heap, epoch, heap_state, state)
                .map(lambda)
        })
    }

    fn gather_from_current_thread_state<'state>(
        &self,
        heap: HeapId,
        epoch: HeapEpoch,
        heap_state: HeapStateDescriptor,
        state: &'state JscCurrentThreadState,
    ) -> Result<JscMachineStackConservativeRootingProof<'state>, JscMachineStackMarkerError> {
        validate_collection_state(heap_state)?;

        let register_span = validate_span(
            JscMachineStackRootSpanKind::RegisterState,
            state.register_state_span(),
        )?;
        let stack_span = validate_span(JscMachineStackRootSpanKind::Stack, state.stack_span())?;

        let spans = [
            JscMachineStackRootSpan {
                kind: JscMachineStackRootSpanKind::RegisterState,
                span: register_span,
            },
            JscMachineStackRootSpan {
                kind: JscMachineStackRootSpanKind::Stack,
                span: stack_span,
            },
        ];
        validate_current_thread_gather_order(&spans)?;

        let mut roots = ConservativeRoots::new();
        for span in spans {
            roots.add_span(span.span);
        }

        Ok(JscMachineStackConservativeRootingProof {
            heap,
            epoch,
            phase: heap_state.phase,
            mutator_state: heap_state.mutator_state,
            roots,
            spans,
            _state: PhantomData,
        })
    }
}

fn validate_collection_state(
    heap_state: HeapStateDescriptor,
) -> Result<(), JscMachineStackMarkerError> {
    if heap_state.phase == GcPhase::NotRunning {
        return Err(JscMachineStackMarkerError::CollectionPhaseRequired {
            phase: heap_state.phase,
        });
    }
    if heap_state.mutator_state != MutatorState::Collecting {
        return Err(JscMachineStackMarkerError::MutatorMustBeCollecting {
            mutator_state: heap_state.mutator_state,
        });
    }
    Ok(())
}

fn validate_span(
    kind: JscMachineStackRootSpanKind,
    span: ConservativeRootSpan,
) -> Result<ConservativeRootSpan, JscMachineStackMarkerError> {
    let span = normalize_span(span);
    if span.begin == span.end {
        return Err(JscMachineStackMarkerError::EmptySpan { kind, span });
    }

    let pointer_width = core::mem::size_of::<usize>();
    if span.begin % pointer_width != 0 || span.end % pointer_width != 0 {
        return Err(JscMachineStackMarkerError::UnalignedSpan {
            kind,
            span,
            pointer_width,
        });
    }

    Ok(span)
}

fn normalize_span(span: ConservativeRootSpan) -> ConservativeRootSpan {
    if span.begin <= span.end {
        span
    } else {
        ConservativeRootSpan {
            begin: span.end,
            end: span.begin,
        }
    }
}

fn round_up_to_multiple(value: usize, multiple: usize) -> usize {
    debug_assert!(multiple.is_power_of_two());
    (value + multiple - 1) & !(multiple - 1)
}

fn validate_current_thread_gather_order(
    spans: &[JscMachineStackRootSpan; 2],
) -> Result<(), JscMachineStackMarkerError> {
    if spans[0].kind == JscMachineStackRootSpanKind::RegisterState
        && spans[1].kind == JscMachineStackRootSpanKind::Stack
    {
        Ok(())
    } else {
        Err(
            JscMachineStackMarkerError::UnexpectedCurrentThreadGatherOrder {
                observed: [Some(spans[0].kind), Some(spans[1].kind)],
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{
        GcConductor, Heap, HeapEpoch, HeapId, HeapIntegrationError, HeapSemanticError,
        HeapSemanticOperation, RootPlanStep,
    };

    fn forged_register_span() -> ConservativeRootSpan {
        ConservativeRootSpan {
            begin: 0x1000,
            end: 0x1050,
        }
    }

    fn stack_span() -> ConservativeRootSpan {
        ConservativeRootSpan {
            begin: 0x2000,
            end: 0x3000,
        }
    }

    fn snapshot(stack_span: ConservativeRootSpan) -> JscCurrentThreadStateSnapshot {
        JscCurrentThreadStateSnapshot::new(
            stack_span.begin,
            stack_span.end,
            JscArm64RegisterState::default(),
        )
    }

    fn collecting_heap() -> Heap {
        let mut heap = Heap::new();
        heap.enter_phase(
            GcPhase::Fixpoint,
            MutatorState::Collecting,
            GcConductor::Collector,
        );
        heap
    }

    fn forged_proof(
        heap: HeapId,
        epoch: HeapEpoch,
        phase: GcPhase,
        mutator_state: MutatorState,
    ) -> JscMachineStackConservativeRootingProof<'static> {
        let spans = [
            JscMachineStackRootSpan {
                kind: JscMachineStackRootSpanKind::RegisterState,
                span: forged_register_span(),
            },
            JscMachineStackRootSpan {
                kind: JscMachineStackRootSpanKind::Stack,
                span: stack_span(),
            },
        ];
        let mut roots = ConservativeRoots::new();
        for span in spans {
            roots.add_span(span.span);
        }
        JscMachineStackConservativeRootingProof {
            heap,
            epoch,
            phase,
            mutator_state,
            roots,
            spans,
            _state: PhantomData,
        }
    }

    #[test]
    fn rejects_not_running_gc_phase() {
        let marker = JscMachineStackMarker::new();
        let heap = Heap::new();

        assert_eq!(
            marker.with_current_thread_conservative_roots(
                heap.id(),
                heap.epoch(),
                heap.state_descriptor(),
                &snapshot(stack_span()),
                |_| ()
            ),
            Err(JscMachineStackMarkerError::CollectionPhaseRequired {
                phase: GcPhase::NotRunning
            })
        );
    }

    #[test]
    fn heap_current_thread_ingest_rejects_not_running_gc_phase() {
        let marker = JscMachineStackMarker::new();
        let mut heap = Heap::new();

        assert_eq!(
            heap.ingest_current_thread_machine_stack_conservative_roots(
                &marker,
                &snapshot(stack_span())
            ),
            Err(JscMachineStackRootingIngestError::Marker(
                JscMachineStackMarkerError::CollectionPhaseRequired {
                    phase: GcPhase::NotRunning
                }
            ))
        );
    }

    #[test]
    fn rejects_running_mutator_state() {
        let marker = JscMachineStackMarker::new();
        let mut heap = Heap::new();
        heap.enter_phase(
            GcPhase::Fixpoint,
            MutatorState::Running,
            GcConductor::Collector,
        );

        assert_eq!(
            marker.with_current_thread_conservative_roots(
                heap.id(),
                heap.epoch(),
                heap.state_descriptor(),
                &snapshot(stack_span()),
                |_| ()
            ),
            Err(JscMachineStackMarkerError::MutatorMustBeCollecting {
                mutator_state: MutatorState::Running
            })
        );
    }

    #[test]
    fn heap_current_thread_ingest_rejects_running_mutator_state() {
        let marker = JscMachineStackMarker::new();
        let mut heap = Heap::new();
        heap.enter_phase(
            GcPhase::Fixpoint,
            MutatorState::Running,
            GcConductor::Collector,
        );

        assert_eq!(
            heap.ingest_current_thread_machine_stack_conservative_roots(
                &marker,
                &snapshot(stack_span())
            ),
            Err(JscMachineStackRootingIngestError::Marker(
                JscMachineStackMarkerError::MutatorMustBeCollecting {
                    mutator_state: MutatorState::Running
                }
            ))
        );
    }

    #[test]
    fn rejects_empty_stack_span() {
        let marker = JscMachineStackMarker::new();
        let heap = collecting_heap();
        let empty_stack = ConservativeRootSpan {
            begin: 0x2000,
            end: 0x2000,
        };

        assert_eq!(
            marker.with_current_thread_conservative_roots(
                heap.id(),
                heap.epoch(),
                heap.state_descriptor(),
                &snapshot(empty_stack),
                |_| ()
            ),
            Err(JscMachineStackMarkerError::EmptySpan {
                kind: JscMachineStackRootSpanKind::Stack,
                span: empty_stack
            })
        );
    }

    #[test]
    fn heap_current_thread_ingest_rejects_empty_stack_span() {
        let marker = JscMachineStackMarker::new();
        let mut heap = collecting_heap();
        let empty_stack = ConservativeRootSpan {
            begin: 0x2000,
            end: 0x2000,
        };

        assert_eq!(
            heap.ingest_current_thread_machine_stack_conservative_roots(
                &marker,
                &snapshot(empty_stack)
            ),
            Err(JscMachineStackRootingIngestError::Marker(
                JscMachineStackMarkerError::EmptySpan {
                    kind: JscMachineStackRootSpanKind::Stack,
                    span: empty_stack
                }
            ))
        );
    }

    #[test]
    fn rejects_unaligned_stack_span() {
        let marker = JscMachineStackMarker::new();
        let heap = collecting_heap();
        let unaligned_stack = ConservativeRootSpan {
            begin: 0x2000,
            end: 0x3001,
        };

        assert_eq!(
            marker.with_current_thread_conservative_roots(
                heap.id(),
                heap.epoch(),
                heap.state_descriptor(),
                &snapshot(unaligned_stack),
                |_| ()
            ),
            Err(JscMachineStackMarkerError::UnalignedSpan {
                kind: JscMachineStackRootSpanKind::Stack,
                span: unaligned_stack,
                pointer_width: core::mem::size_of::<usize>()
            })
        );
    }

    #[test]
    fn heap_current_thread_ingest_rejects_unaligned_stack_span() {
        let marker = JscMachineStackMarker::new();
        let mut heap = collecting_heap();
        let unaligned_stack = ConservativeRootSpan {
            begin: 0x2000,
            end: 0x3001,
        };

        assert_eq!(
            heap.ingest_current_thread_machine_stack_conservative_roots(
                &marker,
                &snapshot(unaligned_stack)
            ),
            Err(JscMachineStackRootingIngestError::Marker(
                JscMachineStackMarkerError::UnalignedSpan {
                    kind: JscMachineStackRootSpanKind::Stack,
                    span: unaligned_stack,
                    pointer_width: core::mem::size_of::<usize>()
                }
            ))
        );
    }

    #[test]
    fn records_register_span_before_stack_span() {
        let marker = JscMachineStackMarker::new();
        let heap = collecting_heap();
        let (spans, root_spans) = marker
            .with_current_thread_conservative_roots(
                heap.id(),
                heap.epoch(),
                heap.state_descriptor(),
                &snapshot(stack_span()),
                |proof| {
                    (
                        [proof.spans()[0], proof.spans()[1]],
                        proof.conservative_roots().spans().to_vec(),
                    )
                },
            )
            .expect("machine-stack proof");

        let register_span = spans[0].span;
        assert_eq!(spans[0].kind, JscMachineStackRootSpanKind::RegisterState);
        assert_eq!(spans[1].kind, JscMachineStackRootSpanKind::Stack);
        assert_eq!(spans[1].span, stack_span());
        assert_eq!(
            register_span.end - register_span.begin,
            core::mem::size_of::<JscArm64RegisterState>()
        );
        assert_eq!(register_span.begin % core::mem::size_of::<usize>(), 0);
        assert_eq!(root_spans, vec![register_span, stack_span()]);
    }

    #[test]
    fn register_span_is_derived_from_lexical_register_storage() {
        let marker = JscMachineStackMarker::new();
        let observed = marker.call_with_current_thread_state(&snapshot(stack_span()), |state| {
            let register_address = state.register_state() as *const JscArm64RegisterState as usize;
            let span = state.register_state_span();
            (register_address, span)
        });

        assert_eq!(observed.1.begin, observed.0);
        assert_eq!(
            observed.1.end,
            round_up_to_multiple(
                observed.0 + core::mem::size_of::<JscArm64RegisterState>(),
                core::mem::size_of::<usize>(),
            )
        );
    }

    #[test]
    fn heap_ingests_current_thread_roots_inside_marker_closure() {
        let marker = JscMachineStackMarker::new();
        let mut heap = collecting_heap();

        heap.ingest_current_thread_machine_stack_conservative_roots(
            &marker,
            &snapshot(stack_span()),
        )
        .expect("proof-minted ingest");

        let steps = heap.root_marking_plan().planned_steps().expect("root plan");
        assert_eq!(steps.len(), 2);
        let RootPlanStep::Conservative {
            span: register_span,
            source: crate::gc::ConservativeRootSource::MachineStack,
        } = steps[0]
        else {
            panic!("register span should be a conservative root step");
        };
        assert_eq!(
            register_span.end - register_span.begin,
            core::mem::size_of::<JscArm64RegisterState>()
        );
        assert_eq!(
            steps[1],
            RootPlanStep::Conservative {
                span: stack_span(),
                source: crate::gc::ConservativeRootSource::MachineStack
            }
        );
    }

    #[test]
    fn ingest_rejects_proof_after_heap_leaves_collection_phase() {
        let mut heap = collecting_heap();
        let proof = forged_proof(
            heap.id(),
            heap.epoch(),
            heap.phase(),
            MutatorState::Collecting,
        );
        heap.leave_phase();

        assert_eq!(
            heap.ingest_machine_stack_conservative_roots(proof),
            Err(HeapIntegrationError::HeapSemantic(
                HeapSemanticError::WrongPhase {
                    operation: HeapSemanticOperation::TraceRoots,
                    phase: GcPhase::NotRunning
                }
            ))
        );
    }

    #[test]
    fn ingest_rejects_proof_after_heap_collection_state_changes() {
        let mut heap = collecting_heap();
        let proof = forged_proof(
            heap.id(),
            heap.epoch(),
            heap.phase(),
            MutatorState::Collecting,
        );
        heap.enter_phase(
            GcPhase::End,
            MutatorState::Collecting,
            GcConductor::Collector,
        );

        assert_eq!(
            heap.ingest_machine_stack_conservative_roots(proof),
            Err(
                HeapIntegrationError::MachineStackConservativeRootingProofStateMismatch {
                    expected_phase: GcPhase::End,
                    actual_phase: GcPhase::Fixpoint,
                    expected_mutator_state: MutatorState::Collecting,
                    actual_mutator_state: MutatorState::Collecting
                }
            )
        );
    }

    #[test]
    fn ingest_rejects_wrong_heap_and_epoch_proofs() {
        let mut heap = collecting_heap();

        assert_eq!(
            heap.ingest_machine_stack_conservative_roots(forged_proof(
                HeapId(99),
                heap.epoch(),
                heap.phase(),
                MutatorState::Collecting,
            )),
            Err(HeapIntegrationError::HeapMismatch {
                expected: heap.id(),
                actual: HeapId(99)
            })
        );

        assert_eq!(
            heap.ingest_machine_stack_conservative_roots(forged_proof(
                heap.id(),
                HeapEpoch(99),
                heap.phase(),
                MutatorState::Collecting,
            )),
            Err(
                HeapIntegrationError::StaleMachineStackConservativeRootingProof {
                    expected: heap.epoch(),
                    actual: HeapEpoch(99)
                }
            )
        );
    }
}
