//! ARM64 native-entry GC/rooting proof support.
//!
//! This is an extraction-only Rust organization boundary for the C++
//! `Heap::addCoreConstraints` native-entry rooting chain. It keeps
//! `native_reentry.rs` below the oversized-file guardrail while preserving
//! the same descriptor validation behavior; it does not add engine behavior,
//! an admission `Ok` path, verifier mark-map storage, verifier drain, or real
//! native rooting.

use core::{marker::PhantomData, ops::Deref};

use crate::gc::{
    CellId, CellState, ConservativeRootCell, ConservativeRootSpan, ConservativeRoots, GcPhase,
    Heap, HeapCellKind, HeapConservativeScanAppendReceipt, HeapEpoch, HeapId,
    JscMachineStackConservativeRootingProof, JscMachineStackRootSpanKind, MarkDependency,
    MarkWorklistId, MutatorState, RootMarkReason, SlotVisitorAppendToMarkStackRecord,
    SlotVisitorCollectorEffectAction, SlotVisitorCollectorEffectsError,
    SlotVisitorCollectorEffectsPlan, SlotVisitorConservativeRootAppendRecord,
    SlotVisitorConservativeRootMarkingAction, SlotVisitorConservativeRootMarkingError,
    SlotVisitorConservativeRootMarkingPlan, SlotVisitorContainerNoteMarkedRecord,
    SlotVisitorDescriptor, SlotVisitorNoteLiveAuxiliaryCellRecord,
    VerifierSlotVisitorConservativeRootAppendError, VerifierSlotVisitorConservativeRootAppendPlan,
    VerifierSlotVisitorConservativeRootAppendProof,
};
use crate::jit::arm64_baseline::{
    produce_arm64_baseline_generated_native_frame_materialization_descriptor,
    validate_arm64_baseline_generated_native_frame_materialization,
    Arm64BaselineGeneratedNativeFrameMaterializationDescriptor,
    Arm64BaselineGeneratedNativeFrameMaterializationMismatch,
    Arm64BaselineGeneratedNativeFrameMaterializationProductionError,
    Arm64BaselineGeneratedNativeFrameMaterializationProductionRequest,
    Arm64BaselineGeneratedNativeFrameMaterializationValidationContext,
    Arm64BaselineLiveRootSlotKind, Arm64BaselineMachineStackRootSlotDescriptor,
    Arm64BaselineMachineStackSpanKind,
};
use crate::jit::{
    JitStubRoutineCandidateAddress, JitStubRoutineConservativeScanPlan,
    JitStubRoutineSetDescriptor, JitStubRoutineTraceError, JitStubRoutineTracePlan,
};

use super::super::arm64_native_entry::Arm64NativeEntryStackPublicationGuard;
use super::super::entry::FrameAddress;
use super::super::vm_roots::{VmRootGatherError, VmRootGatherPlan};

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64BranchAwareCallableTopCallFramePublicationRecord {
    pub(in crate::vm) published_top_frame: FrameAddress,
    pub(in crate::vm) current_entry_frame: FrameAddress,
    pub(in crate::vm) vm_entry_record: FrameAddress,
    pub(in crate::vm) vm_entry_previous_top_call_frame: Option<FrameAddress>,
    pub(in crate::vm) vm_entry_previous_top_entry_frame: Option<FrameAddress>,
    pub(in crate::vm) argument_count_excluding_this: usize,
    pub(in crate::vm) padded_argument_count: usize,
    pub(in crate::vm) live_local_count: usize,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication> {
    // C++ JSC stores a raw stack CallFrame* in VM::topCallFrame. Rust accepts
    // this dormant admission evidence only from an active
    // `Arm64NativeEntryStackPublicationGuard`, so boxed entry/call-frame storage
    // cannot masquerade as stack-local `doVMEntry` publication.
    pub(in crate::vm) publication: P6Arm64BranchAwareCallableTopCallFramePublicationRecord,
    _stack_publication_guard: PhantomData<&'publication ()>,
}

impl P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_> {
    #[allow(dead_code)]
    pub(in crate::vm) fn from_stack_publication_guard<'publication>(
        publication: &'publication Arm64NativeEntryStackPublicationGuard<'_, '_>,
    ) -> P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication> {
        let proof = publication.proof();
        P6Arm64BranchAwareCallableTopCallFramePublicationProof {
            publication: P6Arm64BranchAwareCallableTopCallFramePublicationRecord {
                published_top_frame: proof.top_call_frame,
                current_entry_frame: proof.top_entry_frame,
                vm_entry_record: proof.vm_entry_record,
                vm_entry_previous_top_call_frame: proof.previous_top_call_frame,
                vm_entry_previous_top_entry_frame: proof.previous_top_entry_frame,
                argument_count_excluding_this: proof.argument_count_excluding_this,
                padded_argument_count: proof.padded_argument_count,
                live_local_count: proof.live_local_count,
            },
            _stack_publication_guard: PhantomData,
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64NativeFrameMachineStackSpanKind {
    RegisterState,
    Stack,
}

impl From<JscMachineStackRootSpanKind> for P6Arm64NativeFrameMachineStackSpanKind {
    fn from(kind: JscMachineStackRootSpanKind) -> Self {
        match kind {
            JscMachineStackRootSpanKind::RegisterState => Self::RegisterState,
            JscMachineStackRootSpanKind::Stack => Self::Stack,
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64NativeFrameMachineStackSpanRecord {
    pub(in crate::vm) kind: P6Arm64NativeFrameMachineStackSpanKind,
    pub(in crate::vm) span: ConservativeRootSpan,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64MachineStackConservativeRootingProof {
    // Maps C++ `MachineThreads::gatherFromCurrentThread` plus the
    // `Heap::addCoreConstraints` conservative-scan append: the current-thread
    // register/stack spans must be the source of the appended ConservativeRoots,
    // and the active stack-local `doVMEntry` top CallFrame must be covered by the
    // scanned stack span. Rust keeps this as descriptor evidence until real
    // MarkedBlock / PreciseAllocation scanning and JIT-stub hooks are ported.
    pub(in crate::vm) heap: HeapId,
    pub(in crate::vm) marking_epoch: HeapEpoch,
    pub(in crate::vm) phase: GcPhase,
    pub(in crate::vm) mutator_state: MutatorState,
    pub(in crate::vm) published_top_frame: FrameAddress,
    pub(in crate::vm) top_call_frame_span: P6Arm64NativeFrameMachineStackSpanKind,
    pub(in crate::vm) machine_stack_spans: Vec<P6Arm64NativeFrameMachineStackSpanRecord>,
    pub(in crate::vm) machine_stack_roots: ConservativeRoots,
    pub(in crate::vm) conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
}

impl P6Arm64MachineStackConservativeRootingProof {
    #[allow(dead_code)]
    pub(in crate::vm) fn from_machine_stack_proof(
        top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
        machine_stack_proof: &JscMachineStackConservativeRootingProof<'_>,
        machine_stack_roots: ConservativeRoots,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        top_call_frame_span: P6Arm64NativeFrameMachineStackSpanKind,
    ) -> Self {
        Self {
            heap: machine_stack_proof.heap(),
            marking_epoch: machine_stack_proof.epoch(),
            phase: machine_stack_proof.phase(),
            mutator_state: machine_stack_proof.mutator_state(),
            published_top_frame: top_call_frame_publication.publication.published_top_frame,
            top_call_frame_span,
            machine_stack_spans: machine_stack_proof
                .spans()
                .iter()
                .map(|span| P6Arm64NativeFrameMachineStackSpanRecord {
                    kind: span.kind.into(),
                    span: span.span,
                })
                .collect(),
            machine_stack_roots,
            conservative_scan_append_receipt,
        }
    }

    pub(in crate::vm) fn conservative_scan_append_receipt(
        &self,
    ) -> &HeapConservativeScanAppendReceipt {
        &self.conservative_scan_append_receipt
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64SlotVisitorConservativeRootMarkingProof {
    // C++ `SlotVisitor::append(ConservativeRoots)` performs marking through the
    // visitor and heap. Rust admission must not accept a caller-supplied marking
    // plan as equivalent evidence, so this proof can only be built by replaying
    // the conservative-scan append plan against the live `Heap`.
    marking_plan: SlotVisitorConservativeRootMarkingPlan,
}

impl P6Arm64SlotVisitorConservativeRootMarkingProof {
    #[allow(dead_code)]
    pub(in crate::vm) fn from_conservative_scan_append_receipt(
        receipt: &HeapConservativeScanAppendReceipt,
        heap: &mut Heap,
    ) -> Result<Self, SlotVisitorConservativeRootMarkingError> {
        Ok(Self {
            marking_plan: receipt.append_plan.clone().mark_conservative_roots(heap)?,
        })
    }

    pub(in crate::vm) fn marking_plan(&self) -> &SlotVisitorConservativeRootMarkingPlan {
        &self.marking_plan
    }
}

impl Deref for P6Arm64SlotVisitorConservativeRootMarkingProof {
    type Target = SlotVisitorConservativeRootMarkingPlan;

    fn deref(&self) -> &Self::Target {
        self.marking_plan()
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64SlotVisitorCollectorEffectsProof {
    // C++ `SlotVisitor::appendJSCellOrAuxiliary` mutates JSCell state, notes
    // containers, and appends to the collector stack after `Heap::testAndSetMarked`.
    // Rust admission must not accept a caller-supplied collector-effects plan as
    // equivalent evidence, so this proof can only be built by replaying the
    // prior heap-produced marking proof against the live `Heap`.
    collector_effects_plan: SlotVisitorCollectorEffectsPlan,
}

impl P6Arm64SlotVisitorCollectorEffectsProof {
    #[allow(dead_code)]
    pub(in crate::vm) fn from_conservative_root_marking_proof(
        marking_proof: &P6Arm64SlotVisitorConservativeRootMarkingProof,
        heap: &mut Heap,
    ) -> Result<Self, SlotVisitorCollectorEffectsError> {
        Ok(Self {
            collector_effects_plan: marking_proof
                .marking_plan()
                .clone()
                .apply_collector_effects(heap)?,
        })
    }

    pub(in crate::vm) fn collector_effects_plan(&self) -> &SlotVisitorCollectorEffectsPlan {
        &self.collector_effects_plan
    }
}

impl Deref for P6Arm64SlotVisitorCollectorEffectsProof {
    type Target = SlotVisitorCollectorEffectsPlan;

    fn deref(&self) -> &Self::Target {
        self.collector_effects_plan()
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64VerifierSlotVisitorConservativeRootAppendProof {
    // C++ `Heap::addCoreConstraints` appends conservative roots to
    // `m_verifierSlotVisitor` only when that optional visitor is installed.
    // Rust has no heap-owned verifier visitor state yet, so current ARM64
    // admission can only prove the faithful `m_verifierSlotVisitor == nullptr`
    // branch from the prior heap-produced collector-effects proof.
    verifier_append_proof: VerifierSlotVisitorConservativeRootAppendProof,
}

impl P6Arm64VerifierSlotVisitorConservativeRootAppendProof {
    #[allow(dead_code)]
    pub(in crate::vm) fn no_verifier_slot_visitor_from_collector_effects_proof(
        collector_effects_proof: &P6Arm64SlotVisitorCollectorEffectsProof,
    ) -> Self {
        let collector_effects_plan = collector_effects_proof.collector_effects_plan();
        Self {
            verifier_append_proof:
                VerifierSlotVisitorConservativeRootAppendProof::NoVerifierSlotVisitor {
                    heap: collector_effects_plan.heap,
                    marking_epoch: collector_effects_plan.marking_epoch,
                },
        }
    }

    pub(in crate::vm) fn verifier_append_proof(
        &self,
    ) -> &VerifierSlotVisitorConservativeRootAppendProof {
        &self.verifier_append_proof
    }

    pub(in crate::vm) fn heap(&self) -> HeapId {
        self.verifier_append_proof.heap()
    }

    pub(in crate::vm) fn marking_epoch(&self) -> HeapEpoch {
        self.verifier_append_proof.marking_epoch()
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64JitStubRoutineConservativeScanHookProof {
    // C++ `Heap::addCoreConstraints` calls
    // `JITStubRoutineSet::prepareForConservativeScan()` before conservative
    // roots are gathered, and `ConservativeRoots` forwards every scanned
    // candidate through the mark hook to `JITStubRoutineSet::mark(void*)`.
    // Rust carries descriptor candidates instead of real executable addresses,
    // so this proof must be produced by prepare + hook replay rather than by
    // accepting a caller-supplied prepared scan plan.
    scan_plan: JitStubRoutineConservativeScanPlan,
}

impl P6Arm64JitStubRoutineConservativeScanHookProof {
    #[allow(dead_code)]
    pub(in crate::vm) fn from_prepared_set_and_conservative_scan_hook_candidates(
        set: &JitStubRoutineSetDescriptor,
        candidates: impl IntoIterator<Item = JitStubRoutineCandidateAddress>,
    ) -> Result<Self, JitStubRoutineTraceError> {
        let mut scan_plan = set.prepare_for_conservative_scan()?;
        for candidate in candidates {
            scan_plan.mark_candidate(candidate)?;
        }
        scan_plan.validate_consistency()?;
        Ok(Self { scan_plan })
    }

    pub(in crate::vm) fn scan_plan(&self) -> &JitStubRoutineConservativeScanPlan {
        &self.scan_plan
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64JitStubRoutineTraceProof {
    // C++ traces `m_jitStubRoutines` only after SlotVisitor consumed the
    // conservative roots because marked stubs depend on gather-hook state.
    // This remains descriptor-only until real executable-address hooks and
    // `GCAwareJITStubRoutine::markRequiredObjects` traversal are ported.
    trace_plan: JitStubRoutineTracePlan,
}

impl P6Arm64JitStubRoutineTraceProof {
    #[allow(dead_code)]
    pub(in crate::vm) fn from_collector_effects_verifier_and_scan_hook_proofs(
        collector_effects_proof: &P6Arm64SlotVisitorCollectorEffectsProof,
        _verifier_append_proof: &P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
        scan_hook_proof: &P6Arm64JitStubRoutineConservativeScanHookProof,
    ) -> Result<Self, JitStubRoutineTraceError> {
        let collector_effects_plan = collector_effects_proof.collector_effects_plan();
        let mut visitor = SlotVisitorDescriptor::new(
            collector_effects_plan.heap,
            "arm64-jit-stub-routine-trace-proof",
            collector_effects_plan.marking_epoch,
        );
        visitor.worklist = collector_effects_plan.worklist;
        visitor.root_mark_reason = RootMarkReason::JitStubRoutines;

        Ok(Self {
            trace_plan: scan_hook_proof
                .scan_plan()
                .trace_marked_stub_routines(&visitor)?,
        })
    }

    pub(in crate::vm) fn trace_plan(&self) -> &JitStubRoutineTracePlan {
        &self.trace_plan
    }
}

impl Deref for P6Arm64JitStubRoutineTraceProof {
    type Target = JitStubRoutineTracePlan;

    fn deref(&self) -> &Self::Target {
        self.trace_plan()
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64NativeRootSlotKind {
    Callee,
    ThisValue,
    Argument,
    Local,
    Scratch,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64NativeRootSlotRecord {
    pub(in crate::vm) kind: P6Arm64NativeRootSlotKind,
    pub(in crate::vm) slot_address: usize,
    pub(in crate::vm) encoded_payload: usize,
    pub(in crate::vm) expected_root: ConservativeRootCell,
    pub(in crate::vm) containing_span: P6Arm64NativeFrameMachineStackSpanKind,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64NativeFrameMachineStackResidencyProof {
    // Maps C++ `NativeCallFrameTracer` plus
    // `MachineThreads::gatherFromCurrentThread`: the published CallFrame* and
    // live JSValue words must be inside scanned current-thread spans. This raw
    // value is descriptor evidence only; public ARM64 admission must consume
    // `P6Arm64VerifiedNativeFrameMachineStackResidencyProof`, which ties it to
    // stack-local publication plus the current conservative scan. Exact
    // payload-to-cell validation remains heap-derived descriptor state until
    // C++ MarkedBlock/PreciseAllocation scanning is ported.
    pub(in crate::vm) heap: HeapId,
    pub(in crate::vm) marking_epoch: HeapEpoch,
    pub(in crate::vm) phase: GcPhase,
    pub(in crate::vm) mutator_state: MutatorState,
    pub(in crate::vm) conservative_scan_root_mark_reason: RootMarkReason,
    pub(in crate::vm) published_top_frame: FrameAddress,
    pub(in crate::vm) top_call_frame_span: P6Arm64NativeFrameMachineStackSpanKind,
    pub(in crate::vm) machine_stack_spans: Vec<P6Arm64NativeFrameMachineStackSpanRecord>,
    pub(in crate::vm) machine_stack_roots: ConservativeRoots,
    pub(in crate::vm) slot_records: Vec<P6Arm64NativeRootSlotRecord>,
    pub(in crate::vm) generated_native_frame_materialization:
        Option<Arm64BaselineGeneratedNativeFrameMaterializationDescriptor>,
}

impl P6Arm64NativeFrameMachineStackResidencyProof {
    #[allow(dead_code)]
    pub(in crate::vm) fn from_machine_stack_proof(
        top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
        machine_stack_proof: &JscMachineStackConservativeRootingProof<'_>,
        machine_stack_roots: ConservativeRoots,
        top_call_frame_span: P6Arm64NativeFrameMachineStackSpanKind,
        slot_records: Vec<P6Arm64NativeRootSlotRecord>,
    ) -> Self {
        Self {
            heap: machine_stack_proof.heap(),
            marking_epoch: machine_stack_proof.epoch(),
            phase: machine_stack_proof.phase(),
            mutator_state: machine_stack_proof.mutator_state(),
            conservative_scan_root_mark_reason: RootMarkReason::ConservativeScan,
            published_top_frame: top_call_frame_publication.publication.published_top_frame,
            top_call_frame_span,
            machine_stack_spans: machine_stack_proof
                .spans()
                .iter()
                .map(|span| P6Arm64NativeFrameMachineStackSpanRecord {
                    kind: span.kind.into(),
                    span: span.span,
                })
                .collect(),
            machine_stack_roots,
            slot_records,
            generated_native_frame_materialization: None,
        }
    }

    #[allow(dead_code)]
    pub(in crate::vm) fn with_generated_native_frame_materialization(
        mut self,
        top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
        frame_top_offset_bytes: isize,
        expected_live_local_slots: u32,
    ) -> Result<Self, P6Arm64GeneratedNativeFrameMaterializationAttachError> {
        let live_root_slots = self
            .slot_records
            .iter()
            .enumerate()
            .map(|(order, slot)| {
                let span = containing_span_for_word_slot(
                    &self.machine_stack_spans,
                    slot.slot_address,
                )
                .ok_or(
                    P6Arm64GeneratedNativeFrameMaterializationAttachError::LiveRootSlotSpanMissing {
                        order,
                        slot_address: slot.slot_address,
                    },
                )?;
                Ok(Arm64BaselineMachineStackRootSlotDescriptor {
                    kind: slot.kind.into(),
                    slot_address: slot.slot_address,
                    encoded_payload: slot.encoded_payload,
                    expected_root: slot.expected_root,
                    containing_span: slot.containing_span.into(),
                    span,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let argument_count_excluding_this = top_call_frame_publication
            .publication
            .argument_count_excluding_this
            .try_into()
            .map_err(|_| {
                P6Arm64GeneratedNativeFrameMaterializationAttachError::ArgumentCountExcludingThisOverflow {
                    count: top_call_frame_publication
                        .publication
                        .argument_count_excluding_this,
                }
            })?;
        let descriptor = produce_arm64_baseline_generated_native_frame_materialization_descriptor(
            Arm64BaselineGeneratedNativeFrameMaterializationProductionRequest {
                call_frame: top_call_frame_publication.publication.published_top_frame.0,
                frame_top_offset_bytes,
                argument_count_excluding_this,
                live_local_count: expected_live_local_slots,
                live_root_slots,
                vm_entry_previous_top_call_frame: top_call_frame_publication
                    .publication
                    .vm_entry_previous_top_call_frame
                    .map(|frame| frame.0),
                vm_entry_previous_top_entry_frame: top_call_frame_publication
                    .publication
                    .vm_entry_previous_top_entry_frame
                    .map(|frame| frame.0),
                published_top_entry_frame: top_call_frame_publication
                    .publication
                    .current_entry_frame
                    .0,
            },
        )
        .map_err(P6Arm64GeneratedNativeFrameMaterializationAttachError::Producer)?;
        self.generated_native_frame_materialization = Some(descriptor);
        Ok(self)
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64VerifiedNativeFrameMachineStackResidencyProof<'publication> {
    // C++ has no separate wrapper: `prepareCallOperation` /
    // `NativeCallFrameTracer` publish a native CallFrame* and
    // `Heap::addCoreConstraints` later scans the machine stack. Rust keeps this
    // newtype as admission authority because raw descriptors are caller-owned
    // test evidence. The wrapper consumes the stack-local top-frame publication
    // proof and the current machine-stack conservative-rooting proof; it proves
    // scan residency, not final generated native-frame materialization or real
    // generated live-slot discovery.
    residency_proof: P6Arm64NativeFrameMachineStackResidencyProof,
    _stack_publication_guard: PhantomData<&'publication ()>,
}

impl<'publication> P6Arm64VerifiedNativeFrameMachineStackResidencyProof<'publication> {
    #[allow(dead_code)]
    pub(in crate::vm) fn from_machine_stack_conservative_rooting_proof(
        top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<
            'publication,
        >,
        machine_stack_conservative_rooting_proof: &P6Arm64MachineStackConservativeRootingProof,
        slot_records: Vec<P6Arm64NativeRootSlotRecord>,
    ) -> Result<Self, P6Arm64VerifiedNativeFrameMachineStackResidencyProofError> {
        validate_p6_arm64_machine_stack_conservative_rooting_proof(
            top_call_frame_publication,
            machine_stack_conservative_rooting_proof,
        )
        .map_err(P6Arm64VerifiedNativeFrameMachineStackResidencyProofError::MachineStack)?;

        let residency_proof = P6Arm64NativeFrameMachineStackResidencyProof {
            heap: machine_stack_conservative_rooting_proof.heap,
            marking_epoch: machine_stack_conservative_rooting_proof.marking_epoch,
            phase: machine_stack_conservative_rooting_proof.phase,
            mutator_state: machine_stack_conservative_rooting_proof.mutator_state,
            conservative_scan_root_mark_reason: RootMarkReason::ConservativeScan,
            published_top_frame: top_call_frame_publication.publication.published_top_frame,
            top_call_frame_span: machine_stack_conservative_rooting_proof.top_call_frame_span,
            machine_stack_spans: machine_stack_conservative_rooting_proof
                .machine_stack_spans
                .clone(),
            machine_stack_roots: machine_stack_conservative_rooting_proof
                .machine_stack_roots
                .clone(),
            slot_records,
            generated_native_frame_materialization: None,
        };

        validate_p6_arm64_native_frame_machine_stack_residency_proof(
            top_call_frame_publication,
            machine_stack_conservative_rooting_proof.conservative_scan_append_receipt(),
            &residency_proof,
        )
        .map_err(P6Arm64VerifiedNativeFrameMachineStackResidencyProofError::Residency)?;

        Ok(Self {
            residency_proof,
            _stack_publication_guard: PhantomData,
        })
    }

    pub(in crate::vm) fn residency_proof(&self) -> &P6Arm64NativeFrameMachineStackResidencyProof {
        &self.residency_proof
    }

    #[allow(dead_code)]
    pub(in crate::vm) fn with_generated_native_frame_materialization(
        mut self,
        top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<
            'publication,
        >,
        frame_top_offset_bytes: isize,
        expected_live_local_slots: u32,
    ) -> Result<Self, P6Arm64GeneratedNativeFrameMaterializationAttachError> {
        self.residency_proof = self
            .residency_proof
            .with_generated_native_frame_materialization(
                top_call_frame_publication,
                frame_top_offset_bytes,
                expected_live_local_slots,
            )?;
        Ok(self)
    }
}

impl Deref for P6Arm64VerifiedNativeFrameMachineStackResidencyProof<'_> {
    type Target = P6Arm64NativeFrameMachineStackResidencyProof;

    fn deref(&self) -> &Self::Target {
        self.residency_proof()
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VerifiedNativeFrameMachineStackResidencyProofError {
    MachineStack(P6Arm64MachineStackConservativeRootingProofMismatch),
    Residency(P6Arm64NativeFrameMachineStackResidencyProofMismatch),
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64GeneratedNativeFrameMaterializationAttachError {
    LiveRootSlotSpanMissing { order: usize, slot_address: usize },
    ArgumentCountExcludingThisOverflow { count: usize },
    Producer(Arm64BaselineGeneratedNativeFrameMaterializationProductionError),
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64BranchAwareCallableFallbackRootingProof<'publication> {
    MissingTopCallFramePublication,
    TopCallFramePublicationWithoutConservativeScanAppend(
        P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
    ),
    TopCallFramePublicationWithMachineStackConservativeRootingProof {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        machine_stack_conservative_rooting_proof: P6Arm64MachineStackConservativeRootingProof,
    },
    TopCallFramePublicationWithVmRootGatherPlan {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        machine_stack_conservative_rooting_proof: P6Arm64MachineStackConservativeRootingProof,
        vm_root_gather_plan: VmRootGatherPlan,
    },
    TopCallFramePublicationWithVmRootGatherAndConservativeRootMarkingPlan {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        machine_stack_conservative_rooting_proof: P6Arm64MachineStackConservativeRootingProof,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
    },
    TopCallFramePublicationWithVmRootGatherAndCollectorEffectsPlan {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        machine_stack_conservative_rooting_proof: P6Arm64MachineStackConservativeRootingProof,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
    },
    TopCallFramePublicationWithVmRootGatherCollectorEffectsAndVerifierAppendProof {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        machine_stack_conservative_rooting_proof: P6Arm64MachineStackConservativeRootingProof,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
        verifier_append_proof: P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
    },
    TopCallFramePublicationWithVmRootGatherCollectorEffectsVerifierAppendAndJitStubTracePlan {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        machine_stack_conservative_rooting_proof: P6Arm64MachineStackConservativeRootingProof,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
        verifier_append_proof: P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
        jit_stub_trace_plan: P6Arm64JitStubRoutineTraceProof,
    },
    TopCallFramePublicationWithVmRootGatherCollectorEffectsVerifierJitStubTraceAndMachineStackResidencyProof
    {
        top_call_frame_publication:
            P6Arm64BranchAwareCallableTopCallFramePublicationProof<'publication>,
        machine_stack_conservative_rooting_proof: P6Arm64MachineStackConservativeRootingProof,
        vm_root_gather_plan: VmRootGatherPlan,
        conservative_root_marking_plan: P6Arm64SlotVisitorConservativeRootMarkingProof,
        collector_effects_plan: P6Arm64SlotVisitorCollectorEffectsProof,
        verifier_append_proof: P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
        jit_stub_trace_plan: P6Arm64JitStubRoutineTraceProof,
        native_frame_residency_proof:
            P6Arm64VerifiedNativeFrameMachineStackResidencyProof<'publication>,
    },
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64ConservativeRootMarkingProofMismatch {
    HeapMismatch {
        receipt: HeapId,
        marking: HeapId,
    },
    MarkingEpochMismatch {
        receipt: HeapEpoch,
        marking: HeapEpoch,
    },
    WorklistMismatch {
        receipt: MarkWorklistId,
        marking: MarkWorklistId,
    },
    RootMarkReasonMismatch {
        receipt: RootMarkReason,
        marking: RootMarkReason,
    },
    DependencyMismatch {
        receipt: MarkDependency,
        marking: MarkDependency,
    },
    AppendReceiptRecordCountMismatch {
        receipt: usize,
        append_plan: usize,
    },
    MarkingRecordCountMismatch {
        receipt: usize,
        marking: usize,
    },
    AppendRecordMismatch {
        order: usize,
        receipt: SlotVisitorConservativeRootAppendRecord,
        marking: SlotVisitorConservativeRootAppendRecord,
    },
    HeapMarkingHeapMismatch {
        order: usize,
        plan: HeapId,
        marking: HeapId,
    },
    HeapMarkingEpochMismatch {
        order: usize,
        plan: HeapEpoch,
        marking: HeapEpoch,
    },
    HeapMarkingRootMismatch {
        order: usize,
        append: ConservativeRootCell,
        marking: ConservativeRootCell,
    },
    HeapMarkingCellMismatch {
        order: usize,
        append: CellId,
        marking: CellId,
    },
    MarkingActionMismatch {
        order: usize,
        expected: SlotVisitorConservativeRootMarkingAction,
        actual: SlotVisitorConservativeRootMarkingAction,
    },
    VisitCountDeltaMismatch {
        order: usize,
        expected: usize,
        actual: usize,
    },
    BytesVisitedDeltaMismatch {
        order: usize,
        expected: usize,
        actual: usize,
    },
    NonCellVisitCountDeltaMismatch {
        order: usize,
        expected: usize,
        actual: usize,
    },
    QueuedJsCellCountMismatch {
        expected: usize,
        actual: usize,
    },
    LiveAuxiliaryCountMismatch {
        expected: usize,
        actual: usize,
    },
    AlreadyMarkedCountMismatch {
        expected: usize,
        actual: usize,
    },
    VisitCountTotalMismatch {
        expected: usize,
        actual: usize,
    },
    BytesVisitedTotalMismatch {
        expected: usize,
        actual: usize,
    },
    NonCellVisitCountTotalMismatch {
        expected: usize,
        actual: usize,
    },
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64CollectorEffectsProofMismatch {
    HeapMismatch {
        marking: HeapId,
        effects: HeapId,
    },
    MarkingEpochMismatch {
        marking: HeapEpoch,
        effects: HeapEpoch,
    },
    WorklistMismatch {
        marking: MarkWorklistId,
        effects: MarkWorklistId,
    },
    RootMarkReasonMismatch {
        marking: RootMarkReason,
        effects: RootMarkReason,
    },
    DependencyMismatch {
        marking: MarkDependency,
        effects: MarkDependency,
    },
    CollectorRecordCountMismatch {
        marking: usize,
        effects: usize,
    },
    CollectorRecordOrderMismatch {
        expected: usize,
        actual: usize,
    },
    CollectorMarkingRecordMismatch {
        order: usize,
    },
    CollectorActionMismatch {
        order: usize,
        expected: SlotVisitorCollectorEffectAction,
        actual: SlotVisitorCollectorEffectAction,
    },
    VisitCountDeltaMismatch {
        order: usize,
        expected: usize,
        actual: usize,
    },
    BytesVisitedDeltaMismatch {
        order: usize,
        expected: usize,
        actual: usize,
    },
    NonCellVisitCountDeltaMismatch {
        order: usize,
        expected: usize,
        actual: usize,
    },
    JsCellStateUpdateCountMismatch {
        expected: usize,
        actual: usize,
    },
    ContainerNoteMarkedCountMismatch {
        expected: usize,
        actual: usize,
    },
    MarkStackAppendCountMismatch {
        expected: usize,
        actual: usize,
    },
    LiveAuxiliaryCountMismatch {
        expected: usize,
        actual: usize,
    },
    AlreadyMarkedCountMismatch {
        expected: usize,
        actual: usize,
    },
    VisitCountTotalMismatch {
        expected: usize,
        actual: usize,
    },
    BytesVisitedTotalMismatch {
        expected: usize,
        actual: usize,
    },
    NonCellVisitCountTotalMismatch {
        expected: usize,
        actual: usize,
    },
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64JitStubRoutineTraceProofMismatch {
    HeapMismatch {
        collector: HeapId,
        jit_stub_trace: HeapId,
    },
    MarkingEpochMismatch {
        collector: HeapEpoch,
        jit_stub_trace: HeapEpoch,
    },
    WorklistMismatch {
        collector: MarkWorklistId,
        jit_stub_trace: MarkWorklistId,
    },
    InvalidRootMarkReason {
        actual: RootMarkReason,
    },
    TracePlanMismatch(JitStubRoutineTraceError),
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64MachineStackConservativeRootingProofMismatch {
    HeapMismatch {
        receipt: HeapId,
        machine_stack: HeapId,
    },
    MarkingEpochMismatch {
        receipt: HeapEpoch,
        machine_stack: HeapEpoch,
    },
    PhaseMismatch {
        receipt: GcPhase,
        machine_stack: GcPhase,
    },
    MutatorStateMismatch {
        receipt: MutatorState,
        machine_stack: MutatorState,
    },
    InvalidRootMarkReason {
        actual: RootMarkReason,
    },
    MachineStackRootSpanMismatch {
        expected: Vec<ConservativeRootSpan>,
        actual: Vec<ConservativeRootSpan>,
    },
    CurrentThreadSpanOrderMismatch {
        observed: Vec<P6Arm64NativeFrameMachineStackSpanKind>,
    },
    TopCallFrameAddressMismatch {
        publication: FrameAddress,
        machine_stack: FrameAddress,
    },
    TopCallFrameAddressUnaligned {
        address: FrameAddress,
    },
    TopCallFrameOutsideScannedSpans {
        address: FrameAddress,
    },
    TopCallFrameContainingSpanMismatch {
        expected: P6Arm64NativeFrameMachineStackSpanKind,
        actual: P6Arm64NativeFrameMachineStackSpanKind,
    },
    ConservativeScanAppendRecordCountMismatch {
        machine_stack_roots: usize,
        receipt: usize,
    },
    ConservativeScanAppendRootMismatch {
        order: usize,
        machine_stack: ConservativeRootCell,
        receipt: ConservativeRootCell,
    },
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64NativeFrameMachineStackResidencyProofMismatch {
    HeapMismatch {
        receipt: HeapId,
        machine_stack: HeapId,
    },
    MarkingEpochMismatch {
        receipt: HeapEpoch,
        machine_stack: HeapEpoch,
    },
    PhaseMismatch {
        receipt: GcPhase,
        machine_stack: GcPhase,
    },
    MutatorStateMismatch {
        receipt: MutatorState,
        machine_stack: MutatorState,
    },
    InvalidRootMarkReason {
        actual: RootMarkReason,
    },
    MachineStackRootSpanMismatch {
        expected: Vec<ConservativeRootSpan>,
        actual: Vec<ConservativeRootSpan>,
    },
    CurrentThreadSpanOrderMismatch {
        observed: Vec<P6Arm64NativeFrameMachineStackSpanKind>,
    },
    TopCallFrameAddressMismatch {
        publication: FrameAddress,
        machine_stack: FrameAddress,
    },
    TopCallFrameAddressUnaligned {
        address: FrameAddress,
    },
    TopCallFrameOutsideScannedSpans {
        address: FrameAddress,
    },
    TopCallFrameContainingSpanMismatch {
        expected: P6Arm64NativeFrameMachineStackSpanKind,
        actual: P6Arm64NativeFrameMachineStackSpanKind,
    },
    ResidencySourceTopFrameMismatch {
        source: FrameAddress,
        residency: FrameAddress,
    },
    ResidencySourceTopCallFrameSpanMismatch {
        source: P6Arm64NativeFrameMachineStackSpanKind,
        residency: P6Arm64NativeFrameMachineStackSpanKind,
    },
    ResidencySourceMachineStackSpansMismatch {
        source: Vec<P6Arm64NativeFrameMachineStackSpanRecord>,
        residency: Vec<P6Arm64NativeFrameMachineStackSpanRecord>,
    },
    ResidencySourceMachineStackRootsMismatch {
        source: ConservativeRoots,
        residency: ConservativeRoots,
    },
    SlotAddressUnaligned {
        order: usize,
        slot_address: usize,
    },
    SlotAddressOutsideScannedSpans {
        order: usize,
        slot_address: usize,
    },
    SlotContainingSpanMismatch {
        order: usize,
        expected: P6Arm64NativeFrameMachineStackSpanKind,
        actual: P6Arm64NativeFrameMachineStackSpanKind,
    },
    SlotPayloadRootMismatch {
        order: usize,
        encoded_payload: usize,
        expected_root: ConservativeRootCell,
    },
    SlotRootAbsentFromMachineStackRoots {
        order: usize,
        root: ConservativeRootCell,
    },
    SlotRootAbsentFromConservativeScanAppendReceipt {
        order: usize,
        root: ConservativeRootCell,
    },
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VmRootGatherProofMismatch {
    HeapMismatch {
        receipt: HeapId,
        vm_roots: HeapId,
    },
    MarkingEpochMismatch {
        receipt: HeapEpoch,
        vm_roots: HeapEpoch,
    },
    InvalidAppendRootMarkReason {
        actual: RootMarkReason,
    },
    GatherPlanMismatch(VmRootGatherError),
    ReceiptMissingVmRoot {
        root: ConservativeRootCell,
    },
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64VerifierAppendProofMismatch {
    HeapMismatch {
        receipt: HeapId,
        verifier: HeapId,
    },
    MarkingEpochMismatch {
        receipt: HeapEpoch,
        verifier: HeapEpoch,
    },
    VmRootGatherHeapMismatch {
        vm_roots: HeapId,
        verifier: HeapId,
    },
    VmRootGatherMarkingEpochMismatch {
        vm_roots: HeapEpoch,
        verifier: HeapEpoch,
    },
    MissingVerifierAppendPlan {
        heap: HeapId,
        marking_epoch: HeapEpoch,
    },
    InvalidRootMarkReason {
        actual: RootMarkReason,
    },
    VerifierPlanMismatch(VerifierSlotVisitorConservativeRootAppendError),
    VerifierRecordCountMismatch {
        receipt: usize,
        verifier: usize,
    },
    VerifierMarkingRecordCountMismatch {
        marking: usize,
        verifier: usize,
    },
    VerifierAppendRecordMismatch {
        order: usize,
        receipt: SlotVisitorConservativeRootAppendRecord,
        verifier: SlotVisitorConservativeRootAppendRecord,
    },
    VerifierHeapCellKindMismatch {
        order: usize,
        marking: HeapCellKind,
        verifier: HeapCellKind,
    },
}

pub(super) fn validate_p6_arm64_conservative_root_marking_plan(
    receipt: &HeapConservativeScanAppendReceipt,
    marking_plan: &SlotVisitorConservativeRootMarkingPlan,
) -> Result<(), P6Arm64ConservativeRootMarkingProofMismatch> {
    if marking_plan.heap != receipt.heap {
        return Err(P6Arm64ConservativeRootMarkingProofMismatch::HeapMismatch {
            receipt: receipt.heap,
            marking: marking_plan.heap,
        });
    }

    if marking_plan.marking_epoch != receipt.epoch {
        return Err(
            P6Arm64ConservativeRootMarkingProofMismatch::MarkingEpochMismatch {
                receipt: receipt.epoch,
                marking: marking_plan.marking_epoch,
            },
        );
    }

    if marking_plan.worklist != receipt.append_plan.worklist {
        return Err(
            P6Arm64ConservativeRootMarkingProofMismatch::WorklistMismatch {
                receipt: receipt.append_plan.worklist,
                marking: marking_plan.worklist,
            },
        );
    }

    if marking_plan.root_mark_reason != receipt.append_plan.root_mark_reason {
        return Err(
            P6Arm64ConservativeRootMarkingProofMismatch::RootMarkReasonMismatch {
                receipt: receipt.append_plan.root_mark_reason,
                marking: marking_plan.root_mark_reason,
            },
        );
    }

    if marking_plan.dependency != receipt.append_plan.dependency {
        return Err(
            P6Arm64ConservativeRootMarkingProofMismatch::DependencyMismatch {
                receipt: receipt.append_plan.dependency,
                marking: marking_plan.dependency,
            },
        );
    }

    let append_plan_record_count = receipt.append_plan.records.len();
    if append_plan_record_count != receipt.appended_record_count {
        return Err(
            P6Arm64ConservativeRootMarkingProofMismatch::AppendReceiptRecordCountMismatch {
                receipt: receipt.appended_record_count,
                append_plan: append_plan_record_count,
            },
        );
    }

    if marking_plan.records.len() != receipt.appended_record_count {
        return Err(
            P6Arm64ConservativeRootMarkingProofMismatch::MarkingRecordCountMismatch {
                receipt: receipt.appended_record_count,
                marking: marking_plan.records.len(),
            },
        );
    }

    let mut queued_js_cell_count = 0;
    let mut live_auxiliary_count = 0;
    let mut already_marked_count = 0;
    let mut visit_count_delta = 0;
    let mut bytes_visited_delta = 0;
    let mut non_cell_visit_count_delta = 0;

    for (order, (receipt_record, marking_record)) in receipt
        .append_plan
        .records
        .iter()
        .copied()
        .zip(marking_plan.records.iter())
        .enumerate()
    {
        if marking_record.append_record != receipt_record {
            return Err(
                P6Arm64ConservativeRootMarkingProofMismatch::AppendRecordMismatch {
                    order,
                    receipt: receipt_record,
                    marking: marking_record.append_record,
                },
            );
        }

        let heap_marking = marking_record.heap_marking;
        if heap_marking.heap != marking_plan.heap {
            return Err(
                P6Arm64ConservativeRootMarkingProofMismatch::HeapMarkingHeapMismatch {
                    order,
                    plan: marking_plan.heap,
                    marking: heap_marking.heap,
                },
            );
        }

        if heap_marking.marking_epoch != marking_plan.marking_epoch {
            return Err(
                P6Arm64ConservativeRootMarkingProofMismatch::HeapMarkingEpochMismatch {
                    order,
                    plan: marking_plan.marking_epoch,
                    marking: heap_marking.marking_epoch,
                },
            );
        }

        if heap_marking.root != receipt_record.root {
            return Err(
                P6Arm64ConservativeRootMarkingProofMismatch::HeapMarkingRootMismatch {
                    order,
                    append: receipt_record.root,
                    marking: heap_marking.root,
                },
            );
        }

        if heap_marking.cell != receipt_record.cell {
            return Err(
                P6Arm64ConservativeRootMarkingProofMismatch::HeapMarkingCellMismatch {
                    order,
                    append: receipt_record.cell,
                    marking: heap_marking.cell,
                },
            );
        }

        let (expected_action, expected_visit_delta, expected_bytes_delta, expected_non_cell_delta) =
            if heap_marking.already_marked {
                already_marked_count += 1;
                (
                    SlotVisitorConservativeRootMarkingAction::AlreadyMarked,
                    0,
                    0,
                    0,
                )
            } else {
                match heap_marking.heap_cell_kind {
                    HeapCellKind::JsCell | HeapCellKind::JsCellWithIndexingHeader => {
                        queued_js_cell_count += 1;
                        (
                            SlotVisitorConservativeRootMarkingAction::QueueJsCell {
                                cell_state: CellState::PossiblyGrey,
                                worklist: marking_plan.worklist,
                            },
                            1,
                            heap_marking.byte_size,
                            0,
                        )
                    }
                    HeapCellKind::Auxiliary => {
                        live_auxiliary_count += 1;
                        (
                            SlotVisitorConservativeRootMarkingAction::NoteLiveAuxiliary,
                            1,
                            heap_marking.byte_size,
                            heap_marking.byte_size,
                        )
                    }
                }
            };

        if marking_record.action != expected_action {
            return Err(
                P6Arm64ConservativeRootMarkingProofMismatch::MarkingActionMismatch {
                    order,
                    expected: expected_action,
                    actual: marking_record.action,
                },
            );
        }

        if marking_record.visit_count_delta != expected_visit_delta {
            return Err(
                P6Arm64ConservativeRootMarkingProofMismatch::VisitCountDeltaMismatch {
                    order,
                    expected: expected_visit_delta,
                    actual: marking_record.visit_count_delta,
                },
            );
        }

        if marking_record.bytes_visited_delta != expected_bytes_delta {
            return Err(
                P6Arm64ConservativeRootMarkingProofMismatch::BytesVisitedDeltaMismatch {
                    order,
                    expected: expected_bytes_delta,
                    actual: marking_record.bytes_visited_delta,
                },
            );
        }

        if marking_record.non_cell_visit_count_delta != expected_non_cell_delta {
            return Err(
                P6Arm64ConservativeRootMarkingProofMismatch::NonCellVisitCountDeltaMismatch {
                    order,
                    expected: expected_non_cell_delta,
                    actual: marking_record.non_cell_visit_count_delta,
                },
            );
        }

        visit_count_delta += expected_visit_delta;
        bytes_visited_delta += expected_bytes_delta;
        non_cell_visit_count_delta += expected_non_cell_delta;
    }

    if marking_plan.queued_js_cell_count != queued_js_cell_count {
        return Err(
            P6Arm64ConservativeRootMarkingProofMismatch::QueuedJsCellCountMismatch {
                expected: queued_js_cell_count,
                actual: marking_plan.queued_js_cell_count,
            },
        );
    }

    if marking_plan.live_auxiliary_count != live_auxiliary_count {
        return Err(
            P6Arm64ConservativeRootMarkingProofMismatch::LiveAuxiliaryCountMismatch {
                expected: live_auxiliary_count,
                actual: marking_plan.live_auxiliary_count,
            },
        );
    }

    if marking_plan.already_marked_count != already_marked_count {
        return Err(
            P6Arm64ConservativeRootMarkingProofMismatch::AlreadyMarkedCountMismatch {
                expected: already_marked_count,
                actual: marking_plan.already_marked_count,
            },
        );
    }

    if marking_plan.visit_count_delta != visit_count_delta {
        return Err(
            P6Arm64ConservativeRootMarkingProofMismatch::VisitCountTotalMismatch {
                expected: visit_count_delta,
                actual: marking_plan.visit_count_delta,
            },
        );
    }

    if marking_plan.bytes_visited_delta != bytes_visited_delta {
        return Err(
            P6Arm64ConservativeRootMarkingProofMismatch::BytesVisitedTotalMismatch {
                expected: bytes_visited_delta,
                actual: marking_plan.bytes_visited_delta,
            },
        );
    }

    if marking_plan.non_cell_visit_count_delta != non_cell_visit_count_delta {
        return Err(
            P6Arm64ConservativeRootMarkingProofMismatch::NonCellVisitCountTotalMismatch {
                expected: non_cell_visit_count_delta,
                actual: marking_plan.non_cell_visit_count_delta,
            },
        );
    }

    Ok(())
}

pub(super) fn validate_p6_arm64_collector_effects_plan(
    marking_plan: &SlotVisitorConservativeRootMarkingPlan,
    effects_plan: &SlotVisitorCollectorEffectsPlan,
) -> Result<(), P6Arm64CollectorEffectsProofMismatch> {
    if effects_plan.heap != marking_plan.heap {
        return Err(P6Arm64CollectorEffectsProofMismatch::HeapMismatch {
            marking: marking_plan.heap,
            effects: effects_plan.heap,
        });
    }

    if effects_plan.marking_epoch != marking_plan.marking_epoch {
        return Err(P6Arm64CollectorEffectsProofMismatch::MarkingEpochMismatch {
            marking: marking_plan.marking_epoch,
            effects: effects_plan.marking_epoch,
        });
    }

    if effects_plan.worklist != marking_plan.worklist {
        return Err(P6Arm64CollectorEffectsProofMismatch::WorklistMismatch {
            marking: marking_plan.worklist,
            effects: effects_plan.worklist,
        });
    }

    if effects_plan.root_mark_reason != marking_plan.root_mark_reason {
        return Err(
            P6Arm64CollectorEffectsProofMismatch::RootMarkReasonMismatch {
                marking: marking_plan.root_mark_reason,
                effects: effects_plan.root_mark_reason,
            },
        );
    }

    if effects_plan.dependency != marking_plan.dependency {
        return Err(P6Arm64CollectorEffectsProofMismatch::DependencyMismatch {
            marking: marking_plan.dependency,
            effects: effects_plan.dependency,
        });
    }

    if effects_plan.records.len() != marking_plan.records.len() {
        return Err(
            P6Arm64CollectorEffectsProofMismatch::CollectorRecordCountMismatch {
                marking: marking_plan.records.len(),
                effects: effects_plan.records.len(),
            },
        );
    }

    let mut js_cell_state_update_count = 0;
    let mut container_note_marked_count = 0;
    let mut mark_stack_append_count = 0;
    let mut live_auxiliary_count = 0;
    let mut already_marked_count = 0;
    let mut visit_count_delta = 0;
    let mut bytes_visited_delta = 0;
    let mut non_cell_visit_count_delta = 0;

    for (order, (marking_record, effect_record)) in marking_plan
        .records
        .iter()
        .zip(effects_plan.records.iter())
        .enumerate()
    {
        if effect_record.order != order {
            return Err(
                P6Arm64CollectorEffectsProofMismatch::CollectorRecordOrderMismatch {
                    expected: order,
                    actual: effect_record.order,
                },
            );
        }

        if effect_record.marking_record != *marking_record {
            return Err(
                P6Arm64CollectorEffectsProofMismatch::CollectorMarkingRecordMismatch { order },
            );
        }

        let expected_action = expected_p6_arm64_collector_effect_action(marking_record);
        if effect_record.action != expected_action {
            return Err(
                P6Arm64CollectorEffectsProofMismatch::CollectorActionMismatch {
                    order,
                    expected: expected_action,
                    actual: effect_record.action,
                },
            );
        }

        if effect_record.visit_count_delta != marking_record.visit_count_delta {
            return Err(
                P6Arm64CollectorEffectsProofMismatch::VisitCountDeltaMismatch {
                    order,
                    expected: marking_record.visit_count_delta,
                    actual: effect_record.visit_count_delta,
                },
            );
        }

        if effect_record.bytes_visited_delta != marking_record.bytes_visited_delta {
            return Err(
                P6Arm64CollectorEffectsProofMismatch::BytesVisitedDeltaMismatch {
                    order,
                    expected: marking_record.bytes_visited_delta,
                    actual: effect_record.bytes_visited_delta,
                },
            );
        }

        if effect_record.non_cell_visit_count_delta != marking_record.non_cell_visit_count_delta {
            return Err(
                P6Arm64CollectorEffectsProofMismatch::NonCellVisitCountDeltaMismatch {
                    order,
                    expected: marking_record.non_cell_visit_count_delta,
                    actual: effect_record.non_cell_visit_count_delta,
                },
            );
        }

        match expected_action {
            SlotVisitorCollectorEffectAction::AlreadyMarkedReturn => {
                already_marked_count += 1;
            }
            SlotVisitorCollectorEffectAction::AppendToMarkStack(_) => {
                js_cell_state_update_count += 1;
                container_note_marked_count += 1;
                mark_stack_append_count += 1;
            }
            SlotVisitorCollectorEffectAction::NoteLiveAuxiliaryCell(_) => {
                container_note_marked_count += 1;
                live_auxiliary_count += 1;
            }
        }

        visit_count_delta += marking_record.visit_count_delta;
        bytes_visited_delta += marking_record.bytes_visited_delta;
        non_cell_visit_count_delta += marking_record.non_cell_visit_count_delta;
    }

    if effects_plan.js_cell_state_update_count != js_cell_state_update_count {
        return Err(
            P6Arm64CollectorEffectsProofMismatch::JsCellStateUpdateCountMismatch {
                expected: js_cell_state_update_count,
                actual: effects_plan.js_cell_state_update_count,
            },
        );
    }

    if effects_plan.container_note_marked_count != container_note_marked_count {
        return Err(
            P6Arm64CollectorEffectsProofMismatch::ContainerNoteMarkedCountMismatch {
                expected: container_note_marked_count,
                actual: effects_plan.container_note_marked_count,
            },
        );
    }

    if effects_plan.mark_stack_append_count != mark_stack_append_count {
        return Err(
            P6Arm64CollectorEffectsProofMismatch::MarkStackAppendCountMismatch {
                expected: mark_stack_append_count,
                actual: effects_plan.mark_stack_append_count,
            },
        );
    }

    if effects_plan.live_auxiliary_count != live_auxiliary_count {
        return Err(
            P6Arm64CollectorEffectsProofMismatch::LiveAuxiliaryCountMismatch {
                expected: live_auxiliary_count,
                actual: effects_plan.live_auxiliary_count,
            },
        );
    }

    if effects_plan.already_marked_count != already_marked_count {
        return Err(
            P6Arm64CollectorEffectsProofMismatch::AlreadyMarkedCountMismatch {
                expected: already_marked_count,
                actual: effects_plan.already_marked_count,
            },
        );
    }

    if effects_plan.visit_count_delta != visit_count_delta {
        return Err(
            P6Arm64CollectorEffectsProofMismatch::VisitCountTotalMismatch {
                expected: visit_count_delta,
                actual: effects_plan.visit_count_delta,
            },
        );
    }

    if effects_plan.bytes_visited_delta != bytes_visited_delta {
        return Err(
            P6Arm64CollectorEffectsProofMismatch::BytesVisitedTotalMismatch {
                expected: bytes_visited_delta,
                actual: effects_plan.bytes_visited_delta,
            },
        );
    }

    if effects_plan.non_cell_visit_count_delta != non_cell_visit_count_delta {
        return Err(
            P6Arm64CollectorEffectsProofMismatch::NonCellVisitCountTotalMismatch {
                expected: non_cell_visit_count_delta,
                actual: effects_plan.non_cell_visit_count_delta,
            },
        );
    }

    Ok(())
}

pub(super) fn expected_p6_arm64_collector_effect_action(
    marking_record: &crate::gc::SlotVisitorConservativeRootMarkingRecord,
) -> SlotVisitorCollectorEffectAction {
    let heap_marking = marking_record.heap_marking;
    match marking_record.action {
        SlotVisitorConservativeRootMarkingAction::AlreadyMarked => {
            SlotVisitorCollectorEffectAction::AlreadyMarkedReturn
        }
        SlotVisitorConservativeRootMarkingAction::QueueJsCell {
            cell_state,
            worklist,
        } => {
            let container_note_marked = SlotVisitorContainerNoteMarkedRecord {
                cell: heap_marking.cell,
                heap_cell_kind: heap_marking.heap_cell_kind,
                byte_size: heap_marking.byte_size,
            };
            SlotVisitorCollectorEffectAction::AppendToMarkStack(
                SlotVisitorAppendToMarkStackRecord {
                    cell: heap_marking.cell,
                    heap_cell_kind: heap_marking.heap_cell_kind,
                    cell_state,
                    worklist,
                    root_mark_reason: marking_record.append_record.root_mark_reason,
                    dependency: marking_record.append_record.dependency,
                    container_note_marked,
                },
            )
        }
        SlotVisitorConservativeRootMarkingAction::NoteLiveAuxiliary => {
            let container_note_marked = SlotVisitorContainerNoteMarkedRecord {
                cell: heap_marking.cell,
                heap_cell_kind: heap_marking.heap_cell_kind,
                byte_size: heap_marking.byte_size,
            };
            SlotVisitorCollectorEffectAction::NoteLiveAuxiliaryCell(
                SlotVisitorNoteLiveAuxiliaryCellRecord {
                    cell: heap_marking.cell,
                    heap_cell_kind: heap_marking.heap_cell_kind,
                    root_mark_reason: marking_record.append_record.root_mark_reason,
                    dependency: marking_record.append_record.dependency,
                    container_note_marked,
                },
            )
        }
    }
}

pub(super) fn validate_p6_arm64_jit_stub_routine_trace_plan(
    collector_effects_plan: &SlotVisitorCollectorEffectsPlan,
    jit_stub_trace_plan: &JitStubRoutineTracePlan,
) -> Result<(), P6Arm64JitStubRoutineTraceProofMismatch> {
    if jit_stub_trace_plan.heap != collector_effects_plan.heap {
        return Err(P6Arm64JitStubRoutineTraceProofMismatch::HeapMismatch {
            collector: collector_effects_plan.heap,
            jit_stub_trace: jit_stub_trace_plan.heap,
        });
    }

    if jit_stub_trace_plan.marking_epoch != collector_effects_plan.marking_epoch {
        return Err(
            P6Arm64JitStubRoutineTraceProofMismatch::MarkingEpochMismatch {
                collector: collector_effects_plan.marking_epoch,
                jit_stub_trace: jit_stub_trace_plan.marking_epoch,
            },
        );
    }

    if jit_stub_trace_plan.worklist != collector_effects_plan.worklist {
        return Err(P6Arm64JitStubRoutineTraceProofMismatch::WorklistMismatch {
            collector: collector_effects_plan.worklist,
            jit_stub_trace: jit_stub_trace_plan.worklist,
        });
    }

    if jit_stub_trace_plan.root_mark_reason != RootMarkReason::JitStubRoutines {
        return Err(
            P6Arm64JitStubRoutineTraceProofMismatch::InvalidRootMarkReason {
                actual: jit_stub_trace_plan.root_mark_reason,
            },
        );
    }

    jit_stub_trace_plan
        .validate_consistency()
        .map_err(P6Arm64JitStubRoutineTraceProofMismatch::TracePlanMismatch)
}

pub(super) fn validate_p6_arm64_machine_stack_conservative_rooting_proof(
    top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
    proof: &P6Arm64MachineStackConservativeRootingProof,
) -> Result<(), P6Arm64MachineStackConservativeRootingProofMismatch> {
    let receipt = proof.conservative_scan_append_receipt();
    if proof.heap != receipt.heap {
        return Err(
            P6Arm64MachineStackConservativeRootingProofMismatch::HeapMismatch {
                receipt: receipt.heap,
                machine_stack: proof.heap,
            },
        );
    }
    if proof.marking_epoch != receipt.epoch {
        return Err(
            P6Arm64MachineStackConservativeRootingProofMismatch::MarkingEpochMismatch {
                receipt: receipt.epoch,
                machine_stack: proof.marking_epoch,
            },
        );
    }
    if proof.phase != receipt.phase {
        return Err(
            P6Arm64MachineStackConservativeRootingProofMismatch::PhaseMismatch {
                receipt: receipt.phase,
                machine_stack: proof.phase,
            },
        );
    }
    if proof.mutator_state != receipt.mutator_state {
        return Err(
            P6Arm64MachineStackConservativeRootingProofMismatch::MutatorStateMismatch {
                receipt: receipt.mutator_state,
                machine_stack: proof.mutator_state,
            },
        );
    }
    if receipt.append_plan.root_mark_reason != RootMarkReason::ConservativeScan {
        return Err(
            P6Arm64MachineStackConservativeRootingProofMismatch::InvalidRootMarkReason {
                actual: receipt.append_plan.root_mark_reason,
            },
        );
    }

    let expected_root_spans = proof
        .machine_stack_spans
        .iter()
        .map(|record| record.span)
        .collect::<Vec<_>>();
    let actual_root_spans = proof.machine_stack_roots.spans().to_vec();
    if expected_root_spans != actual_root_spans {
        return Err(
            P6Arm64MachineStackConservativeRootingProofMismatch::MachineStackRootSpanMismatch {
                expected: expected_root_spans,
                actual: actual_root_spans,
            },
        );
    }

    if !current_thread_spans_begin_register_then_stack(&proof.machine_stack_spans) {
        return Err(
            P6Arm64MachineStackConservativeRootingProofMismatch::CurrentThreadSpanOrderMismatch {
                observed: proof
                    .machine_stack_spans
                    .iter()
                    .map(|record| record.kind)
                    .collect(),
            },
        );
    }

    let publication_address = top_call_frame_publication.publication.published_top_frame;
    if proof.published_top_frame != publication_address {
        return Err(
            P6Arm64MachineStackConservativeRootingProofMismatch::TopCallFrameAddressMismatch {
                publication: publication_address,
                machine_stack: proof.published_top_frame,
            },
        );
    }
    if publication_address.0 % core::mem::size_of::<usize>() != 0 {
        return Err(
            P6Arm64MachineStackConservativeRootingProofMismatch::TopCallFrameAddressUnaligned {
                address: publication_address,
            },
        );
    }
    let Some(actual_top_span) =
        containing_span_kind_for_address(&proof.machine_stack_spans, publication_address.0)
    else {
        return Err(
            P6Arm64MachineStackConservativeRootingProofMismatch::TopCallFrameOutsideScannedSpans {
                address: publication_address,
            },
        );
    };
    if actual_top_span != P6Arm64NativeFrameMachineStackSpanKind::Stack {
        return Err(
            P6Arm64MachineStackConservativeRootingProofMismatch::TopCallFrameContainingSpanMismatch {
                expected: P6Arm64NativeFrameMachineStackSpanKind::Stack,
                actual: actual_top_span,
            },
        );
    }
    if proof.top_call_frame_span != P6Arm64NativeFrameMachineStackSpanKind::Stack {
        return Err(
            P6Arm64MachineStackConservativeRootingProofMismatch::TopCallFrameContainingSpanMismatch {
                expected: P6Arm64NativeFrameMachineStackSpanKind::Stack,
                actual: proof.top_call_frame_span,
            },
        );
    }

    let machine_stack_roots = proof.machine_stack_roots.validated_cells();
    if receipt.appended_record_count < machine_stack_roots.len()
        || receipt.append_plan.records.len() < machine_stack_roots.len()
    {
        return Err(
            P6Arm64MachineStackConservativeRootingProofMismatch::ConservativeScanAppendRecordCountMismatch {
                machine_stack_roots: machine_stack_roots.len(),
                receipt: receipt.appended_record_count,
            },
        );
    }
    for (order, expected_root) in machine_stack_roots.iter().copied().enumerate() {
        let receipt_root = receipt.append_plan.records[order].root;
        if receipt_root != expected_root {
            return Err(
                P6Arm64MachineStackConservativeRootingProofMismatch::ConservativeScanAppendRootMismatch {
                    order,
                    machine_stack: expected_root,
                    receipt: receipt_root,
                },
            );
        }
    }

    Ok(())
}

pub(super) fn validate_p6_arm64_native_frame_machine_stack_residency_proof(
    top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
    receipt: &HeapConservativeScanAppendReceipt,
    residency_proof: &P6Arm64NativeFrameMachineStackResidencyProof,
) -> Result<(), P6Arm64NativeFrameMachineStackResidencyProofMismatch> {
    // C++ `NativeCallFrameTracer` publishes a `CallFrame*` backed by native
    // stack/register state that `MachineThreads::gatherFromCurrentThread` later
    // scans. Rust's publication proof is now stack-local, but it is still
    // descriptor evidence; this proof requires independent machine-stack
    // span/root evidence before progressing to generated-frame materialization.
    if residency_proof.heap != receipt.heap {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::HeapMismatch {
                receipt: receipt.heap,
                machine_stack: residency_proof.heap,
            },
        );
    }
    if residency_proof.marking_epoch != receipt.epoch {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::MarkingEpochMismatch {
                receipt: receipt.epoch,
                machine_stack: residency_proof.marking_epoch,
            },
        );
    }
    if residency_proof.phase != receipt.phase {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::PhaseMismatch {
                receipt: receipt.phase,
                machine_stack: residency_proof.phase,
            },
        );
    }
    if residency_proof.mutator_state != receipt.mutator_state {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::MutatorStateMismatch {
                receipt: receipt.mutator_state,
                machine_stack: residency_proof.mutator_state,
            },
        );
    }
    if residency_proof.conservative_scan_root_mark_reason != RootMarkReason::ConservativeScan {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::InvalidRootMarkReason {
                actual: residency_proof.conservative_scan_root_mark_reason,
            },
        );
    }

    let expected_root_spans = residency_proof
        .machine_stack_spans
        .iter()
        .map(|record| record.span)
        .collect::<Vec<_>>();
    let actual_root_spans = residency_proof.machine_stack_roots.spans().to_vec();
    if expected_root_spans != actual_root_spans {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::MachineStackRootSpanMismatch {
                expected: expected_root_spans,
                actual: actual_root_spans,
            },
        );
    }

    if !current_thread_spans_begin_register_then_stack(&residency_proof.machine_stack_spans) {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::CurrentThreadSpanOrderMismatch {
                observed: residency_proof
                    .machine_stack_spans
                    .iter()
                    .map(|record| record.kind)
                    .collect(),
            },
        );
    }

    let publication_address = top_call_frame_publication.publication.published_top_frame;
    if residency_proof.published_top_frame != publication_address {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::TopCallFrameAddressMismatch {
                publication: publication_address,
                machine_stack: residency_proof.published_top_frame,
            },
        );
    }
    if publication_address.0 % core::mem::size_of::<usize>() != 0 {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::TopCallFrameAddressUnaligned {
                address: publication_address,
            },
        );
    }
    let Some(actual_top_span) = containing_span_kind_for_address(
        &residency_proof.machine_stack_spans,
        publication_address.0,
    ) else {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::TopCallFrameOutsideScannedSpans {
                address: publication_address,
            },
        );
    };
    if actual_top_span != P6Arm64NativeFrameMachineStackSpanKind::Stack {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::TopCallFrameContainingSpanMismatch {
                expected: P6Arm64NativeFrameMachineStackSpanKind::Stack,
                actual: actual_top_span,
            },
        );
    }
    if residency_proof.top_call_frame_span != P6Arm64NativeFrameMachineStackSpanKind::Stack {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::TopCallFrameContainingSpanMismatch {
                expected: P6Arm64NativeFrameMachineStackSpanKind::Stack,
                actual: residency_proof.top_call_frame_span,
            },
        );
    }

    for (order, slot) in residency_proof.slot_records.iter().enumerate() {
        if slot.slot_address % core::mem::size_of::<usize>() != 0 {
            return Err(
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::SlotAddressUnaligned {
                    order,
                    slot_address: slot.slot_address,
                },
            );
        }
        let Some(actual_slot_span) = containing_span_kind_for_word_slot(
            &residency_proof.machine_stack_spans,
            slot.slot_address,
        ) else {
            return Err(
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::SlotAddressOutsideScannedSpans {
                    order,
                    slot_address: slot.slot_address,
                },
            );
        };
        if actual_slot_span != slot.containing_span {
            return Err(
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::SlotContainingSpanMismatch {
                    order,
                    expected: slot.containing_span,
                    actual: actual_slot_span,
                },
            );
        }
        if slot.encoded_payload != slot.expected_root.candidate_address {
            return Err(
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::SlotPayloadRootMismatch {
                    order,
                    encoded_payload: slot.encoded_payload,
                    expected_root: slot.expected_root,
                },
            );
        }
        if !residency_proof
            .machine_stack_roots
            .validated_cells()
            .contains(&slot.expected_root)
        {
            return Err(
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::SlotRootAbsentFromMachineStackRoots {
                    order,
                    root: slot.expected_root,
                },
            );
        }
        if !receipt
            .append_plan
            .records
            .iter()
            .any(|record| record.root == slot.expected_root)
        {
            return Err(
                P6Arm64NativeFrameMachineStackResidencyProofMismatch::SlotRootAbsentFromConservativeScanAppendReceipt {
                    order,
                    root: slot.expected_root,
                },
            );
        }
    }

    Ok(())
}

pub(super) fn validate_p6_arm64_verified_native_frame_machine_stack_residency_proof(
    top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
    machine_stack_conservative_rooting_proof: &P6Arm64MachineStackConservativeRootingProof,
    residency_proof: &P6Arm64VerifiedNativeFrameMachineStackResidencyProof<'_>,
) -> Result<(), P6Arm64NativeFrameMachineStackResidencyProofMismatch> {
    let residency = residency_proof.residency_proof();
    if residency.published_top_frame != machine_stack_conservative_rooting_proof.published_top_frame
    {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::ResidencySourceTopFrameMismatch {
                source: machine_stack_conservative_rooting_proof.published_top_frame,
                residency: residency.published_top_frame,
            },
        );
    }
    if residency.top_call_frame_span != machine_stack_conservative_rooting_proof.top_call_frame_span
    {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::ResidencySourceTopCallFrameSpanMismatch {
                source: machine_stack_conservative_rooting_proof.top_call_frame_span,
                residency: residency.top_call_frame_span,
            },
        );
    }
    if residency.machine_stack_spans != machine_stack_conservative_rooting_proof.machine_stack_spans
    {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::ResidencySourceMachineStackSpansMismatch {
                source: machine_stack_conservative_rooting_proof.machine_stack_spans.clone(),
                residency: residency.machine_stack_spans.clone(),
            },
        );
    }
    if residency.machine_stack_roots != machine_stack_conservative_rooting_proof.machine_stack_roots
    {
        return Err(
            P6Arm64NativeFrameMachineStackResidencyProofMismatch::ResidencySourceMachineStackRootsMismatch {
                source: machine_stack_conservative_rooting_proof.machine_stack_roots.clone(),
                residency: residency.machine_stack_roots.clone(),
            },
        );
    }

    validate_p6_arm64_native_frame_machine_stack_residency_proof(
        top_call_frame_publication,
        machine_stack_conservative_rooting_proof.conservative_scan_append_receipt(),
        residency,
    )
}

pub(super) fn validate_p6_arm64_generated_native_frame_materialization_proof(
    top_call_frame_publication: &P6Arm64BranchAwareCallableTopCallFramePublicationProof<'_>,
    residency_proof: &P6Arm64NativeFrameMachineStackResidencyProof,
    descriptor: &Arm64BaselineGeneratedNativeFrameMaterializationDescriptor,
    expected_live_local_slots: usize,
) -> Result<(), Arm64BaselineGeneratedNativeFrameMaterializationMismatch> {
    if top_call_frame_publication.publication.live_local_count != expected_live_local_slots {
        return Err(
            Arm64BaselineGeneratedNativeFrameMaterializationMismatch::LiveLocalSlotCountMismatch {
                expected: expected_live_local_slots,
                actual: top_call_frame_publication.publication.live_local_count,
            },
        );
    }

    let context = Arm64BaselineGeneratedNativeFrameMaterializationValidationContext {
        published_top_frame: top_call_frame_publication.publication.published_top_frame.0,
        residency_top_frame: residency_proof.published_top_frame.0,
        expected_argument_slots_excluding_this: top_call_frame_publication
            .publication
            .argument_count_excluding_this,
        expected_live_local_slots,
        vm_entry_previous_top_call_frame: top_call_frame_publication
            .publication
            .vm_entry_previous_top_call_frame
            .map(|frame| frame.0),
        vm_entry_previous_top_entry_frame: top_call_frame_publication
            .publication
            .vm_entry_previous_top_entry_frame
            .map(|frame| frame.0),
        current_top_entry_frame: top_call_frame_publication.publication.current_entry_frame.0,
        residency_live_root_slots: residency_proof
            .slot_records
            .iter()
            .map(|slot| {
                let span = containing_span_for_word_slot(
                    &residency_proof.machine_stack_spans,
                    slot.slot_address,
                )
                .unwrap_or(ConservativeRootSpan { begin: 0, end: 0 });
                Arm64BaselineMachineStackRootSlotDescriptor {
                    kind: slot.kind.into(),
                    slot_address: slot.slot_address,
                    encoded_payload: slot.encoded_payload,
                    expected_root: slot.expected_root,
                    containing_span: slot.containing_span.into(),
                    span,
                }
            })
            .collect(),
    };
    validate_arm64_baseline_generated_native_frame_materialization(&context, descriptor)
}

impl From<P6Arm64NativeRootSlotKind> for Arm64BaselineLiveRootSlotKind {
    fn from(kind: P6Arm64NativeRootSlotKind) -> Self {
        match kind {
            P6Arm64NativeRootSlotKind::Callee => Self::Callee,
            P6Arm64NativeRootSlotKind::ThisValue => Self::ThisValue,
            P6Arm64NativeRootSlotKind::Argument => Self::Argument,
            P6Arm64NativeRootSlotKind::Local => Self::Local,
            P6Arm64NativeRootSlotKind::Scratch => Self::Scratch,
        }
    }
}

impl From<P6Arm64NativeFrameMachineStackSpanKind> for Arm64BaselineMachineStackSpanKind {
    fn from(kind: P6Arm64NativeFrameMachineStackSpanKind) -> Self {
        match kind {
            P6Arm64NativeFrameMachineStackSpanKind::RegisterState => Self::RegisterState,
            P6Arm64NativeFrameMachineStackSpanKind::Stack => Self::Stack,
        }
    }
}

fn current_thread_spans_begin_register_then_stack(
    spans: &[P6Arm64NativeFrameMachineStackSpanRecord],
) -> bool {
    matches!(
        spans,
        [
            P6Arm64NativeFrameMachineStackSpanRecord {
                kind: P6Arm64NativeFrameMachineStackSpanKind::RegisterState,
                ..
            },
            P6Arm64NativeFrameMachineStackSpanRecord {
                kind: P6Arm64NativeFrameMachineStackSpanKind::Stack,
                ..
            }
        ]
    )
}

fn containing_span_kind_for_address(
    spans: &[P6Arm64NativeFrameMachineStackSpanRecord],
    address: usize,
) -> Option<P6Arm64NativeFrameMachineStackSpanKind> {
    spans
        .iter()
        .find(|record| address >= record.span.begin && address < record.span.end)
        .map(|record| record.kind)
}

fn containing_span_kind_for_word_slot(
    spans: &[P6Arm64NativeFrameMachineStackSpanRecord],
    address: usize,
) -> Option<P6Arm64NativeFrameMachineStackSpanKind> {
    containing_span_record_for_word_slot(spans, address).map(|record| record.kind)
}

fn containing_span_for_word_slot(
    spans: &[P6Arm64NativeFrameMachineStackSpanRecord],
    address: usize,
) -> Option<ConservativeRootSpan> {
    containing_span_record_for_word_slot(spans, address).map(|record| record.span)
}

fn containing_span_record_for_word_slot(
    spans: &[P6Arm64NativeFrameMachineStackSpanRecord],
    address: usize,
) -> Option<&P6Arm64NativeFrameMachineStackSpanRecord> {
    let end = address.checked_add(core::mem::size_of::<usize>())?;
    spans
        .iter()
        .find(|record| address >= record.span.begin && end <= record.span.end)
}

pub(super) fn validate_p6_arm64_vm_root_gather_plan(
    receipt: &HeapConservativeScanAppendReceipt,
    vm_root_gather_plan: &VmRootGatherPlan,
) -> Result<(), P6Arm64VmRootGatherProofMismatch> {
    if vm_root_gather_plan.heap != receipt.heap {
        return Err(P6Arm64VmRootGatherProofMismatch::HeapMismatch {
            receipt: receipt.heap,
            vm_roots: vm_root_gather_plan.heap,
        });
    }

    if vm_root_gather_plan.marking_epoch != receipt.epoch {
        return Err(P6Arm64VmRootGatherProofMismatch::MarkingEpochMismatch {
            receipt: receipt.epoch,
            vm_roots: vm_root_gather_plan.marking_epoch,
        });
    }

    if receipt.append_plan.root_mark_reason != RootMarkReason::ConservativeScan {
        return Err(
            P6Arm64VmRootGatherProofMismatch::InvalidAppendRootMarkReason {
                actual: receipt.append_plan.root_mark_reason,
            },
        );
    }

    vm_root_gather_plan
        .validate_consistency()
        .map_err(P6Arm64VmRootGatherProofMismatch::GatherPlanMismatch)?;

    // C++ `Heap::addCoreConstraints` gathers VM roots before the
    // `visitor.append(conservativeRoots)` call. This descriptor is consumed
    // later in the Rust proof chain, so require its validated VM-root cells to
    // have already appeared in that conservative-scan append receipt.
    let mut receipt_roots = receipt
        .append_plan
        .records
        .iter()
        .map(|record| record.root)
        .collect::<Vec<_>>();
    for root in vm_root_gather_plan
        .conservative_roots
        .validated_cells()
        .iter()
        .copied()
    {
        if let Some(position) = receipt_roots
            .iter()
            .position(|receipt_root| *receipt_root == root)
        {
            receipt_roots.remove(position);
        } else {
            return Err(P6Arm64VmRootGatherProofMismatch::ReceiptMissingVmRoot { root });
        }
    }

    Ok(())
}

pub(super) fn validate_p6_arm64_verifier_append_proof(
    receipt: &HeapConservativeScanAppendReceipt,
    marking_plan: &SlotVisitorConservativeRootMarkingPlan,
    vm_root_gather_plan: &VmRootGatherPlan,
    verifier_append_proof: &P6Arm64VerifierSlotVisitorConservativeRootAppendProof,
) -> Result<(), P6Arm64VerifierAppendProofMismatch> {
    if verifier_append_proof.heap() != receipt.heap {
        return Err(P6Arm64VerifierAppendProofMismatch::HeapMismatch {
            receipt: receipt.heap,
            verifier: verifier_append_proof.heap(),
        });
    }

    if verifier_append_proof.marking_epoch() != receipt.epoch {
        return Err(P6Arm64VerifierAppendProofMismatch::MarkingEpochMismatch {
            receipt: receipt.epoch,
            verifier: verifier_append_proof.marking_epoch(),
        });
    }

    if verifier_append_proof.heap() != vm_root_gather_plan.heap {
        return Err(
            P6Arm64VerifierAppendProofMismatch::VmRootGatherHeapMismatch {
                vm_roots: vm_root_gather_plan.heap,
                verifier: verifier_append_proof.heap(),
            },
        );
    }

    if verifier_append_proof.marking_epoch() != vm_root_gather_plan.marking_epoch {
        return Err(
            P6Arm64VerifierAppendProofMismatch::VmRootGatherMarkingEpochMismatch {
                vm_roots: vm_root_gather_plan.marking_epoch,
                verifier: verifier_append_proof.marking_epoch(),
            },
        );
    }

    match verifier_append_proof.verifier_append_proof() {
        VerifierSlotVisitorConservativeRootAppendProof::NoVerifierSlotVisitor { .. } => Ok(()),
        VerifierSlotVisitorConservativeRootAppendProof::AppendPlan(verifier_append_plan) => {
            validate_p6_arm64_verifier_append_plan(receipt, marking_plan, verifier_append_plan)
        }
    }
}

fn validate_p6_arm64_verifier_append_plan(
    receipt: &HeapConservativeScanAppendReceipt,
    marking_plan: &SlotVisitorConservativeRootMarkingPlan,
    verifier_append_plan: &VerifierSlotVisitorConservativeRootAppendPlan,
) -> Result<(), P6Arm64VerifierAppendProofMismatch> {
    if verifier_append_plan.root_mark_reason != RootMarkReason::ConservativeScan {
        return Err(P6Arm64VerifierAppendProofMismatch::InvalidRootMarkReason {
            actual: verifier_append_plan.root_mark_reason,
        });
    }

    verifier_append_plan
        .validate_consistency()
        .map_err(P6Arm64VerifierAppendProofMismatch::VerifierPlanMismatch)?;

    if verifier_append_plan.records.len() != receipt.appended_record_count {
        return Err(
            P6Arm64VerifierAppendProofMismatch::VerifierRecordCountMismatch {
                receipt: receipt.appended_record_count,
                verifier: verifier_append_plan.records.len(),
            },
        );
    }

    if verifier_append_plan.records.len() != marking_plan.records.len() {
        return Err(
            P6Arm64VerifierAppendProofMismatch::VerifierMarkingRecordCountMismatch {
                marking: marking_plan.records.len(),
                verifier: verifier_append_plan.records.len(),
            },
        );
    }

    for (order, verifier_record) in verifier_append_plan.records.iter().enumerate() {
        let receipt_record = receipt.append_plan.records[order];
        if verifier_record.append_record != receipt_record {
            return Err(
                P6Arm64VerifierAppendProofMismatch::VerifierAppendRecordMismatch {
                    order,
                    receipt: receipt_record,
                    verifier: verifier_record.append_record,
                },
            );
        }

        let marking_kind = marking_plan.records[order].heap_marking.heap_cell_kind;
        if verifier_record.heap_cell_kind != marking_kind {
            return Err(
                P6Arm64VerifierAppendProofMismatch::VerifierHeapCellKindMismatch {
                    order,
                    marking: marking_kind,
                    verifier: verifier_record.heap_cell_kind,
                },
            );
        }
    }

    Ok(())
}
