//! ARM64-specific baseline byte emission.
//!
//! C++ JSC map: this module is the ARM64 baseline-JIT emission boundary layered
//! over `MacroAssemblerARM64`, while `emitter.rs` keeps the shared P6 semantic
//! selection/byte-image machinery. It is intentionally ready to host later
//! ARM64 labels, slow cases, and side exits, but this batch only moves the
//! existing no-call/no-heap return-seed callable lane.
//!
//! C++ `AssemblyHelpers::emitFunctionPrologue()` tags the return address before
//! pushing `fp/lr`. This raw Rust C-ABI seed is invoked as a function pointer
//! over anonymous RX memory, outside JSC's tagged-entry/PAC contract, so it
//! intentionally emits no PAC instruction until the full ARM64E entry model is
//! ported.

use crate::assembler::{AssemblerArchitecture, AssemblerBufferId, AssemblerByteImageId};
use crate::bytecode::{BytecodeIndex, VirtualRegister};
use crate::jit::emitter::{
    finish_p6_x86_64_semantic_byte_emission, p6_x86_64_checked_byte_len,
    p6_x86_64_semantic_source_buffer_id, validate_p6_x86_64_semantic_selection_effects,
    validate_p6_x86_64_semantic_terminal_policy, validate_p6_x86_64_semantic_value_layout,
    P6X86_64BaselineBackendContractRecord, P6X86_64BaselineCallableEpilogueRecord,
    P6X86_64BaselineCallablePrologueRecord, P6X86_64BaselineInstructionByteRecord,
    P6X86_64BaselineInstructionSelectionPlan, P6X86_64BaselineLoweredOperation,
    P6X86_64BaselineOperandLocation, P6X86_64BaselineOperandRole,
    P6X86_64BaselinePhysicalRegisterBinding, P6X86_64BaselinePhysicalRegisterMap,
    P6X86_64BaselineSelectedInstruction, P6X86_64BaselineSemanticByteEmissionAuthority,
    P6X86_64BaselineSemanticByteEmissionError, P6X86_64BaselineSemanticByteEmissionResult,
    P6X86_64BaselineSemanticByteEmissionShape, P6X86_64BaselineSemanticOperandRejectionReason,
    P6X86_64BaselineSymbolicRegister, P6X86_64BaselineTerminalPolicy,
    P6X86_64BaselineTerminalPolicyRecord, P6X86_64SemanticEncodedSelection,
    P6X86_64SemanticTerminalSelection,
};

// ARM64 JSC map: GPRInfo::callFrameRegister is fp/x29 and returnValueGPR is
// x0. The C ABI passes Rust's VM/frame/callee/IC-store carrier in x0/x1/x2/x3;
// this seed pins x29 to the Rust frame-base carrier, emits only no-call/no-heap
// return-shaped code, and leaves JSC metadata/jitData materialization for the
// later full ARM64 baseline lane.
const P6_ARM64_CALLABLE_PROLOGUE_BYTES: &[u8] = &[
    0xfd, 0x7b, 0xbf, 0xa9, // stp fp, lr, [sp, #-16]!
    0xfd, 0x03, 0x01, 0xaa, // mov fp, x1
];
const P6_ARM64_CALLABLE_EPILOGUE_BYTES: &[u8] = &[
    0xfd, 0x7b, 0xc1, 0xa8, // ldp fp, lr, [sp], #16
    0xc0, 0x03, 0x5f, 0xd6, // ret
];

pub fn emit_p6_arm64_baseline_callable_semantic_bytes(
    contract: P6X86_64BaselineBackendContractRecord,
    selection: P6X86_64BaselineInstructionSelectionPlan,
) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError> {
    selection
        .validate_against(&contract)
        .map_err(|error| P6X86_64BaselineSemanticByteEmissionError::Selection { error })?;
    validate_p6_x86_64_semantic_selection_effects(&selection)?;
    let terminal = validate_p6_x86_64_semantic_terminal_policy(&selection)?;
    let encoded = encode_p6_arm64_callable_return_seed_selection(&contract, &selection, terminal)?;
    finish_p6_x86_64_semantic_byte_emission(
        &contract,
        encoded,
        P6X86_64BaselineSemanticByteEmissionShape::P3cCallableCAbiSemanticArm64ReturnSeedFromAcceptedP6Selection,
        P6X86_64BaselineSemanticByteEmissionAuthority::NonExecutableCallableSemanticBytesOnlyNoVmOrPlatformAuthority,
        p6_arm64_callable_semantic_source_buffer_id(&contract),
        p6_arm64_callable_semantic_source_image_id(&contract),
        0,
        AssemblerArchitecture::Arm64,
        p6_arm64_semantic_physical_register_map(),
        0,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum P6Arm64ReturnSeedValue {
    Immediate(u64),
    Frame(P6X86_64BaselineOperandLocation),
    CalleeValue,
}

fn encode_p6_arm64_callable_return_seed_selection(
    contract: &P6X86_64BaselineBackendContractRecord,
    selection: &P6X86_64BaselineInstructionSelectionPlan,
    terminal: P6X86_64SemanticTerminalSelection,
) -> Result<P6X86_64SemanticEncodedSelection, P6X86_64BaselineSemanticByteEmissionError> {
    validate_p6_x86_64_semantic_value_layout(contract.value_layout)?;
    if terminal.branch_aware_returns {
        let branch = selection
            .instructions
            .iter()
            .find(|instruction| {
                matches!(
                    instruction.lowered.operation,
                    P6X86_64BaselineLoweredOperation::Jump { .. }
                        | P6X86_64BaselineLoweredOperation::JumpIfNotNullish { .. }
                        | P6X86_64BaselineLoweredOperation::JumpIfFalse { .. }
                )
            })
            .ok_or(P6X86_64BaselineSemanticByteEmissionError::MissingReturn)?;
        return Err(p6_arm64_seed_unsupported_operation(branch));
    }

    let mut bytes = Vec::new();
    let prologue_start = p6_x86_64_checked_byte_len(bytes.len())?;
    p6_arm64_emit_bytes(&mut bytes, P6_ARM64_CALLABLE_PROLOGUE_BYTES)?;
    let prologue_end = p6_x86_64_checked_byte_len(bytes.len())?;
    let callable_prologue = P6X86_64BaselineCallablePrologueRecord {
        start_offset: prologue_start,
        end_offset: prologue_end,
        byte_len: prologue_end.checked_sub(prologue_start).ok_or(
            P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                actual: bytes.len(),
            },
        )?,
        bytes: p6_arm64_bytes_for_range(&bytes, prologue_start, prologue_end)?,
    };

    let mut known_values: Vec<(VirtualRegister, P6Arm64ReturnSeedValue)> = Vec::new();
    let mut instruction_bytes = Vec::with_capacity(selection.instructions.len());
    let mut normal_epilogue = None;

    for instruction in &selection.instructions {
        let start_offset = p6_x86_64_checked_byte_len(bytes.len())?;
        match instruction.lowered.operation {
            P6X86_64BaselineLoweredOperation::LoadUndefined { destination } => {
                p6_arm64_seed_set_value(
                    &mut known_values,
                    destination,
                    P6Arm64ReturnSeedValue::Immediate(
                        contract.value_layout.immediate_undefined_tag,
                    ),
                );
            }
            P6X86_64BaselineLoweredOperation::LoadNull { destination } => {
                p6_arm64_seed_set_value(
                    &mut known_values,
                    destination,
                    P6Arm64ReturnSeedValue::Immediate(contract.value_layout.immediate_null_tag),
                );
            }
            P6X86_64BaselineLoweredOperation::LoadBool {
                destination, value, ..
            } => {
                let bits = if value {
                    contract.value_layout.immediate_true_tag
                } else {
                    contract.value_layout.immediate_false_tag
                };
                p6_arm64_seed_set_value(
                    &mut known_values,
                    destination,
                    P6Arm64ReturnSeedValue::Immediate(bits),
                );
            }
            P6X86_64BaselineLoweredOperation::LoadInt32 { destination, value } => {
                let bits = ((value as u32 as u64) << contract.value_layout.payload_shift)
                    | contract.value_layout.immediate_int32_tag;
                p6_arm64_seed_set_value(
                    &mut known_values,
                    destination,
                    P6Arm64ReturnSeedValue::Immediate(bits),
                );
            }
            P6X86_64BaselineLoweredOperation::LoadCallee { destination } => {
                p6_arm64_seed_set_value(
                    &mut known_values,
                    destination,
                    P6Arm64ReturnSeedValue::CalleeValue,
                );
            }
            P6X86_64BaselineLoweredOperation::Move {
                destination,
                source,
            } => {
                let Some(value) = p6_arm64_seed_known_value(&known_values, source) else {
                    return Err(p6_arm64_seed_unsupported_operation(instruction));
                };
                p6_arm64_seed_set_value(&mut known_values, destination, value);
            }
            P6X86_64BaselineLoweredOperation::Return { source } => {
                // C++ baseline stores virtual registers and `op_ret` then loads
                // returnValueJSR before jumping ReturnFromBaseline. This Rust
                // ARM64 seed fuses terminal no-side-effect immediate/local/callee
                // returns because the destination slot is unobservable at return;
                // unsupported bodies remain on the generated/fallback paths.
                let value = p6_arm64_seed_known_value(&known_values, source).unwrap_or_else(|| {
                    p6_arm64_seed_return_frame_value(instruction).unwrap_or(
                        P6Arm64ReturnSeedValue::Frame(
                            P6X86_64BaselineOperandLocation::ReturnCarrier {
                                role: contract.abi.js_value_return.role,
                                value: contract.abi.js_value_return.value,
                            },
                        ),
                    )
                });
                if matches!(
                    value,
                    P6Arm64ReturnSeedValue::Frame(
                        P6X86_64BaselineOperandLocation::ReturnCarrier { .. }
                    )
                ) {
                    return Err(p6_arm64_seed_unsupported_operation(instruction));
                }
                p6_arm64_emit_return_value(&mut bytes, instruction.bytecode_index, value)?;
                let epilogue_start = p6_x86_64_checked_byte_len(bytes.len())?;
                p6_arm64_emit_bytes(&mut bytes, P6_ARM64_CALLABLE_EPILOGUE_BYTES)?;
                let epilogue_end = p6_x86_64_checked_byte_len(bytes.len())?;
                normal_epilogue = Some(P6X86_64BaselineCallableEpilogueRecord {
                    start_offset: epilogue_start,
                    ret_offset: epilogue_end.checked_sub(4).ok_or(
                        P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                            actual: bytes.len(),
                        },
                    )?,
                    end_offset: epilogue_end,
                    byte_len: epilogue_end.checked_sub(epilogue_start).ok_or(
                        P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                            actual: bytes.len(),
                        },
                    )?,
                    bytes: p6_arm64_bytes_for_range(&bytes, epilogue_start, epilogue_end)?,
                });
            }
            _ => return Err(p6_arm64_seed_unsupported_operation(instruction)),
        }
        let end_offset = p6_x86_64_checked_byte_len(bytes.len())?;
        instruction_bytes.push(P6X86_64BaselineInstructionByteRecord {
            bytecode_index: instruction.bytecode_index,
            start_offset,
            end_offset,
            byte_len: end_offset.checked_sub(start_offset).ok_or(
                P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                    actual: bytes.len(),
                },
            )?,
            machine_instruction_count: instruction.machine_instructions.len() as u32,
            bytes: p6_arm64_bytes_for_range(&bytes, start_offset, end_offset)?,
        });
    }

    let normal_epilogue =
        normal_epilogue.ok_or(P6X86_64BaselineSemanticByteEmissionError::MissingReturn)?;
    let normal_path_end_offset = p6_x86_64_checked_byte_len(bytes.len())?;
    Ok(P6X86_64SemanticEncodedSelection {
        bytes,
        terminal_policy: P6X86_64BaselineTerminalPolicyRecord {
            policy: P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueSingleFinalEpilogueThenInlinePayloadSideExitStubs,
            return_bytecode_index: terminal.return_bytecode_index,
            ret_offset: normal_epilogue.ret_offset,
            normal_path_end_offset,
        },
        callable_prologue: Some(callable_prologue),
        callable_normal_epilogue: Some(normal_epilogue),
        instruction_bytes,
        bytecode_branches: Vec::new(),
        side_exit_placeholders: Vec::new(),
        side_exit_return_stubs: Vec::new(),
        loop_backedge_safepoint_stubs: Vec::new(),
        runtime_helper_native_exit_stubs: Vec::new(),
        js_call_native_exit_stubs: Vec::new(),
        js_call_owner_post_call_stubs: Vec::new(),
        js_call_owner_post_call_reentry_stubs: Vec::new(),
        property_native_exit_stubs: Vec::new(),
    })
}

fn p6_arm64_seed_set_value(
    values: &mut Vec<(VirtualRegister, P6Arm64ReturnSeedValue)>,
    register: VirtualRegister,
    value: P6Arm64ReturnSeedValue,
) {
    if let Some((_, existing)) = values
        .iter_mut()
        .find(|(known_register, _)| *known_register == register)
    {
        *existing = value;
    } else {
        values.push((register, value));
    }
}

fn p6_arm64_seed_known_value(
    values: &[(VirtualRegister, P6Arm64ReturnSeedValue)],
    register: VirtualRegister,
) -> Option<P6Arm64ReturnSeedValue> {
    values
        .iter()
        .find(|(known_register, _)| *known_register == register)
        .map(|(_, value)| *value)
}

fn p6_arm64_seed_return_frame_value(
    instruction: &P6X86_64BaselineSelectedInstruction,
) -> Option<P6Arm64ReturnSeedValue> {
    let location = instruction
        .operand_locations
        .iter()
        .find(|record| record.role == P6X86_64BaselineOperandRole::ReturnValue)
        .map(|record| record.location)?;
    match location {
        P6X86_64BaselineOperandLocation::FrameLocal { .. }
        | P6X86_64BaselineOperandLocation::FrameArgument { .. } => {
            Some(P6Arm64ReturnSeedValue::Frame(location))
        }
        _ => None,
    }
}

fn p6_arm64_emit_return_value(
    bytes: &mut Vec<u8>,
    bytecode_index: BytecodeIndex,
    value: P6Arm64ReturnSeedValue,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    match value {
        P6Arm64ReturnSeedValue::Immediate(bits) => p6_arm64_emit_mov_x0_u64(bytes, bits),
        P6Arm64ReturnSeedValue::Frame(location) => {
            p6_arm64_emit_ldr_x0_from_frame_location(bytes, bytecode_index, location)
        }
        P6Arm64ReturnSeedValue::CalleeValue => {
            p6_arm64_emit_word(bytes, 0xaa02_03e0) // mov x0, x2
        }
    }
}

fn p6_arm64_emit_ldr_x0_from_frame_location(
    bytes: &mut Vec<u8>,
    bytecode_index: BytecodeIndex,
    location: P6X86_64BaselineOperandLocation,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    let byte_offset = match location {
        P6X86_64BaselineOperandLocation::FrameLocal { byte_offset, .. } => byte_offset,
        P6X86_64BaselineOperandLocation::FrameArgument {
            byte_offset_from_frame_base,
            ..
        } => byte_offset_from_frame_base,
        _ => {
            return Err(
                P6X86_64BaselineSemanticByteEmissionError::UnsupportedOperandLocation {
                    bytecode_index,
                    location,
                    reason:
                        P6X86_64BaselineSemanticOperandRejectionReason::ExpectedFrameLocalMemory,
                },
            );
        }
    };
    if !byte_offset.is_multiple_of(8) || byte_offset / 8 > 0x0fff {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::FrameOffsetOutOfDisp32 {
                bytecode_index,
                location,
                byte_offset,
            },
        );
    }
    let imm12 = (byte_offset / 8) as u32;
    let instruction = 0xf940_0000_u32 | (imm12 << 10) | (29 << 5);
    p6_arm64_emit_word(bytes, instruction)
}

fn p6_arm64_emit_mov_x0_u64(
    bytes: &mut Vec<u8>,
    value: u64,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    let mut emitted_movz = false;
    for halfword in 0..4_u32 {
        let chunk = ((value >> (halfword * 16)) & 0xffff) as u32;
        if !emitted_movz {
            if chunk == 0 && halfword != 3 && value != 0 {
                continue;
            }
            p6_arm64_emit_word(bytes, 0xd280_0000_u32 | (halfword << 21) | (chunk << 5))?;
            emitted_movz = true;
        } else if chunk != 0 {
            p6_arm64_emit_word(bytes, 0xf280_0000_u32 | (halfword << 21) | (chunk << 5))?;
        }
    }
    if !emitted_movz {
        p6_arm64_emit_word(bytes, 0xd280_0000)?;
    }
    Ok(())
}

fn p6_arm64_emit_bytes(
    bytes: &mut Vec<u8>,
    emitted: &[u8],
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    let next_len = bytes.len().checked_add(emitted.len()).ok_or(
        P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 { actual: usize::MAX },
    )?;
    p6_x86_64_checked_byte_len(next_len)?;
    bytes.extend_from_slice(emitted);
    Ok(())
}

fn p6_arm64_emit_word(
    bytes: &mut Vec<u8>,
    word: u32,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    p6_arm64_emit_bytes(bytes, &word.to_le_bytes())
}

fn p6_arm64_bytes_for_range(
    bytes: &[u8],
    start_offset: u32,
    end_offset: u32,
) -> Result<Vec<u8>, P6X86_64BaselineSemanticByteEmissionError> {
    let start = start_offset as usize;
    let end = end_offset as usize;
    let Some(slice) = bytes.get(start..end) else {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::BranchPatchOutOfRange {
                branch_offset: start_offset,
            },
        );
    };
    Ok(slice.to_vec())
}

fn p6_arm64_seed_unsupported_operation(
    instruction: &P6X86_64BaselineSelectedInstruction,
) -> P6X86_64BaselineSemanticByteEmissionError {
    P6X86_64BaselineSemanticByteEmissionError::UnsupportedArm64SeedLoweredOperation {
        bytecode_index: instruction.bytecode_index,
        operation: instruction.lowered.operation,
    }
}

fn p6_arm64_semantic_physical_register_map() -> P6X86_64BaselinePhysicalRegisterMap {
    P6X86_64BaselinePhysicalRegisterMap {
        bindings: [
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::ReturnGpr,
                physical: "x0",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::Scratch0,
                physical: "x9",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::Scratch1,
                physical: "x10",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::Scratch2,
                physical: "x11",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::PropertyBase,
                physical: "x0 (reserved)",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::PinnedCalleeValue,
                physical: "x2",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::PinnedCallFrameBase,
                physical: "fp/x29",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::PinnedVm,
                physical: "x0 (C ABI arg, unused by seed)",
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::MetadataTableBase,
                physical: "x25 (reserved)",
            },
        ],
    }
}

fn p6_arm64_callable_semantic_source_buffer_id(
    contract: &P6X86_64BaselineBackendContractRecord,
) -> AssemblerBufferId {
    let raw_id = p6_x86_64_semantic_source_buffer_id(contract).0 ^ 0x0000_0000_a64c_ab1e;
    AssemblerBufferId(if raw_id == 0 { 1 } else { raw_id })
}

fn p6_arm64_callable_semantic_source_image_id(
    contract: &P6X86_64BaselineBackendContractRecord,
) -> AssemblerByteImageId {
    let raw_id = p6_arm64_callable_semantic_source_buffer_id(contract).0 ^ 0x1f00_0000_a64c_ab1e;
    AssemblerByteImageId(if raw_id == 0 { 1 } else { raw_id })
}
