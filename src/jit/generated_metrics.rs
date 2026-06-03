//! Runtime metrics for the temporary baseline-generated executor.
//!
//! C++ Baseline JIT emits machine code per bytecode in `JIT.cpp` and routes
//! calls/properties through generated fast paths plus slow-path thunks. Rust's
//! generated executor is still a bytecode-dispatch shim, so these metrics are a
//! Rust diagnostic bridge: they identify which C++ `JIT::emit_op_*` families
//! should be ported next without changing generated-code behavior.

use crate::bytecode::{BytecodeIndex, CoreOpcode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedLoopHintObservation {
    pub(crate) bytecode_index: BytecodeIndex,
    pub(crate) count: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedDispatchedOpcodeCount {
    pub(crate) opcode: CoreOpcode,
    pub(crate) count: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct BaselineGeneratedExecutionMetrics {
    pub(crate) executed_bytecode_count: u64,
    loop_hint_observations: Vec<BaselineGeneratedLoopHintObservation>,
    dispatched_opcode_counts: Vec<BaselineGeneratedDispatchedOpcodeCount>,
}

impl BaselineGeneratedExecutionMetrics {
    pub(crate) fn record_dispatched_instruction(
        &mut self,
        bytecode_index: BytecodeIndex,
        opcode: Option<CoreOpcode>,
    ) {
        self.executed_bytecode_count = self.executed_bytecode_count.saturating_add(1);
        let Some(opcode) = opcode else {
            return;
        };
        self.record_dispatched_opcode(opcode);
        if opcode == CoreOpcode::LoopHint {
            self.record_loop_hint(bytecode_index);
        }
    }

    pub(crate) fn record_skipped_bytecodes(&mut self, count: u64) {
        self.executed_bytecode_count = self.executed_bytecode_count.saturating_add(count);
    }

    fn record_dispatched_opcode(&mut self, opcode: CoreOpcode) {
        if let Some(record) = self
            .dispatched_opcode_counts
            .iter_mut()
            .find(|record| record.opcode == opcode)
        {
            record.count = record.count.saturating_add(1);
            return;
        }

        self.dispatched_opcode_counts
            .push(BaselineGeneratedDispatchedOpcodeCount { opcode, count: 1 });
    }

    fn record_loop_hint(&mut self, bytecode_index: BytecodeIndex) {
        if let Some(observation) = self
            .loop_hint_observations
            .iter_mut()
            .find(|observation| observation.bytecode_index == bytecode_index)
        {
            observation.count = observation.count.saturating_add(1);
            return;
        }

        self.loop_hint_observations
            .push(BaselineGeneratedLoopHintObservation {
                bytecode_index,
                count: 1,
            });
    }

    pub(crate) fn loop_hint_observations(&self) -> &[BaselineGeneratedLoopHintObservation] {
        &self.loop_hint_observations
    }

    pub(crate) fn dispatched_opcode_counts(&self) -> &[BaselineGeneratedDispatchedOpcodeCount] {
        &self.dispatched_opcode_counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_dispatched_opcode_heat_separately_from_skipped_bytecodes() {
        let loop_hint_index = BytecodeIndex::from_offset(24);
        let mut metrics = BaselineGeneratedExecutionMetrics::default();

        metrics.record_dispatched_instruction(
            BytecodeIndex::from_offset(8),
            Some(CoreOpcode::LoadInt32),
        );
        metrics.record_dispatched_instruction(
            BytecodeIndex::from_offset(16),
            Some(CoreOpcode::LoadInt32),
        );
        metrics.record_dispatched_instruction(loop_hint_index, Some(CoreOpcode::LoopHint));
        metrics.record_skipped_bytecodes(4);

        assert_eq!(metrics.executed_bytecode_count, 7);
        assert_eq!(
            metrics.dispatched_opcode_counts(),
            &[
                BaselineGeneratedDispatchedOpcodeCount {
                    opcode: CoreOpcode::LoadInt32,
                    count: 2,
                },
                BaselineGeneratedDispatchedOpcodeCount {
                    opcode: CoreOpcode::LoopHint,
                    count: 1,
                },
            ]
        );
        assert_eq!(
            metrics.loop_hint_observations(),
            &[BaselineGeneratedLoopHintObservation {
                bytecode_index: loop_hint_index,
                count: 1,
            }]
        );
    }
}
