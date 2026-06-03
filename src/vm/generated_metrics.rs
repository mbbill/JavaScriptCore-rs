//! VM aggregation for temporary baseline-generated executor metrics.
//!
//! C++ Baseline JIT emits one machine-code path per bytecode family from
//! `JIT.cpp`. Rust still routes generated entries through a bytecode-dispatch
//! shim, so VM tiering keeps owner-scoped opcode heat only to select the next
//! C++ `JIT::emit_op_*` family to port.

use crate::bytecode::CoreOpcode;
use crate::jit::generated_metrics::BaselineGeneratedDispatchedOpcodeCount;
use crate::runtime::CodeBlockId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmBaselineGeneratedDispatchedOpcodeCount {
    pub owner: CodeBlockId,
    pub opcode: CoreOpcode,
    pub count: u64,
}

pub(crate) fn record_vm_baseline_generated_dispatched_opcode_counts(
    summaries: &mut Vec<VmBaselineGeneratedDispatchedOpcodeCount>,
    owner: CodeBlockId,
    opcode_counts: &[BaselineGeneratedDispatchedOpcodeCount],
) {
    for opcode_count in opcode_counts {
        if let Some(summary) = summaries
            .iter_mut()
            .find(|summary| summary.owner == owner && summary.opcode == opcode_count.opcode)
        {
            summary.count = summary.count.saturating_add(opcode_count.count);
            continue;
        }

        summaries.push(VmBaselineGeneratedDispatchedOpcodeCount {
            owner,
            opcode: opcode_count.opcode,
            count: opcode_count.count,
        });
    }
}
