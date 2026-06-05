//! C++ `SlotVisitor::appendJSCellOrAuxiliary` marking evidence.
//!
//! This consumes conservative-root append descriptors and records the collector
//! actions that C++ performs after `Heap::testAndSetMarked`.

#![allow(dead_code)]

use crate::gc::{
    CellId, CellState, Heap, HeapCellKind, HeapEpoch, HeapId, HeapMarkingError, HeapMarkingRecord,
    MarkDependency, MarkWorklistId, RootMarkReason, SlotVisitorConservativeRootAppendPlan,
    SlotVisitorConservativeRootAppendRecord,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SlotVisitorConservativeRootMarkingPlan {
    pub heap: HeapId,
    pub marking_epoch: HeapEpoch,
    pub worklist: MarkWorklistId,
    pub root_mark_reason: RootMarkReason,
    pub dependency: MarkDependency,
    pub records: Vec<SlotVisitorConservativeRootMarkingRecord>,
    pub queued_js_cell_count: usize,
    pub live_auxiliary_count: usize,
    pub already_marked_count: usize,
    pub visit_count_delta: usize,
    pub bytes_visited_delta: usize,
    pub non_cell_visit_count_delta: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SlotVisitorConservativeRootMarkingRecord {
    pub append_record: SlotVisitorConservativeRootAppendRecord,
    pub heap_marking: HeapMarkingRecord,
    pub action: SlotVisitorConservativeRootMarkingAction,
    pub visit_count_delta: usize,
    pub bytes_visited_delta: usize,
    pub non_cell_visit_count_delta: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SlotVisitorConservativeRootMarkingAction {
    AlreadyMarked,
    QueueJsCell {
        cell_state: CellState,
        worklist: MarkWorklistId,
    },
    NoteLiveAuxiliary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SlotVisitorConservativeRootMarkingError {
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
    RecordCellMismatch {
        order: usize,
        root: CellId,
        record: CellId,
    },
    HeapMarking(HeapMarkingError),
}

impl From<HeapMarkingError> for SlotVisitorConservativeRootMarkingError {
    fn from(error: HeapMarkingError) -> Self {
        Self::HeapMarking(error)
    }
}

impl SlotVisitorConservativeRootAppendPlan {
    pub(crate) fn mark_conservative_roots(
        self,
        heap: &mut Heap,
    ) -> Result<SlotVisitorConservativeRootMarkingPlan, SlotVisitorConservativeRootMarkingError>
    {
        let SlotVisitorConservativeRootAppendPlan {
            heap: plan_heap,
            marking_epoch,
            worklist,
            root_mark_reason,
            dependency,
            referrer: _,
            records: append_records,
        } = self;

        if plan_heap != heap.id() {
            return Err(SlotVisitorConservativeRootMarkingError::HeapMismatch {
                plan: plan_heap,
                heap: heap.id(),
            });
        }

        if marking_epoch != heap.epoch() {
            return Err(
                SlotVisitorConservativeRootMarkingError::MarkingEpochMismatch {
                    plan: marking_epoch,
                    heap: heap.epoch(),
                },
            );
        }

        if root_mark_reason != RootMarkReason::ConservativeScan {
            return Err(
                SlotVisitorConservativeRootMarkingError::InvalidRootMarkReason {
                    actual: root_mark_reason,
                },
            );
        }

        if dependency != MarkDependency::Conservative {
            return Err(SlotVisitorConservativeRootMarkingError::InvalidDependency {
                actual: dependency,
            });
        }

        let mut records = Vec::with_capacity(append_records.len());
        let mut queued_js_cell_count = 0;
        let mut live_auxiliary_count = 0;
        let mut already_marked_count = 0;
        let mut visit_count_delta = 0;
        let mut bytes_visited_delta = 0;
        let mut non_cell_visit_count_delta = 0;

        for (expected_order, append_record) in append_records.into_iter().enumerate() {
            validate_append_record(
                expected_order,
                plan_heap,
                marking_epoch,
                worklist,
                root_mark_reason,
                dependency,
                append_record,
            )?;

            let heap_marking =
                heap.test_and_set_marked_for_conservative_root_append_record(&append_record)?;

            let (action, record_visit_delta, record_bytes_delta, record_non_cell_delta) =
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
                            // C++ sets `JSCell::m_cellState` to
                            // `PossiblyGrey` and appends the cell to the
                            // collector stack. Rust has no per-allocation
                            // header store or mark stack yet, so this action
                            // is the collector-owned evidence for both effects.
                            (
                                SlotVisitorConservativeRootMarkingAction::QueueJsCell {
                                    cell_state: CellState::PossiblyGrey,
                                    worklist,
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

            visit_count_delta += record_visit_delta;
            bytes_visited_delta += record_bytes_delta;
            non_cell_visit_count_delta += record_non_cell_delta;
            records.push(SlotVisitorConservativeRootMarkingRecord {
                append_record,
                heap_marking,
                action,
                visit_count_delta: record_visit_delta,
                bytes_visited_delta: record_bytes_delta,
                non_cell_visit_count_delta: record_non_cell_delta,
            });
        }

        Ok(SlotVisitorConservativeRootMarkingPlan {
            heap: plan_heap,
            marking_epoch,
            worklist,
            root_mark_reason,
            dependency,
            records,
            queued_js_cell_count,
            live_auxiliary_count,
            already_marked_count,
            visit_count_delta,
            bytes_visited_delta,
            non_cell_visit_count_delta,
        })
    }
}

fn validate_append_record(
    expected_order: usize,
    plan_heap: HeapId,
    marking_epoch: HeapEpoch,
    worklist: MarkWorklistId,
    root_mark_reason: RootMarkReason,
    dependency: MarkDependency,
    record: SlotVisitorConservativeRootAppendRecord,
) -> Result<(), SlotVisitorConservativeRootMarkingError> {
    if record.order != expected_order {
        return Err(
            SlotVisitorConservativeRootMarkingError::RecordOrderMismatch {
                expected: expected_order,
                actual: record.order,
            },
        );
    }

    if record.heap != plan_heap {
        return Err(
            SlotVisitorConservativeRootMarkingError::RecordHeapMismatch {
                order: expected_order,
                plan: plan_heap,
                record: record.heap,
            },
        );
    }

    if record.marking_epoch != marking_epoch {
        return Err(
            SlotVisitorConservativeRootMarkingError::RecordMarkingEpochMismatch {
                order: expected_order,
                plan: marking_epoch,
                record: record.marking_epoch,
            },
        );
    }

    if record.worklist != worklist {
        return Err(
            SlotVisitorConservativeRootMarkingError::RecordWorklistMismatch {
                order: expected_order,
                plan: worklist,
                record: record.worklist,
            },
        );
    }

    if record.root_mark_reason != root_mark_reason {
        return Err(
            SlotVisitorConservativeRootMarkingError::RecordRootMarkReasonMismatch {
                order: expected_order,
                plan: root_mark_reason,
                record: record.root_mark_reason,
            },
        );
    }

    if record.dependency != dependency {
        return Err(
            SlotVisitorConservativeRootMarkingError::RecordDependencyMismatch {
                order: expected_order,
                plan: dependency,
                record: record.dependency,
            },
        );
    }

    if record.root.cell != record.cell {
        return Err(
            SlotVisitorConservativeRootMarkingError::RecordCellMismatch {
                order: expected_order,
                root: record.root.cell,
                record: record.cell,
            },
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{
        AllocationMode, CellMetadata, ConservativeRoots, GcConductor, GcPhase,
        HeapAllocationRequest, HeapSemanticError, HeapSemanticOperation, MutatorState,
        ReferrerToken, ReferrerTokenKind, SlotVisitorConservativeRootAppendError,
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
                subspace: "slot-visitor-conservative-marking-test",
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

    fn append_plan(heap: &Heap) -> SlotVisitorConservativeRootAppendPlan {
        let mut visitor = heap.slot_visitor_descriptor("slot-visitor-conservative-marking-test");
        visitor.worklist = MarkWorklistId(77);
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

    #[test]
    fn conservative_marking_queues_js_cells_notes_auxiliary_and_preserves_order() {
        let mut heap = Heap::new();
        let js_cell = allocate_root(&mut heap, HeapCellKind::JsCell, 0x1000, 64);
        let indexed = allocate_root(
            &mut heap,
            HeapCellKind::JsCellWithIndexingHeader,
            0x2000,
            96,
        );
        let auxiliary = allocate_root(&mut heap, HeapCellKind::Auxiliary, 0x3000, 128);
        ingest_root(&mut heap, 0x1000);
        ingest_root(&mut heap, 0x2000);
        ingest_root(&mut heap, 0x3000);
        enter_collecting(&mut heap);

        let plan = append_plan(&heap);
        let marking = plan
            .clone()
            .mark_conservative_roots(&mut heap)
            .expect("first conservative marking");

        assert_eq!(marking.records.len(), 3);
        assert_eq!(marking.queued_js_cell_count, 2);
        assert_eq!(marking.live_auxiliary_count, 1);
        assert_eq!(marking.already_marked_count, 0);
        assert_eq!(marking.visit_count_delta, 3);
        assert_eq!(marking.bytes_visited_delta, 64 + 96 + 128);
        assert_eq!(marking.non_cell_visit_count_delta, 128);
        assert_eq!(marking.records[0].append_record.cell, js_cell);
        assert_eq!(marking.records[1].append_record.cell, indexed);
        assert_eq!(marking.records[2].append_record.cell, auxiliary);
        assert_eq!(
            marking.records[0].action,
            SlotVisitorConservativeRootMarkingAction::QueueJsCell {
                cell_state: CellState::PossiblyGrey,
                worklist: MarkWorklistId(77)
            }
        );
        assert_eq!(
            marking.records[1].action,
            SlotVisitorConservativeRootMarkingAction::QueueJsCell {
                cell_state: CellState::PossiblyGrey,
                worklist: MarkWorklistId(77)
            }
        );
        assert_eq!(
            marking.records[2].action,
            SlotVisitorConservativeRootMarkingAction::NoteLiveAuxiliary
        );

        let second = plan
            .mark_conservative_roots(&mut heap)
            .expect("second conservative marking");
        assert_eq!(second.queued_js_cell_count, 0);
        assert_eq!(second.live_auxiliary_count, 0);
        assert_eq!(second.already_marked_count, 3);
        assert_eq!(second.visit_count_delta, 0);
        assert_eq!(second.bytes_visited_delta, 0);
        assert_eq!(second.non_cell_visit_count_delta, 0);
        assert!(second.records.iter().all(|record| {
            record.action == SlotVisitorConservativeRootMarkingAction::AlreadyMarked
                && record.visit_count_delta == 0
                && record.bytes_visited_delta == 0
                && record.non_cell_visit_count_delta == 0
        }));
        assert_eq!(second.records[0].append_record.cell, js_cell);
        assert_eq!(second.records[1].append_record.cell, indexed);
        assert_eq!(second.records[2].append_record.cell, auxiliary);
    }

    #[test]
    fn conservative_marking_rejects_non_collecting_heap_state_before_marking() {
        let mut heap = Heap::new();
        let _cell = allocate_root(&mut heap, HeapCellKind::JsCell, 0x1000, 64);
        ingest_root(&mut heap, 0x1000);
        let plan = append_plan(&heap);

        assert_eq!(
            plan.clone().mark_conservative_roots(&mut heap),
            Err(SlotVisitorConservativeRootMarkingError::HeapMarking(
                HeapMarkingError::HeapSemantic(HeapSemanticError::WrongPhase {
                    operation: HeapSemanticOperation::TraceRoots,
                    phase: GcPhase::NotRunning
                })
            ))
        );

        heap.enter_phase(
            GcPhase::Fixpoint,
            MutatorState::Running,
            GcConductor::Mutator,
        );
        assert_eq!(
            plan.mark_conservative_roots(&mut heap),
            Err(SlotVisitorConservativeRootMarkingError::HeapMarking(
                HeapMarkingError::HeapSemantic(HeapSemanticError::MutatorMustBeStopped {
                    operation: HeapSemanticOperation::TraceRoots,
                    mutator_state: MutatorState::Running
                })
            ))
        );
    }

    #[test]
    fn conservative_marking_rejects_wrong_heap_epoch_and_unknown_cell() {
        let mut heap = Heap::new();
        let _cell = allocate_root(&mut heap, HeapCellKind::JsCell, 0x1000, 64);
        ingest_root(&mut heap, 0x1000);
        enter_collecting(&mut heap);
        let plan = append_plan(&heap);

        let mut wrong_heap = plan.clone();
        wrong_heap.heap = HeapId(99);
        assert_eq!(
            wrong_heap.mark_conservative_roots(&mut heap),
            Err(SlotVisitorConservativeRootMarkingError::HeapMismatch {
                plan: HeapId(99),
                heap: heap.id()
            })
        );

        let mut wrong_epoch = plan.clone();
        wrong_epoch.marking_epoch = HeapEpoch(heap.epoch().0 + 1);
        assert_eq!(
            wrong_epoch.mark_conservative_roots(&mut heap),
            Err(
                SlotVisitorConservativeRootMarkingError::MarkingEpochMismatch {
                    plan: HeapEpoch(heap.epoch().0 + 1),
                    heap: heap.epoch()
                }
            )
        );

        let mut unknown = plan.clone();
        unknown.records[0].cell = CellId(99);
        unknown.records[0].root.cell = CellId(99);
        assert_eq!(
            unknown.mark_conservative_roots(&mut heap),
            Err(SlotVisitorConservativeRootMarkingError::HeapMarking(
                HeapMarkingError::UnknownCell(CellId(99))
            ))
        );
    }

    #[test]
    fn conservative_marking_rejects_non_conservative_append_plans() {
        let mut heap = Heap::new();
        let _cell = allocate_root(&mut heap, HeapCellKind::JsCell, 0x1000, 64);
        ingest_root(&mut heap, 0x1000);
        enter_collecting(&mut heap);

        let mut invalid_reason = append_plan(&heap);
        invalid_reason.root_mark_reason = RootMarkReason::StrongHandles;
        assert_eq!(
            invalid_reason.mark_conservative_roots(&mut heap),
            Err(
                SlotVisitorConservativeRootMarkingError::InvalidRootMarkReason {
                    actual: RootMarkReason::StrongHandles
                }
            )
        );

        let mut invalid_dependency = append_plan(&heap);
        invalid_dependency.dependency = MarkDependency::Strong;
        assert_eq!(
            invalid_dependency.mark_conservative_roots(&mut heap),
            Err(SlotVisitorConservativeRootMarkingError::InvalidDependency {
                actual: MarkDependency::Strong
            })
        );
    }

    #[test]
    fn conservative_marking_rejects_record_context_mismatches_before_heap_marking() {
        let mut heap = Heap::new();
        let _cell = allocate_root(&mut heap, HeapCellKind::JsCell, 0x1000, 64);
        ingest_root(&mut heap, 0x1000);
        enter_collecting(&mut heap);

        let mut wrong_order = append_plan(&heap);
        wrong_order.records[0].order = 1;
        assert_eq!(
            wrong_order.mark_conservative_roots(&mut heap),
            Err(
                SlotVisitorConservativeRootMarkingError::RecordOrderMismatch {
                    expected: 0,
                    actual: 1
                }
            )
        );

        let mut wrong_cell = append_plan(&heap);
        wrong_cell.records[0].cell = CellId(2);
        let root_cell = wrong_cell.records[0].root.cell;
        assert_eq!(
            wrong_cell.mark_conservative_roots(&mut heap),
            Err(
                SlotVisitorConservativeRootMarkingError::RecordCellMismatch {
                    order: 0,
                    root: root_cell,
                    record: CellId(2)
                }
            )
        );
    }

    #[test]
    fn conservative_marking_still_uses_append_descriptor_validation_for_bad_visitors() {
        let heap = Heap::new();
        let mut visitor = SlotVisitorDescriptor::new(heap.id(), "slot-visitor-test", heap.epoch());
        visitor.root_mark_reason = RootMarkReason::StrongHandles;

        assert_eq!(
            visitor.append_conservative_roots_descriptor(
                &heap.conservative_roots(),
                heap.id(),
                heap.epoch(),
            ),
            Err(
                SlotVisitorConservativeRootAppendError::InvalidRootMarkReason {
                    actual: RootMarkReason::StrongHandles
                }
            )
        );
    }
}
