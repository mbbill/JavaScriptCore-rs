//! C++ `Heap::runCurrentPhase` current-thread-state handshake.
//!
//! This module is descriptor-only: it mirrors the `NeedCurrentThreadState`
//! boundary used by C++ JSC's mutator fixpoint path without claiming to run the
//! full collector phase loop or `runFixpointPhase`.

#![allow(dead_code)]

use crate::gc::{
    machine_stack_marker::{
        JscCurrentThreadState, JscMachineStackMarker, JscMachineStackMarkerError,
        JscMachineStackRootingIngestError,
    },
    GcConductor, GcPhase, Heap,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::gc) enum HeapRunCurrentPhaseResult {
    Finished,
    Continue,
    NeedCurrentThreadState,
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::gc) enum HeapRunCurrentPhaseError {
    MachineStack(JscMachineStackRootingIngestError),
    UnsupportedConductor {
        actual: GcConductor,
    },
    ConductorMismatch {
        expected: GcConductor,
        actual: GcConductor,
    },
    RepeatedNeedCurrentThreadState,
}

impl From<JscMachineStackRootingIngestError> for HeapRunCurrentPhaseError {
    fn from(error: JscMachineStackRootingIngestError) -> Self {
        Self::MachineStack(error)
    }
}

impl From<JscMachineStackMarkerError> for HeapRunCurrentPhaseError {
    fn from(error: JscMachineStackMarkerError) -> Self {
        Self::MachineStack(JscMachineStackRootingIngestError::Marker(error))
    }
}

impl Heap {
    pub(in crate::gc) fn run_current_phase_descriptor<'state>(
        &mut self,
        conductor: GcConductor,
        marker: &JscMachineStackMarker,
        current_thread_state: Option<&'state JscCurrentThreadState<'state>>,
    ) -> Result<HeapRunCurrentPhaseResult, HeapRunCurrentPhaseError> {
        // C++ `runCurrentPhase` stores `currentThreadState` in
        // `m_currentThreadState`; Rust keeps it lexical through this descriptor
        // call so the machine-stack proof cannot outlive the marker closure.
        if !matches!(
            self.conductor,
            GcConductor::Mutator | GcConductor::Collector
        ) {
            return Err(HeapRunCurrentPhaseError::UnsupportedConductor {
                actual: self.conductor,
            });
        }
        if !matches!(conductor, GcConductor::Mutator | GcConductor::Collector) {
            return Err(HeapRunCurrentPhaseError::UnsupportedConductor { actual: conductor });
        }
        if self.conductor != conductor {
            return Err(HeapRunCurrentPhaseError::ConductorMismatch {
                expected: self.conductor,
                actual: conductor,
            });
        }

        match self.phase {
            GcPhase::NotRunning => Ok(HeapRunCurrentPhaseResult::Finished),
            GcPhase::Fixpoint
                if conductor == GcConductor::Mutator && current_thread_state.is_none() =>
            {
                Ok(HeapRunCurrentPhaseResult::NeedCurrentThreadState)
            }
            GcPhase::Fixpoint => {
                if conductor == GcConductor::Mutator {
                    if let Some(state) = current_thread_state {
                        self.ingest_current_thread_machine_stack_conservative_roots_from_state(
                            marker, state,
                        )?;
                    } else {
                        return Ok(HeapRunCurrentPhaseResult::NeedCurrentThreadState);
                    }
                }

                // C++ continues into `runFixpointPhase(conn)` here. Rust only
                // proves the current-thread-state handoff for now; real
                // fixpoint progress, SlotVisitor draining, and phase advance
                // remain separate JSC-mapped batches.
                Ok(HeapRunCurrentPhaseResult::Continue)
            }
            GcPhase::Begin | GcPhase::Concurrent | GcPhase::Reloop | GcPhase::End => {
                Ok(HeapRunCurrentPhaseResult::Continue)
            }
        }
    }

    pub(in crate::gc) fn collect_in_mutator_thread_current_phase_descriptor(
        &mut self,
        marker: &JscMachineStackMarker,
    ) -> Result<HeapRunCurrentPhaseResult, HeapRunCurrentPhaseError> {
        match self.run_current_phase_descriptor(GcConductor::Mutator, marker, None)? {
            HeapRunCurrentPhaseResult::NeedCurrentThreadState => {
                let result = marker.call_with_current_thread_state(|state| {
                    self.run_current_phase_descriptor(GcConductor::Mutator, marker, Some(state))
                })??;
                if result == HeapRunCurrentPhaseResult::NeedCurrentThreadState {
                    return Err(HeapRunCurrentPhaseError::RepeatedNeedCurrentThreadState);
                }
                Ok(result)
            }
            result => Ok(result),
        }
    }

    pub(in crate::gc) fn ingest_current_thread_machine_stack_conservative_roots_from_state<
        'state,
    >(
        &mut self,
        marker: &JscMachineStackMarker,
        current_thread_state: &'state JscCurrentThreadState<'state>,
    ) -> Result<(), JscMachineStackRootingIngestError> {
        let heap = self.id;
        let epoch = self.epoch;
        let state = self.state_descriptor();
        marker
            .with_current_thread_conservative_roots_from_state(
                heap,
                epoch,
                state,
                current_thread_state,
                |proof| self.ingest_machine_stack_conservative_roots(proof),
            )?
            .map_err(JscMachineStackRootingIngestError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{
        ConservativeRootSource, ConservativeRootSpan, GcPhase, Heap, MutatorState, RootPlanStep,
    };

    fn fixpoint_heap(conductor: GcConductor) -> Heap {
        let mut heap = Heap::new();
        heap.enter_phase(GcPhase::Fixpoint, MutatorState::Collecting, conductor);
        heap
    }

    fn stack_span() -> ConservativeRootSpan {
        ConservativeRootSpan {
            begin: 0x2000,
            end: 0x3000,
        }
    }

    fn span_for_words(words: &[usize]) -> ConservativeRootSpan {
        let begin = words.as_ptr() as usize;
        ConservativeRootSpan {
            begin,
            end: begin + core::mem::size_of_val(words),
        }
    }

    #[test]
    fn mutator_fixpoint_without_current_thread_state_requests_state() {
        let marker = JscMachineStackMarker::new();
        let mut heap = fixpoint_heap(GcConductor::Mutator);

        assert_eq!(
            heap.run_current_phase_descriptor(GcConductor::Mutator, &marker, None),
            Ok(HeapRunCurrentPhaseResult::NeedCurrentThreadState)
        );
    }

    #[test]
    fn collector_fixpoint_without_current_thread_state_does_not_request_state() {
        let marker = JscMachineStackMarker::new();
        let mut heap = fixpoint_heap(GcConductor::Collector);

        assert_eq!(
            heap.run_current_phase_descriptor(GcConductor::Collector, &marker, None),
            Ok(HeapRunCurrentPhaseResult::Continue)
        );
    }

    #[test]
    fn collector_fixpoint_with_state_does_not_ingest_current_thread_roots() {
        let marker = JscMachineStackMarker::new();
        let mut heap = fixpoint_heap(GcConductor::Collector);

        let result = marker.synthetic_current_thread_state_for_testing(stack_span(), |state| {
            heap.run_current_phase_descriptor(GcConductor::Collector, &marker, Some(state))
        });

        assert_eq!(result, Ok(HeapRunCurrentPhaseResult::Continue));
        assert_eq!(
            heap.root_marking_plan().planned_steps().expect("root plan"),
            Vec::new()
        );
    }

    #[test]
    fn conductor_mismatch_rejects_before_requesting_or_ingesting_state() {
        let marker = JscMachineStackMarker::new();
        let mut heap = fixpoint_heap(GcConductor::Collector);

        assert_eq!(
            heap.run_current_phase_descriptor(GcConductor::Mutator, &marker, None),
            Err(HeapRunCurrentPhaseError::ConductorMismatch {
                expected: GcConductor::Collector,
                actual: GcConductor::Mutator
            })
        );

        let result = marker.synthetic_current_thread_state_for_testing(stack_span(), |state| {
            heap.run_current_phase_descriptor(GcConductor::Mutator, &marker, Some(state))
        });

        assert_eq!(
            result,
            Err(HeapRunCurrentPhaseError::ConductorMismatch {
                expected: GcConductor::Collector,
                actual: GcConductor::Mutator
            })
        );
        assert_eq!(
            heap.root_marking_plan().planned_steps().expect("root plan"),
            Vec::new()
        );
    }

    #[test]
    fn rust_only_helper_conductor_is_not_a_run_current_phase_conductor() {
        let marker = JscMachineStackMarker::new();
        let mut helper_heap = fixpoint_heap(GcConductor::Helper);

        assert_eq!(
            helper_heap.run_current_phase_descriptor(GcConductor::Helper, &marker, None),
            Err(HeapRunCurrentPhaseError::UnsupportedConductor {
                actual: GcConductor::Helper
            })
        );

        let mut mutator_heap = fixpoint_heap(GcConductor::Mutator);
        assert_eq!(
            mutator_heap.run_current_phase_descriptor(GcConductor::Helper, &marker, None),
            Err(HeapRunCurrentPhaseError::UnsupportedConductor {
                actual: GcConductor::Helper
            })
        );
    }

    #[test]
    fn mutator_rerun_with_synthetic_state_ingests_registers_before_stack() {
        let marker = JscMachineStackMarker::new();
        let mut heap = fixpoint_heap(GcConductor::Mutator);
        let stack_words = [0usize; 2];
        let stack_span = span_for_words(&stack_words);

        let result = marker.synthetic_current_thread_state_for_testing(stack_span, |state| {
            heap.run_current_phase_descriptor(GcConductor::Mutator, &marker, Some(state))
        });

        assert_eq!(result, Ok(HeapRunCurrentPhaseResult::Continue));
        let steps = heap.root_marking_plan().planned_steps().expect("root plan");
        assert_eq!(steps.len(), 2);
        assert!(matches!(
            steps[0],
            RootPlanStep::Conservative {
                source: ConservativeRootSource::MachineStack,
                ..
            }
        ));
        assert_eq!(
            steps[1],
            RootPlanStep::Conservative {
                span: stack_span,
                source: ConservativeRootSource::MachineStack
            }
        );
    }

    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    #[test]
    fn mutator_collect_descriptor_reports_unsupported_capture() {
        let marker = JscMachineStackMarker::new();
        let mut heap = fixpoint_heap(GcConductor::Mutator);

        assert_eq!(
            heap.collect_in_mutator_thread_current_phase_descriptor(&marker),
            Err(HeapRunCurrentPhaseError::MachineStack(
                JscMachineStackRootingIngestError::Marker(
                    JscMachineStackMarkerError::CurrentThreadCaptureUnsupported {
                        target_os: std::env::consts::OS,
                        target_arch: std::env::consts::ARCH,
                    }
                )
            ))
        );
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn mutator_collect_descriptor_captures_and_ingests_registers_before_stack() {
        let marker = JscMachineStackMarker::new();
        let mut heap = fixpoint_heap(GcConductor::Mutator);

        assert_eq!(
            heap.collect_in_mutator_thread_current_phase_descriptor(&marker),
            Ok(HeapRunCurrentPhaseResult::Continue)
        );

        let steps = heap.root_marking_plan().planned_steps().expect("root plan");
        assert_eq!(steps.len(), 2);
        assert!(matches!(
            steps[0],
            RootPlanStep::Conservative {
                source: ConservativeRootSource::MachineStack,
                ..
            }
        ));
        assert!(matches!(
            steps[1],
            RootPlanStep::Conservative {
                source: ConservativeRootSource::MachineStack,
                ..
            }
        ));
    }
}
