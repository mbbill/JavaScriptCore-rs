//! C++ `Heap.cpp` conservative-scan append boundary.
//!
//! This module mirrors the `addCoreConstraints` conservative-scan handoff from
//! heap-owned conservative roots to `SlotVisitor::append(ConservativeRoots)`.
//! It is descriptor-only and does not claim to mutate mark bits or mark stacks.

#![allow(dead_code)]

use crate::gc::{
    GcPhase, Heap, HeapEpoch, HeapId, HeapSemanticError, HeapSemanticOperation, MutatorState,
    ReferrerToken, ReferrerTokenKind, RootMarkReason, SlotVisitorConservativeRootAppendError,
    SlotVisitorConservativeRootAppendPlan, SlotVisitorDescriptor,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HeapConservativeScanAppendReceipt {
    pub heap: HeapId,
    pub epoch: HeapEpoch,
    pub phase: GcPhase,
    pub mutator_state: MutatorState,
    pub prior_root_mark_reason: RootMarkReason,
    pub prior_referrer: Option<ReferrerToken>,
    pub append_plan: SlotVisitorConservativeRootAppendPlan,
    pub conservative_root_count: usize,
    pub appended_record_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HeapConservativeScanAppendError {
    HeapSemantic(HeapSemanticError),
    SlotVisitorAppend(SlotVisitorConservativeRootAppendError),
}

impl From<HeapSemanticError> for HeapConservativeScanAppendError {
    fn from(error: HeapSemanticError) -> Self {
        Self::HeapSemantic(error)
    }
}

impl From<SlotVisitorConservativeRootAppendError> for HeapConservativeScanAppendError {
    fn from(error: SlotVisitorConservativeRootAppendError) -> Self {
        Self::SlotVisitorAppend(error)
    }
}

impl Heap {
    pub(crate) fn append_conservative_roots_to_slot_visitor_descriptor(
        &self,
        visitor: &SlotVisitorDescriptor,
    ) -> Result<HeapConservativeScanAppendReceipt, HeapConservativeScanAppendError> {
        let state = self.state_descriptor();
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

        let roots = self.conservative_roots();
        let prior_root_mark_reason = visitor.root_mark_reason;
        let prior_referrer = visitor.current_referrer;
        let mut scoped_visitor = visitor.clone();
        scoped_visitor.root_mark_reason = RootMarkReason::ConservativeScan;
        scoped_visitor.current_referrer = Some(ReferrerToken {
            kind: ReferrerTokenKind::RootMarkReason,
            address: 0,
            root_mark_reason: RootMarkReason::ConservativeScan,
        });

        // C++ `Heap` conservative scan also prepares object space/JIT stubs,
        // gathers VM roots, mirrors the verifier visitor append, then traces
        // JIT stubs after scan because gather hooks mark stubs. Rust only
        // proves that already-validated heap conservative roots were appended
        // through the descriptor boundary under the local root-reason and
        // referrer context scope.
        let conservative_root_count = roots.size();
        let append_plan =
            scoped_visitor.append_conservative_roots_descriptor(&roots, self.id(), self.epoch())?;
        let appended_record_count = append_plan.records.len();

        Ok(HeapConservativeScanAppendReceipt {
            heap: self.id(),
            epoch: self.epoch(),
            phase: state.phase,
            mutator_state: state.mutator_state,
            prior_root_mark_reason,
            prior_referrer,
            append_plan,
            conservative_root_count,
            appended_record_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{
        AllocationMode, CellId, CellMetadata, ConservativeRoots, GcConductor,
        HeapAllocationRequest, MarkWorklistId, ReferrerToken, ReferrerTokenKind,
        SlotVisitorConservativeRootAppendError,
    };

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

    fn enter_collecting(heap: &mut Heap) {
        heap.enter_phase(
            GcPhase::Fixpoint,
            MutatorState::Collecting,
            GcConductor::Mutator,
        );
    }

    fn add_published_payload_root(heap: &mut Heap, payload: usize) -> CellId {
        let cell = allocate_test_cell(heap);
        heap.bind_cell_payload(cell, payload).expect("bind payload");
        heap.publish_cell(cell).expect("publish cell");

        let mut roots = ConservativeRoots::new();
        roots.add_validated_cell(
            heap.validate_conservative_root_candidate_exact_payload(payload)
                .expect("validated root"),
        );
        heap.ingest_conservative_roots(roots)
            .expect("ingest conservative root");
        cell
    }

    #[test]
    fn heap_conservative_scan_scopes_default_visitor_and_preserves_ordered_roots() {
        let mut heap = Heap::new();
        let first = add_published_payload_root(&mut heap, 0x1000);
        let second = add_published_payload_root(&mut heap, 0x2000);
        enter_collecting(&mut heap);

        let mut visitor = heap.slot_visitor_descriptor("heap-conservative-scan-test");
        visitor.worklist = MarkWorklistId(41);
        visitor.current_referrer = Some(ReferrerToken {
            kind: ReferrerTokenKind::RootMarkReason,
            address: 0,
            root_mark_reason: RootMarkReason::StrongHandles,
        });

        let receipt = heap
            .append_conservative_roots_to_slot_visitor_descriptor(&visitor)
            .expect("heap append receipt");

        assert_eq!(visitor.root_mark_reason, RootMarkReason::None);
        assert_eq!(
            visitor.current_referrer,
            Some(ReferrerToken {
                kind: ReferrerTokenKind::RootMarkReason,
                address: 0,
                root_mark_reason: RootMarkReason::StrongHandles,
            })
        );
        assert_eq!(receipt.heap, heap.id());
        assert_eq!(receipt.epoch, heap.epoch());
        assert_eq!(receipt.phase, GcPhase::Fixpoint);
        assert_eq!(receipt.mutator_state, MutatorState::Collecting);
        assert_eq!(receipt.prior_root_mark_reason, RootMarkReason::None);
        assert_eq!(receipt.prior_referrer, visitor.current_referrer);
        assert_eq!(receipt.conservative_root_count, 2);
        assert_eq!(receipt.appended_record_count, 2);
        assert_eq!(
            receipt.append_plan.root_mark_reason,
            RootMarkReason::ConservativeScan
        );
        assert_eq!(
            receipt.append_plan.referrer,
            Some(ReferrerToken {
                kind: ReferrerTokenKind::RootMarkReason,
                address: 0,
                root_mark_reason: RootMarkReason::ConservativeScan,
            })
        );
        assert_eq!(receipt.append_plan.worklist, MarkWorklistId(41));
        assert_eq!(receipt.append_plan.records[0].cell, first);
        assert_eq!(
            receipt.append_plan.records[0].root.candidate_address,
            0x1000
        );
        assert_eq!(receipt.append_plan.records[1].cell, second);
        assert_eq!(
            receipt.append_plan.records[1].root.candidate_address,
            0x2000
        );
    }

    #[test]
    fn heap_conservative_scan_rejects_not_running_and_running_mutator_before_append() {
        let mut heap = Heap::new();
        let mismatched_visitor =
            SlotVisitorDescriptor::new(HeapId(99), "heap-conservative-scan-test", heap.epoch());

        assert_eq!(
            heap.append_conservative_roots_to_slot_visitor_descriptor(&mismatched_visitor),
            Err(HeapConservativeScanAppendError::HeapSemantic(
                HeapSemanticError::WrongPhase {
                    operation: HeapSemanticOperation::TraceRoots,
                    phase: GcPhase::NotRunning
                }
            ))
        );

        heap.enter_phase(
            GcPhase::Fixpoint,
            MutatorState::Running,
            GcConductor::Mutator,
        );
        assert_eq!(
            heap.append_conservative_roots_to_slot_visitor_descriptor(&mismatched_visitor),
            Err(HeapConservativeScanAppendError::HeapSemantic(
                HeapSemanticError::MutatorMustBeStopped {
                    operation: HeapSemanticOperation::TraceRoots,
                    mutator_state: MutatorState::Running
                }
            ))
        );
    }

    #[test]
    fn heap_conservative_scan_visitor_mismatch_rejects_through_append_descriptor() {
        let mut heap = Heap::new();
        enter_collecting(&mut heap);

        let wrong_heap =
            SlotVisitorDescriptor::new(HeapId(99), "heap-conservative-scan-test", heap.epoch());
        assert_eq!(
            heap.append_conservative_roots_to_slot_visitor_descriptor(&wrong_heap),
            Err(HeapConservativeScanAppendError::SlotVisitorAppend(
                SlotVisitorConservativeRootAppendError::HeapMismatch {
                    visitor: HeapId(99),
                    roots: heap.id()
                }
            ))
        );

        let wrong_epoch = SlotVisitorDescriptor::new(
            heap.id(),
            "heap-conservative-scan-test",
            HeapEpoch(heap.epoch().0 + 1),
        );
        assert_eq!(
            heap.append_conservative_roots_to_slot_visitor_descriptor(&wrong_epoch),
            Err(HeapConservativeScanAppendError::SlotVisitorAppend(
                SlotVisitorConservativeRootAppendError::MarkingEpochMismatch {
                    visitor: HeapEpoch(heap.epoch().0 + 1),
                    roots: heap.epoch()
                }
            ))
        );
    }

    #[test]
    fn heap_conservative_scan_accepts_empty_roots_while_collecting() {
        let mut heap = Heap::new();
        enter_collecting(&mut heap);
        let visitor = heap.slot_visitor_descriptor("heap-conservative-scan-test");

        let receipt = heap
            .append_conservative_roots_to_slot_visitor_descriptor(&visitor)
            .expect("empty heap append receipt");

        assert_eq!(receipt.conservative_root_count, 0);
        assert_eq!(receipt.appended_record_count, 0);
        assert!(receipt.append_plan.records.is_empty());
        assert_eq!(
            receipt.append_plan.root_mark_reason,
            RootMarkReason::ConservativeScan
        );
    }
}
