//! VM aggregation for temporary baseline-generated executor metrics.
//!
//! C++ Baseline JIT emits one machine-code path per bytecode family from
//! `JIT.cpp`. Rust still routes generated entries through a bytecode-dispatch
//! shim, so VM tiering keeps owner-scoped opcode heat only to select the next
//! C++ `JIT::emit_op_*` family to port.

use crate::bytecode::{BytecodeIndex, CoreOpcode};
use crate::jit::generated_metrics::{
    BaselineGeneratedDispatchedOpcodeCount, BaselineGeneratedDispatchedSiteOpcodeCount,
    BaselineGeneratedPropertyLoadSidecarReadiness,
};
use crate::runtime::CodeBlockId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmBaselineGeneratedDispatchedOpcodeCount {
    pub owner: CodeBlockId,
    pub opcode: CoreOpcode,
    pub count: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmBaselineGeneratedDispatchedSiteOpcodeCount {
    pub owner: CodeBlockId,
    pub bytecode_index: BytecodeIndex,
    pub opcode: CoreOpcode,
    pub property_load_sidecar_readiness: BaselineGeneratedPropertyLoadSidecarReadiness,
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

pub(crate) fn record_vm_baseline_generated_dispatched_site_opcode_counts(
    summaries: &mut Vec<VmBaselineGeneratedDispatchedSiteOpcodeCount>,
    owner: CodeBlockId,
    site_opcode_counts: &[BaselineGeneratedDispatchedSiteOpcodeCount],
) {
    for site_opcode_count in site_opcode_counts {
        if let Some(summary) = summaries.iter_mut().find(|summary| {
            summary.owner == owner
                && summary.bytecode_index == site_opcode_count.bytecode_index
                && summary.opcode == site_opcode_count.opcode
                && summary.property_load_sidecar_readiness
                    == site_opcode_count.property_load_sidecar_readiness
        }) {
            summary.count = summary.count.saturating_add(site_opcode_count.count);
            continue;
        }

        summaries.push(VmBaselineGeneratedDispatchedSiteOpcodeCount {
            owner,
            bytecode_index: site_opcode_count.bytecode_index,
            opcode: site_opcode_count.opcode,
            property_load_sidecar_readiness: site_opcode_count.property_load_sidecar_readiness,
            count: site_opcode_count.count,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(value: u32) -> CodeBlockId {
        CodeBlockId(crate::gc::CellId(value))
    }

    #[test]
    fn aggregates_site_opcode_counts_by_owner_site_opcode_and_readiness() {
        let mut summaries = Vec::new();
        let bytecode_index = BytecodeIndex::from_offset(19);

        record_vm_baseline_generated_dispatched_site_opcode_counts(
            &mut summaries,
            owner(7),
            &[
                BaselineGeneratedDispatchedSiteOpcodeCount {
                    bytecode_index,
                    opcode: CoreOpcode::GetByName,
                    property_load_sidecar_readiness:
                        BaselineGeneratedPropertyLoadSidecarReadiness::OwnDataPlan,
                    count: 3,
                },
                BaselineGeneratedDispatchedSiteOpcodeCount {
                    bytecode_index,
                    opcode: CoreOpcode::GetByName,
                    property_load_sidecar_readiness:
                        BaselineGeneratedPropertyLoadSidecarReadiness::GuardedPrototypeData,
                    count: 2,
                },
            ],
        );
        record_vm_baseline_generated_dispatched_site_opcode_counts(
            &mut summaries,
            owner(7),
            &[BaselineGeneratedDispatchedSiteOpcodeCount {
                bytecode_index,
                opcode: CoreOpcode::GetByName,
                property_load_sidecar_readiness:
                    BaselineGeneratedPropertyLoadSidecarReadiness::OwnDataPlan,
                count: 5,
            }],
        );

        assert_eq!(
            summaries,
            vec![
                VmBaselineGeneratedDispatchedSiteOpcodeCount {
                    owner: owner(7),
                    bytecode_index,
                    opcode: CoreOpcode::GetByName,
                    property_load_sidecar_readiness:
                        BaselineGeneratedPropertyLoadSidecarReadiness::OwnDataPlan,
                    count: 8,
                },
                VmBaselineGeneratedDispatchedSiteOpcodeCount {
                    owner: owner(7),
                    bytecode_index,
                    opcode: CoreOpcode::GetByName,
                    property_load_sidecar_readiness:
                        BaselineGeneratedPropertyLoadSidecarReadiness::GuardedPrototypeData,
                    count: 2,
                },
            ]
        );
    }
}
