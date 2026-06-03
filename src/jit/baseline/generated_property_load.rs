//! Generated baseline property-load sidecar execution.
//!
//! This module maps the Rust generated-entry property-load sidecar to C++
//! `JITPropertyAccess.cpp` GetById/GetLength DataIC setup and
//! `JITInlineCacheGenerator.cpp` Baseline DataIC inline access generation.
//! Rust still executes a generated-body sidecar through the interpreter
//! register boundary. The established Rust sidecar priority remains
//! megamorphic/own before guarded; the guarded-prototype hit shape and receiver
//! structure guard mirror the C++ inline cache facts.

use super::{
    element_sidecar_cache_key_from_runtime_value, execution_error_abort,
    named_property_sidecar_cache_key, read_register_or_outcome, register_operand_or_fallback,
    write_register_or_outcome, BaselineGeneratedGuardedPropertyLoadProbeMissRecord,
    BaselineGeneratedPropertyExecutionSidecars,
    BaselineGeneratedPropertyLoadDestinationRootSyncRequest,
    BaselineGeneratedPropertyLoadProbeMissRecord, BaselineInstructionAbort,
    BaselineInstructionOutcome, PropertyLoadSidecarAttempt,
};
use crate::bytecode::instruction::DecodedInstruction;
use crate::bytecode::{BytecodeIndex, CodeBlock, CoreOpcode, VirtualRegister};
use crate::gc::StructureId;
use crate::interpreter::{DispatchHost, InterpreterExecutionState, RegisterWindow};
use crate::jit::{
    CacheKey, GeneratedGuardedPropertyLoadProbeRequest, GeneratedGuardedPropertyLoadProbeResult,
    GeneratedPropertyLoadMegamorphicHolderProbeRequest, GeneratedPropertyLoadMegamorphicLookup,
    GeneratedPropertyLoadProbeRequest, GeneratedPropertyLoadProbeResult,
    PropertyLoadAccessCasePlan, PropertyLoadAccessCasePlanKind, PropertyLoadGuardChainOutcome,
    PropertyLoadGuardedCandidate, PropertyLoadGuardedCandidateKind,
};
use crate::object::PropertyOffset;
use crate::runtime::{ObjectId, RuntimeValue};

pub(super) fn execute_property_load_sidecar_candidate(
    sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: PropertyLoadSidecarAttempt<'_, '_, '_>,
) -> Result<Option<BaselineInstructionOutcome>, BaselineInstructionAbort> {
    let PropertyLoadSidecarAttempt {
        window,
        code_block,
        fallback,
        frame,
        instruction,
        site,
    } = attempt;

    let mut operands = None;
    let BaselineGeneratedPropertyExecutionSidecars {
        property_load_plan_table,
        property_load_guarded_candidate_table,
        property_load_megamorphic_candidate_table,
        dispatch_host,
        destination_root_sync_requests,
        property_load_probe_miss_records,
        guarded_property_load_probe_miss_records,
        ..
    } = sidecars;

    let mut current_base_structure = None;

    if let Some(megamorphic_table) = *property_load_megamorphic_candidate_table {
        if megamorphic_table.owner() == site.owner && site.opcode == CoreOpcode::GetByName {
            let Some(site_key) = named_property_sidecar_cache_key(site) else {
                return Ok(None);
            };
            if megamorphic_table.contains_site(site.slot, site.bytecode_index.offset(), site_key) {
                if dispatch_host.has_pending_structure_chain_invalidation_events() {
                    return Ok(None);
                }
                let (destination, base, _) = property_load_sidecar_operands(
                    &mut operands,
                    execution,
                    code_block,
                    window,
                    instruction,
                    fallback,
                )?;
                let actual_structure = *current_base_structure.get_or_insert_with(|| {
                    dispatch_host.generated_property_sidecar_base_structure(base)
                });
                let lookup = match actual_structure {
                    Some(actual_structure) => megamorphic_table.lookup(
                        site.slot,
                        site.bytecode_index.offset(),
                        site_key,
                        actual_structure,
                    ),
                    None => return Ok(None),
                };
                match lookup {
                    GeneratedPropertyLoadMegamorphicLookup::NoSite => {}
                    GeneratedPropertyLoadMegamorphicLookup::Miss => return Ok(None),
                    GeneratedPropertyLoadMegamorphicLookup::Missing => {
                        let outcome = write_register_or_outcome(
                            execution,
                            window,
                            destination,
                            RuntimeValue::undefined(),
                            fallback,
                        )?;
                        return Ok(Some(outcome));
                    }
                    GeneratedPropertyLoadMegamorphicLookup::PrototypeData {
                        key,
                        base_structure,
                        holder,
                        offset,
                    } => {
                        let result = dispatch_host
                            .probe_generated_property_load_megamorphic_holder(
                                GeneratedPropertyLoadMegamorphicHolderProbeRequest {
                                    key,
                                    base_structure,
                                    holder,
                                    offset,
                                },
                            );
                        let hit = match result {
                            GeneratedPropertyLoadProbeResult::Hit(hit) => hit,
                            GeneratedPropertyLoadProbeResult::Miss(miss) => {
                                property_load_probe_miss_records.push(
                                    BaselineGeneratedPropertyLoadProbeMissRecord {
                                        owner: site.owner,
                                        bytecode_index: site.bytecode_index,
                                        key,
                                        base_structure: Some(base_structure),
                                        offset: Some(offset),
                                        reason: miss.reason,
                                    },
                                );
                                return Ok(None);
                            }
                        };

                        let outcome = write_register_or_outcome(
                            execution,
                            window,
                            destination,
                            hit.value,
                            fallback,
                        )?;
                        if hit.destination_root_sync.requires_targeted_register_sync() {
                            destination_root_sync_requests.push(
                                BaselineGeneratedPropertyLoadDestinationRootSyncRequest {
                                    frame,
                                    bytecode_index: site.bytecode_index,
                                    destination,
                                },
                            );
                        }
                        return Ok(Some(outcome));
                    }
                    GeneratedPropertyLoadMegamorphicLookup::Hit(plan) => {
                        let result = dispatch_host.probe_generated_property_load(
                            GeneratedPropertyLoadProbeRequest { plan: &plan, base },
                        );
                        let hit = match result {
                            GeneratedPropertyLoadProbeResult::Hit(hit) => hit,
                            GeneratedPropertyLoadProbeResult::Miss(miss) => {
                                property_load_probe_miss_records.push(
                                    BaselineGeneratedPropertyLoadProbeMissRecord {
                                        owner: plan.owner,
                                        bytecode_index: BytecodeIndex::from_offset(
                                            plan.bytecode_index,
                                        ),
                                        key: plan.key,
                                        base_structure: plan.access_case.base_structure,
                                        offset: plan.access_case.offset,
                                        reason: miss.reason,
                                    },
                                );
                                return Ok(None);
                            }
                        };

                        let outcome = write_register_or_outcome(
                            execution,
                            window,
                            destination,
                            hit.value,
                            fallback,
                        )?;
                        if hit.destination_root_sync.requires_targeted_register_sync() {
                            destination_root_sync_requests.push(
                                BaselineGeneratedPropertyLoadDestinationRootSyncRequest {
                                    frame,
                                    bytecode_index: site.bytecode_index,
                                    destination,
                                },
                            );
                        }
                        return Ok(Some(outcome));
                    }
                }
            }
        }
    }

    if let Some(plan_table) = *property_load_plan_table {
        if plan_table.owner() == site.owner {
            for plan in
                plan_table.candidates_for_bytecode_index_newest_first(site.bytecode_index.offset())
            {
                let (destination, base, runtime_key) = property_load_sidecar_operands(
                    &mut operands,
                    execution,
                    code_block,
                    window,
                    instruction,
                    fallback,
                )?;
                let probe_plan;
                let plan = match site.opcode {
                    CoreOpcode::GetByName
                    | CoreOpcode::GetGlobalObjectProperty
                    | CoreOpcode::GetLength => {
                        let Some(site_key) = named_property_sidecar_cache_key(site) else {
                            return Ok(None);
                        };
                        if plan.plan_kind != PropertyLoadAccessCasePlanKind::DataOnlyOwnLoad
                            || plan.key != site_key
                        {
                            continue;
                        }
                        plan
                    }
                    CoreOpcode::GetByValue => {
                        if plan.plan_kind != PropertyLoadAccessCasePlanKind::DataOnlyIndexedLoad {
                            continue;
                        }
                        let Some(runtime_key) = runtime_key else {
                            return Ok(None);
                        };
                        probe_plan = property_load_plan_with_runtime_key(plan, runtime_key);
                        &probe_plan
                    }
                    _ => return Ok(None),
                };

                if property_load_sidecar_structure_guard_misses(
                    &mut **dispatch_host,
                    &mut current_base_structure,
                    base,
                    plan,
                ) {
                    continue;
                }

                let result = dispatch_host.probe_generated_property_load(
                    GeneratedPropertyLoadProbeRequest { plan, base },
                );
                let hit = match result {
                    GeneratedPropertyLoadProbeResult::Hit(hit) => hit,
                    GeneratedPropertyLoadProbeResult::Miss(miss) => {
                        property_load_probe_miss_records.push(
                            BaselineGeneratedPropertyLoadProbeMissRecord {
                                owner: plan.owner,
                                bytecode_index: BytecodeIndex::from_offset(plan.bytecode_index),
                                key: plan.key,
                                base_structure: plan.access_case.base_structure,
                                offset: plan.access_case.offset,
                                reason: miss.reason,
                            },
                        );
                        continue;
                    }
                };

                let outcome =
                    write_register_or_outcome(execution, window, destination, hit.value, fallback)?;
                if hit.destination_root_sync.requires_targeted_register_sync() {
                    destination_root_sync_requests.push(
                        BaselineGeneratedPropertyLoadDestinationRootSyncRequest {
                            frame,
                            bytecode_index: site.bytecode_index,
                            destination,
                        },
                    );
                }
                return Ok(Some(outcome));
            }
        }
    }

    if let Some(outcome) = try_guarded_prototype_data_inline_access(
        *property_load_guarded_candidate_table,
        &mut **dispatch_host,
        destination_root_sync_requests,
        &mut current_base_structure,
        &mut operands,
        execution,
        window,
        code_block,
        fallback,
        frame,
        instruction,
        site,
    )? {
        return Ok(Some(outcome));
    }

    if let Some(guarded_candidate_table) = *property_load_guarded_candidate_table {
        if guarded_candidate_table.owner() == site.owner {
            for candidate in
                guarded_candidate_table.candidates_for_bytecode_index(site.bytecode_index.offset())
            {
                if !matches!(site.opcode, CoreOpcode::GetByName | CoreOpcode::GetLength) {
                    continue;
                }
                let Some(site_key) = named_property_sidecar_cache_key(site) else {
                    return Ok(None);
                };
                let plan = &candidate.plan;
                if plan.descriptor.key != site_key {
                    continue;
                }

                let (destination, base, _) = property_load_sidecar_operands(
                    &mut operands,
                    execution,
                    code_block,
                    window,
                    instruction,
                    fallback,
                )?;

                let result = dispatch_host.probe_generated_guarded_property_load(
                    GeneratedGuardedPropertyLoadProbeRequest::new(plan, base),
                );
                let hit = match result {
                    GeneratedGuardedPropertyLoadProbeResult::Hit(hit) => hit,
                    GeneratedGuardedPropertyLoadProbeResult::Miss(miss) => {
                        guarded_property_load_probe_miss_records.push(
                            BaselineGeneratedGuardedPropertyLoadProbeMissRecord {
                                owner: plan.owner,
                                bytecode_index: BytecodeIndex::from_offset(plan.bytecode_index),
                                slot: plan.slot,
                                guard_plan_ordinal: candidate.guard_plan_ordinal,
                                materialization_ordinal: candidate.materialization_ordinal,
                                dependency_ordinals: candidate.dependency_ordinals.clone(),
                                binding_set_ids: candidate.binding_set_ids.clone(),
                                candidate_kind: candidate.candidate_kind,
                                base_structure: plan.descriptor.base_structure,
                                reason: miss.reason,
                                requirement: miss.requirement,
                                key: miss.key,
                                prototype_depth: miss.prototype_depth,
                                chain_index: miss.chain_index,
                                outcome: miss.outcome,
                            },
                        );
                        continue;
                    }
                };

                let outcome =
                    write_register_or_outcome(execution, window, destination, hit.value, fallback)?;
                if hit.destination_root_sync.requires_targeted_register_sync() {
                    destination_root_sync_requests.push(
                        BaselineGeneratedPropertyLoadDestinationRootSyncRequest {
                            frame,
                            bytecode_index: site.bytecode_index,
                            destination,
                        },
                    );
                }
                return Ok(Some(outcome));
            }
        }
    }

    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn try_guarded_prototype_data_inline_access(
    guarded_candidate_table: Option<&crate::jit::PropertyLoadGuardedCandidateTable>,
    dispatch_host: &mut dyn DispatchHost,
    destination_root_sync_requests: &mut Vec<
        BaselineGeneratedPropertyLoadDestinationRootSyncRequest,
    >,
    current_base_structure: &mut Option<Option<StructureId>>,
    operands: &mut Option<(VirtualRegister, RuntimeValue, Option<CacheKey>)>,
    execution: &mut InterpreterExecutionState<'_>,
    window: RegisterWindow,
    code_block: &CodeBlock,
    fallback: super::BaselineGeneratedFallbackSite,
    frame: crate::runtime::CallFrameId,
    instruction: DecodedInstruction<'_>,
    site: &crate::jit::plan::BaselineGeneratedPropertyHandoffSite,
) -> Result<Option<BaselineInstructionOutcome>, BaselineInstructionAbort> {
    if !matches!(site.opcode, CoreOpcode::GetByName | CoreOpcode::GetLength) {
        return Ok(None);
    }
    let Some(guarded_candidate_table) = guarded_candidate_table else {
        return Ok(None);
    };
    if guarded_candidate_table.owner() != site.owner {
        return Ok(None);
    };
    let Some(site_key) = named_property_sidecar_cache_key(site) else {
        return Ok(None);
    };
    if dispatch_host.has_pending_structure_chain_invalidation_events() {
        return Ok(None);
    }

    for candidate in
        guarded_candidate_table.candidates_for_bytecode_index(site.bytecode_index.offset())
    {
        let Some((key, base_structure, holder, offset)) =
            guarded_prototype_data_inline_access_descriptor(candidate, site, site_key)
        else {
            continue;
        };

        let (destination, base, _) = property_load_sidecar_operands(
            operands,
            execution,
            code_block,
            window,
            instruction,
            fallback,
        )?;

        let actual_structure = *current_base_structure
            .get_or_insert_with(|| dispatch_host.generated_property_sidecar_base_structure(base));
        if !matches!(actual_structure, Some(actual_structure) if actual_structure == base_structure)
        {
            continue;
        }

        let result = dispatch_host.probe_generated_property_load_megamorphic_holder(
            GeneratedPropertyLoadMegamorphicHolderProbeRequest {
                key,
                base_structure,
                holder,
                offset,
            },
        );
        let hit = match result {
            GeneratedPropertyLoadProbeResult::Hit(hit) => hit,
            GeneratedPropertyLoadProbeResult::Miss(_) => continue,
        };

        let outcome =
            write_register_or_outcome(execution, window, destination, hit.value, fallback)?;
        if hit.destination_root_sync.requires_targeted_register_sync() {
            destination_root_sync_requests.push(
                BaselineGeneratedPropertyLoadDestinationRootSyncRequest {
                    frame,
                    bytecode_index: site.bytecode_index,
                    destination,
                },
            );
        }
        return Ok(Some(outcome));
    }

    Ok(None)
}

fn guarded_prototype_data_inline_access_descriptor(
    candidate: &PropertyLoadGuardedCandidate,
    site: &crate::jit::plan::BaselineGeneratedPropertyHandoffSite,
    site_key: CacheKey,
) -> Option<(CacheKey, StructureId, ObjectId, PropertyOffset)> {
    let plan = &candidate.plan;
    if plan.owner != site.owner
        || plan.slot != site.slot
        || plan.bytecode_index != site.bytecode_index.offset()
        || plan.descriptor.key != site_key
        || candidate.candidate_kind != PropertyLoadGuardedCandidateKind::PrototypeData
        || plan.descriptor.prototype_depth != 1
    {
        return None;
    }
    let PropertyLoadGuardChainOutcome::PrototypeData { offset, .. } = plan.descriptor.chain.outcome
    else {
        return None;
    };
    if plan.descriptor.offset != Some(offset) {
        return None;
    }
    let holder = plan.descriptor.holder_object?;
    Some((
        plan.descriptor.key,
        plan.descriptor.base_structure,
        holder,
        offset,
    ))
}

fn property_load_sidecar_structure_guard_misses(
    dispatch_host: &mut dyn DispatchHost,
    current_base_structure: &mut Option<Option<StructureId>>,
    base: RuntimeValue,
    plan: &PropertyLoadAccessCasePlan,
) -> bool {
    if plan.plan_kind != PropertyLoadAccessCasePlanKind::DataOnlyOwnLoad {
        return false;
    }
    let Some(expected_structure) = plan.access_case.base_structure else {
        return false;
    };
    if expected_structure == StructureId::INVALID {
        return false;
    }
    let actual_structure = *current_base_structure
        .get_or_insert_with(|| dispatch_host.generated_property_sidecar_base_structure(base));
    matches!(actual_structure, Some(actual_structure) if actual_structure != expected_structure)
}

fn property_load_plan_with_runtime_key(
    plan: &crate::jit::PropertyLoadAccessCasePlan,
    runtime_key: CacheKey,
) -> crate::jit::PropertyLoadAccessCasePlan {
    let mut plan = plan.clone();
    plan.key = runtime_key;
    plan.access_case.key = runtime_key;
    plan
}

fn property_load_sidecar_operands(
    operands: &mut Option<(VirtualRegister, RuntimeValue, Option<CacheKey>)>,
    execution: &mut InterpreterExecutionState<'_>,
    code_block: &CodeBlock,
    window: RegisterWindow,
    instruction: DecodedInstruction<'_>,
    fallback: super::BaselineGeneratedFallbackSite,
) -> Result<(VirtualRegister, RuntimeValue, Option<CacheKey>), BaselineInstructionAbort> {
    if let Some(operands) = *operands {
        return Ok(operands);
    }

    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let opcode = CoreOpcode::from_opcode(instruction.opcode);
    let (base, runtime_key) = match opcode {
        Some(CoreOpcode::GetGlobalObjectProperty) => (
            execution
                .stack
                .active_global_this_value()
                .map_err(execution_error_abort)?,
            None,
        ),
        Some(CoreOpcode::GetByValue) => {
            let base_register = register_operand_or_fallback(instruction, 1, fallback)?;
            let base =
                read_register_or_outcome(execution, code_block, window, base_register, fallback)?;
            let key_register = register_operand_or_fallback(instruction, 2, fallback)?;
            let key_value =
                read_register_or_outcome(execution, code_block, window, key_register, fallback)?;
            (
                base,
                element_sidecar_cache_key_from_runtime_value(key_value),
            )
        }
        _ => {
            let base_register = register_operand_or_fallback(instruction, 1, fallback)?;
            (
                read_register_or_outcome(execution, code_block, window, base_register, fallback)?,
                None,
            )
        }
    };
    let decoded_operands = (destination, base, runtime_key);
    *operands = Some(decoded_operands);
    Ok(decoded_operands)
}
