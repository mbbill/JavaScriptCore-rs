//! Generated baseline call-link sidecar execution.
//!
//! This module maps the Rust generated-entry call sidecar to C++ baseline
//! `JITCall.cpp` call-frame setup plus `CallLinkInfo.cpp` DataIC fast-path
//! callee matching. Rust still executes a sidecar through the interpreter
//! register boundary, so the payload captured here is a same-dispatch frame
//! setup snapshot rather than target/route authority; `vm/call_link.rs` still
//! owns the slow `linkFor()`-style target, arity, artifact, continuation, and
//! rooting checks before a direct call can execute.

use super::{
    object_id_for_runtime_value, read_register_or_outcome, register_operand_or_fallback,
    unsigned_immediate_operand_or_fallback, write_register_or_outcome,
    BaselineGeneratedCallLinkExecutionSidecar, BaselineGeneratedCallLinkProbeBlockedRecord,
    BaselineGeneratedCallLinkProbeMissRecord, BaselineGeneratedFallbackSite,
    BaselineGeneratedJsDirectCall, BaselineGeneratedJsDirectCallHotSlot,
    BaselineGeneratedJsDirectCallSetupPayload, BaselineGeneratedPropertyExecutionSidecars,
    BaselineInstructionAbort, BaselineInstructionOutcome,
};
use crate::bytecode::instruction::DecodedInstruction;
use crate::bytecode::{BytecodeIndex, CodeBlock, CoreOpcode};
use crate::interpreter::{
    DispatchHost, GeneratedNativeIntrinsicCallRequest, GeneratedNativeIntrinsicCallResult,
    InterpreterExecutionState, RegisterWindow,
};
use crate::jit::{
    GeneratedCallLinkCandidate, GeneratedCallLinkCandidateTable, GeneratedCallLinkDirectCallStatus,
    GeneratedCallLinkProbeMissReason, GeneratedCallLinkProbeRequest, GeneratedCallLinkProbeResult,
};
use crate::runtime::{CodeBlockId, ObjectId, RuntimeValue};
use crate::value::ValueKind;

pub(super) struct GeneratedCallLinkSidecarAttempt<'code, 'instruction> {
    pub(super) window: RegisterWindow,
    pub(super) code_block: &'code CodeBlock,
    pub(super) fallback: BaselineGeneratedFallbackSite,
    pub(super) owner: CodeBlockId,
    pub(super) opcode: CoreOpcode,
    pub(super) instruction: DecodedInstruction<'instruction>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GeneratedCallLinkSidecarOperands {
    argument_count_including_this: u32,
    callee_value: RuntimeValue,
    callee_value_kind: ValueKind,
    callee_object: Option<ObjectId>,
    this_value: RuntimeValue,
    this_value_kind: ValueKind,
    this_object: Option<ObjectId>,
    setup_payload: BaselineGeneratedJsDirectCallSetupPayload,
}

pub(super) fn execute_generated_native_intrinsic_call_sidecar_probe(
    sidecar: &mut BaselineGeneratedCallLinkExecutionSidecar<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: GeneratedCallLinkSidecarAttempt<'_, '_>,
) -> Result<Option<BaselineInstructionOutcome>, BaselineInstructionAbort> {
    let BaselineGeneratedCallLinkExecutionSidecar { dispatch_host, .. } = sidecar;
    execute_generated_native_intrinsic_call_probe_with_host(
        &mut **dispatch_host,
        execution,
        attempt,
    )
}

pub(super) fn execute_generated_native_intrinsic_call_property_sidecar_probe(
    sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: GeneratedCallLinkSidecarAttempt<'_, '_>,
) -> Result<Option<BaselineInstructionOutcome>, BaselineInstructionAbort> {
    let BaselineGeneratedPropertyExecutionSidecars { dispatch_host, .. } = sidecars;
    execute_generated_native_intrinsic_call_probe_with_host(
        &mut **dispatch_host,
        execution,
        attempt,
    )
}

fn execute_generated_native_intrinsic_call_probe_with_host(
    dispatch_host: &mut dyn DispatchHost,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: GeneratedCallLinkSidecarAttempt<'_, '_>,
) -> Result<Option<BaselineInstructionOutcome>, BaselineInstructionAbort> {
    let GeneratedCallLinkSidecarAttempt {
        window,
        code_block,
        fallback,
        owner,
        opcode,
        instruction,
    } = attempt;
    let destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let callee_register = register_operand_or_fallback(instruction, 1, fallback)?;
    let callee_value =
        read_register_or_outcome(execution, code_block, window, callee_register, fallback)?;
    let (this_value, provided_argument_count, first_argument_operand) = match opcode {
        CoreOpcode::CallWithThis => {
            let this_register = register_operand_or_fallback(instruction, 2, fallback)?;
            let this_value =
                read_register_or_outcome(execution, code_block, window, this_register, fallback)?;
            let argument_count = unsigned_immediate_operand_or_fallback(instruction, 3, fallback)?;
            (this_value, argument_count, 4)
        }
        CoreOpcode::Call => {
            let argument_count = unsigned_immediate_operand_or_fallback(instruction, 2, fallback)?;
            (RuntimeValue::undefined(), argument_count, 3)
        }
        _ => return Ok(None),
    };
    let first_argument = if provided_argument_count == 0 {
        None
    } else {
        let argument_register =
            register_operand_or_fallback(instruction, first_argument_operand, fallback)?;
        Some(read_register_or_outcome(
            execution,
            code_block,
            window,
            argument_register,
            fallback,
        )?)
    };
    let second_argument = if provided_argument_count <= 1 {
        None
    } else {
        let second_argument_operand = first_argument_operand.saturating_add(1);
        let argument_register =
            register_operand_or_fallback(instruction, second_argument_operand, fallback)?;
        Some(read_register_or_outcome(
            execution,
            code_block,
            window,
            argument_register,
            fallback,
        )?)
    };
    let request = GeneratedNativeIntrinsicCallRequest {
        owner,
        bytecode_index: instruction.bytecode_index,
        opcode,
        callee_value,
        this_value,
        provided_argument_count,
        first_argument,
        second_argument,
    };
    match DispatchHost::dispatch_generated_native_intrinsic_call(
        dispatch_host,
        execution.heap,
        request,
    ) {
        GeneratedNativeIntrinsicCallResult::Hit(hit) => {
            write_register_or_outcome(execution, window, destination, hit.value, fallback).map(Some)
        }
        GeneratedNativeIntrinsicCallResult::Miss(_) => Ok(None),
    }
}

pub(super) fn execute_generated_call_link_sidecar_probe(
    sidecar: &mut BaselineGeneratedCallLinkExecutionSidecar<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: GeneratedCallLinkSidecarAttempt<'_, '_>,
) -> Result<Option<BaselineGeneratedJsDirectCall>, BaselineInstructionAbort> {
    let BaselineGeneratedCallLinkExecutionSidecar {
        candidate_table,
        direct_call_hot_slots,
        dispatch_host,
        probe_miss_records,
        probe_blocked_records,
        direct_call_hot_slot_hits,
    } = sidecar;
    execute_generated_call_link_sidecar_probe_with_host(
        candidate_table,
        direct_call_hot_slots,
        &mut **dispatch_host,
        probe_miss_records,
        probe_blocked_records,
        direct_call_hot_slot_hits,
        execution,
        attempt,
    )
}

pub(super) fn execute_generated_call_link_property_sidecar_probe(
    sidecars: &mut BaselineGeneratedPropertyExecutionSidecars<'_, '_>,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: GeneratedCallLinkSidecarAttempt<'_, '_>,
) -> Result<Option<BaselineGeneratedJsDirectCall>, BaselineInstructionAbort> {
    let BaselineGeneratedPropertyExecutionSidecars {
        generated_call_link_candidate_table,
        generated_direct_call_hot_slots,
        dispatch_host,
        generated_call_link_probe_miss_records,
        generated_call_link_probe_blocked_records,
        generated_direct_call_hot_slot_hits,
        ..
    } = sidecars;
    let Some(candidate_table) = *generated_call_link_candidate_table else {
        return Ok(None);
    };
    execute_generated_call_link_sidecar_probe_with_host(
        candidate_table,
        generated_direct_call_hot_slots,
        &mut **dispatch_host,
        generated_call_link_probe_miss_records,
        generated_call_link_probe_blocked_records,
        generated_direct_call_hot_slot_hits,
        execution,
        attempt,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn execute_generated_call_link_sidecar_probe_with_host(
    candidate_table: &GeneratedCallLinkCandidateTable,
    direct_call_hot_slots: &[BaselineGeneratedJsDirectCallHotSlot],
    dispatch_host: &mut dyn DispatchHost,
    probe_miss_records: &mut Vec<BaselineGeneratedCallLinkProbeMissRecord>,
    probe_blocked_records: &mut Vec<BaselineGeneratedCallLinkProbeBlockedRecord>,
    direct_call_hot_slot_hits: &mut usize,
    execution: &mut InterpreterExecutionState<'_>,
    attempt: GeneratedCallLinkSidecarAttempt<'_, '_>,
) -> Result<Option<BaselineGeneratedJsDirectCall>, BaselineInstructionAbort> {
    let GeneratedCallLinkSidecarAttempt {
        window,
        code_block,
        fallback,
        owner,
        opcode,
        instruction,
    } = attempt;
    let bytecode_index = instruction.bytecode_index;
    if candidate_table.owner() != owner {
        return Ok(None);
    }

    let candidates = candidate_table
        .candidates_for_bytecode_index(bytecode_index.offset())
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        probe_miss_records.push(call_link_probe_miss_record(
            owner,
            bytecode_index,
            None,
            GeneratedCallLinkProbeMissReason::CandidateNotFound,
        ));
        return Ok(None);
    }

    let operands =
        generated_call_link_sidecar_operands(execution, code_block, window, instruction, fallback)?;
    if let Some(direct_call) = generated_call_link_hot_slot_direct_call(
        direct_call_hot_slots,
        &candidates,
        owner,
        opcode,
        bytecode_index,
        &operands,
    ) {
        *direct_call_hot_slot_hits = direct_call_hot_slot_hits.saturating_add(1);
        return Ok(Some(direct_call));
    }

    let candidates_to_probe = match operands.callee_object {
        Some(callee_object) => {
            let matching = candidates
                .iter()
                .copied()
                .filter(|candidate| candidate.target.callee == callee_object)
                .collect::<Vec<_>>();
            if matching.is_empty() {
                probe_miss_records.push(call_link_probe_miss_record(
                    owner,
                    bytecode_index,
                    candidates.first().copied(),
                    GeneratedCallLinkProbeMissReason::CalleeMismatch,
                ));
                return Ok(None);
            }
            matching
        }
        None => candidates,
    };
    for candidate in candidates_to_probe {
        let request = GeneratedCallLinkProbeRequest::new(
            candidate,
            owner,
            opcode,
            bytecode_index.offset(),
            operands.argument_count_including_this,
            operands.callee_value,
            operands.callee_value_kind,
            operands.callee_object,
            operands.this_value,
            operands.this_value_kind,
            operands.this_object,
        );
        match DispatchHost::probe_generated_call_link(dispatch_host, execution.heap, request) {
            GeneratedCallLinkProbeResult::DirectCall(authorization) => {
                let Some(callee_object) = operands.callee_object else {
                    probe_miss_records.push(call_link_probe_miss_record(
                        owner,
                        bytecode_index,
                        Some(candidate),
                        GeneratedCallLinkProbeMissReason::MissingCalleeIdentity,
                    ));
                    continue;
                };
                return Ok(Some(BaselineGeneratedJsDirectCall {
                    candidate: candidate.clone(),
                    authorization,
                    callee_value: operands.callee_value,
                    callee_object,
                    this_value: operands.this_value,
                    this_object: operands.this_object,
                    argument_count_including_this: operands.argument_count_including_this,
                    setup_payload: Some(operands.setup_payload.clone()),
                }));
            }
            GeneratedCallLinkProbeResult::Blocked(blocked) => {
                probe_blocked_records
                    .push(call_link_probe_blocked_record(candidate, blocked.reason));
            }
            GeneratedCallLinkProbeResult::Miss(miss) => {
                probe_miss_records.push(call_link_probe_miss_record(
                    owner,
                    bytecode_index,
                    Some(candidate),
                    miss.reason,
                ));
            }
        }
    }

    Ok(None)
}

fn generated_call_link_hot_slot_direct_call(
    hot_slots: &[BaselineGeneratedJsDirectCallHotSlot],
    current_candidates: &[&GeneratedCallLinkCandidate],
    owner: CodeBlockId,
    opcode: CoreOpcode,
    bytecode_index: BytecodeIndex,
    operands: &GeneratedCallLinkSidecarOperands,
) -> Option<BaselineGeneratedJsDirectCall> {
    let callee_object = operands.callee_object?;
    hot_slots
        .iter()
        .find(|slot| {
            slot.candidate.owner == owner
                && slot.candidate.opcode == opcode
                && slot.candidate.bytecode_index == bytecode_index.offset()
                && slot.candidate.direct_call_status
                    == GeneratedCallLinkDirectCallStatus::Authorized
                && current_candidates
                    .iter()
                    .any(|candidate| *candidate == &slot.candidate)
                && slot.candidate.target.callee == callee_object
                && slot.callee_object == callee_object
                && slot.argument_count_including_this == operands.argument_count_including_this
        })
        .map(|slot| BaselineGeneratedJsDirectCall {
            candidate: slot.candidate.clone(),
            authorization: slot.authorization,
            callee_value: operands.callee_value,
            callee_object,
            this_value: operands.this_value,
            this_object: operands.this_object,
            argument_count_including_this: operands.argument_count_including_this,
            setup_payload: Some(operands.setup_payload.clone()),
        })
}

fn generated_call_link_sidecar_operands(
    execution: &mut InterpreterExecutionState<'_>,
    code_block: &CodeBlock,
    window: RegisterWindow,
    instruction: DecodedInstruction<'_>,
    fallback: BaselineGeneratedFallbackSite,
) -> Result<GeneratedCallLinkSidecarOperands, BaselineInstructionAbort> {
    let opcode = CoreOpcode::from_opcode(instruction.opcode);
    let _destination = register_operand_or_fallback(instruction, 0, fallback)?;
    let callee_register = register_operand_or_fallback(instruction, 1, fallback)?;
    let callee_value =
        read_register_or_outcome(execution, code_block, window, callee_register, fallback)?;
    let callee_value_kind = callee_value.kind();
    let callee_object = object_id_for_runtime_value(execution, callee_value);

    let (this_value, provided_argument_count, first_argument_operand) = match opcode {
        Some(CoreOpcode::CallWithThis) => {
            let this_register = register_operand_or_fallback(instruction, 2, fallback)?;
            let this_value =
                read_register_or_outcome(execution, code_block, window, this_register, fallback)?;
            let argument_count = unsigned_immediate_operand_or_fallback(instruction, 3, fallback)?;
            (this_value, argument_count, 4usize)
        }
        _ => {
            let argument_count = unsigned_immediate_operand_or_fallback(instruction, 2, fallback)?;
            (RuntimeValue::undefined(), argument_count, 3usize)
        }
    };
    let this_value_kind = this_value.kind();
    let this_object = object_id_for_runtime_value(execution, this_value);
    let mut argument_values_including_this = Vec::with_capacity(
        usize::try_from(provided_argument_count.saturating_add(1)).unwrap_or(usize::MAX),
    );
    argument_values_including_this.push(this_value);
    for argument_index in 0..provided_argument_count {
        let operand_index = usize::try_from(argument_index)
            .unwrap_or(usize::MAX)
            .saturating_add(first_argument_operand);
        let argument_register = register_operand_or_fallback(instruction, operand_index, fallback)?;
        let argument_value =
            read_register_or_outcome(execution, code_block, window, argument_register, fallback)?;
        argument_values_including_this.push(argument_value);
    }
    let setup_payload = BaselineGeneratedJsDirectCallSetupPayload {
        this_value,
        this_object,
        argument_values_including_this,
    };

    Ok(GeneratedCallLinkSidecarOperands {
        argument_count_including_this: provided_argument_count.saturating_add(1),
        callee_value,
        callee_value_kind,
        callee_object,
        this_value,
        this_value_kind,
        this_object,
        setup_payload,
    })
}

fn call_link_probe_miss_record(
    owner: CodeBlockId,
    bytecode_index: BytecodeIndex,
    candidate: Option<&GeneratedCallLinkCandidate>,
    reason: GeneratedCallLinkProbeMissReason,
) -> BaselineGeneratedCallLinkProbeMissRecord {
    let (
        slot,
        attachment_ordinal,
        attachment_plan_ordinal,
        install_recheck_ordinal,
        boundary_validation_ordinal,
        descriptor_ordinal,
        observation_ordinal,
        readiness_ordinal,
        target_executable,
        target_callee,
        target_code_block,
        target_boundary,
        direct_call_status,
    ) = match candidate {
        Some(candidate) => (
            Some(candidate.slot),
            Some(candidate.attachment_ordinal),
            Some(candidate.attachment_plan_ordinal),
            Some(candidate.install_recheck_ordinal),
            candidate.boundary_validation_ordinal,
            candidate.descriptor_ordinal,
            candidate.observation_ordinal,
            candidate.readiness_ordinal,
            Some(candidate.target.executable),
            Some(candidate.target.callee),
            Some(candidate.target.target_code_block),
            Some(candidate.boundary.id),
            Some(candidate.direct_call_status),
        ),
        None => (
            None, None, None, None, None, None, None, None, None, None, None, None, None,
        ),
    };

    BaselineGeneratedCallLinkProbeMissRecord {
        owner,
        bytecode_index,
        slot,
        attachment_ordinal,
        attachment_plan_ordinal,
        install_recheck_ordinal,
        boundary_validation_ordinal,
        descriptor_ordinal,
        observation_ordinal,
        readiness_ordinal,
        target_executable,
        target_callee,
        target_code_block,
        target_boundary,
        direct_call_status,
        reason,
    }
}

fn call_link_probe_blocked_record(
    candidate: &GeneratedCallLinkCandidate,
    reason: GeneratedCallLinkProbeMissReason,
) -> BaselineGeneratedCallLinkProbeBlockedRecord {
    BaselineGeneratedCallLinkProbeBlockedRecord {
        owner: candidate.owner,
        bytecode_index: BytecodeIndex::from_offset(candidate.bytecode_index),
        slot: candidate.slot,
        attachment_ordinal: candidate.attachment_ordinal,
        attachment_plan_ordinal: candidate.attachment_plan_ordinal,
        install_recheck_ordinal: candidate.install_recheck_ordinal,
        boundary_validation_ordinal: candidate.boundary_validation_ordinal,
        descriptor_ordinal: candidate.descriptor_ordinal,
        observation_ordinal: candidate.observation_ordinal,
        readiness_ordinal: candidate.readiness_ordinal,
        target_executable: candidate.target.executable,
        target_callee: candidate.target.callee,
        target_code_block: candidate.target.target_code_block,
        target_boundary: candidate.boundary.id,
        direct_call_status: candidate.direct_call_status,
        reason,
    }
}
