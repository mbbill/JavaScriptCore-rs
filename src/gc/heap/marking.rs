//! C++ `Heap::testAndSetMarked` marking evidence.
//!
//! C++ stores mark bits in `MarkedBlock` bitsets or `PreciseAllocation`.
//! Rust has no per-allocation container/header mark storage yet, so `Heap`
//! owns epoch-keyed mark evidence for the same test-and-set boundary.

#![allow(dead_code)]

use super::Heap;
use crate::gc::{
    CellDestructionState, CellId, ConservativeRootCell, GcPhase, HeapCellKind, HeapEpoch, HeapId,
    HeapSemanticError, HeapSemanticOperation, MutatorState,
    SlotVisitorConservativeRootAppendRecord,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HeapMarkingRecord {
    pub heap: HeapId,
    pub marking_epoch: HeapEpoch,
    pub root: ConservativeRootCell,
    pub cell: CellId,
    pub heap_cell_kind: HeapCellKind,
    pub byte_size: usize,
    pub already_marked: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HeapMarkingError {
    HeapSemantic(HeapSemanticError),
    HeapMismatch {
        expected: HeapId,
        actual: HeapId,
    },
    MarkingEpochMismatch {
        expected: HeapEpoch,
        actual: HeapEpoch,
    },
    InvalidConservativeRootCell(ConservativeRootCell),
    ConservativeRootCellMismatch {
        root: ConservativeRootCell,
        record_cell: CellId,
    },
    ConservativeRootPayloadMismatch {
        root: ConservativeRootCell,
        expected_payload: Option<usize>,
    },
    UnknownCell(CellId),
    UnpublishedCell(CellId),
    PendingDestruction {
        cell: CellId,
        state: CellDestructionState,
    },
}

impl From<HeapSemanticError> for HeapMarkingError {
    fn from(error: HeapSemanticError) -> Self {
        Self::HeapSemantic(error)
    }
}

impl Heap {
    pub(crate) fn test_and_set_marked_for_conservative_root_append_record(
        &mut self,
        record: &SlotVisitorConservativeRootAppendRecord,
    ) -> Result<HeapMarkingRecord, HeapMarkingError> {
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

        if record.heap != self.id {
            return Err(HeapMarkingError::HeapMismatch {
                expected: self.id,
                actual: record.heap,
            });
        }

        if record.marking_epoch != self.epoch {
            return Err(HeapMarkingError::MarkingEpochMismatch {
                expected: self.epoch,
                actual: record.marking_epoch,
            });
        }

        if record.root.candidate_address == 0 || record.root.cell == CellId::default() {
            return Err(HeapMarkingError::InvalidConservativeRootCell(record.root));
        }

        if record.root.cell != record.cell {
            return Err(HeapMarkingError::ConservativeRootCellMismatch {
                root: record.root,
                record_cell: record.cell,
            });
        }

        let allocation = self
            .allocations
            .iter()
            .find(|allocation| allocation.response.cell == record.cell)
            .ok_or(HeapMarkingError::UnknownCell(record.cell))?;

        if !allocation.published {
            return Err(HeapMarkingError::UnpublishedCell(record.cell));
        }

        if allocation.lifecycle.destruction_state != CellDestructionState::NotPending {
            return Err(HeapMarkingError::PendingDestruction {
                cell: record.cell,
                state: allocation.lifecycle.destruction_state,
            });
        }

        let expected_payload = self.cell_to_payload.get(&record.cell).copied();
        if expected_payload != Some(record.root.candidate_address) {
            return Err(HeapMarkingError::ConservativeRootPayloadMismatch {
                root: record.root,
                expected_payload,
            });
        }

        let heap_cell_kind = allocation.response.metadata.heap_cell_kind;
        let byte_size = allocation.response.byte_size;

        let already_marked = self.marked_cells.get(&record.cell).copied() == Some(self.epoch);
        if !already_marked {
            // C++ `Heap::testAndSetMarked` mutates `MarkedBlock` or
            // `PreciseAllocation` mark bits for the requested marking version.
            // Rust does not yet own those containers, so the heap records the
            // equivalent per-cell epoch mark fact here.
            self.marked_cells.insert(record.cell, self.epoch);
        }

        Ok(HeapMarkingRecord {
            heap: self.id,
            marking_epoch: self.epoch,
            root: record.root,
            cell: record.cell,
            heap_cell_kind,
            byte_size,
            already_marked,
        })
    }
}
