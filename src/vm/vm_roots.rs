//! C++ `VM::gatherScratchBufferRoots` / `VM::scanSideState` root evidence.
//!
//! JSC gathers VM-owned DFG roots by adding active scratch-buffer byte ranges
//! and every `CheckpointOSRExitSideState::tmps` array to `ConservativeRoots`.
//! Rust keeps this as descriptor evidence until real scratch buffers, side-state
//! storage, and raw conservative memory scanning are available.

#![allow(dead_code)]

use crate::gc::{CellId, ConservativeRootCell, ConservativeRootSpan, ConservativeRoots};
use crate::gc::{HeapEpoch, HeapId};
use crate::value::EncodedJsValue;

pub(crate) const MAX_NUM_CHECKPOINT_TMPS: usize = 4;
pub(crate) const ENCODED_JS_VALUE_BYTES: usize = core::mem::size_of::<EncodedJsValue>();

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub(crate) struct VmScratchBufferId(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub(crate) struct VmCheckpointOsrExitSideStateId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VmRootSource {
    ScratchBuffer,
    CheckpointOsrExitSideState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VmScratchBufferCandidateSlot {
    pub offset: usize,
    pub candidate_address: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VmScratchBufferDescriptor {
    pub id: VmScratchBufferId,
    pub data_begin: usize,
    pub byte_length: usize,
    pub active_length: usize,
    pub candidate_slots: Vec<VmScratchBufferCandidateSlot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VmCheckpointOsrExitSideStateDescriptor {
    pub id: VmCheckpointOsrExitSideStateId,
    pub tmps_begin: usize,
    pub tmp_slots: Vec<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VmRootGatherDescriptor {
    pub heap: HeapId,
    pub marking_epoch: HeapEpoch,
    pub world_stopped: bool,
    pub jit_enabled: bool,
    pub scratch_buffers: Vec<VmScratchBufferDescriptor>,
    pub checkpoint_side_states: Vec<VmCheckpointOsrExitSideStateDescriptor>,
    pub validated_cells: Vec<ConservativeRootCell>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VmRootGatherPlan {
    pub heap: HeapId,
    pub marking_epoch: HeapEpoch,
    pub world_stopped: bool,
    pub jit_enabled: bool,
    pub scratch_buffer_records: Vec<VmScratchBufferRootRecord>,
    pub inactive_scratch_buffer_count: usize,
    pub side_state_records: Vec<VmCheckpointOsrExitSideStateRootRecord>,
    pub conservative_roots: ConservativeRoots,
    pub span_count: usize,
    pub candidate_address_count: usize,
    pub validated_cell_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VmScratchBufferRootRecord {
    pub order: usize,
    pub source: VmRootSource,
    pub buffer: VmScratchBufferId,
    pub span: ConservativeRootSpan,
    pub active_length: usize,
    pub byte_length: usize,
    pub candidates: Vec<VmScratchBufferCandidateRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VmScratchBufferCandidateRecord {
    pub order: usize,
    pub source: VmRootSource,
    pub buffer: VmScratchBufferId,
    pub slot_offset: usize,
    pub slot_address: usize,
    pub candidate_address: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VmCheckpointOsrExitSideStateRootRecord {
    pub order: usize,
    pub source: VmRootSource,
    pub side_state: VmCheckpointOsrExitSideStateId,
    pub span: ConservativeRootSpan,
    pub tmp_records: Vec<VmCheckpointOsrExitSideStateTmpRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VmCheckpointOsrExitSideStateTmpRecord {
    pub order: usize,
    pub source: VmRootSource,
    pub side_state: VmCheckpointOsrExitSideStateId,
    pub tmp_index: usize,
    pub slot_address: usize,
    pub candidate_address: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum VmRootGatherError {
    WorldNotStopped,
    ScratchBufferActiveLengthExceedsCapacity {
        buffer: VmScratchBufferId,
        active_length: usize,
        byte_length: usize,
    },
    ScratchBufferDataBeginMissing {
        buffer: VmScratchBufferId,
    },
    ScratchBufferActiveLengthUnaligned {
        buffer: VmScratchBufferId,
        active_length: usize,
    },
    ScratchBufferRangeOverflow {
        buffer: VmScratchBufferId,
        data_begin: usize,
        active_length: usize,
    },
    ScratchBufferCandidateSlotUnaligned {
        buffer: VmScratchBufferId,
        offset: usize,
    },
    ScratchBufferCandidateSlotOutsideActiveLength {
        buffer: VmScratchBufferId,
        offset: usize,
        active_length: usize,
    },
    ScratchBufferCandidateSlotCountMismatch {
        buffer: VmScratchBufferId,
        expected: usize,
        actual: usize,
    },
    ScratchBufferCandidateSlotOrderMismatch {
        buffer: VmScratchBufferId,
        order: usize,
        expected_offset: usize,
        actual_offset: usize,
    },
    ScratchBufferCandidateSlotAddressMismatch {
        buffer: VmScratchBufferId,
        offset: usize,
        expected: usize,
        actual: usize,
    },
    ScratchBufferSourceMismatch {
        order: usize,
        actual: VmRootSource,
    },
    ScratchBufferCandidateSourceMismatch {
        order: usize,
        actual: VmRootSource,
    },
    SideStateTmpCountMismatch {
        side_state: VmCheckpointOsrExitSideStateId,
        expected: usize,
        actual: usize,
    },
    SideStateTmpsBeginMissing {
        side_state: VmCheckpointOsrExitSideStateId,
    },
    SideStateTmpsBeginUnaligned {
        side_state: VmCheckpointOsrExitSideStateId,
        tmps_begin: usize,
    },
    SideStateRangeOverflow {
        side_state: VmCheckpointOsrExitSideStateId,
        tmps_begin: usize,
    },
    SideStateSourceMismatch {
        order: usize,
        actual: VmRootSource,
    },
    SideStateTmpSourceMismatch {
        order: usize,
        actual: VmRootSource,
    },
    SideStateTmpIndexMismatch {
        order: usize,
        expected: usize,
        actual: usize,
    },
    SideStateTmpSlotAddressMismatch {
        side_state: VmCheckpointOsrExitSideStateId,
        tmp_index: usize,
        expected: usize,
        actual: usize,
    },
    JitDisabledVmRootRecordsPresent {
        inactive_scratch_buffer_count: usize,
        scratch_buffer_record_count: usize,
        side_state_record_count: usize,
    },
    ConservativeRootSpanMismatch {
        expected: Vec<ConservativeRootSpan>,
        actual: Vec<ConservativeRootSpan>,
    },
    ConservativeRootCandidateAddressMismatch {
        expected: Vec<usize>,
        actual: Vec<usize>,
    },
    InvalidConservativeRootCell(ConservativeRootCell),
    ValidatedCellCandidateMissing(ConservativeRootCell),
    SpanCountMismatch {
        expected: usize,
        actual: usize,
    },
    CandidateAddressCountMismatch {
        expected: usize,
        actual: usize,
    },
    ValidatedCellCountMismatch {
        expected: usize,
        actual: usize,
    },
}

impl VmRootGatherDescriptor {
    pub(crate) fn gather_vm_roots(self) -> Result<VmRootGatherPlan, VmRootGatherError> {
        if !self.world_stopped {
            return Err(VmRootGatherError::WorldNotStopped);
        }

        let mut roots = ConservativeRoots::new();
        let mut scratch_buffer_records = Vec::new();
        let mut inactive_scratch_buffer_count = 0;
        let mut side_state_records = Vec::new();

        if self.jit_enabled {
            for (order, scratch_buffer) in self.scratch_buffers.into_iter().enumerate() {
                if scratch_buffer.active_length == 0 {
                    inactive_scratch_buffer_count += 1;
                    continue;
                }

                let span = scratch_buffer_span(&scratch_buffer)?;
                validate_scratch_candidate_slots(&scratch_buffer)?;
                let mut candidates = Vec::with_capacity(scratch_buffer.candidate_slots.len());
                for (candidate_order, candidate) in
                    scratch_buffer.candidate_slots.into_iter().enumerate()
                {
                    if candidate.offset % ENCODED_JS_VALUE_BYTES != 0 {
                        return Err(VmRootGatherError::ScratchBufferCandidateSlotUnaligned {
                            buffer: scratch_buffer.id,
                            offset: candidate.offset,
                        });
                    }
                    if candidate.offset >= scratch_buffer.active_length {
                        return Err(
                            VmRootGatherError::ScratchBufferCandidateSlotOutsideActiveLength {
                                buffer: scratch_buffer.id,
                                offset: candidate.offset,
                                active_length: scratch_buffer.active_length,
                            },
                        );
                    }

                    let slot_address = scratch_buffer
                        .data_begin
                        .checked_add(candidate.offset)
                        .ok_or(VmRootGatherError::ScratchBufferRangeOverflow {
                            buffer: scratch_buffer.id,
                            data_begin: scratch_buffer.data_begin,
                            active_length: scratch_buffer.active_length,
                        })?;
                    if candidate.candidate_address != 0 {
                        roots.add_candidate_address(candidate.candidate_address);
                    }
                    candidates.push(VmScratchBufferCandidateRecord {
                        order: candidate_order,
                        source: VmRootSource::ScratchBuffer,
                        buffer: scratch_buffer.id,
                        slot_offset: candidate.offset,
                        slot_address,
                        candidate_address: candidate.candidate_address,
                    });
                }

                roots.add_span(span);
                scratch_buffer_records.push(VmScratchBufferRootRecord {
                    order,
                    source: VmRootSource::ScratchBuffer,
                    buffer: scratch_buffer.id,
                    span,
                    active_length: scratch_buffer.active_length,
                    byte_length: scratch_buffer.byte_length,
                    candidates,
                });
            }

            for (order, side_state) in self.checkpoint_side_states.into_iter().enumerate() {
                if side_state.tmp_slots.len() != MAX_NUM_CHECKPOINT_TMPS {
                    return Err(VmRootGatherError::SideStateTmpCountMismatch {
                        side_state: side_state.id,
                        expected: MAX_NUM_CHECKPOINT_TMPS,
                        actual: side_state.tmp_slots.len(),
                    });
                }

                let span = side_state_span(&side_state)?;
                let mut tmp_records = Vec::with_capacity(MAX_NUM_CHECKPOINT_TMPS);
                for (tmp_index, candidate_address) in side_state.tmp_slots.into_iter().enumerate() {
                    let slot_address = side_state
                        .tmps_begin
                        .checked_add(tmp_index * ENCODED_JS_VALUE_BYTES)
                        .ok_or(VmRootGatherError::SideStateRangeOverflow {
                            side_state: side_state.id,
                            tmps_begin: side_state.tmps_begin,
                        })?;
                    if candidate_address != 0 {
                        roots.add_candidate_address(candidate_address);
                    }
                    tmp_records.push(VmCheckpointOsrExitSideStateTmpRecord {
                        order: tmp_index,
                        source: VmRootSource::CheckpointOsrExitSideState,
                        side_state: side_state.id,
                        tmp_index,
                        slot_address,
                        candidate_address,
                    });
                }

                roots.add_span(span);
                side_state_records.push(VmCheckpointOsrExitSideStateRootRecord {
                    order,
                    source: VmRootSource::CheckpointOsrExitSideState,
                    side_state: side_state.id,
                    span,
                    tmp_records,
                });
            }
        }

        for root in self.validated_cells {
            if root.candidate_address == 0 || root.cell == CellId::default() {
                return Err(VmRootGatherError::InvalidConservativeRootCell(root));
            }
            if !roots
                .candidate_addresses()
                .contains(&root.candidate_address)
            {
                return Err(VmRootGatherError::ValidatedCellCandidateMissing(root));
            }
            roots.add_validated_cell(root);
        }

        let plan = VmRootGatherPlan {
            heap: self.heap,
            marking_epoch: self.marking_epoch,
            world_stopped: self.world_stopped,
            jit_enabled: self.jit_enabled,
            scratch_buffer_records,
            inactive_scratch_buffer_count,
            side_state_records,
            span_count: roots.spans().len(),
            candidate_address_count: roots.candidate_addresses().len(),
            validated_cell_count: roots.validated_cells().len(),
            conservative_roots: roots,
        };
        plan.validate_consistency()?;
        Ok(plan)
    }
}

impl VmRootGatherPlan {
    pub(crate) fn validate_consistency(&self) -> Result<(), VmRootGatherError> {
        if !self.world_stopped {
            return Err(VmRootGatherError::WorldNotStopped);
        }

        let mut expected_spans =
            Vec::with_capacity(self.scratch_buffer_records.len() + self.side_state_records.len());
        let mut expected_candidate_addresses = Vec::new();

        if !self.jit_enabled {
            if self.inactive_scratch_buffer_count != 0
                || !self.scratch_buffer_records.is_empty()
                || !self.side_state_records.is_empty()
            {
                return Err(VmRootGatherError::JitDisabledVmRootRecordsPresent {
                    inactive_scratch_buffer_count: self.inactive_scratch_buffer_count,
                    scratch_buffer_record_count: self.scratch_buffer_records.len(),
                    side_state_record_count: self.side_state_records.len(),
                });
            }
        } else {
            for (record_order, record) in self.scratch_buffer_records.iter().enumerate() {
                if record.source != VmRootSource::ScratchBuffer {
                    return Err(VmRootGatherError::ScratchBufferSourceMismatch {
                        order: record_order,
                        actual: record.source,
                    });
                }
                validate_scratch_record(record)?;
                expected_spans.push(record.span);
                for (candidate_order, candidate) in record.candidates.iter().enumerate() {
                    if candidate.source != VmRootSource::ScratchBuffer {
                        return Err(VmRootGatherError::ScratchBufferCandidateSourceMismatch {
                            order: candidate_order,
                            actual: candidate.source,
                        });
                    }
                    if candidate.order != candidate_order {
                        return Err(VmRootGatherError::ScratchBufferCandidateSourceMismatch {
                            order: candidate_order,
                            actual: candidate.source,
                        });
                    }
                    if candidate.slot_address != record.span.begin + candidate.slot_offset {
                        return Err(
                            VmRootGatherError::ScratchBufferCandidateSlotAddressMismatch {
                                buffer: candidate.buffer,
                                offset: candidate.slot_offset,
                                expected: record.span.begin + candidate.slot_offset,
                                actual: candidate.slot_address,
                            },
                        );
                    }
                    if candidate.candidate_address != 0 {
                        expected_candidate_addresses.push(candidate.candidate_address);
                    }
                }
            }

            for (record_order, record) in self.side_state_records.iter().enumerate() {
                if record.source != VmRootSource::CheckpointOsrExitSideState {
                    return Err(VmRootGatherError::SideStateSourceMismatch {
                        order: record_order,
                        actual: record.source,
                    });
                }
                validate_side_state_record(record)?;
                expected_spans.push(record.span);
                for tmp in &record.tmp_records {
                    if tmp.candidate_address != 0 {
                        expected_candidate_addresses.push(tmp.candidate_address);
                    }
                }
            }
        }

        let actual_spans = self.conservative_roots.spans().to_vec();
        if expected_spans != actual_spans {
            return Err(VmRootGatherError::ConservativeRootSpanMismatch {
                expected: expected_spans,
                actual: actual_spans,
            });
        }

        let actual_candidate_addresses = self.conservative_roots.candidate_addresses().to_vec();
        if expected_candidate_addresses != actual_candidate_addresses {
            return Err(
                VmRootGatherError::ConservativeRootCandidateAddressMismatch {
                    expected: expected_candidate_addresses,
                    actual: actual_candidate_addresses,
                },
            );
        }

        for root in self.conservative_roots.validated_cells() {
            if root.candidate_address == 0 || root.cell == CellId::default() {
                return Err(VmRootGatherError::InvalidConservativeRootCell(*root));
            }
            if !actual_candidate_addresses.contains(&root.candidate_address) {
                return Err(VmRootGatherError::ValidatedCellCandidateMissing(*root));
            }
        }

        if self.span_count != self.conservative_roots.spans().len() {
            return Err(VmRootGatherError::SpanCountMismatch {
                expected: self.conservative_roots.spans().len(),
                actual: self.span_count,
            });
        }

        if self.candidate_address_count != self.conservative_roots.candidate_addresses().len() {
            return Err(VmRootGatherError::CandidateAddressCountMismatch {
                expected: self.conservative_roots.candidate_addresses().len(),
                actual: self.candidate_address_count,
            });
        }

        if self.validated_cell_count != self.conservative_roots.validated_cells().len() {
            return Err(VmRootGatherError::ValidatedCellCountMismatch {
                expected: self.conservative_roots.validated_cells().len(),
                actual: self.validated_cell_count,
            });
        }

        Ok(())
    }
}

fn validate_scratch_candidate_slots(
    scratch_buffer: &VmScratchBufferDescriptor,
) -> Result<(), VmRootGatherError> {
    let expected_slot_count = scratch_buffer.active_length / ENCODED_JS_VALUE_BYTES;
    if scratch_buffer.candidate_slots.len() != expected_slot_count {
        return Err(VmRootGatherError::ScratchBufferCandidateSlotCountMismatch {
            buffer: scratch_buffer.id,
            expected: expected_slot_count,
            actual: scratch_buffer.candidate_slots.len(),
        });
    }

    for (order, candidate) in scratch_buffer.candidate_slots.iter().enumerate() {
        let expected_offset = order * ENCODED_JS_VALUE_BYTES;
        if candidate.offset != expected_offset {
            return Err(VmRootGatherError::ScratchBufferCandidateSlotOrderMismatch {
                buffer: scratch_buffer.id,
                order,
                expected_offset,
                actual_offset: candidate.offset,
            });
        }
    }

    Ok(())
}

fn scratch_buffer_span(
    scratch_buffer: &VmScratchBufferDescriptor,
) -> Result<ConservativeRootSpan, VmRootGatherError> {
    if scratch_buffer.active_length > scratch_buffer.byte_length {
        return Err(
            VmRootGatherError::ScratchBufferActiveLengthExceedsCapacity {
                buffer: scratch_buffer.id,
                active_length: scratch_buffer.active_length,
                byte_length: scratch_buffer.byte_length,
            },
        );
    }
    if scratch_buffer.data_begin == 0 {
        return Err(VmRootGatherError::ScratchBufferDataBeginMissing {
            buffer: scratch_buffer.id,
        });
    }
    if scratch_buffer.active_length % ENCODED_JS_VALUE_BYTES != 0 {
        return Err(VmRootGatherError::ScratchBufferActiveLengthUnaligned {
            buffer: scratch_buffer.id,
            active_length: scratch_buffer.active_length,
        });
    }
    let end = scratch_buffer
        .data_begin
        .checked_add(scratch_buffer.active_length)
        .ok_or(VmRootGatherError::ScratchBufferRangeOverflow {
            buffer: scratch_buffer.id,
            data_begin: scratch_buffer.data_begin,
            active_length: scratch_buffer.active_length,
        })?;
    Ok(ConservativeRootSpan {
        begin: scratch_buffer.data_begin,
        end,
    })
}

fn validate_scratch_record(record: &VmScratchBufferRootRecord) -> Result<(), VmRootGatherError> {
    let descriptor = VmScratchBufferDescriptor {
        id: record.buffer,
        data_begin: record.span.begin,
        byte_length: record.byte_length,
        active_length: record.active_length,
        candidate_slots: Vec::new(),
    };
    let expected = scratch_buffer_span(&descriptor)?;
    if expected != record.span {
        return Err(VmRootGatherError::ConservativeRootSpanMismatch {
            expected: vec![expected],
            actual: vec![record.span],
        });
    }
    let expected_slot_count = record.active_length / ENCODED_JS_VALUE_BYTES;
    if record.candidates.len() != expected_slot_count {
        return Err(VmRootGatherError::ScratchBufferCandidateSlotCountMismatch {
            buffer: record.buffer,
            expected: expected_slot_count,
            actual: record.candidates.len(),
        });
    }
    for (candidate_order, candidate) in record.candidates.iter().enumerate() {
        if candidate.buffer != record.buffer {
            return Err(
                VmRootGatherError::ScratchBufferCandidateSlotAddressMismatch {
                    buffer: candidate.buffer,
                    offset: candidate.slot_offset,
                    expected: record.span.begin + candidate.slot_offset,
                    actual: candidate.slot_address,
                },
            );
        }
        let expected_offset = candidate_order * ENCODED_JS_VALUE_BYTES;
        if candidate.order != candidate_order || candidate.slot_offset != expected_offset {
            return Err(VmRootGatherError::ScratchBufferCandidateSlotOrderMismatch {
                buffer: candidate.buffer,
                order: candidate_order,
                expected_offset,
                actual_offset: candidate.slot_offset,
            });
        }
        if candidate.slot_offset % ENCODED_JS_VALUE_BYTES != 0 {
            return Err(VmRootGatherError::ScratchBufferCandidateSlotUnaligned {
                buffer: candidate.buffer,
                offset: candidate.slot_offset,
            });
        }
        if candidate.slot_offset >= record.active_length {
            return Err(
                VmRootGatherError::ScratchBufferCandidateSlotOutsideActiveLength {
                    buffer: candidate.buffer,
                    offset: candidate.slot_offset,
                    active_length: record.active_length,
                },
            );
        }
    }
    Ok(())
}

fn side_state_span(
    side_state: &VmCheckpointOsrExitSideStateDescriptor,
) -> Result<ConservativeRootSpan, VmRootGatherError> {
    if side_state.tmps_begin == 0 {
        return Err(VmRootGatherError::SideStateTmpsBeginMissing {
            side_state: side_state.id,
        });
    }
    if side_state.tmps_begin % ENCODED_JS_VALUE_BYTES != 0 {
        return Err(VmRootGatherError::SideStateTmpsBeginUnaligned {
            side_state: side_state.id,
            tmps_begin: side_state.tmps_begin,
        });
    }
    let end = side_state
        .tmps_begin
        .checked_add(MAX_NUM_CHECKPOINT_TMPS * ENCODED_JS_VALUE_BYTES)
        .ok_or(VmRootGatherError::SideStateRangeOverflow {
            side_state: side_state.id,
            tmps_begin: side_state.tmps_begin,
        })?;
    Ok(ConservativeRootSpan {
        begin: side_state.tmps_begin,
        end,
    })
}

fn validate_side_state_record(
    record: &VmCheckpointOsrExitSideStateRootRecord,
) -> Result<(), VmRootGatherError> {
    if record.tmp_records.len() != MAX_NUM_CHECKPOINT_TMPS {
        return Err(VmRootGatherError::SideStateTmpCountMismatch {
            side_state: record.side_state,
            expected: MAX_NUM_CHECKPOINT_TMPS,
            actual: record.tmp_records.len(),
        });
    }
    if record.span.begin == 0 {
        return Err(VmRootGatherError::SideStateTmpsBeginMissing {
            side_state: record.side_state,
        });
    }
    if record.span.begin % ENCODED_JS_VALUE_BYTES != 0 {
        return Err(VmRootGatherError::SideStateTmpsBeginUnaligned {
            side_state: record.side_state,
            tmps_begin: record.span.begin,
        });
    }
    let expected_end = record
        .span
        .begin
        .checked_add(MAX_NUM_CHECKPOINT_TMPS * ENCODED_JS_VALUE_BYTES)
        .ok_or(VmRootGatherError::SideStateRangeOverflow {
            side_state: record.side_state,
            tmps_begin: record.span.begin,
        })?;
    if record.span.end != expected_end {
        return Err(VmRootGatherError::ConservativeRootSpanMismatch {
            expected: vec![ConservativeRootSpan {
                begin: record.span.begin,
                end: expected_end,
            }],
            actual: vec![record.span],
        });
    }
    for (expected_index, tmp) in record.tmp_records.iter().enumerate() {
        if tmp.source != VmRootSource::CheckpointOsrExitSideState {
            return Err(VmRootGatherError::SideStateTmpSourceMismatch {
                order: expected_index,
                actual: tmp.source,
            });
        }
        if tmp.tmp_index != expected_index || tmp.order != expected_index {
            return Err(VmRootGatherError::SideStateTmpIndexMismatch {
                order: expected_index,
                expected: expected_index,
                actual: tmp.tmp_index,
            });
        }
        let expected_address = record.span.begin + expected_index * ENCODED_JS_VALUE_BYTES;
        if tmp.slot_address != expected_address {
            return Err(VmRootGatherError::SideStateTmpSlotAddressMismatch {
                side_state: tmp.side_state,
                tmp_index: tmp.tmp_index,
                expected: expected_address,
                actual: tmp.slot_address,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_buffer(active_length: usize) -> VmScratchBufferDescriptor {
        let slot_count = active_length / ENCODED_JS_VALUE_BYTES;
        VmScratchBufferDescriptor {
            id: VmScratchBufferId(1),
            data_begin: 0x1000,
            byte_length: 64,
            active_length,
            candidate_slots: (0..slot_count)
                .map(|slot| VmScratchBufferCandidateSlot {
                    offset: slot * ENCODED_JS_VALUE_BYTES,
                    candidate_address: if slot == 0 { 0x5000 } else { 0 },
                })
                .collect(),
        }
    }

    fn descriptor() -> VmRootGatherDescriptor {
        VmRootGatherDescriptor {
            heap: HeapId(7),
            marking_epoch: HeapEpoch(11),
            world_stopped: true,
            jit_enabled: true,
            scratch_buffers: Vec::new(),
            checkpoint_side_states: Vec::new(),
            validated_cells: Vec::new(),
        }
    }

    #[test]
    fn vm_root_gather_records_active_scratch_buffer_ranges() {
        let mut descriptor = descriptor();
        descriptor.scratch_buffers.push(scratch_buffer(16));
        descriptor.validated_cells.push(ConservativeRootCell {
            candidate_address: 0x5000,
            cell: CellId(3),
        });

        let plan = descriptor.gather_vm_roots().expect("vm root gather");

        assert_eq!(
            plan.conservative_roots.spans(),
            &[ConservativeRootSpan {
                begin: 0x1000,
                end: 0x1010,
            }]
        );
        assert_eq!(plan.conservative_roots.candidate_addresses(), &[0x5000]);
        assert_eq!(
            plan.conservative_roots.validated_cells(),
            &[ConservativeRootCell {
                candidate_address: 0x5000,
                cell: CellId(3),
            }]
        );
        assert_eq!(plan.scratch_buffer_records[0].active_length, 16);
        assert_eq!(plan.span_count, 1);
        assert_eq!(plan.candidate_address_count, 1);
        assert_eq!(plan.validated_cell_count, 1);
    }

    #[test]
    fn vm_root_gather_skips_inactive_scratch_buffers() {
        let mut descriptor = descriptor();
        descriptor.scratch_buffers.push(scratch_buffer(0));

        let plan = descriptor.gather_vm_roots().expect("vm root gather");

        assert_eq!(plan.inactive_scratch_buffer_count, 1);
        assert!(plan.scratch_buffer_records.is_empty());
        assert!(plan.conservative_roots.spans().is_empty());
        assert!(plan.conservative_roots.candidate_addresses().is_empty());
        assert!(plan.conservative_roots.validated_cells().is_empty());
    }

    #[test]
    fn vm_root_gather_rejects_missing_active_scratch_buffer_slot_evidence() {
        let mut descriptor = descriptor();
        let mut scratch = scratch_buffer(2 * ENCODED_JS_VALUE_BYTES);
        scratch.candidate_slots.pop();
        descriptor.scratch_buffers.push(scratch);

        assert_eq!(
            descriptor.gather_vm_roots(),
            Err(VmRootGatherError::ScratchBufferCandidateSlotCountMismatch {
                buffer: VmScratchBufferId(1),
                expected: 2,
                actual: 1,
            })
        );
    }

    #[test]
    fn vm_root_gather_includes_side_state_tmp_slots() {
        let mut descriptor = descriptor();
        descriptor
            .checkpoint_side_states
            .push(VmCheckpointOsrExitSideStateDescriptor {
                id: VmCheckpointOsrExitSideStateId(9),
                tmps_begin: 0x3000,
                tmp_slots: vec![0x5000, 0, 0x6000, 0x7000],
            });
        descriptor.validated_cells.push(ConservativeRootCell {
            candidate_address: 0x6000,
            cell: CellId(6),
        });

        let plan = descriptor.gather_vm_roots().expect("vm root gather");

        assert_eq!(
            plan.conservative_roots.spans(),
            &[ConservativeRootSpan {
                begin: 0x3000,
                end: 0x3000 + MAX_NUM_CHECKPOINT_TMPS * ENCODED_JS_VALUE_BYTES,
            }]
        );
        assert_eq!(
            plan.conservative_roots.candidate_addresses(),
            &[0x5000, 0x6000, 0x7000]
        );
        assert_eq!(plan.side_state_records[0].tmp_records.len(), 4);
        assert_eq!(
            plan.side_state_records[0].tmp_records[2].slot_address,
            0x3010
        );
        assert_eq!(
            plan.conservative_roots.validated_cells(),
            &[ConservativeRootCell {
                candidate_address: 0x6000,
                cell: CellId(6),
            }]
        );
    }

    #[test]
    fn vm_root_gather_rejects_malformed_side_state_tmp_count() {
        let mut descriptor = descriptor();
        descriptor
            .checkpoint_side_states
            .push(VmCheckpointOsrExitSideStateDescriptor {
                id: VmCheckpointOsrExitSideStateId(4),
                tmps_begin: 0x4000,
                tmp_slots: vec![0x5000, 0x6000, 0x7000],
            });

        assert_eq!(
            descriptor.gather_vm_roots(),
            Err(VmRootGatherError::SideStateTmpCountMismatch {
                side_state: VmCheckpointOsrExitSideStateId(4),
                expected: 4,
                actual: 3,
            })
        );
    }

    #[test]
    fn vm_root_gather_rejects_forged_source_mismatch() {
        let mut descriptor = descriptor();
        descriptor.scratch_buffers.push(scratch_buffer(8));
        let mut plan = descriptor.gather_vm_roots().expect("vm root gather");
        plan.scratch_buffer_records[0].source = VmRootSource::CheckpointOsrExitSideState;

        assert_eq!(
            plan.validate_consistency(),
            Err(VmRootGatherError::ScratchBufferSourceMismatch {
                order: 0,
                actual: VmRootSource::CheckpointOsrExitSideState,
            })
        );
    }

    #[test]
    fn vm_root_gather_rejects_forged_jit_disabled_vm_root_records() {
        let mut plan = VmRootGatherDescriptor {
            jit_enabled: false,
            ..descriptor()
        }
        .gather_vm_roots()
        .expect("jit-disabled VM root gather");
        plan.scratch_buffer_records.push(VmScratchBufferRootRecord {
            order: 0,
            source: VmRootSource::ScratchBuffer,
            buffer: VmScratchBufferId(1),
            span: ConservativeRootSpan {
                begin: 0x1000,
                end: 0x1000 + ENCODED_JS_VALUE_BYTES,
            },
            active_length: ENCODED_JS_VALUE_BYTES,
            byte_length: ENCODED_JS_VALUE_BYTES,
            candidates: vec![VmScratchBufferCandidateRecord {
                order: 0,
                source: VmRootSource::ScratchBuffer,
                buffer: VmScratchBufferId(1),
                slot_offset: 0,
                slot_address: 0x1000,
                candidate_address: 0,
            }],
        });

        assert_eq!(
            plan.validate_consistency(),
            Err(VmRootGatherError::JitDisabledVmRootRecordsPresent {
                inactive_scratch_buffer_count: 0,
                scratch_buffer_record_count: 1,
                side_state_record_count: 0,
            })
        );
    }
}
