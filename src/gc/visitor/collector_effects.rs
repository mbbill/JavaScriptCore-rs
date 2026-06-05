//! C++ `SlotVisitor::appendToMarkStack` / `noteLiveAuxiliaryCell` evidence.
//!
//! This consumes conservative-root marking records after
//! `Heap::testAndSetMarked`. It records the collector effects C++ performs on
//! JSCell state, cell containers, visit counters, and the collector stack.

#![allow(dead_code)]

use crate::gc::{
    CellId, CellState, ConservativeRootCell, GcPhase, Heap, HeapCellKind, HeapEpoch, HeapId,
    HeapMarkingRecord, HeapSemanticError, HeapSemanticOperation, MarkDependency, MarkWorklistId,
    MutatorState, RootMarkReason, SlotVisitorConservativeRootAppendRecord,
    SlotVisitorConservativeRootMarkingAction, SlotVisitorConservativeRootMarkingPlan,
    SlotVisitorConservativeRootMarkingRecord,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SlotVisitorCollectorEffectsPlan {
    pub heap: HeapId,
    pub marking_epoch: HeapEpoch,
    pub worklist: MarkWorklistId,
    pub root_mark_reason: RootMarkReason,
    pub dependency: MarkDependency,
    pub records: Vec<SlotVisitorCollectorEffectRecord>,
    pub js_cell_state_update_count: usize,
    pub container_note_marked_count: usize,
    pub mark_stack_append_count: usize,
    pub live_auxiliary_count: usize,
    pub already_marked_count: usize,
    pub visit_count_delta: usize,
    pub bytes_visited_delta: usize,
    pub non_cell_visit_count_delta: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SlotVisitorCollectorEffectRecord {
    pub order: usize,
    pub marking_record: SlotVisitorConservativeRootMarkingRecord,
    pub action: SlotVisitorCollectorEffectAction,
    pub visit_count_delta: usize,
    pub bytes_visited_delta: usize,
    pub non_cell_visit_count_delta: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SlotVisitorCollectorEffectAction {
    AlreadyMarkedReturn,
    AppendToMarkStack(SlotVisitorAppendToMarkStackRecord),
    NoteLiveAuxiliaryCell(SlotVisitorNoteLiveAuxiliaryCellRecord),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SlotVisitorAppendToMarkStackRecord {
    pub cell: CellId,
    pub heap_cell_kind: HeapCellKind,
    pub cell_state: CellState,
    pub worklist: MarkWorklistId,
    pub root_mark_reason: RootMarkReason,
    pub dependency: MarkDependency,
    pub container_note_marked: SlotVisitorContainerNoteMarkedRecord,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SlotVisitorNoteLiveAuxiliaryCellRecord {
    pub cell: CellId,
    pub heap_cell_kind: HeapCellKind,
    pub root_mark_reason: RootMarkReason,
    pub dependency: MarkDependency,
    pub container_note_marked: SlotVisitorContainerNoteMarkedRecord,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SlotVisitorContainerNoteMarkedRecord {
    pub cell: CellId,
    pub heap_cell_kind: HeapCellKind,
    pub byte_size: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SlotVisitorCollectorEffectsError {
    HeapSemantic(HeapSemanticError),
    HeapMismatch {
        plan: HeapId,
        heap: HeapId,
    },
    MarkingEpochMismatch {
        plan: HeapEpoch,
        heap: HeapEpoch,
    },
    InvalidRootMarkReason {
        actual: RootMarkReason,
    },
    InvalidDependency {
        actual: MarkDependency,
    },
    RecordOrderMismatch {
        expected: usize,
        actual: usize,
    },
    RecordHeapMismatch {
        order: usize,
        plan: HeapId,
        record: HeapId,
    },
    RecordMarkingEpochMismatch {
        order: usize,
        plan: HeapEpoch,
        record: HeapEpoch,
    },
    RecordWorklistMismatch {
        order: usize,
        plan: MarkWorklistId,
        record: MarkWorklistId,
    },
    RecordRootMarkReasonMismatch {
        order: usize,
        plan: RootMarkReason,
        record: RootMarkReason,
    },
    RecordDependencyMismatch {
        order: usize,
        plan: MarkDependency,
        record: MarkDependency,
    },
    AppendRecordCellMismatch {
        order: usize,
        root: CellId,
        record: CellId,
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

impl From<HeapSemanticError> for SlotVisitorCollectorEffectsError {
    fn from(error: HeapSemanticError) -> Self {
        Self::HeapSemantic(error)
    }
}

impl SlotVisitorConservativeRootMarkingPlan {
    pub(crate) fn apply_collector_effects(
        self,
        heap: &mut Heap,
    ) -> Result<SlotVisitorCollectorEffectsPlan, SlotVisitorCollectorEffectsError> {
        let state = heap.state_descriptor();
        if state.phase == GcPhase::NotRunning {
            return Err(HeapSemanticError::WrongPhase {
                operation: HeapSemanticOperation::TraceRoots,
                phase: state.phase,
            }
            .into());
        }
        if state.mutator_state != MutatorState::Collecting {
            return Err(HeapSemanticError::MutatorMustBeStopped {
                operation: HeapSemanticOperation::TraceRoots,
                mutator_state: state.mutator_state,
            }
            .into());
        }

        let SlotVisitorConservativeRootMarkingPlan {
            heap: plan_heap,
            marking_epoch,
            worklist,
            root_mark_reason,
            dependency,
            records: marking_records,
            queued_js_cell_count,
            live_auxiliary_count,
            already_marked_count,
            visit_count_delta,
            bytes_visited_delta,
            non_cell_visit_count_delta,
        } = self;

        if plan_heap != heap.id() {
            return Err(SlotVisitorCollectorEffectsError::HeapMismatch {
                plan: plan_heap,
                heap: heap.id(),
            });
        }

        if marking_epoch != heap.epoch() {
            return Err(SlotVisitorCollectorEffectsError::MarkingEpochMismatch {
                plan: marking_epoch,
                heap: heap.epoch(),
            });
        }

        if root_mark_reason != RootMarkReason::ConservativeScan {
            return Err(SlotVisitorCollectorEffectsError::InvalidRootMarkReason {
                actual: root_mark_reason,
            });
        }

        if dependency != MarkDependency::Conservative {
            return Err(SlotVisitorCollectorEffectsError::InvalidDependency { actual: dependency });
        }

        let mut records = Vec::with_capacity(marking_records.len());
        let mut computed_queued_js_cell_count = 0;
        let mut computed_live_auxiliary_count = 0;
        let mut computed_already_marked_count = 0;
        let mut computed_visit_count_delta = 0;
        let mut computed_bytes_visited_delta = 0;
        let mut computed_non_cell_visit_count_delta = 0;

        for (expected_order, marking_record) in marking_records.into_iter().enumerate() {
            let expected = validate_marking_record(
                expected_order,
                plan_heap,
                marking_epoch,
                worklist,
                root_mark_reason,
                dependency,
                &marking_record,
            )?;

            let action = match marking_record.action {
                SlotVisitorConservativeRootMarkingAction::AlreadyMarked => {
                    computed_already_marked_count += 1;
                    SlotVisitorCollectorEffectAction::AlreadyMarkedReturn
                }
                SlotVisitorConservativeRootMarkingAction::QueueJsCell {
                    cell_state,
                    worklist,
                } => {
                    computed_queued_js_cell_count += 1;
                    let container_note_marked = SlotVisitorContainerNoteMarkedRecord {
                        cell: marking_record.heap_marking.cell,
                        heap_cell_kind: marking_record.heap_marking.heap_cell_kind,
                        byte_size: marking_record.heap_marking.byte_size,
                    };
                    // C++ calls `container.noteMarked()` before appending the
                    // JSCell. Rust records that call and the JSCell header
                    // state update as collector evidence until real
                    // MarkedBlock/PreciseAllocation storage and JSCell header
                    // mutation exist. `PreciseAllocation::noteMarked()` is a
                    // no-op in C++; this proof records the call site, not a
                    // per-container mark-count side effect.
                    SlotVisitorCollectorEffectAction::AppendToMarkStack(
                        SlotVisitorAppendToMarkStackRecord {
                            cell: marking_record.heap_marking.cell,
                            heap_cell_kind: marking_record.heap_marking.heap_cell_kind,
                            cell_state,
                            worklist,
                            root_mark_reason,
                            dependency,
                            container_note_marked,
                        },
                    )
                }
                SlotVisitorConservativeRootMarkingAction::NoteLiveAuxiliary => {
                    computed_live_auxiliary_count += 1;
                    let container_note_marked = SlotVisitorContainerNoteMarkedRecord {
                        cell: marking_record.heap_marking.cell,
                        heap_cell_kind: marking_record.heap_marking.heap_cell_kind,
                        byte_size: marking_record.heap_marking.byte_size,
                    };
                    SlotVisitorCollectorEffectAction::NoteLiveAuxiliaryCell(
                        SlotVisitorNoteLiveAuxiliaryCellRecord {
                            cell: marking_record.heap_marking.cell,
                            heap_cell_kind: marking_record.heap_marking.heap_cell_kind,
                            root_mark_reason,
                            dependency,
                            container_note_marked,
                        },
                    )
                }
            };

            computed_visit_count_delta += expected.visit_count_delta;
            computed_bytes_visited_delta += expected.bytes_visited_delta;
            computed_non_cell_visit_count_delta += expected.non_cell_visit_count_delta;

            records.push(SlotVisitorCollectorEffectRecord {
                order: expected_order,
                marking_record,
                action,
                visit_count_delta: expected.visit_count_delta,
                bytes_visited_delta: expected.bytes_visited_delta,
                non_cell_visit_count_delta: expected.non_cell_visit_count_delta,
            });
        }

        if queued_js_cell_count != computed_queued_js_cell_count {
            return Err(
                SlotVisitorCollectorEffectsError::QueuedJsCellCountMismatch {
                    expected: computed_queued_js_cell_count,
                    actual: queued_js_cell_count,
                },
            );
        }

        if live_auxiliary_count != computed_live_auxiliary_count {
            return Err(
                SlotVisitorCollectorEffectsError::LiveAuxiliaryCountMismatch {
                    expected: computed_live_auxiliary_count,
                    actual: live_auxiliary_count,
                },
            );
        }

        if already_marked_count != computed_already_marked_count {
            return Err(
                SlotVisitorCollectorEffectsError::AlreadyMarkedCountMismatch {
                    expected: computed_already_marked_count,
                    actual: already_marked_count,
                },
            );
        }

        if visit_count_delta != computed_visit_count_delta {
            return Err(SlotVisitorCollectorEffectsError::VisitCountTotalMismatch {
                expected: computed_visit_count_delta,
                actual: visit_count_delta,
            });
        }

        if bytes_visited_delta != computed_bytes_visited_delta {
            return Err(
                SlotVisitorCollectorEffectsError::BytesVisitedTotalMismatch {
                    expected: computed_bytes_visited_delta,
                    actual: bytes_visited_delta,
                },
            );
        }

        if non_cell_visit_count_delta != computed_non_cell_visit_count_delta {
            return Err(
                SlotVisitorCollectorEffectsError::NonCellVisitCountTotalMismatch {
                    expected: computed_non_cell_visit_count_delta,
                    actual: non_cell_visit_count_delta,
                },
            );
        }

        Ok(SlotVisitorCollectorEffectsPlan {
            heap: plan_heap,
            marking_epoch,
            worklist,
            root_mark_reason,
            dependency,
            js_cell_state_update_count: computed_queued_js_cell_count,
            container_note_marked_count: computed_queued_js_cell_count
                + computed_live_auxiliary_count,
            mark_stack_append_count: computed_queued_js_cell_count,
            live_auxiliary_count: computed_live_auxiliary_count,
            already_marked_count: computed_already_marked_count,
            visit_count_delta: computed_visit_count_delta,
            bytes_visited_delta: computed_bytes_visited_delta,
            non_cell_visit_count_delta: computed_non_cell_visit_count_delta,
            records,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExpectedCollectorEffect {
    action: SlotVisitorConservativeRootMarkingAction,
    visit_count_delta: usize,
    bytes_visited_delta: usize,
    non_cell_visit_count_delta: usize,
}

fn validate_marking_record(
    expected_order: usize,
    plan_heap: HeapId,
    marking_epoch: HeapEpoch,
    worklist: MarkWorklistId,
    root_mark_reason: RootMarkReason,
    dependency: MarkDependency,
    record: &SlotVisitorConservativeRootMarkingRecord,
) -> Result<ExpectedCollectorEffect, SlotVisitorCollectorEffectsError> {
    let append_record = record.append_record;
    validate_append_record(
        expected_order,
        plan_heap,
        marking_epoch,
        worklist,
        root_mark_reason,
        dependency,
        append_record,
    )?;
    validate_heap_marking(
        expected_order,
        plan_heap,
        marking_epoch,
        append_record,
        record,
    )?;

    let heap_marking = record.heap_marking;
    let expected = expected_collector_effect(heap_marking, worklist);

    if record.action != expected.action {
        return Err(SlotVisitorCollectorEffectsError::MarkingActionMismatch {
            order: expected_order,
            expected: expected.action,
            actual: record.action,
        });
    }

    if record.visit_count_delta != expected.visit_count_delta {
        return Err(SlotVisitorCollectorEffectsError::VisitCountDeltaMismatch {
            order: expected_order,
            expected: expected.visit_count_delta,
            actual: record.visit_count_delta,
        });
    }

    if record.bytes_visited_delta != expected.bytes_visited_delta {
        return Err(
            SlotVisitorCollectorEffectsError::BytesVisitedDeltaMismatch {
                order: expected_order,
                expected: expected.bytes_visited_delta,
                actual: record.bytes_visited_delta,
            },
        );
    }

    if record.non_cell_visit_count_delta != expected.non_cell_visit_count_delta {
        return Err(
            SlotVisitorCollectorEffectsError::NonCellVisitCountDeltaMismatch {
                order: expected_order,
                expected: expected.non_cell_visit_count_delta,
                actual: record.non_cell_visit_count_delta,
            },
        );
    }

    Ok(expected)
}

fn validate_append_record(
    expected_order: usize,
    plan_heap: HeapId,
    marking_epoch: HeapEpoch,
    worklist: MarkWorklistId,
    root_mark_reason: RootMarkReason,
    dependency: MarkDependency,
    record: SlotVisitorConservativeRootAppendRecord,
) -> Result<(), SlotVisitorCollectorEffectsError> {
    if record.order != expected_order {
        return Err(SlotVisitorCollectorEffectsError::RecordOrderMismatch {
            expected: expected_order,
            actual: record.order,
        });
    }

    if record.heap != plan_heap {
        return Err(SlotVisitorCollectorEffectsError::RecordHeapMismatch {
            order: expected_order,
            plan: plan_heap,
            record: record.heap,
        });
    }

    if record.marking_epoch != marking_epoch {
        return Err(
            SlotVisitorCollectorEffectsError::RecordMarkingEpochMismatch {
                order: expected_order,
                plan: marking_epoch,
                record: record.marking_epoch,
            },
        );
    }

    if record.worklist != worklist {
        return Err(SlotVisitorCollectorEffectsError::RecordWorklistMismatch {
            order: expected_order,
            plan: worklist,
            record: record.worklist,
        });
    }

    if record.root_mark_reason != root_mark_reason {
        return Err(
            SlotVisitorCollectorEffectsError::RecordRootMarkReasonMismatch {
                order: expected_order,
                plan: root_mark_reason,
                record: record.root_mark_reason,
            },
        );
    }

    if record.dependency != dependency {
        return Err(SlotVisitorCollectorEffectsError::RecordDependencyMismatch {
            order: expected_order,
            plan: dependency,
            record: record.dependency,
        });
    }

    if record.root.cell != record.cell {
        return Err(SlotVisitorCollectorEffectsError::AppendRecordCellMismatch {
            order: expected_order,
            root: record.root.cell,
            record: record.cell,
        });
    }

    Ok(())
}

fn validate_heap_marking(
    order: usize,
    plan_heap: HeapId,
    marking_epoch: HeapEpoch,
    append_record: SlotVisitorConservativeRootAppendRecord,
    record: &SlotVisitorConservativeRootMarkingRecord,
) -> Result<(), SlotVisitorCollectorEffectsError> {
    let heap_marking = record.heap_marking;
    if heap_marking.heap != plan_heap {
        return Err(SlotVisitorCollectorEffectsError::HeapMarkingHeapMismatch {
            order,
            plan: plan_heap,
            marking: heap_marking.heap,
        });
    }

    if heap_marking.marking_epoch != marking_epoch {
        return Err(SlotVisitorCollectorEffectsError::HeapMarkingEpochMismatch {
            order,
            plan: marking_epoch,
            marking: heap_marking.marking_epoch,
        });
    }

    if heap_marking.root != append_record.root {
        return Err(SlotVisitorCollectorEffectsError::HeapMarkingRootMismatch {
            order,
            append: append_record.root,
            marking: heap_marking.root,
        });
    }

    if heap_marking.cell != append_record.cell {
        return Err(SlotVisitorCollectorEffectsError::HeapMarkingCellMismatch {
            order,
            append: append_record.cell,
            marking: heap_marking.cell,
        });
    }

    Ok(())
}

fn expected_collector_effect(
    heap_marking: HeapMarkingRecord,
    worklist: MarkWorklistId,
) -> ExpectedCollectorEffect {
    if heap_marking.already_marked {
        return ExpectedCollectorEffect {
            action: SlotVisitorConservativeRootMarkingAction::AlreadyMarked,
            visit_count_delta: 0,
            bytes_visited_delta: 0,
            non_cell_visit_count_delta: 0,
        };
    }

    match heap_marking.heap_cell_kind {
        HeapCellKind::JsCell | HeapCellKind::JsCellWithIndexingHeader => ExpectedCollectorEffect {
            action: SlotVisitorConservativeRootMarkingAction::QueueJsCell {
                cell_state: CellState::PossiblyGrey,
                worklist,
            },
            visit_count_delta: 1,
            bytes_visited_delta: heap_marking.byte_size,
            non_cell_visit_count_delta: 0,
        },
        HeapCellKind::Auxiliary => ExpectedCollectorEffect {
            action: SlotVisitorConservativeRootMarkingAction::NoteLiveAuxiliary,
            visit_count_delta: 1,
            bytes_visited_delta: heap_marking.byte_size,
            non_cell_visit_count_delta: heap_marking.byte_size,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{
        AllocationMode, CellMetadata, ConservativeRoots, GcConductor, GcPhase,
        HeapAllocationRequest, HeapSemanticError, MutatorState, ReferrerToken, ReferrerTokenKind,
        SlotVisitorDescriptor,
    };

    fn metadata(kind: HeapCellKind) -> CellMetadata {
        CellMetadata {
            heap_cell_kind: kind,
            ..CellMetadata::default()
        }
    }

    fn allocate_root(
        heap: &mut Heap,
        kind: HeapCellKind,
        payload: usize,
        byte_size: usize,
    ) -> CellId {
        let cell = heap
            .allocate_record(HeapAllocationRequest {
                heap: heap.id(),
                subspace: "slot-visitor-collector-effects-test",
                metadata: metadata(kind),
                byte_size,
                mode: AllocationMode::Normal,
                may_trigger_collection: false,
            })
            .map(|response| response.cell)
            .expect("test allocation");
        heap.bind_cell_payload(cell, payload).expect("bind payload");
        heap.publish_cell(cell).expect("publish cell");
        cell
    }

    fn ingest_root(heap: &mut Heap, payload: usize) {
        let mut roots = ConservativeRoots::new();
        roots.add_validated_cell(
            heap.validate_conservative_root_candidate_exact_payload(payload)
                .expect("validated root"),
        );
        heap.ingest_conservative_roots(roots)
            .expect("ingest conservative root");
    }

    fn enter_collecting(heap: &mut Heap) {
        heap.enter_phase(
            GcPhase::Fixpoint,
            MutatorState::Collecting,
            GcConductor::Mutator,
        );
    }

    fn append_plan(heap: &Heap) -> crate::gc::SlotVisitorConservativeRootAppendPlan {
        let mut visitor = SlotVisitorDescriptor::new(
            heap.id(),
            "slot-visitor-collector-effects-test",
            heap.epoch(),
        );
        visitor.worklist = MarkWorklistId(91);
        visitor.root_mark_reason = RootMarkReason::ConservativeScan;
        visitor.current_referrer = Some(ReferrerToken {
            kind: ReferrerTokenKind::RootMarkReason,
            address: 0,
            root_mark_reason: RootMarkReason::ConservativeScan,
        });
        visitor
            .append_conservative_roots_descriptor(
                &heap.conservative_roots(),
                heap.id(),
                heap.epoch(),
            )
            .expect("append descriptor")
    }

    fn marking_plan_for(
        heap: &mut Heap,
        kind: HeapCellKind,
        payload: usize,
        byte_size: usize,
    ) -> SlotVisitorConservativeRootMarkingPlan {
        allocate_root(heap, kind, payload, byte_size);
        ingest_root(heap, payload);
        enter_collecting(heap);
        append_plan(heap)
            .mark_conservative_roots(heap)
            .expect("conservative marking plan")
    }

    #[test]
    fn collector_effects_append_js_cell_to_mark_stack_records_state_container_and_counters() {
        let mut heap = Heap::new();
        let marking_plan = marking_plan_for(&mut heap, HeapCellKind::JsCell, 0x1000, 64);
        let cell = marking_plan.records[0].heap_marking.cell;
        let effects = marking_plan
            .apply_collector_effects(&mut heap)
            .expect("collector effects");

        assert_eq!(effects.js_cell_state_update_count, 1);
        assert_eq!(effects.container_note_marked_count, 1);
        assert_eq!(effects.mark_stack_append_count, 1);
        assert_eq!(effects.live_auxiliary_count, 0);
        assert_eq!(effects.already_marked_count, 0);
        assert_eq!(effects.visit_count_delta, 1);
        assert_eq!(effects.bytes_visited_delta, 64);
        assert_eq!(effects.non_cell_visit_count_delta, 0);
        assert_eq!(
            effects.records[0].action,
            SlotVisitorCollectorEffectAction::AppendToMarkStack(
                SlotVisitorAppendToMarkStackRecord {
                    cell,
                    heap_cell_kind: HeapCellKind::JsCell,
                    cell_state: CellState::PossiblyGrey,
                    worklist: MarkWorklistId(91),
                    root_mark_reason: RootMarkReason::ConservativeScan,
                    dependency: MarkDependency::Conservative,
                    container_note_marked: SlotVisitorContainerNoteMarkedRecord {
                        cell,
                        heap_cell_kind: HeapCellKind::JsCell,
                        byte_size: 64,
                    },
                }
            )
        );
    }

    #[test]
    fn collector_effects_note_live_auxiliary_records_container_and_non_cell_bytes() {
        let mut heap = Heap::new();
        let marking_plan = marking_plan_for(&mut heap, HeapCellKind::Auxiliary, 0x2000, 128);
        let cell = marking_plan.records[0].heap_marking.cell;
        let effects = marking_plan
            .apply_collector_effects(&mut heap)
            .expect("collector effects");

        assert_eq!(effects.js_cell_state_update_count, 0);
        assert_eq!(effects.container_note_marked_count, 1);
        assert_eq!(effects.mark_stack_append_count, 0);
        assert_eq!(effects.live_auxiliary_count, 1);
        assert_eq!(effects.already_marked_count, 0);
        assert_eq!(effects.visit_count_delta, 1);
        assert_eq!(effects.bytes_visited_delta, 128);
        assert_eq!(effects.non_cell_visit_count_delta, 128);
        assert_eq!(
            effects.records[0].action,
            SlotVisitorCollectorEffectAction::NoteLiveAuxiliaryCell(
                SlotVisitorNoteLiveAuxiliaryCellRecord {
                    cell,
                    heap_cell_kind: HeapCellKind::Auxiliary,
                    root_mark_reason: RootMarkReason::ConservativeScan,
                    dependency: MarkDependency::Conservative,
                    container_note_marked: SlotVisitorContainerNoteMarkedRecord {
                        cell,
                        heap_cell_kind: HeapCellKind::Auxiliary,
                        byte_size: 128,
                    },
                }
            )
        );
    }

    #[test]
    fn collector_effects_already_marked_returns_before_state_container_stack_and_counters() {
        let mut heap = Heap::new();
        let cell = allocate_root(
            &mut heap,
            HeapCellKind::JsCellWithIndexingHeader,
            0x3000,
            96,
        );
        ingest_root(&mut heap, 0x3000);
        enter_collecting(&mut heap);
        let append_plan = append_plan(&heap);
        let first = append_plan
            .clone()
            .mark_conservative_roots(&mut heap)
            .expect("first marking");
        assert_eq!(
            first.records[0].action,
            SlotVisitorConservativeRootMarkingAction::QueueJsCell {
                cell_state: CellState::PossiblyGrey,
                worklist: MarkWorklistId(91),
            }
        );

        let second = append_plan
            .mark_conservative_roots(&mut heap)
            .expect("second marking");
        let effects = second
            .apply_collector_effects(&mut heap)
            .expect("collector effects");

        assert_eq!(effects.records.len(), 1);
        assert_eq!(effects.records[0].marking_record.heap_marking.cell, cell);
        assert_eq!(
            effects.records[0].action,
            SlotVisitorCollectorEffectAction::AlreadyMarkedReturn
        );
        assert_eq!(effects.js_cell_state_update_count, 0);
        assert_eq!(effects.container_note_marked_count, 0);
        assert_eq!(effects.mark_stack_append_count, 0);
        assert_eq!(effects.live_auxiliary_count, 0);
        assert_eq!(effects.already_marked_count, 1);
        assert_eq!(effects.visit_count_delta, 0);
        assert_eq!(effects.bytes_visited_delta, 0);
        assert_eq!(effects.non_cell_visit_count_delta, 0);
    }

    #[test]
    fn collector_effects_rejects_inconsistent_marking_action_and_totals() {
        let mut heap = Heap::new();
        let mut wrong_action = marking_plan_for(&mut heap, HeapCellKind::JsCell, 0x4000, 64);
        wrong_action.records[0].action = SlotVisitorConservativeRootMarkingAction::AlreadyMarked;
        assert_eq!(
            wrong_action.apply_collector_effects(&mut heap),
            Err(SlotVisitorCollectorEffectsError::MarkingActionMismatch {
                order: 0,
                expected: SlotVisitorConservativeRootMarkingAction::QueueJsCell {
                    cell_state: CellState::PossiblyGrey,
                    worklist: MarkWorklistId(91),
                },
                actual: SlotVisitorConservativeRootMarkingAction::AlreadyMarked,
            })
        );

        let mut heap = Heap::new();
        let mut wrong_total = marking_plan_for(&mut heap, HeapCellKind::Auxiliary, 0x5000, 128);
        wrong_total.live_auxiliary_count = 0;
        assert_eq!(
            wrong_total.apply_collector_effects(&mut heap),
            Err(
                SlotVisitorCollectorEffectsError::LiveAuxiliaryCountMismatch {
                    expected: 1,
                    actual: 0,
                }
            )
        );
    }

    #[test]
    fn collector_effects_rejects_non_collecting_heap_state_before_effects() {
        let mut heap = Heap::new();
        let mut collecting_heap = Heap::new();
        let marking_plan = marking_plan_for(&mut collecting_heap, HeapCellKind::JsCell, 0x6000, 64);

        assert_eq!(
            marking_plan.clone().apply_collector_effects(&mut heap),
            Err(SlotVisitorCollectorEffectsError::HeapSemantic(
                HeapSemanticError::WrongPhase {
                    operation: HeapSemanticOperation::TraceRoots,
                    phase: GcPhase::NotRunning,
                }
            ))
        );

        heap.enter_phase(
            GcPhase::Fixpoint,
            MutatorState::Running,
            GcConductor::Mutator,
        );
        assert_eq!(
            marking_plan.apply_collector_effects(&mut heap),
            Err(SlotVisitorCollectorEffectsError::HeapSemantic(
                HeapSemanticError::MutatorMustBeStopped {
                    operation: HeapSemanticOperation::TraceRoots,
                    mutator_state: MutatorState::Running,
                }
            ))
        );
    }
}
