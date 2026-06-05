//! C++ `VerifierSlotVisitor::append(const ConservativeRoots&)` evidence.
//!
//! C++ uses `VerifierSlotVisitor` as a separate visitor with verifier-owned
//! precise-allocation / marked-block mark maps and its own `m_collectorStack`.
//! This Rust module is descriptor-only: it replays verifier test-and-set facts
//! over the same conservative-root append records, but it does not implement
//! `MarkedBlock::verifierMemo`, the precise-allocation verifier map, stack
//! traces, verifier child tracing, or verifier drain.

#![allow(dead_code)]

use crate::gc::{
    CellId, HeapCellKind, HeapEpoch, HeapId, MarkDependency, MarkWorklistId, RootMarkReason,
    SlotVisitorConservativeRootAppendRecord, SlotVisitorConservativeRootMarkingPlan,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifierSlotVisitorDescriptor {
    pub heap: HeapId,
    pub marking_epoch: HeapEpoch,
    pub worklist: MarkWorklistId,
    pub root_mark_reason: RootMarkReason,
    pub initially_marked_cells: Vec<CellId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum VerifierSlotVisitorConservativeRootAppendProof {
    NoVerifierSlotVisitor {
        heap: HeapId,
        marking_epoch: HeapEpoch,
    },
    AppendPlan(VerifierSlotVisitorConservativeRootAppendPlan),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifierSlotVisitorConservativeRootAppendPlan {
    pub heap: HeapId,
    pub marking_epoch: HeapEpoch,
    pub worklist: MarkWorklistId,
    pub root_mark_reason: RootMarkReason,
    pub initially_marked_cells: Vec<CellId>,
    pub source_record_count: usize,
    pub records: Vec<VerifierSlotVisitorConservativeRootAppendRecord>,
    pub test_and_set_count: usize,
    pub newly_marked_count: usize,
    pub already_marked_count: usize,
    pub collector_stack_append_count: usize,
    pub auxiliary_mark_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VerifierSlotVisitorConservativeRootAppendRecord {
    pub order: usize,
    pub append_record: SlotVisitorConservativeRootAppendRecord,
    pub heap_cell_kind: HeapCellKind,
    pub test_and_set: VerifierSlotVisitorTestAndSetMarkRecord,
    pub action: VerifierSlotVisitorConservativeRootAppendAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VerifierSlotVisitorTestAndSetMarkRecord {
    pub cell: CellId,
    pub heap_cell_kind: HeapCellKind,
    pub was_marked: bool,
    pub marked_after: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VerifierSlotVisitorConservativeRootAppendAction {
    AlreadyMarkedReturn,
    AppendToCollectorStack(VerifierSlotVisitorCollectorStackAppendRecord),
    MarkAuxiliaryOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VerifierSlotVisitorCollectorStackAppendRecord {
    pub cell: CellId,
    pub heap_cell_kind: HeapCellKind,
    pub worklist: MarkWorklistId,
    pub root_mark_reason: RootMarkReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VerifierSlotVisitorConservativeRootAppendError {
    HeapMismatch {
        visitor: HeapId,
        marking: HeapId,
    },
    MarkingEpochMismatch {
        visitor: HeapEpoch,
        marking: HeapEpoch,
    },
    InvalidRootMarkReason {
        actual: RootMarkReason,
    },
    InvalidMarkingRootMarkReason {
        actual: RootMarkReason,
    },
    InvalidDependency {
        actual: MarkDependency,
    },
    DuplicateInitiallyMarkedCell {
        cell: CellId,
    },
    SourceRecordCountMismatch {
        expected: usize,
        actual: usize,
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
    RecordRootMarkReasonMismatch {
        order: usize,
        plan: RootMarkReason,
        record: RootMarkReason,
    },
    RecordDependencyMismatch {
        order: usize,
        expected: MarkDependency,
        actual: MarkDependency,
    },
    RecordCellMismatch {
        order: usize,
        root: CellId,
        record: CellId,
    },
    TestAndSetCellMismatch {
        order: usize,
        append: CellId,
        test_and_set: CellId,
    },
    TestAndSetKindMismatch {
        order: usize,
        record: HeapCellKind,
        test_and_set: HeapCellKind,
    },
    TestAndSetWasMarkedMismatch {
        order: usize,
        expected: bool,
        actual: bool,
    },
    TestAndSetMarkedAfterMismatch {
        order: usize,
        actual: bool,
    },
    ActionMismatch {
        order: usize,
        expected: VerifierSlotVisitorConservativeRootAppendAction,
        actual: VerifierSlotVisitorConservativeRootAppendAction,
    },
    TestAndSetCountMismatch {
        expected: usize,
        actual: usize,
    },
    NewlyMarkedCountMismatch {
        expected: usize,
        actual: usize,
    },
    AlreadyMarkedCountMismatch {
        expected: usize,
        actual: usize,
    },
    CollectorStackAppendCountMismatch {
        expected: usize,
        actual: usize,
    },
    AuxiliaryMarkCountMismatch {
        expected: usize,
        actual: usize,
    },
}

impl VerifierSlotVisitorDescriptor {
    pub(crate) const fn new(heap: HeapId, marking_epoch: HeapEpoch) -> Self {
        Self {
            heap,
            marking_epoch,
            worklist: MarkWorklistId(0),
            root_mark_reason: RootMarkReason::None,
            initially_marked_cells: Vec::new(),
        }
    }

    pub(crate) fn append_conservative_roots_from_marking_plan(
        &self,
        marking_plan: &SlotVisitorConservativeRootMarkingPlan,
    ) -> Result<
        VerifierSlotVisitorConservativeRootAppendPlan,
        VerifierSlotVisitorConservativeRootAppendError,
    > {
        if self.heap != marking_plan.heap {
            return Err(
                VerifierSlotVisitorConservativeRootAppendError::HeapMismatch {
                    visitor: self.heap,
                    marking: marking_plan.heap,
                },
            );
        }

        if self.marking_epoch != marking_plan.marking_epoch {
            return Err(
                VerifierSlotVisitorConservativeRootAppendError::MarkingEpochMismatch {
                    visitor: self.marking_epoch,
                    marking: marking_plan.marking_epoch,
                },
            );
        }

        if self.root_mark_reason != RootMarkReason::ConservativeScan {
            return Err(
                VerifierSlotVisitorConservativeRootAppendError::InvalidRootMarkReason {
                    actual: self.root_mark_reason,
                },
            );
        }

        if marking_plan.root_mark_reason != RootMarkReason::ConservativeScan {
            return Err(
                VerifierSlotVisitorConservativeRootAppendError::InvalidMarkingRootMarkReason {
                    actual: marking_plan.root_mark_reason,
                },
            );
        }

        if marking_plan.dependency != MarkDependency::Conservative {
            return Err(
                VerifierSlotVisitorConservativeRootAppendError::InvalidDependency {
                    actual: marking_plan.dependency,
                },
            );
        }

        reject_duplicate_initial_marks(&self.initially_marked_cells)?;

        let mut replayed_marks = self.initially_marked_cells.clone();
        let mut records = Vec::with_capacity(marking_plan.records.len());

        for (order, marking_record) in marking_plan.records.iter().enumerate() {
            let append_record = marking_record.append_record;
            validate_append_record(
                order,
                self.heap,
                self.marking_epoch,
                self.root_mark_reason,
                append_record,
            )?;

            // C++ re-reads `HeapCell::cellKind()` from real heap storage while
            // appending verifier roots. Rust has no verifier-visible HeapCell
            // header storage yet, so this descriptor borrows only the cell-kind
            // classification already proven for the same conservative root.
            let heap_cell_kind = marking_record.heap_marking.heap_cell_kind;
            let was_marked = replayed_marks.contains(&append_record.cell);
            if !was_marked {
                replayed_marks.push(append_record.cell);
            }

            let test_and_set = VerifierSlotVisitorTestAndSetMarkRecord {
                cell: append_record.cell,
                heap_cell_kind,
                was_marked,
                marked_after: true,
            };
            let action = expected_action_for(
                heap_cell_kind,
                append_record.cell,
                was_marked,
                self.worklist,
                self.root_mark_reason,
            );
            records.push(VerifierSlotVisitorConservativeRootAppendRecord {
                order,
                append_record,
                heap_cell_kind,
                test_and_set,
                action,
            });
        }

        let mut plan = VerifierSlotVisitorConservativeRootAppendPlan {
            heap: self.heap,
            marking_epoch: self.marking_epoch,
            worklist: self.worklist,
            root_mark_reason: self.root_mark_reason,
            initially_marked_cells: self.initially_marked_cells.clone(),
            source_record_count: marking_plan.records.len(),
            records,
            test_and_set_count: 0,
            newly_marked_count: 0,
            already_marked_count: 0,
            collector_stack_append_count: 0,
            auxiliary_mark_count: 0,
        };
        plan.recompute_counts();
        plan.validate_consistency()?;
        Ok(plan)
    }
}

impl VerifierSlotVisitorConservativeRootAppendProof {
    pub(crate) const fn heap(&self) -> HeapId {
        match self {
            Self::NoVerifierSlotVisitor { heap, .. } => *heap,
            Self::AppendPlan(plan) => plan.heap,
        }
    }

    pub(crate) const fn marking_epoch(&self) -> HeapEpoch {
        match self {
            Self::NoVerifierSlotVisitor { marking_epoch, .. } => *marking_epoch,
            Self::AppendPlan(plan) => plan.marking_epoch,
        }
    }
}

impl VerifierSlotVisitorConservativeRootAppendPlan {
    pub(crate) fn validate_consistency(
        &self,
    ) -> Result<(), VerifierSlotVisitorConservativeRootAppendError> {
        if self.root_mark_reason != RootMarkReason::ConservativeScan {
            return Err(
                VerifierSlotVisitorConservativeRootAppendError::InvalidRootMarkReason {
                    actual: self.root_mark_reason,
                },
            );
        }

        reject_duplicate_initial_marks(&self.initially_marked_cells)?;

        if self.source_record_count != self.records.len() {
            return Err(
                VerifierSlotVisitorConservativeRootAppendError::SourceRecordCountMismatch {
                    expected: self.records.len(),
                    actual: self.source_record_count,
                },
            );
        }

        let mut replayed_marks = self.initially_marked_cells.clone();
        let mut test_and_set_count = 0;
        let mut newly_marked_count = 0;
        let mut already_marked_count = 0;
        let mut collector_stack_append_count = 0;
        let mut auxiliary_mark_count = 0;

        for (order, record) in self.records.iter().enumerate() {
            if record.order != order {
                return Err(
                    VerifierSlotVisitorConservativeRootAppendError::RecordOrderMismatch {
                        expected: order,
                        actual: record.order,
                    },
                );
            }

            validate_append_record(
                order,
                self.heap,
                self.marking_epoch,
                self.root_mark_reason,
                record.append_record,
            )?;

            if record.test_and_set.cell != record.append_record.cell {
                return Err(
                    VerifierSlotVisitorConservativeRootAppendError::TestAndSetCellMismatch {
                        order,
                        append: record.append_record.cell,
                        test_and_set: record.test_and_set.cell,
                    },
                );
            }

            if record.test_and_set.heap_cell_kind != record.heap_cell_kind {
                return Err(
                    VerifierSlotVisitorConservativeRootAppendError::TestAndSetKindMismatch {
                        order,
                        record: record.heap_cell_kind,
                        test_and_set: record.test_and_set.heap_cell_kind,
                    },
                );
            }

            let expected_was_marked = replayed_marks.contains(&record.append_record.cell);
            if record.test_and_set.was_marked != expected_was_marked {
                return Err(
                    VerifierSlotVisitorConservativeRootAppendError::TestAndSetWasMarkedMismatch {
                        order,
                        expected: expected_was_marked,
                        actual: record.test_and_set.was_marked,
                    },
                );
            }

            if !record.test_and_set.marked_after {
                return Err(
                    VerifierSlotVisitorConservativeRootAppendError::TestAndSetMarkedAfterMismatch {
                        order,
                        actual: record.test_and_set.marked_after,
                    },
                );
            }

            if !expected_was_marked {
                replayed_marks.push(record.append_record.cell);
            }

            let expected_action = expected_action_for(
                record.heap_cell_kind,
                record.append_record.cell,
                expected_was_marked,
                self.worklist,
                self.root_mark_reason,
            );
            if record.action != expected_action {
                return Err(
                    VerifierSlotVisitorConservativeRootAppendError::ActionMismatch {
                        order,
                        expected: expected_action,
                        actual: record.action,
                    },
                );
            }

            test_and_set_count += 1;
            match expected_action {
                VerifierSlotVisitorConservativeRootAppendAction::AlreadyMarkedReturn => {
                    already_marked_count += 1;
                }
                VerifierSlotVisitorConservativeRootAppendAction::AppendToCollectorStack(_) => {
                    newly_marked_count += 1;
                    collector_stack_append_count += 1;
                }
                VerifierSlotVisitorConservativeRootAppendAction::MarkAuxiliaryOnly => {
                    newly_marked_count += 1;
                    auxiliary_mark_count += 1;
                }
            }
        }

        if self.test_and_set_count != test_and_set_count {
            return Err(
                VerifierSlotVisitorConservativeRootAppendError::TestAndSetCountMismatch {
                    expected: test_and_set_count,
                    actual: self.test_and_set_count,
                },
            );
        }

        if self.newly_marked_count != newly_marked_count {
            return Err(
                VerifierSlotVisitorConservativeRootAppendError::NewlyMarkedCountMismatch {
                    expected: newly_marked_count,
                    actual: self.newly_marked_count,
                },
            );
        }

        if self.already_marked_count != already_marked_count {
            return Err(
                VerifierSlotVisitorConservativeRootAppendError::AlreadyMarkedCountMismatch {
                    expected: already_marked_count,
                    actual: self.already_marked_count,
                },
            );
        }

        if self.collector_stack_append_count != collector_stack_append_count {
            return Err(
                VerifierSlotVisitorConservativeRootAppendError::CollectorStackAppendCountMismatch {
                    expected: collector_stack_append_count,
                    actual: self.collector_stack_append_count,
                },
            );
        }

        if self.auxiliary_mark_count != auxiliary_mark_count {
            return Err(
                VerifierSlotVisitorConservativeRootAppendError::AuxiliaryMarkCountMismatch {
                    expected: auxiliary_mark_count,
                    actual: self.auxiliary_mark_count,
                },
            );
        }

        Ok(())
    }

    fn recompute_counts(&mut self) {
        self.test_and_set_count = self.records.len();
        self.newly_marked_count = self
            .records
            .iter()
            .filter(|record| !record.test_and_set.was_marked)
            .count();
        self.already_marked_count = self
            .records
            .iter()
            .filter(|record| record.test_and_set.was_marked)
            .count();
        self.collector_stack_append_count = self
            .records
            .iter()
            .filter(|record| {
                matches!(
                    record.action,
                    VerifierSlotVisitorConservativeRootAppendAction::AppendToCollectorStack(_)
                )
            })
            .count();
        self.auxiliary_mark_count = self
            .records
            .iter()
            .filter(|record| {
                matches!(
                    record.action,
                    VerifierSlotVisitorConservativeRootAppendAction::MarkAuxiliaryOnly
                )
            })
            .count();
    }
}

fn validate_append_record(
    order: usize,
    plan_heap: HeapId,
    marking_epoch: HeapEpoch,
    root_mark_reason: RootMarkReason,
    append_record: SlotVisitorConservativeRootAppendRecord,
) -> Result<(), VerifierSlotVisitorConservativeRootAppendError> {
    if append_record.heap != plan_heap {
        return Err(
            VerifierSlotVisitorConservativeRootAppendError::RecordHeapMismatch {
                order,
                plan: plan_heap,
                record: append_record.heap,
            },
        );
    }

    if append_record.marking_epoch != marking_epoch {
        return Err(
            VerifierSlotVisitorConservativeRootAppendError::RecordMarkingEpochMismatch {
                order,
                plan: marking_epoch,
                record: append_record.marking_epoch,
            },
        );
    }

    if append_record.root_mark_reason != root_mark_reason {
        return Err(
            VerifierSlotVisitorConservativeRootAppendError::RecordRootMarkReasonMismatch {
                order,
                plan: root_mark_reason,
                record: append_record.root_mark_reason,
            },
        );
    }

    if append_record.dependency != MarkDependency::Conservative {
        return Err(
            VerifierSlotVisitorConservativeRootAppendError::RecordDependencyMismatch {
                order,
                expected: MarkDependency::Conservative,
                actual: append_record.dependency,
            },
        );
    }

    if append_record.root.cell != append_record.cell {
        return Err(
            VerifierSlotVisitorConservativeRootAppendError::RecordCellMismatch {
                order,
                root: append_record.root.cell,
                record: append_record.cell,
            },
        );
    }

    Ok(())
}

fn reject_duplicate_initial_marks(
    initially_marked_cells: &[CellId],
) -> Result<(), VerifierSlotVisitorConservativeRootAppendError> {
    for (index, cell) in initially_marked_cells.iter().copied().enumerate() {
        if initially_marked_cells[..index].contains(&cell) {
            return Err(
                VerifierSlotVisitorConservativeRootAppendError::DuplicateInitiallyMarkedCell {
                    cell,
                },
            );
        }
    }
    Ok(())
}

fn expected_action_for(
    heap_cell_kind: HeapCellKind,
    cell: CellId,
    was_marked: bool,
    worklist: MarkWorklistId,
    root_mark_reason: RootMarkReason,
) -> VerifierSlotVisitorConservativeRootAppendAction {
    if was_marked {
        return VerifierSlotVisitorConservativeRootAppendAction::AlreadyMarkedReturn;
    }

    match heap_cell_kind {
        HeapCellKind::JsCell | HeapCellKind::JsCellWithIndexingHeader => {
            VerifierSlotVisitorConservativeRootAppendAction::AppendToCollectorStack(
                VerifierSlotVisitorCollectorStackAppendRecord {
                    cell,
                    heap_cell_kind,
                    worklist,
                    root_mark_reason,
                },
            )
        }
        HeapCellKind::Auxiliary => {
            VerifierSlotVisitorConservativeRootAppendAction::MarkAuxiliaryOnly
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{
        CellState, ConservativeRootCell, HeapMarkingRecord,
        SlotVisitorConservativeRootAppendRecord, SlotVisitorConservativeRootMarkingAction,
        SlotVisitorConservativeRootMarkingRecord,
    };

    const HEAP: HeapId = HeapId(7);
    const EPOCH: HeapEpoch = HeapEpoch(11);
    const WORKLIST: MarkWorklistId = MarkWorklistId(19);

    fn verifier() -> VerifierSlotVisitorDescriptor {
        let mut verifier = VerifierSlotVisitorDescriptor::new(HEAP, EPOCH);
        verifier.worklist = WORKLIST;
        verifier.root_mark_reason = RootMarkReason::ConservativeScan;
        verifier
    }

    fn marking_plan_for(
        records: Vec<SlotVisitorConservativeRootMarkingRecord>,
    ) -> SlotVisitorConservativeRootMarkingPlan {
        SlotVisitorConservativeRootMarkingPlan {
            heap: HEAP,
            marking_epoch: EPOCH,
            worklist: MarkWorklistId(5),
            root_mark_reason: RootMarkReason::ConservativeScan,
            dependency: MarkDependency::Conservative,
            queued_js_cell_count: records
                .iter()
                .filter(|record| {
                    matches!(
                        record.action,
                        SlotVisitorConservativeRootMarkingAction::QueueJsCell { .. }
                    )
                })
                .count(),
            live_auxiliary_count: records
                .iter()
                .filter(|record| {
                    record.action == SlotVisitorConservativeRootMarkingAction::NoteLiveAuxiliary
                })
                .count(),
            already_marked_count: records
                .iter()
                .filter(|record| {
                    record.action == SlotVisitorConservativeRootMarkingAction::AlreadyMarked
                })
                .count(),
            visit_count_delta: 0,
            bytes_visited_delta: 0,
            non_cell_visit_count_delta: 0,
            records,
        }
    }

    fn marking_record(
        order: usize,
        cell: CellId,
        heap_cell_kind: HeapCellKind,
    ) -> SlotVisitorConservativeRootMarkingRecord {
        let root = ConservativeRootCell {
            candidate_address: 0x1000 + order,
            cell,
        };
        let append_record = SlotVisitorConservativeRootAppendRecord {
            order,
            root,
            cell,
            heap: HEAP,
            marking_epoch: EPOCH,
            worklist: MarkWorklistId(5),
            root_mark_reason: RootMarkReason::ConservativeScan,
            dependency: MarkDependency::Conservative,
            referrer: None,
        };
        let action = match heap_cell_kind {
            HeapCellKind::JsCell | HeapCellKind::JsCellWithIndexingHeader => {
                SlotVisitorConservativeRootMarkingAction::QueueJsCell {
                    cell_state: CellState::PossiblyGrey,
                    worklist: MarkWorklistId(5),
                }
            }
            HeapCellKind::Auxiliary => SlotVisitorConservativeRootMarkingAction::NoteLiveAuxiliary,
        };
        SlotVisitorConservativeRootMarkingRecord {
            append_record,
            heap_marking: HeapMarkingRecord {
                heap: HEAP,
                marking_epoch: EPOCH,
                root,
                cell,
                heap_cell_kind,
                byte_size: 64,
                already_marked: false,
            },
            action,
            visit_count_delta: 1,
            bytes_visited_delta: 64,
            non_cell_visit_count_delta: 0,
        }
    }

    #[test]
    fn verifier_append_js_cell_first_visit_marks_and_appends_collector_stack() {
        let cell = CellId(1);
        let marking_plan = marking_plan_for(vec![marking_record(0, cell, HeapCellKind::JsCell)]);
        let plan = verifier()
            .append_conservative_roots_from_marking_plan(&marking_plan)
            .expect("verifier append");

        assert_eq!(plan.test_and_set_count, 1);
        assert_eq!(plan.newly_marked_count, 1);
        assert_eq!(plan.already_marked_count, 0);
        assert_eq!(plan.collector_stack_append_count, 1);
        assert_eq!(plan.auxiliary_mark_count, 0);
        assert_eq!(
            plan.records[0].action,
            VerifierSlotVisitorConservativeRootAppendAction::AppendToCollectorStack(
                VerifierSlotVisitorCollectorStackAppendRecord {
                    cell,
                    heap_cell_kind: HeapCellKind::JsCell,
                    worklist: WORKLIST,
                    root_mark_reason: RootMarkReason::ConservativeScan,
                }
            )
        );
        assert_eq!(
            plan.records[0].test_and_set,
            VerifierSlotVisitorTestAndSetMarkRecord {
                cell,
                heap_cell_kind: HeapCellKind::JsCell,
                was_marked: false,
                marked_after: true,
            }
        );
    }

    #[test]
    fn verifier_append_already_marked_js_cell_does_not_append() {
        let cell = CellId(2);
        let marking_plan = marking_plan_for(vec![marking_record(
            0,
            cell,
            HeapCellKind::JsCellWithIndexingHeader,
        )]);
        let mut verifier = verifier();
        verifier.initially_marked_cells.push(cell);
        let plan = verifier
            .append_conservative_roots_from_marking_plan(&marking_plan)
            .expect("verifier append");

        assert_eq!(plan.test_and_set_count, 1);
        assert_eq!(plan.newly_marked_count, 0);
        assert_eq!(plan.already_marked_count, 1);
        assert_eq!(plan.collector_stack_append_count, 0);
        assert_eq!(plan.auxiliary_mark_count, 0);
        assert_eq!(
            plan.records[0].action,
            VerifierSlotVisitorConservativeRootAppendAction::AlreadyMarkedReturn
        );
    }

    #[test]
    fn verifier_append_auxiliary_marks_without_collector_stack_append() {
        let cell = CellId(3);
        let marking_plan = marking_plan_for(vec![marking_record(0, cell, HeapCellKind::Auxiliary)]);
        let plan = verifier()
            .append_conservative_roots_from_marking_plan(&marking_plan)
            .expect("verifier append");

        assert_eq!(plan.test_and_set_count, 1);
        assert_eq!(plan.newly_marked_count, 1);
        assert_eq!(plan.already_marked_count, 0);
        assert_eq!(plan.collector_stack_append_count, 0);
        assert_eq!(plan.auxiliary_mark_count, 1);
        assert_eq!(
            plan.records[0].action,
            VerifierSlotVisitorConservativeRootAppendAction::MarkAuxiliaryOnly
        );
    }

    #[test]
    fn verifier_append_rejects_wrong_root_reason_heap_and_epoch() {
        let marking_plan =
            marking_plan_for(vec![marking_record(0, CellId(4), HeapCellKind::JsCell)]);

        let mut wrong_reason = verifier();
        wrong_reason.root_mark_reason = RootMarkReason::JitStubRoutines;
        assert_eq!(
            wrong_reason.append_conservative_roots_from_marking_plan(&marking_plan),
            Err(
                VerifierSlotVisitorConservativeRootAppendError::InvalidRootMarkReason {
                    actual: RootMarkReason::JitStubRoutines,
                }
            )
        );

        let mut heap_mismatch = verifier()
            .append_conservative_roots_from_marking_plan(&marking_plan)
            .expect("verifier append");
        heap_mismatch.heap = HeapId(99);
        assert_eq!(
            heap_mismatch.validate_consistency(),
            Err(
                VerifierSlotVisitorConservativeRootAppendError::RecordHeapMismatch {
                    order: 0,
                    plan: HeapId(99),
                    record: HEAP,
                }
            )
        );

        let mut epoch_mismatch = verifier()
            .append_conservative_roots_from_marking_plan(&marking_plan)
            .expect("verifier append");
        epoch_mismatch.marking_epoch = HeapEpoch(99);
        assert_eq!(
            epoch_mismatch.validate_consistency(),
            Err(
                VerifierSlotVisitorConservativeRootAppendError::RecordMarkingEpochMismatch {
                    order: 0,
                    plan: HeapEpoch(99),
                    record: EPOCH,
                }
            )
        );
    }

    #[test]
    fn verifier_append_rejects_malformed_record_and_count() {
        let marking_plan =
            marking_plan_for(vec![marking_record(0, CellId(5), HeapCellKind::JsCell)]);
        let mut malformed_action = verifier()
            .append_conservative_roots_from_marking_plan(&marking_plan)
            .expect("verifier append");
        let expected = malformed_action.records[0].action;
        malformed_action.records[0].action =
            VerifierSlotVisitorConservativeRootAppendAction::MarkAuxiliaryOnly;
        assert_eq!(
            malformed_action.validate_consistency(),
            Err(
                VerifierSlotVisitorConservativeRootAppendError::ActionMismatch {
                    order: 0,
                    expected,
                    actual: VerifierSlotVisitorConservativeRootAppendAction::MarkAuxiliaryOnly,
                }
            )
        );

        let mut malformed_count = verifier()
            .append_conservative_roots_from_marking_plan(&marking_plan)
            .expect("verifier append");
        malformed_count.collector_stack_append_count = 0;
        assert_eq!(
            malformed_count.validate_consistency(),
            Err(
                VerifierSlotVisitorConservativeRootAppendError::CollectorStackAppendCountMismatch {
                    expected: 1,
                    actual: 0,
                }
            )
        );
    }
}
