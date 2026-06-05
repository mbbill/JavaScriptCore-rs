//! C++ `JITStubRoutineSet` / `GCAwareJITStubRoutine` conservative-scan proof.
//!
//! JSC stores raw executable addresses and lets conservative stack scanning
//! call `JITStubRoutineSet::mark(void*)` through `ConservativeRoots` hooks.
//! Rust intentionally records allocation-relative evidence instead: there are
//! no real JIT stub pointers, executable-memory code tags, or
//! `markRequiredObjects` traversal yet.

#![allow(dead_code)]

use crate::gc::{CellId, HeapEpoch, HeapId, MarkWorklistId, RootMarkReason, SlotVisitorDescriptor};
use crate::jit::{
    CodeLiveness, CodeRetentionPolicy, ExecutableAllocationId, JitCodeId, MachineCodeRange,
    MachineCodeValidationError,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JitStubRoutineCandidateAddress {
    pub allocation: ExecutableAllocationId,
    pub offset: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GcAwareJitStubRoutineDescriptor {
    pub id: JitCodeId,
    pub code: JitCodeId,
    pub range: MachineCodeRange,
    pub liveness: CodeLiveness,
    pub retention: CodeRetentionPolicy,
    pub is_code_immutable: bool,
    pub may_be_executing: bool,
    pub required_object_edges: Vec<CellId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JitStubRoutineSetDescriptor {
    pub routines: Vec<GcAwareJitStubRoutineDescriptor>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JitStubRoutineConservativeScanPlan {
    pub mutable_routines: Vec<GcAwareJitStubRoutineDescriptor>,
    pub immutable_routine_count: usize,
    pub prepared_range: Option<MachineCodeRange>,
    pub mark_records: Vec<JitStubRoutineMarkRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JitStubRoutineMarkRecord {
    pub order: usize,
    pub candidate: JitStubRoutineCandidateAddress,
    pub routine: JitCodeId,
    pub code: JitCodeId,
    pub range: MachineCodeRange,
    pub was_may_be_executing: bool,
    pub may_be_executing_after: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JitStubRoutineTracePlan {
    pub heap: HeapId,
    pub marking_epoch: HeapEpoch,
    pub worklist: MarkWorklistId,
    pub root_mark_reason: RootMarkReason,
    pub prepared_scan: JitStubRoutineConservativeScanPlan,
    pub records: Vec<JitStubRoutineTraceRecord>,
    pub traced_routine_count: usize,
    pub required_edge_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JitStubRoutineTraceRecord {
    pub order: usize,
    pub routine_order: usize,
    pub routine: JitCodeId,
    pub code: JitCodeId,
    pub range: MachineCodeRange,
    pub required_edges: Vec<JitStubRoutineRequiredObjectEdgeRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JitStubRoutineRequiredObjectEdgeRecord {
    pub order: usize,
    pub routine_order: usize,
    pub routine: JitCodeId,
    pub code: JitCodeId,
    pub cell: CellId,
    pub root_mark_reason: RootMarkReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JitStubRoutineTraceError {
    MutableRoutineNotLive {
        routine: JitCodeId,
        liveness: CodeLiveness,
    },
    MutableRoutineRangeInvalid {
        routine: JitCodeId,
        range: MachineCodeRange,
        error: MachineCodeValidationError,
    },
    MutableRoutineAllocationMismatch {
        expected: ExecutableAllocationId,
        actual: ExecutableAllocationId,
    },
    MutableRoutineOrderMismatch {
        order: usize,
        previous_start_offset: u32,
        actual_start_offset: u32,
    },
    PreparedRangeMismatch {
        expected: Option<MachineCodeRange>,
        actual: Option<MachineCodeRange>,
    },
    CandidateOutsidePreparedRange {
        candidate: JitStubRoutineCandidateAddress,
        prepared_range: Option<MachineCodeRange>,
    },
    CandidateOutsideRoutineRange {
        candidate: JitStubRoutineCandidateAddress,
    },
    MarkRecordOrderMismatch {
        expected: usize,
        actual: usize,
    },
    MarkRecordRoutineMismatch {
        order: usize,
        routine: JitCodeId,
    },
    MarkRecordRangeMismatch {
        order: usize,
        expected: MachineCodeRange,
        actual: MachineCodeRange,
    },
    MarkRecordWasMayBeExecutingMismatch {
        order: usize,
        expected: bool,
        actual: bool,
    },
    MarkRecordMayBeExecutingAfterMismatch {
        order: usize,
        actual: bool,
    },
    RoutineMayBeExecutingReplayMismatch {
        routine_order: usize,
        routine: JitCodeId,
        expected: bool,
        actual: bool,
    },
    InvalidRootMarkReason {
        actual: RootMarkReason,
    },
    TraceRecordCountMismatch {
        expected: usize,
        actual: usize,
    },
    TraceRecordMismatch {
        order: usize,
        expected: JitStubRoutineTraceRecord,
        actual: JitStubRoutineTraceRecord,
    },
    TracedRoutineCountMismatch {
        expected: usize,
        actual: usize,
    },
    RequiredEdgeCountMismatch {
        expected: usize,
        actual: usize,
    },
}

impl JitStubRoutineSetDescriptor {
    pub fn new(routines: Vec<GcAwareJitStubRoutineDescriptor>) -> Self {
        Self { routines }
    }

    pub fn clear_marks(&mut self) {
        for routine in &mut self.routines {
            if !routine.is_code_immutable {
                routine.may_be_executing = false;
            }
        }
    }

    pub fn prepare_for_conservative_scan(
        &self,
    ) -> Result<JitStubRoutineConservativeScanPlan, JitStubRoutineTraceError> {
        let mut immutable_routine_count = 0;
        let mut mutable_routines = Vec::new();

        for routine in &self.routines {
            if routine.is_code_immutable {
                immutable_routine_count += 1;
                continue;
            }
            validate_mutable_routine(routine)?;
            mutable_routines.push(routine.clone());
        }

        mutable_routines.sort_by_key(|routine| {
            (
                routine.range.allocation,
                routine.range.start_offset,
                routine.range.end_offset().unwrap_or(u32::MAX),
                routine.id,
                routine.code,
            )
        });
        let prepared_range = prepared_range_for_mutable_routines(&mutable_routines)?;

        Ok(JitStubRoutineConservativeScanPlan {
            mutable_routines,
            immutable_routine_count,
            prepared_range,
            mark_records: Vec::new(),
        })
    }
}

impl JitStubRoutineConservativeScanPlan {
    pub fn mark_candidate(
        &mut self,
        candidate: JitStubRoutineCandidateAddress,
    ) -> Result<JitStubRoutineMarkRecord, JitStubRoutineTraceError> {
        let Some(prepared_range) = self.prepared_range else {
            return Err(JitStubRoutineTraceError::CandidateOutsidePreparedRange {
                candidate,
                prepared_range: self.prepared_range,
            });
        };
        if !candidate_is_inside_range(candidate, prepared_range) {
            return Err(JitStubRoutineTraceError::CandidateOutsidePreparedRange {
                candidate,
                prepared_range: self.prepared_range,
            });
        }

        let Some((_, routine)) = self
            .mutable_routines
            .iter_mut()
            .enumerate()
            .find(|(_, routine)| candidate_is_inside_range(candidate, routine.range))
        else {
            return Err(JitStubRoutineTraceError::CandidateOutsideRoutineRange { candidate });
        };

        let record = JitStubRoutineMarkRecord {
            order: self.mark_records.len(),
            candidate,
            routine: routine.id,
            code: routine.code,
            range: routine.range,
            was_may_be_executing: routine.may_be_executing,
            may_be_executing_after: true,
        };
        routine.may_be_executing = true;
        self.mark_records.push(record);
        Ok(record)
    }

    pub fn trace_marked_stub_routines(
        &self,
        visitor: &SlotVisitorDescriptor,
    ) -> Result<JitStubRoutineTracePlan, JitStubRoutineTraceError> {
        if visitor.root_mark_reason != RootMarkReason::JitStubRoutines {
            return Err(JitStubRoutineTraceError::InvalidRootMarkReason {
                actual: visitor.root_mark_reason,
            });
        }

        let records = expected_trace_records(self);
        let required_edge_count = records
            .iter()
            .map(|record| record.required_edges.len())
            .sum();

        let plan = JitStubRoutineTracePlan {
            heap: visitor.heap,
            marking_epoch: visitor.marking_epoch,
            worklist: visitor.worklist,
            root_mark_reason: visitor.root_mark_reason,
            prepared_scan: self.clone(),
            traced_routine_count: records.len(),
            required_edge_count,
            records,
        };
        plan.validate_consistency()?;
        Ok(plan)
    }

    pub fn validate_consistency(&self) -> Result<(), JitStubRoutineTraceError> {
        validate_prepared_scan(self)?;
        validate_mark_records(self)
    }
}

impl JitStubRoutineTracePlan {
    pub fn validate_consistency(&self) -> Result<(), JitStubRoutineTraceError> {
        if self.root_mark_reason != RootMarkReason::JitStubRoutines {
            return Err(JitStubRoutineTraceError::InvalidRootMarkReason {
                actual: self.root_mark_reason,
            });
        }

        self.prepared_scan.validate_consistency()?;

        let expected_records = expected_trace_records(&self.prepared_scan);
        if self.records.len() != expected_records.len() {
            return Err(JitStubRoutineTraceError::TraceRecordCountMismatch {
                expected: expected_records.len(),
                actual: self.records.len(),
            });
        }
        for (order, (expected, actual)) in expected_records
            .iter()
            .cloned()
            .zip(self.records.iter().cloned())
            .enumerate()
        {
            if expected != actual {
                return Err(JitStubRoutineTraceError::TraceRecordMismatch {
                    order,
                    expected,
                    actual,
                });
            }
        }

        if self.traced_routine_count != expected_records.len() {
            return Err(JitStubRoutineTraceError::TracedRoutineCountMismatch {
                expected: expected_records.len(),
                actual: self.traced_routine_count,
            });
        }

        let expected_required_edge_count = expected_records
            .iter()
            .map(|record| record.required_edges.len())
            .sum();
        if self.required_edge_count != expected_required_edge_count {
            return Err(JitStubRoutineTraceError::RequiredEdgeCountMismatch {
                expected: expected_required_edge_count,
                actual: self.required_edge_count,
            });
        }

        Ok(())
    }
}

fn validate_mutable_routine(
    routine: &GcAwareJitStubRoutineDescriptor,
) -> Result<(), JitStubRoutineTraceError> {
    if routine.liveness != CodeLiveness::Live {
        return Err(JitStubRoutineTraceError::MutableRoutineNotLive {
            routine: routine.id,
            liveness: routine.liveness,
        });
    }

    if let Err(error) = routine.range.validate() {
        return Err(JitStubRoutineTraceError::MutableRoutineRangeInvalid {
            routine: routine.id,
            range: routine.range,
            error,
        });
    }

    Ok(())
}

fn prepared_range_for_mutable_routines(
    mutable_routines: &[GcAwareJitStubRoutineDescriptor],
) -> Result<Option<MachineCodeRange>, JitStubRoutineTraceError> {
    let Some(first) = mutable_routines.first() else {
        return Ok(None);
    };
    let expected_allocation = first.range.allocation;
    for routine in mutable_routines {
        if routine.range.allocation != expected_allocation {
            // C++ `JITStubRoutineSet` stores raw executable addresses and can
            // derive one native address range across all mutable routines.
            // Rust `MachineCodeRange` is allocation-relative, so this evidence
            // layer requires one executable allocation until raw code-pointer
            // identity exists.
            return Err(JitStubRoutineTraceError::MutableRoutineAllocationMismatch {
                expected: expected_allocation,
                actual: routine.range.allocation,
            });
        }
    }

    let last = mutable_routines
        .last()
        .expect("non-empty mutable routine list has a last element");
    let end_offset =
        last.range
            .end_offset()
            .ok_or(JitStubRoutineTraceError::MutableRoutineRangeInvalid {
                routine: last.id,
                range: last.range,
                error: MachineCodeValidationError::RangeEndOverflow,
            })?;
    let size_bytes = end_offset.checked_sub(first.range.start_offset).ok_or(
        JitStubRoutineTraceError::MutableRoutineRangeInvalid {
            routine: last.id,
            range: last.range,
            error: MachineCodeValidationError::RangeEndOverflow,
        },
    )?;

    Ok(Some(MachineCodeRange {
        allocation: expected_allocation,
        start_offset: first.range.start_offset,
        size_bytes,
    }))
}

fn validate_prepared_scan(
    plan: &JitStubRoutineConservativeScanPlan,
) -> Result<(), JitStubRoutineTraceError> {
    let mut previous_start_offset = None;
    for (order, routine) in plan.mutable_routines.iter().enumerate() {
        validate_mutable_routine(routine)?;
        if let Some(previous_start_offset) = previous_start_offset {
            if routine.range.start_offset < previous_start_offset {
                return Err(JitStubRoutineTraceError::MutableRoutineOrderMismatch {
                    order,
                    previous_start_offset,
                    actual_start_offset: routine.range.start_offset,
                });
            }
        }
        previous_start_offset = Some(routine.range.start_offset);
    }

    let expected_range = prepared_range_for_mutable_routines(&plan.mutable_routines)?;
    if plan.prepared_range != expected_range {
        return Err(JitStubRoutineTraceError::PreparedRangeMismatch {
            expected: expected_range,
            actual: plan.prepared_range,
        });
    }

    Ok(())
}

fn validate_mark_records(
    plan: &JitStubRoutineConservativeScanPlan,
) -> Result<(), JitStubRoutineTraceError> {
    let mut replayed_may_be_executing = vec![false; plan.mutable_routines.len()];

    for (expected_order, record) in plan.mark_records.iter().copied().enumerate() {
        if record.order != expected_order {
            return Err(JitStubRoutineTraceError::MarkRecordOrderMismatch {
                expected: expected_order,
                actual: record.order,
            });
        }

        let Some(prepared_range) = plan.prepared_range else {
            return Err(JitStubRoutineTraceError::CandidateOutsidePreparedRange {
                candidate: record.candidate,
                prepared_range: plan.prepared_range,
            });
        };
        if !candidate_is_inside_range(record.candidate, prepared_range) {
            return Err(JitStubRoutineTraceError::CandidateOutsidePreparedRange {
                candidate: record.candidate,
                prepared_range: plan.prepared_range,
            });
        }

        let Some((routine_index, routine)) = plan
            .mutable_routines
            .iter()
            .enumerate()
            .find(|(_, routine)| routine.id == record.routine && routine.code == record.code)
        else {
            return Err(JitStubRoutineTraceError::MarkRecordRoutineMismatch {
                order: expected_order,
                routine: record.routine,
            });
        };

        if routine.range != record.range {
            return Err(JitStubRoutineTraceError::MarkRecordRangeMismatch {
                order: expected_order,
                expected: routine.range,
                actual: record.range,
            });
        }
        if !record.may_be_executing_after {
            return Err(
                JitStubRoutineTraceError::MarkRecordMayBeExecutingAfterMismatch {
                    order: expected_order,
                    actual: record.may_be_executing_after,
                },
            );
        }
        if record.was_may_be_executing != replayed_may_be_executing[routine_index] {
            return Err(
                JitStubRoutineTraceError::MarkRecordWasMayBeExecutingMismatch {
                    order: expected_order,
                    expected: replayed_may_be_executing[routine_index],
                    actual: record.was_may_be_executing,
                },
            );
        }
        if !candidate_is_inside_range(record.candidate, routine.range) {
            return Err(JitStubRoutineTraceError::CandidateOutsideRoutineRange {
                candidate: record.candidate,
            });
        }
        replayed_may_be_executing[routine_index] = true;
    }

    for (routine_order, (routine, expected)) in plan
        .mutable_routines
        .iter()
        .zip(replayed_may_be_executing.into_iter())
        .enumerate()
    {
        if routine.may_be_executing != expected {
            return Err(
                JitStubRoutineTraceError::RoutineMayBeExecutingReplayMismatch {
                    routine_order,
                    routine: routine.id,
                    expected,
                    actual: routine.may_be_executing,
                },
            );
        }
    }

    Ok(())
}

fn expected_trace_records(
    plan: &JitStubRoutineConservativeScanPlan,
) -> Vec<JitStubRoutineTraceRecord> {
    let mut records = Vec::new();
    for (routine_order, routine) in plan.mutable_routines.iter().enumerate() {
        if !routine.may_be_executing {
            continue;
        }
        let required_edges = routine
            .required_object_edges
            .iter()
            .copied()
            .enumerate()
            .map(|(order, cell)| JitStubRoutineRequiredObjectEdgeRecord {
                order,
                routine_order,
                routine: routine.id,
                code: routine.code,
                cell,
                root_mark_reason: RootMarkReason::JitStubRoutines,
            })
            .collect();
        records.push(JitStubRoutineTraceRecord {
            order: records.len(),
            routine_order,
            routine: routine.id,
            code: routine.code,
            range: routine.range,
            required_edges,
        });
    }
    records
}

fn candidate_is_inside_range(
    candidate: JitStubRoutineCandidateAddress,
    range: MachineCodeRange,
) -> bool {
    if candidate.allocation != range.allocation {
        return false;
    }
    let Some(end_offset) = range.end_offset() else {
        return false;
    };
    candidate.offset >= range.start_offset && candidate.offset < end_offset
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allocation(id: u64) -> ExecutableAllocationId {
        ExecutableAllocationId(id)
    }

    fn range(start_offset: u32, size_bytes: u32) -> MachineCodeRange {
        MachineCodeRange {
            allocation: allocation(7),
            start_offset,
            size_bytes,
        }
    }

    fn routine(
        id: u64,
        start_offset: u32,
        size_bytes: u32,
        immutable: bool,
        edges: Vec<CellId>,
    ) -> GcAwareJitStubRoutineDescriptor {
        GcAwareJitStubRoutineDescriptor {
            id: JitCodeId(id),
            code: JitCodeId(1000 + id),
            range: range(start_offset, size_bytes),
            liveness: CodeLiveness::Live,
            retention: CodeRetentionPolicy::SharedStubRegistry,
            is_code_immutable: immutable,
            may_be_executing: false,
            required_object_edges: edges,
        }
    }

    fn jit_stub_visitor() -> SlotVisitorDescriptor {
        let mut visitor =
            SlotVisitorDescriptor::new(HeapId(3), "jit-stub-routine-test", HeapEpoch(11));
        visitor.root_mark_reason = RootMarkReason::JitStubRoutines;
        visitor
    }

    #[test]
    fn stub_routine_prepare_sorts_mutable_routines_and_derives_scan_range() {
        let set = JitStubRoutineSetDescriptor::new(vec![
            routine(2, 160, 20, false, vec![]),
            routine(9, 120, 16, true, vec![]),
            routine(1, 100, 12, false, vec![]),
        ]);

        let plan = set
            .prepare_for_conservative_scan()
            .expect("prepare JIT stub routine scan plan");

        assert_eq!(plan.immutable_routine_count, 1);
        assert_eq!(
            plan.mutable_routines
                .iter()
                .map(|routine| routine.id)
                .collect::<Vec<_>>(),
            vec![JitCodeId(1), JitCodeId(2)]
        );
        assert_eq!(
            plan.prepared_range,
            Some(MachineCodeRange {
                allocation: allocation(7),
                start_offset: 100,
                size_bytes: 80,
            })
        );
    }

    #[test]
    fn stub_routine_immutable_routines_are_ignored_by_scan_and_marking() {
        let set =
            JitStubRoutineSetDescriptor::new(vec![routine(9, 120, 16, true, vec![CellId(1)])]);
        let mut plan = set
            .prepare_for_conservative_scan()
            .expect("prepare immutable-only set");

        assert_eq!(plan.immutable_routine_count, 1);
        assert_eq!(plan.mutable_routines, Vec::new());
        assert_eq!(plan.prepared_range, None);
        assert_eq!(
            plan.mark_candidate(JitStubRoutineCandidateAddress {
                allocation: allocation(7),
                offset: 124,
            }),
            Err(JitStubRoutineTraceError::CandidateOutsidePreparedRange {
                candidate: JitStubRoutineCandidateAddress {
                    allocation: allocation(7),
                    offset: 124,
                },
                prepared_range: None,
            })
        );
    }

    #[test]
    fn stub_routine_candidate_mark_selects_only_containing_routine() {
        let set = JitStubRoutineSetDescriptor::new(vec![
            routine(1, 100, 20, false, vec![CellId(1)]),
            routine(2, 140, 20, false, vec![CellId(2)]),
        ]);
        let mut plan = set
            .prepare_for_conservative_scan()
            .expect("prepare mutable stub routines");

        let record = plan
            .mark_candidate(JitStubRoutineCandidateAddress {
                allocation: allocation(7),
                offset: 145,
            })
            .expect("mark containing routine");

        assert_eq!(record.routine, JitCodeId(2));
        assert!(!plan.mutable_routines[0].may_be_executing);
        assert!(plan.mutable_routines[1].may_be_executing);
        assert_eq!(plan.mark_records, vec![record]);
    }

    #[test]
    fn stub_routine_trace_emits_only_may_be_executing_required_edges() {
        let set = JitStubRoutineSetDescriptor::new(vec![
            routine(1, 100, 20, false, vec![CellId(1)]),
            routine(2, 140, 20, false, vec![CellId(2), CellId(3)]),
        ]);
        let mut scan = set
            .prepare_for_conservative_scan()
            .expect("prepare mutable stub routines");
        scan.mark_candidate(JitStubRoutineCandidateAddress {
            allocation: allocation(7),
            offset: 145,
        })
        .expect("mark containing routine");

        let trace = scan
            .trace_marked_stub_routines(&jit_stub_visitor())
            .expect("trace marked stub routine");

        assert_eq!(trace.traced_routine_count, 1);
        assert_eq!(trace.required_edge_count, 2);
        assert_eq!(trace.records[0].routine, JitCodeId(2));
        assert_eq!(
            trace.records[0]
                .required_edges
                .iter()
                .map(|edge| edge.cell)
                .collect::<Vec<_>>(),
            vec![CellId(2), CellId(3)]
        );
    }

    #[test]
    fn stub_routine_rejects_malformed_mutable_routines_and_candidates() {
        let mut not_live = routine(1, 100, 20, false, vec![]);
        not_live.liveness = CodeLiveness::PendingInvalidation;
        let set = JitStubRoutineSetDescriptor::new(vec![not_live]);
        assert_eq!(
            set.prepare_for_conservative_scan(),
            Err(JitStubRoutineTraceError::MutableRoutineNotLive {
                routine: JitCodeId(1),
                liveness: CodeLiveness::PendingInvalidation,
            })
        );

        let zero_range = JitStubRoutineSetDescriptor::new(vec![routine(2, 100, 0, false, vec![])]);
        assert_eq!(
            zero_range.prepare_for_conservative_scan(),
            Err(JitStubRoutineTraceError::MutableRoutineRangeInvalid {
                routine: JitCodeId(2),
                range: range(100, 0),
                error: MachineCodeValidationError::EmptyRange,
            })
        );

        let mut different_allocation = routine(4, 140, 20, false, vec![]);
        different_allocation.range.allocation = allocation(8);
        let cross_allocation = JitStubRoutineSetDescriptor::new(vec![
            routine(3, 100, 20, false, vec![]),
            different_allocation,
        ]);
        assert_eq!(
            cross_allocation.prepare_for_conservative_scan(),
            Err(JitStubRoutineTraceError::MutableRoutineAllocationMismatch {
                expected: allocation(7),
                actual: allocation(8),
            })
        );

        let set = JitStubRoutineSetDescriptor::new(vec![routine(3, 100, 20, false, vec![])]);
        let mut scan = set
            .prepare_for_conservative_scan()
            .expect("prepare mutable stub routine");
        assert_eq!(
            scan.mark_candidate(JitStubRoutineCandidateAddress {
                allocation: allocation(7),
                offset: 200,
            }),
            Err(JitStubRoutineTraceError::CandidateOutsidePreparedRange {
                candidate: JitStubRoutineCandidateAddress {
                    allocation: allocation(7),
                    offset: 200,
                },
                prepared_range: Some(range(100, 20)),
            })
        );
    }

    #[test]
    fn stub_routine_trace_requires_jit_stub_root_reason() {
        let set =
            JitStubRoutineSetDescriptor::new(vec![routine(1, 100, 20, false, vec![CellId(1)])]);
        let mut scan = set
            .prepare_for_conservative_scan()
            .expect("prepare mutable stub routine");
        scan.mark_candidate(JitStubRoutineCandidateAddress {
            allocation: allocation(7),
            offset: 108,
        })
        .expect("mark containing routine");

        let wrong_visitor =
            SlotVisitorDescriptor::new(HeapId(3), "jit-stub-routine-test", HeapEpoch(11));
        assert_eq!(
            scan.trace_marked_stub_routines(&wrong_visitor),
            Err(JitStubRoutineTraceError::InvalidRootMarkReason {
                actual: RootMarkReason::None,
            })
        );
    }

    #[test]
    fn stub_routine_trace_validation_rejects_forged_trace_records() {
        let set =
            JitStubRoutineSetDescriptor::new(vec![routine(1, 100, 20, false, vec![CellId(1)])]);
        let mut scan = set
            .prepare_for_conservative_scan()
            .expect("prepare mutable stub routine");
        scan.mark_candidate(JitStubRoutineCandidateAddress {
            allocation: allocation(7),
            offset: 108,
        })
        .expect("mark containing routine");

        let mut trace = scan
            .trace_marked_stub_routines(&jit_stub_visitor())
            .expect("trace marked stub routine");
        trace.records[0].required_edges[0].cell = CellId(99);

        assert!(matches!(
            trace.validate_consistency(),
            Err(JitStubRoutineTraceError::TraceRecordMismatch { order: 0, .. })
        ));
    }

    #[test]
    fn stub_routine_trace_validation_rejects_forged_pre_marked_routine_without_mark_record() {
        let set =
            JitStubRoutineSetDescriptor::new(vec![routine(1, 100, 20, false, vec![CellId(1)])]);
        let mut scan = set
            .prepare_for_conservative_scan()
            .expect("prepare mutable stub routine");
        scan.mutable_routines[0].may_be_executing = true;

        assert_eq!(
            scan.trace_marked_stub_routines(&jit_stub_visitor()),
            Err(
                JitStubRoutineTraceError::RoutineMayBeExecutingReplayMismatch {
                    routine_order: 0,
                    routine: JitCodeId(1),
                    expected: false,
                    actual: true,
                }
            )
        );
    }
}
