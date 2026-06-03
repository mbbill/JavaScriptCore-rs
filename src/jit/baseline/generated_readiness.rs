//! Diagnostic readiness classification for generated baseline property loads.
//!
//! C++ Baseline JIT emits one `JIT::emit_op_*` body per bytecode site and
//! attaches DataIC/AccessCase state at that site. Rust's generated executor is
//! still a bytecode-dispatch shim, so this module records which generated-side
//! sidecar projection exists for the dispatched site. It must not affect JS
//! execution decisions.

use super::{
    named_property_sidecar_cache_key, property_handoff_site_for_instruction,
    BaselineGeneratedPropertyExecutionSidecars,
};
use crate::bytecode::instruction::DecodedInstruction;
use crate::bytecode::{CodeBlock, CoreOpcode};
use crate::jit::generated_metrics::BaselineGeneratedPropertyLoadSidecarReadiness;
use crate::jit::plan::{
    BaselineGeneratedPropertyHandoffPlan, BaselineGeneratedPropertyHandoffSite,
};
use crate::jit::{PropertyLoadAccessCasePlanKind, PropertyLoadGuardedCandidateKind};
use crate::runtime::CodeBlockId;

pub(super) fn property_load_sidecar_readiness_for_instruction(
    owner: CodeBlockId,
    code_block: &CodeBlock,
    instruction: DecodedInstruction<'_>,
    property_handoff_plan: Option<BaselineGeneratedPropertyHandoffPlan<'_>>,
    property_sidecars: Option<&BaselineGeneratedPropertyExecutionSidecars<'_, '_>>,
    opcode: Option<CoreOpcode>,
) -> BaselineGeneratedPropertyLoadSidecarReadiness {
    use BaselineGeneratedPropertyLoadSidecarReadiness as Readiness;

    let Some(opcode) = opcode else {
        return Readiness::NotPropertyLoad;
    };
    if !matches!(
        opcode,
        CoreOpcode::GetByName
            | CoreOpcode::GetGlobalObjectProperty
            | CoreOpcode::GetLength
            | CoreOpcode::GetByValue
    ) {
        return Readiness::NotPropertyLoad;
    }
    let Some(property_sidecars) = property_sidecars else {
        return Readiness::NoLoadSidecar;
    };
    let Ok(site) = property_handoff_site_for_instruction(
        owner,
        code_block,
        instruction,
        property_handoff_plan,
    ) else {
        return Readiness::NoLoadPlan;
    };
    property_load_sidecar_readiness_for_site(property_sidecars, &site)
}

fn property_load_sidecar_readiness_for_site(
    sidecars: &BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    site: &BaselineGeneratedPropertyHandoffSite,
) -> BaselineGeneratedPropertyLoadSidecarReadiness {
    use BaselineGeneratedPropertyLoadSidecarReadiness as Readiness;

    if !matches!(
        site.opcode,
        CoreOpcode::GetByName
            | CoreOpcode::GetGlobalObjectProperty
            | CoreOpcode::GetLength
            | CoreOpcode::GetByValue
    ) {
        return Readiness::NotPropertyLoad;
    }

    if let Some(megamorphic_table) = sidecars.property_load_megamorphic_candidate_table {
        if megamorphic_table.owner() == site.owner && site.opcode == CoreOpcode::GetByName {
            if let Some(site_key) = named_property_sidecar_cache_key(site) {
                if megamorphic_table.contains_site(
                    site.slot,
                    site.bytecode_index.offset(),
                    site_key,
                ) {
                    return Readiness::MegamorphicLoadSite;
                }
            }
        }
    }

    if let Some(plan_table) = sidecars.property_load_plan_table {
        if plan_table.owner() == site.owner {
            match site.opcode {
                CoreOpcode::GetByName
                | CoreOpcode::GetGlobalObjectProperty
                | CoreOpcode::GetLength => {
                    if let Some(site_key) = named_property_sidecar_cache_key(site) {
                        if plan_table
                            .candidates_for_bytecode_index_newest_first(
                                site.bytecode_index.offset(),
                            )
                            .any(|plan| {
                                plan.plan_kind == PropertyLoadAccessCasePlanKind::DataOnlyOwnLoad
                                    && plan.key == site_key
                            })
                        {
                            return Readiness::OwnDataPlan;
                        }
                    }
                }
                CoreOpcode::GetByValue => {
                    if plan_table
                        .candidates_for_bytecode_index_newest_first(site.bytecode_index.offset())
                        .any(|plan| {
                            plan.plan_kind == PropertyLoadAccessCasePlanKind::DataOnlyIndexedLoad
                        })
                    {
                        return Readiness::IndexedDataPlan;
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(guarded_candidate_table) = sidecars.property_load_guarded_candidate_table {
        if guarded_candidate_table.owner() == site.owner
            && matches!(site.opcode, CoreOpcode::GetByName | CoreOpcode::GetLength)
        {
            if let Some(site_key) = named_property_sidecar_cache_key(site) {
                for candidate in guarded_candidate_table
                    .candidates_for_bytecode_index(site.bytecode_index.offset())
                {
                    if candidate.plan.descriptor.key != site_key {
                        continue;
                    }
                    return match candidate.candidate_kind {
                        PropertyLoadGuardedCandidateKind::PrototypeData => {
                            Readiness::GuardedPrototypeData
                        }
                        PropertyLoadGuardedCandidateKind::NegativeLookup => {
                            Readiness::GuardedNegativeLookup
                        }
                    };
                }
            }
        }
    }

    Readiness::NoLoadPlan
}
