//! ARM64 native-entry GC/rooting proof support.
//!
//! This is an extraction-only Rust organization boundary for the C++
//! `Heap::addCoreConstraints` native-entry rooting chain. It keeps
//! `native_reentry.rs` below the oversized-file guardrail while preserving
//! the same descriptor validation behavior; it does not add engine behavior,
//! an admission `Ok` path, verifier mark-map storage, verifier drain, or real
//! native rooting.

use crate::gc::{
    CellId, CellState, ConservativeRootCell, HeapCellKind, HeapConservativeScanAppendReceipt,
    HeapEpoch, HeapId, MarkDependency, MarkWorklistId, RootMarkReason,
    SlotVisitorAppendToMarkStackRecord, SlotVisitorCollectorEffectAction,
    SlotVisitorCollectorEffectsPlan, SlotVisitorConservativeRootAppendRecord,
    SlotVisitorConservativeRootMarkingAction, SlotVisitorConservativeRootMarkingPlan,
    SlotVisitorContainerNoteMarkedRecord, SlotVisitorNoteLiveAuxiliaryCellRecord,
    VerifierSlotVisitorConservativeRootAppendError, VerifierSlotVisitorConservativeRootAppendPlan,
    VerifierSlotVisitorConservativeRootAppendProof,
};
use crate::jit::{JitStubRoutineTraceError, JitStubRoutineTracePlan};

use super::super::entry::VmNativeCallFramePublicationRecord;
use super::super::vm_roots::{VmRootGatherError, VmRootGatherPlan};

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct P6Arm64BranchAwareCallableTopCallFramePublicationProof {
    // C++ JSC stores a raw CallFrame* in VM::topCallFrame; this Rust evidence
    // is tied to the symbolic publication record from `entry.rs`, so it proves
    // top-frame metadata exists but not that conservative machine-stack roots
    // can see generated ARM64 state.
    pub(in crate::vm) publication: VmNativeCallFramePublicationRecord,
}

impl P6Arm64BranchAwareCallableTopCallFramePublicationProof {
    #[allow(dead_code)]
    pub(in crate::vm) const fn from_publication_record(
        publication: VmNativeCallFramePublicationRecord,
    ) -> Self {
        Self { publication }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::vm) enum P6Arm64BranchAwareCallableFallbackRootingProof {
    MissingTopCallFramePublication,
    TopCallFramePublicationWithoutConservativeScanAppend(
        P6Arm64BranchAwareCallableTopCallFramePublicationProof,
    ),
    TopCallFramePublicationWithConservativeScanAppendReceipt {
        top_call_frame_publication: P6Arm64BranchAwareCallableTopCallFramePublicationProof,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
    },
    TopCallFramePublicationWithConservativeRootMarkingPlan {
        top_call_frame_publication: P6Arm64BranchAwareCallableTopCallFramePublicationProof,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        conservative_root_marking_plan: SlotVisitorConservativeRootMarkingPlan,
    },
    TopCallFramePublicationWithCollectorEffectsPlan {
        top_call_frame_publication: P6Arm64BranchAwareCallableTopCallFramePublicationProof,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        conservative_root_marking_plan: SlotVisitorConservativeRootMarkingPlan,
        collector_effects_plan: SlotVisitorCollectorEffectsPlan,
    },
    TopCallFramePublicationWithCollectorEffectsAndJitStubTracePlan {
        top_call_frame_publication: P6Arm64BranchAwareCallableTopCallFramePublicationProof,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        conservative_root_marking_plan: SlotVisitorConservativeRootMarkingPlan,
        collector_effects_plan: SlotVisitorCollectorEffectsPlan,
        jit_stub_trace_plan: JitStubRoutineTracePlan,
    },
    TopCallFramePublicationWithCollectorEffectsJitStubTraceAndVmRootGatherPlan {
        top_call_frame_publication: P6Arm64BranchAwareCallableTopCallFramePublicationProof,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        conservative_root_marking_plan: SlotVisitorConservativeRootMarkingPlan,
        collector_effects_plan: SlotVisitorCollectorEffectsPlan,
        jit_stub_trace_plan: JitStubRoutineTracePlan,
        vm_root_gather_plan: VmRootGatherPlan,
    },
    TopCallFramePublicationWithCollectorEffectsJitStubTraceVmRootGatherAndVerifierAppendProof {
        top_call_frame_publication: P6Arm64BranchAwareCallableTopCallFramePublicationProof,
        conservative_scan_append_receipt: HeapConservativeScanAppendReceipt,
        conservative_root_marking_plan: SlotVisitorConservativeRootMarkingPlan,
        collector_effects_plan: SlotVisitorCollectorEffectsPlan,
        jit_stub_trace_plan: JitStubRoutineTracePlan,
        vm_root_gather_plan: VmRootGatherPlan,
        verifier_append_proof: VerifierSlotVisitorConservativeRootAppendProof,
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
pub(in crate::vm) enum P6Arm64VmRootGatherProofMismatch {
    HeapMismatch {
        receipt: HeapId,
        vm_roots: HeapId,
    },
    MarkingEpochMismatch {
        receipt: HeapEpoch,
        vm_roots: HeapEpoch,
    },
    JitStubTraceHeapMismatch {
        jit_stub_trace: HeapId,
        vm_roots: HeapId,
    },
    JitStubTraceMarkingEpochMismatch {
        jit_stub_trace: HeapEpoch,
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

pub(super) fn validate_p6_arm64_vm_root_gather_plan(
    receipt: &HeapConservativeScanAppendReceipt,
    jit_stub_trace_plan: &JitStubRoutineTracePlan,
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

    if vm_root_gather_plan.heap != jit_stub_trace_plan.heap {
        return Err(P6Arm64VmRootGatherProofMismatch::JitStubTraceHeapMismatch {
            jit_stub_trace: jit_stub_trace_plan.heap,
            vm_roots: vm_root_gather_plan.heap,
        });
    }

    if vm_root_gather_plan.marking_epoch != jit_stub_trace_plan.marking_epoch {
        return Err(
            P6Arm64VmRootGatherProofMismatch::JitStubTraceMarkingEpochMismatch {
                jit_stub_trace: jit_stub_trace_plan.marking_epoch,
                vm_roots: vm_root_gather_plan.marking_epoch,
            },
        );
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
    verifier_append_proof: &VerifierSlotVisitorConservativeRootAppendProof,
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

    match verifier_append_proof {
        VerifierSlotVisitorConservativeRootAppendProof::NoVerifierSlotVisitor {
            heap,
            marking_epoch,
        } => {
            // C++ skips `VerifierSlotVisitor::append(conservativeRoots)` only
            // when the heap has no verifier visitor installed. Rust has no
            // heap-owned verifier-state proof yet, so this native admission
            // path requires the append plan instead of accepting absence.
            Err(
                P6Arm64VerifierAppendProofMismatch::MissingVerifierAppendPlan {
                    heap: *heap,
                    marking_epoch: *marking_epoch,
                },
            )
        }
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
