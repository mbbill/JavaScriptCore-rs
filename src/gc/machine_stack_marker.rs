//! C++ `MachineStackMarker` and ARM64 `RegisterState` conservative-root boundary.

// This module is intentionally dormant until platform register/stack capture is
// wired in; tests exercise the C++-mapped proof boundary without production use.
#![allow(dead_code)]

use core::marker::PhantomData;

use crate::{
    gc::{
        ConservativeRootSpan, ConservativeRoots, GcPhase, HeapEpoch, HeapId, HeapIntegrationError,
        HeapStateDescriptor, MutatorState,
    },
    wtf::{WtfStackBounds, WtfStackBoundsError},
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

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
macro_rules! capture_current_macos_aarch64_register_state {
    ($register_state:expr) => {{
        let register_state_ptr = &mut $register_state as *mut JscArm64RegisterState;

        // SAFETY: `register_state_ptr` points to writable stack-local
        // `JscArm64RegisterState` storage in the current-thread capture function.
        // This macro expands at the capture site, matching C++ JSC's
        // `ALLOCATE_AND_GET_REGISTER_STATE`: no Rust helper-call prologue can
        // move caller callee-saves out of the scanned stack span before the asm
        // stores x19..x28. The asm uses caller-scratch x9 as the base register
        // and intentionally does not claim `nomem` because it writes through
        // `register_state_ptr`.
        unsafe {
            core::arch::asm!(
                "str x19, [x9, #0]",
                "str x20, [x9, #8]",
                "str x21, [x9, #16]",
                "str x22, [x9, #24]",
                "str x23, [x9, #32]",
                "str x24, [x9, #40]",
                "str x25, [x9, #48]",
                "str x26, [x9, #56]",
                "str x27, [x9, #64]",
                "str x28, [x9, #72]",
                in("x9") register_state_ptr,
                options(preserves_flags),
            );
        }
    }};
}

#[derive(Debug)]
pub(in crate::gc) struct JscCurrentThreadState<'registers> {
    stack_origin: usize,
    stack_top: usize,
    register_state: Option<&'registers JscArm64RegisterState>,
}

impl<'registers> JscCurrentThreadState<'registers> {
    pub(in crate::gc) fn stack_origin_address(&self) -> usize {
        self.stack_origin
    }

    pub(in crate::gc) fn stack_top_address(&self) -> usize {
        self.stack_top
    }

    pub(in crate::gc) fn stack_span(&self) -> ConservativeRootSpan {
        normalize_span(ConservativeRootSpan {
            begin: self.stack_top,
            end: self.stack_origin,
        })
    }

    pub(in crate::gc) fn register_state(&self) -> Option<&JscArm64RegisterState> {
        self.register_state
    }

    pub(in crate::gc) fn register_state_span(&self) -> Option<ConservativeRootSpan> {
        let register_state = self.register_state()?;
        let begin = register_state as *const JscArm64RegisterState as usize;
        Some(ConservativeRootSpan {
            begin,
            end: round_up_to_multiple(
                begin + core::mem::size_of::<JscArm64RegisterState>(),
                core::mem::size_of::<usize>(),
            ),
        })
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
    CurrentThreadCaptureUnsupported {
        target_os: &'static str,
        target_arch: &'static str,
    },
    StackBounds(WtfStackBoundsError),
    StackTopOutsideBounds {
        stack_top: usize,
        stack_origin: usize,
        stack_bound: usize,
    },
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
    MissingRegisterState,
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

impl From<WtfStackBoundsError> for JscMachineStackMarkerError {
    fn from(error: WtfStackBoundsError) -> Self {
        Self::StackBounds(error)
    }
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
    _state: PhantomData<&'state JscCurrentThreadState<'state>>,
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
        lambda: impl for<'state> FnOnce(&'state JscCurrentThreadState<'state>) -> R,
    ) -> Result<R, JscMachineStackMarkerError> {
        self.capture_current_thread_state(lambda)
    }

    pub(in crate::gc) fn with_current_thread_conservative_roots<R>(
        &self,
        heap: HeapId,
        epoch: HeapEpoch,
        heap_state: HeapStateDescriptor,
        lambda: impl for<'state> FnOnce(JscMachineStackConservativeRootingProof<'state>) -> R,
    ) -> Result<R, JscMachineStackMarkerError> {
        validate_collection_state(heap_state)?;
        self.call_with_current_thread_state(|state| {
            self.with_current_thread_conservative_roots_from_state(
                heap, epoch, heap_state, state, lambda,
            )
        })?
    }

    pub(in crate::gc) fn with_current_thread_conservative_roots_from_state<'state, R>(
        &self,
        heap: HeapId,
        epoch: HeapEpoch,
        heap_state: HeapStateDescriptor,
        state: &'state JscCurrentThreadState<'state>,
        lambda: impl FnOnce(JscMachineStackConservativeRootingProof<'state>) -> R,
    ) -> Result<R, JscMachineStackMarkerError> {
        validate_collection_state(heap_state)?;
        self.gather_from_current_thread_state(heap, epoch, heap_state, state)
            .map(lambda)
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn capture_current_thread_state<R>(
        &self,
        lambda: impl for<'state> FnOnce(&'state JscCurrentThreadState<'state>) -> R,
    ) -> Result<R, JscMachineStackMarkerError> {
        let stack_bounds = WtfStackBounds::current_thread_stack_bounds()?;
        let mut state = JscCurrentThreadState {
            stack_origin: stack_bounds.origin_address(),
            stack_top: 0,
            register_state: None,
        };

        // C++ `DECLARE_AND_COMPUTE_CURRENT_THREAD_STATE` uses the address of the
        // `CurrentThreadState` stack local as `stackTop`, then records Darwin
        // stack origin and expands `ALLOCATE_AND_GET_REGISTER_STATE` in place.
        // Rust keeps that structure: `register_state` is separate lexical
        // storage, `state` points at it, and the proof borrows `state` inside the
        // marker closure. C++ applies red-zone adjustment only when copying
        // suspended other-thread stacks, not in current-thread gather. Boxed
        // VM/call-frame storage is still not machine-stack evidence and cannot
        // enter this production path.
        state.stack_top = &state as *const _ as usize;
        if !stack_bounds.contains_address(state.stack_top) {
            return Err(JscMachineStackMarkerError::StackTopOutsideBounds {
                stack_top: state.stack_top,
                stack_origin: stack_bounds.origin_address(),
                stack_bound: stack_bounds.bound_address(),
            });
        }
        let mut register_state = JscArm64RegisterState::default();
        capture_current_macos_aarch64_register_state!(register_state);
        state.register_state = Some(&register_state);

        Ok(lambda(&state))
    }

    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    fn capture_current_thread_state<R>(
        &self,
        _lambda: impl for<'state> FnOnce(&'state JscCurrentThreadState<'state>) -> R,
    ) -> Result<R, JscMachineStackMarkerError> {
        Err(
            JscMachineStackMarkerError::CurrentThreadCaptureUnsupported {
                target_os: std::env::consts::OS,
                target_arch: std::env::consts::ARCH,
            },
        )
    }

    #[cfg(test)]
    pub(in crate::gc) fn synthetic_current_thread_state_for_testing<R>(
        &self,
        stack_span: ConservativeRootSpan,
        lambda: impl for<'state> FnOnce(&'state JscCurrentThreadState<'state>) -> R,
    ) -> R {
        let register_state = JscArm64RegisterState::default();
        let state = JscCurrentThreadState {
            stack_origin: stack_span.end,
            stack_top: stack_span.begin,
            register_state: Some(&register_state),
        };
        lambda(&state)
    }

    #[cfg(test)]
    fn with_synthetic_current_thread_conservative_roots_for_testing<R>(
        &self,
        heap: HeapId,
        epoch: HeapEpoch,
        heap_state: HeapStateDescriptor,
        stack_span: ConservativeRootSpan,
        lambda: impl for<'state> FnOnce(JscMachineStackConservativeRootingProof<'state>) -> R,
    ) -> Result<R, JscMachineStackMarkerError> {
        validate_collection_state(heap_state)?;
        self.synthetic_current_thread_state_for_testing(stack_span, |state| {
            self.with_current_thread_conservative_roots_from_state(
                heap, epoch, heap_state, state, lambda,
            )
        })
    }

    fn gather_from_current_thread_state<'state>(
        &self,
        heap: HeapId,
        epoch: HeapEpoch,
        heap_state: HeapStateDescriptor,
        state: &'state JscCurrentThreadState<'state>,
    ) -> Result<JscMachineStackConservativeRootingProof<'state>, JscMachineStackMarkerError> {
        validate_collection_state(heap_state)?;

        let register_state_span = state
            .register_state_span()
            .ok_or(JscMachineStackMarkerError::MissingRegisterState)?;
        let register_span = validate_span(
            JscMachineStackRootSpanKind::RegisterState,
            register_state_span,
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
        AllocationMode, CellId, CellMetadata, ConservativeRootCell, GcConductor, Heap,
        HeapAllocationRequest, HeapEpoch, HeapId, HeapIntegrationError, HeapSemanticError,
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

    fn span_for_words(words: &[usize]) -> ConservativeRootSpan {
        let begin = words.as_ptr() as usize;
        ConservativeRootSpan {
            begin,
            end: begin + core::mem::size_of_val(words),
        }
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

    fn allocate_test_cell(heap: &mut Heap) -> CellId {
        heap.allocate_record(HeapAllocationRequest {
            heap: heap.id(),
            subspace: "object",
            metadata: CellMetadata::default(),
            byte_size: 64,
            mode: AllocationMode::Normal,
            may_trigger_collection: false,
        })
        .map(|response| response.cell)
        .expect("test allocation")
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
            heap.ingest_current_thread_machine_stack_conservative_roots(&marker),
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
            heap.ingest_current_thread_machine_stack_conservative_roots(&marker),
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
            marker.with_synthetic_current_thread_conservative_roots_for_testing(
                heap.id(),
                heap.epoch(),
                heap.state_descriptor(),
                empty_stack,
                |_| ()
            ),
            Err(JscMachineStackMarkerError::EmptySpan {
                kind: JscMachineStackRootSpanKind::Stack,
                span: empty_stack
            })
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
            marker.with_synthetic_current_thread_conservative_roots_for_testing(
                heap.id(),
                heap.epoch(),
                heap.state_descriptor(),
                unaligned_stack,
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
    fn synthetic_gather_records_register_span_before_stack_span_for_testing() {
        let marker = JscMachineStackMarker::new();
        let heap = collecting_heap();
        let (spans, root_spans) = marker
            .with_synthetic_current_thread_conservative_roots_for_testing(
                heap.id(),
                heap.epoch(),
                heap.state_descriptor(),
                stack_span(),
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
    fn synthetic_register_span_is_derived_from_lexical_register_storage_for_testing() {
        let marker = JscMachineStackMarker::new();
        let observed = marker.synthetic_current_thread_state_for_testing(stack_span(), |state| {
            let state_begin = state as *const _ as usize;
            let state_end = state_begin + core::mem::size_of_val(state);
            let register_address = state.register_state().expect("register state")
                as *const JscArm64RegisterState as usize;
            let span = state.register_state_span().expect("register state span");
            (state_begin, state_end, register_address, span)
        });

        assert!(observed.2 < observed.0 || observed.2 >= observed.1);
        assert_eq!(observed.3.begin, observed.2);
        assert_eq!(
            observed.3.end,
            round_up_to_multiple(
                observed.2 + core::mem::size_of::<JscArm64RegisterState>(),
                core::mem::size_of::<usize>(),
            )
        );
    }

    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    #[test]
    fn current_thread_capture_reports_unsupported_on_other_targets() {
        let marker = JscMachineStackMarker::new();

        assert_eq!(
            marker.call_with_current_thread_state(|_| ()),
            Err(
                JscMachineStackMarkerError::CurrentThreadCaptureUnsupported {
                    target_os: std::env::consts::OS,
                    target_arch: std::env::consts::ARCH,
                }
            )
        );
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn captured_stack_top_is_inside_wtf_stack_bounds() {
        let marker = JscMachineStackMarker::new();
        let (bounds, stack_top, stack_span) = marker
            .call_with_current_thread_state(|state| {
                (
                    WtfStackBounds::current_thread_stack_bounds().expect("stack bounds"),
                    state.stack_top_address(),
                    state.stack_span(),
                )
            })
            .expect("current-thread state");

        assert!(bounds.contains_address(stack_top));
        assert_eq!(stack_span.end, bounds.origin_address());
        assert!(stack_span.begin < stack_span.end);
        assert_eq!(stack_span.begin % core::mem::size_of::<usize>(), 0);
        assert_eq!(stack_span.end % core::mem::size_of::<usize>(), 0);
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn captured_register_span_is_lexical_and_pointer_sized() {
        let marker = JscMachineStackMarker::new();
        let (state_begin, state_end, register_address, register_span) = marker
            .call_with_current_thread_state(|state| {
                let state_begin = state as *const _ as usize;
                (
                    state_begin,
                    state_begin + core::mem::size_of_val(state),
                    state.register_state().expect("register state") as *const JscArm64RegisterState
                        as usize,
                    state.register_state_span().expect("register state span"),
                )
            })
            .expect("current-thread state");

        assert!(register_address < state_begin || register_address >= state_end);
        assert_eq!(register_span.begin, register_address);
        assert_eq!(
            register_span.end - register_span.begin,
            core::mem::size_of::<JscArm64RegisterState>()
        );
        assert_eq!(register_span.begin % core::mem::size_of::<usize>(), 0);
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn captured_current_thread_gather_records_register_span_before_stack_span() {
        let marker = JscMachineStackMarker::new();
        let heap = collecting_heap();
        let (spans, root_spans) = marker
            .with_current_thread_conservative_roots(
                heap.id(),
                heap.epoch(),
                heap.state_descriptor(),
                |proof| {
                    (
                        [proof.spans()[0], proof.spans()[1]],
                        proof.conservative_roots().spans().to_vec(),
                    )
                },
            )
            .expect("machine-stack proof");

        assert_eq!(spans[0].kind, JscMachineStackRootSpanKind::RegisterState);
        assert_eq!(spans[1].kind, JscMachineStackRootSpanKind::Stack);
        assert_eq!(root_spans, vec![spans[0].span, spans[1].span]);
    }

    #[test]
    fn heap_ingests_synthetic_roots_inside_marker_closure_for_testing() {
        let marker = JscMachineStackMarker::new();
        let mut heap = collecting_heap();
        let stack_words = [0usize; 2];
        let stack_span = span_for_words(&stack_words);

        marker
            .with_synthetic_current_thread_conservative_roots_for_testing(
                heap.id(),
                heap.epoch(),
                heap.state_descriptor(),
                stack_span,
                |proof| heap.ingest_machine_stack_conservative_roots(proof),
            )
            .expect("marker")
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
                span: stack_span,
                source: crate::gc::ConservativeRootSource::MachineStack
            }
        );
    }

    #[test]
    fn machine_stack_scan_validates_exact_payload_roots_in_register_before_stack_order() {
        let marker = JscMachineStackMarker::new();
        let mut heap = Heap::new();
        let register_cell = allocate_test_cell(&mut heap);
        let stack_cell = allocate_test_cell(&mut heap);
        let register_payload = 0x1000;
        let stack_payload = 0x2000;
        heap.bind_cell_payload(register_cell, register_payload)
            .expect("bind register payload");
        heap.bind_cell_payload(stack_cell, stack_payload)
            .expect("bind stack payload");
        heap.publish_cell(register_cell)
            .expect("publish register cell");
        heap.publish_cell(stack_cell).expect("publish stack cell");
        heap.enter_phase(
            GcPhase::Fixpoint,
            MutatorState::Collecting,
            GcConductor::Collector,
        );

        let register_state = JscArm64RegisterState {
            x19: register_payload as u64,
            ..JscArm64RegisterState::default()
        };
        let stack_words = [0usize, stack_payload, 0x3000];
        let stack_span = span_for_words(&stack_words);
        let state = JscCurrentThreadState {
            stack_origin: stack_span.end,
            stack_top: stack_span.begin,
            register_state: Some(&register_state),
        };

        marker
            .with_current_thread_conservative_roots_from_state(
                heap.id(),
                heap.epoch(),
                heap.state_descriptor(),
                &state,
                |proof| heap.ingest_machine_stack_conservative_roots(proof),
            )
            .expect("marker")
            .expect("heap ingest");

        let steps = heap.root_marking_plan().planned_steps().expect("root plan");
        let conservative_cells: Vec<_> = steps
            .iter()
            .filter_map(|step| match step {
                RootPlanStep::ConservativeCell { root, .. } => Some(*root),
                _ => None,
            })
            .collect();
        assert_eq!(
            conservative_cells,
            vec![
                ConservativeRootCell {
                    candidate_address: register_payload,
                    cell: register_cell
                },
                ConservativeRootCell {
                    candidate_address: stack_payload,
                    cell: stack_cell
                }
            ]
        );
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn heap_ingests_current_thread_roots_inside_marker_closure() {
        let marker = JscMachineStackMarker::new();
        let mut heap = collecting_heap();

        heap.ingest_current_thread_machine_stack_conservative_roots(&marker)
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
        assert!(matches!(
            steps[1],
            RootPlanStep::Conservative {
                source: crate::gc::ConservativeRootSource::MachineStack,
                ..
            }
        ));
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
