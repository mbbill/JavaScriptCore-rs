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
    P6X86_64BaselineBackendContractRecord, P6X86_64BaselineBytecodeBranchKind,
    P6X86_64BaselineCallableEpilogueRecord, P6X86_64BaselineCallablePrologueRecord,
    P6X86_64BaselineControlFlowBranchContract, P6X86_64BaselineImmediateOperand,
    P6X86_64BaselineInstructionByteRecord, P6X86_64BaselineInstructionSelectionPlan,
    P6X86_64BaselineLoweredOperation, P6X86_64BaselineOperandLocation, P6X86_64BaselineOperandRole,
    P6X86_64BaselinePhysicalRegisterBinding, P6X86_64BaselinePhysicalRegisterMap,
    P6X86_64BaselineSelectedInstruction, P6X86_64BaselineSelectedSideExitReason,
    P6X86_64BaselineSemanticByteEmissionAuthority, P6X86_64BaselineSemanticByteEmissionError,
    P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionShape,
    P6X86_64BaselineSemanticOperandRejectionReason, P6X86_64BaselineSideExitDestinationEffect,
    P6X86_64BaselineSideExitLabel, P6X86_64BaselineSideExitReturnPayload,
    P6X86_64BaselineSymbolicRegister, P6X86_64BaselineTerminalPolicy,
    P6X86_64BaselineTerminalPolicyRecord, P6X86_64BaselineValueLayoutContract,
    P6X86_64SemanticEncodedSelection, P6X86_64SemanticTerminalSelection,
};

const P6_ARM64_CALLABLE_PROLOGUE_BYTES: &[u8] =
    entry_prologue::P6_ARM64_RAW_C_ABI_CALLABLE_PROLOGUE_BYTES;
const P6_ARM64_CALLABLE_EPILOGUE_BYTES: &[u8] =
    entry_prologue::P6_ARM64_RAW_C_ABI_CALLABLE_EPILOGUE_BYTES;

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

#[cfg(test)]
pub(crate) fn emit_p6_arm64_dormant_branch_aware_callable_semantic_bytes_for_test(
    contract: P6X86_64BaselineBackendContractRecord,
    selection: P6X86_64BaselineInstructionSelectionPlan,
) -> Result<P6X86_64BaselineSemanticByteEmissionResult, P6X86_64BaselineSemanticByteEmissionError> {
    selection
        .validate_against(&contract)
        .map_err(|error| P6X86_64BaselineSemanticByteEmissionError::Selection { error })?;
    validate_p6_x86_64_semantic_selection_effects(&selection)?;
    validate_p6_x86_64_semantic_terminal_policy(&selection)?;
    let encoded = encode_p6_arm64_dormant_branch_aware_callable_selection(
        contract.value_layout,
        &selection.instructions,
    )?;
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

#[allow(dead_code)]
pub(crate) mod register_contract {
    // C++ JSC map: `GPRInfo` names the ARM64 baseline register identity,
    // `JSRInfo` aliases the JSValue return register, and `AssemblyHelpers`/`JIT`
    // materialize tag and metadata registers before opcode bodies use them.
    // This Rust module is metadata only; it records the dormant contract the
    // future ARM64 baseline emitter must satisfy and emits no instructions.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) struct Arm64Gpr {
        pub(crate) name: &'static str,
        pub(crate) index: u8,
    }

    impl Arm64Gpr {
        const fn new(name: &'static str, index: u8) -> Self {
            Self { name, index }
        }
    }

    pub(crate) const X0: Arm64Gpr = Arm64Gpr::new("x0", 0);
    pub(crate) const X1: Arm64Gpr = Arm64Gpr::new("x1", 1);
    pub(crate) const X2: Arm64Gpr = Arm64Gpr::new("x2", 2);
    pub(crate) const X3: Arm64Gpr = Arm64Gpr::new("x3", 3);
    pub(crate) const X4: Arm64Gpr = Arm64Gpr::new("x4", 4);
    pub(crate) const X5: Arm64Gpr = Arm64Gpr::new("x5", 5);
    pub(crate) const X6: Arm64Gpr = Arm64Gpr::new("x6", 6);
    pub(crate) const X7: Arm64Gpr = Arm64Gpr::new("x7", 7);
    pub(crate) const X8: Arm64Gpr = Arm64Gpr::new("x8", 8);
    pub(crate) const X9: Arm64Gpr = Arm64Gpr::new("x9", 9);
    pub(crate) const X10: Arm64Gpr = Arm64Gpr::new("x10", 10);
    pub(crate) const X11: Arm64Gpr = Arm64Gpr::new("x11", 11);
    pub(crate) const X12: Arm64Gpr = Arm64Gpr::new("x12", 12);
    pub(crate) const X13: Arm64Gpr = Arm64Gpr::new("x13", 13);
    pub(crate) const X14: Arm64Gpr = Arm64Gpr::new("x14", 14);
    pub(crate) const X15: Arm64Gpr = Arm64Gpr::new("x15", 15);
    pub(crate) const X25: Arm64Gpr = Arm64Gpr::new("x25", 25);
    pub(crate) const X26: Arm64Gpr = Arm64Gpr::new("x26", 26);
    pub(crate) const X27: Arm64Gpr = Arm64Gpr::new("x27", 27);
    pub(crate) const X28: Arm64Gpr = Arm64Gpr::new("x28", 28);
    pub(crate) const X29: Arm64Gpr = Arm64Gpr::new("x29", 29);

    pub(crate) const REG_T0: Arm64Gpr = X0;
    pub(crate) const REG_T1: Arm64Gpr = X1;
    pub(crate) const REG_T2: Arm64Gpr = X2;
    pub(crate) const REG_T3: Arm64Gpr = X3;
    pub(crate) const REG_T4: Arm64Gpr = X4;
    pub(crate) const REG_T5: Arm64Gpr = X5;
    pub(crate) const REG_T6: Arm64Gpr = X6;
    pub(crate) const REG_T7: Arm64Gpr = X7;
    pub(crate) const REG_T8: Arm64Gpr = X8;
    pub(crate) const REG_T9: Arm64Gpr = X9;
    pub(crate) const REG_T10: Arm64Gpr = X10;
    pub(crate) const REG_T11: Arm64Gpr = X11;
    pub(crate) const REG_T12: Arm64Gpr = X12;
    pub(crate) const REG_T13: Arm64Gpr = X13;
    pub(crate) const REG_T14: Arm64Gpr = X14;
    pub(crate) const REG_T15: Arm64Gpr = X15;
    pub(crate) const TEMPORARY_GPRS: [Arm64Gpr; 16] = [
        REG_T0, REG_T1, REG_T2, REG_T3, REG_T4, REG_T5, REG_T6, REG_T7, REG_T8, REG_T9, REG_T10,
        REG_T11, REG_T12, REG_T13, REG_T14, REG_T15,
    ];

    pub(crate) const ARGUMENT_GPR0: Arm64Gpr = X0;
    pub(crate) const ARGUMENT_GPR1: Arm64Gpr = X1;
    pub(crate) const ARGUMENT_GPR2: Arm64Gpr = X2;
    pub(crate) const ARGUMENT_GPR3: Arm64Gpr = X3;
    pub(crate) const ARGUMENT_GPR4: Arm64Gpr = X4;
    pub(crate) const ARGUMENT_GPR5: Arm64Gpr = X5;
    pub(crate) const ARGUMENT_GPR6: Arm64Gpr = X6;
    pub(crate) const ARGUMENT_GPR7: Arm64Gpr = X7;
    pub(crate) const ARGUMENT_GPRS: [Arm64Gpr; 8] = [
        ARGUMENT_GPR0,
        ARGUMENT_GPR1,
        ARGUMENT_GPR2,
        ARGUMENT_GPR3,
        ARGUMENT_GPR4,
        ARGUMENT_GPR5,
        ARGUMENT_GPR6,
        ARGUMENT_GPR7,
    ];

    pub(crate) const CALL_FRAME_REGISTER: Arm64Gpr = X29;
    pub(crate) const CALL_FRAME_REGISTER_NAME: &str = "fp/x29";
    pub(crate) const RETURN_VALUE_GPR: Arm64Gpr = REG_T0;
    pub(crate) const RETURN_VALUE_GPR2: Arm64Gpr = REG_T1;
    pub(crate) const JIT_DATA_REGISTER: Arm64Gpr = X26;
    pub(crate) const METADATA_TABLE_REGISTER: Arm64Gpr = X25;
    pub(crate) const NUMBER_TAG_REGISTER: Arm64Gpr = X27;
    pub(crate) const NOT_CELL_MASK_REGISTER: Arm64Gpr = X28;
    pub(crate) const REG_CS6: Arm64Gpr = METADATA_TABLE_REGISTER;
    pub(crate) const REG_CS7: Arm64Gpr = JIT_DATA_REGISTER;
    pub(crate) const REG_CS8: Arm64Gpr = NUMBER_TAG_REGISTER;
    pub(crate) const REG_CS9: Arm64Gpr = NOT_CELL_MASK_REGISTER;
    pub(crate) const BASELINE_RESERVED_CALLEE_SAVE_GPRS: [Arm64Gpr; 4] =
        [REG_CS6, REG_CS7, REG_CS8, REG_CS9];

    pub(crate) const PINNED_CALL_FRAME_BASE_PHYSICAL: &str = CALL_FRAME_REGISTER_NAME;
    pub(crate) const PROPERTY_BASE_RESERVED_PHYSICAL: &str = "x0 (reserved)";
    pub(crate) const PINNED_VM_RETURN_SEED_PHYSICAL: &str = "x0 (C ABI arg, unused by seed)";
    pub(crate) const METADATA_TABLE_BASE_RESERVED_PHYSICAL: &str = "x25 (reserved)";

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) struct Arm64JSValueRegs {
        pub(crate) gpr: Arm64Gpr,
    }

    pub(crate) const RETURN_VALUE_JSR: Arm64JSValueRegs = Arm64JSValueRegs {
        gpr: RETURN_VALUE_GPR,
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Arm64BaselineTagConstant {
        NumberTag,
        NotCellMask,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Arm64BaselineMaterializedState {
        TagConstant(Arm64BaselineTagConstant),
        JitDataFromCodeBlockJitData,
        MetadataTableFromCodeBlockMetadataTable,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Arm64BaselineMaterializationSource {
        AssemblyHelpersTagCheckRegisters,
        CodeBlockJitDataAndMetadataTable,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Arm64BaselineMaterializationEmission {
        DormantMetadataOnlyNoInstructions,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) struct Arm64BaselineMaterializationRequirement {
        pub(crate) register: Arm64Gpr,
        pub(crate) required_state: Arm64BaselineMaterializedState,
        pub(crate) source: Arm64BaselineMaterializationSource,
        pub(crate) emission: Arm64BaselineMaterializationEmission,
    }

    pub(crate) const REQUIRED_MATERIALIZED_REGISTER_STATES:
        [Arm64BaselineMaterializationRequirement; 4] = [
        Arm64BaselineMaterializationRequirement {
            register: NUMBER_TAG_REGISTER,
            required_state: Arm64BaselineMaterializedState::TagConstant(
                Arm64BaselineTagConstant::NumberTag,
            ),
            source: Arm64BaselineMaterializationSource::AssemblyHelpersTagCheckRegisters,
            emission: Arm64BaselineMaterializationEmission::DormantMetadataOnlyNoInstructions,
        },
        Arm64BaselineMaterializationRequirement {
            register: NOT_CELL_MASK_REGISTER,
            required_state: Arm64BaselineMaterializedState::TagConstant(
                Arm64BaselineTagConstant::NotCellMask,
            ),
            source: Arm64BaselineMaterializationSource::AssemblyHelpersTagCheckRegisters,
            emission: Arm64BaselineMaterializationEmission::DormantMetadataOnlyNoInstructions,
        },
        Arm64BaselineMaterializationRequirement {
            register: JIT_DATA_REGISTER,
            required_state: Arm64BaselineMaterializedState::JitDataFromCodeBlockJitData,
            source: Arm64BaselineMaterializationSource::CodeBlockJitDataAndMetadataTable,
            emission: Arm64BaselineMaterializationEmission::DormantMetadataOnlyNoInstructions,
        },
        Arm64BaselineMaterializationRequirement {
            register: METADATA_TABLE_REGISTER,
            required_state: Arm64BaselineMaterializedState::MetadataTableFromCodeBlockMetadataTable,
            source: Arm64BaselineMaterializationSource::CodeBlockJitDataAndMetadataTable,
            emission: Arm64BaselineMaterializationEmission::DormantMetadataOnlyNoInstructions,
        },
    ];
}

mod control_flow;
mod entry_prologue;
mod frame_addressing;
// Generated-frame materialization proof submodules: consumed only by the gated
// ARM64 native-entry/admission proof apparatus (vm/arm64_native_entry.rs proof
// fn + vm/native_reentry cluster). Gated off by default; the live baseline
// codegen path in this file does not use them.
#[cfg(feature = "arm64_native_entry_proof")]
mod frame_materialization;
#[cfg(feature = "arm64_native_entry_proof")]
mod frame_materialization_producer;

#[cfg(feature = "arm64_native_entry_proof")]
pub(crate) use frame_materialization::{
    validate_arm64_baseline_generated_native_frame_materialization,
    Arm64BaselineGeneratedNativeFrameMaterializationDescriptor,
    Arm64BaselineGeneratedNativeFrameMaterializationMismatch,
    Arm64BaselineGeneratedNativeFrameMaterializationValidationContext,
    Arm64BaselineLiveRootSlotKind, Arm64BaselineMachineStackRootSlotDescriptor,
    Arm64BaselineMachineStackSpanKind,
};
#[cfg(all(test, feature = "arm64_native_entry_proof"))]
pub(crate) use frame_materialization::{JSC_REGISTER_BYTES, JSC_STACK_ALIGNMENT_BYTES};
#[cfg(feature = "arm64_native_entry_proof")]
pub(crate) use frame_materialization_producer::{
    produce_arm64_baseline_generated_native_frame_materialization_descriptor,
    Arm64BaselineGeneratedNativeFrameMaterializationProductionError,
    Arm64BaselineGeneratedNativeFrameMaterializationProductionRequest,
};

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
                // boxInt32: NumberTag | static_cast<uint32_t>(value)
                // (JSCJSValue.h:1023-1026). The int32 lives in the low 32 bits
                // under NumberTag, not shifted under a low-byte tag. The 64-bit
                // immediate emitter lowers this to a MOVZ/MOVK chain.
                let bits = contract.value_layout.number_tag | (value as u32 as u64);
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

// C++ JSC `emit_op_jfalse` records target jumps with `addJump`, and `op_ret`
// jumps through ReturnFromBaseline after loading returnValueJSR. This private
// ARM64 skeleton is dormant: it keeps public branch-aware admission rejected and
// uses one local C-ABI epilogue until VM/native-entry payload decode is ported.
#[allow(dead_code)]
fn encode_p6_arm64_dormant_branch_aware_callable_selection(
    value_layout: P6X86_64BaselineValueLayoutContract,
    instructions: &[P6X86_64BaselineSelectedInstruction],
) -> Result<P6X86_64SemanticEncodedSelection, P6X86_64BaselineSemanticByteEmissionError> {
    validate_p6_x86_64_semantic_value_layout(value_layout)?;
    let return_instruction_indices: Vec<usize> = instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| {
            matches!(
                instruction.lowered.operation,
                P6X86_64BaselineLoweredOperation::Return { .. }
            )
            .then_some(index)
        })
        .collect();
    let Some(&final_return_instruction_index) = return_instruction_indices.last() else {
        return Err(P6X86_64BaselineSemanticByteEmissionError::MissingReturn);
    };
    let return_bytecode_index = instructions[final_return_instruction_index].bytecode_index;
    if final_return_instruction_index != instructions.len().saturating_sub(1) {
        let next_bytecode_index = instructions[final_return_instruction_index + 1].bytecode_index;
        return Err(P6X86_64BaselineSemanticByteEmissionError::NonFinalReturn {
            bytecode_index: return_bytecode_index,
            next_bytecode_index,
        });
    }

    let mut builder = control_flow::P6Arm64SemanticByteBuilder::default();
    let callable_prologue = p6_arm64_builder_emit_callable_prologue(&mut builder)?;
    let mut instruction_bytes = Vec::with_capacity(instructions.len());
    let mut pending_return_branches = Vec::new();
    let mut normal_epilogue = None;
    let mut saw_jump_if_false = false;

    for (index, instruction) in instructions.iter().enumerate() {
        let start_offset = builder.offset()?;
        match instruction.lowered.operation {
            P6X86_64BaselineLoweredOperation::LoadUndefined { .. }
            | P6X86_64BaselineLoweredOperation::LoadNull { .. }
            | P6X86_64BaselineLoweredOperation::LoadBool { .. }
            | P6X86_64BaselineLoweredOperation::LoadInt32 { .. }
            | P6X86_64BaselineLoweredOperation::LoadCallee { .. }
            | P6X86_64BaselineLoweredOperation::Move { .. } => {
                p6_arm64_builder_emit_materialized_frame_write(
                    &mut builder,
                    value_layout,
                    instruction,
                )?;
            }
            P6X86_64BaselineLoweredOperation::JumpIfFalse { target, .. } => {
                if target <= instruction.bytecode_index {
                    return Err(p6_arm64_seed_unsupported_operation(instruction));
                }
                let Some(fallthrough_instruction) = instructions.get(index.saturating_add(1))
                else {
                    return Err(p6_arm64_seed_unsupported_operation(instruction));
                };
                let source = p6_arm64_seed_source_location(instruction)?;
                p6_arm64_seed_validate_branch_target_operand(instruction, target)?;
                let target = P6X86_64BaselineControlFlowBranchContract {
                    kind: P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken,
                    source_bytecode_index: instruction.bytecode_index,
                    target_bytecode_index: target,
                };
                let side_exit_native_reentry_targets = [
                    target.target_bytecode_index,
                    fallthrough_instruction.bytecode_index,
                ];
                builder.emit_branch_if_false_primitive(
                    instruction.bytecode_index,
                    source,
                    value_layout,
                    p6_arm64_unsupported_truthiness_side_exit_label(instruction.bytecode_index),
                    target,
                    &side_exit_native_reentry_targets,
                )?;
                saw_jump_if_false = true;
            }
            P6X86_64BaselineLoweredOperation::Return { .. } => {
                let value = p6_arm64_seed_return_frame_value(instruction)
                    .ok_or_else(|| p6_arm64_seed_unsupported_operation(instruction))?;
                p6_arm64_builder_emit_return_value(
                    &mut builder,
                    instruction.bytecode_index,
                    value,
                )?;
                if !return_instruction_indices.contains(&index) {
                    return Err(p6_arm64_seed_unsupported_operation(instruction));
                }
                if index != final_return_instruction_index {
                    pending_return_branches.push(builder.emit_unconditional_internal_branch()?);
                } else {
                    let epilogue = p6_arm64_builder_emit_callable_epilogue(&mut builder)?;
                    for branch in pending_return_branches.drain(..) {
                        builder.patch_internal_branch_to_target(branch, epilogue.start_offset)?;
                    }
                    normal_epilogue = Some(epilogue);
                }
            }
            P6X86_64BaselineLoweredOperation::Jump { .. }
            | P6X86_64BaselineLoweredOperation::JumpIfNotNullish { .. } => {
                return Err(p6_arm64_seed_unsupported_operation(instruction));
            }
            _ => return Err(p6_arm64_seed_unsupported_operation(instruction)),
        }

        let end_offset = builder.offset()?;
        instruction_bytes.push(P6X86_64BaselineInstructionByteRecord {
            bytecode_index: instruction.bytecode_index,
            start_offset,
            end_offset,
            byte_len: end_offset.checked_sub(start_offset).ok_or(
                P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                    actual: builder.bytes().len(),
                },
            )?,
            machine_instruction_count: instruction.machine_instructions.len() as u32,
            bytes: p6_arm64_bytes_for_range(builder.bytes(), start_offset, end_offset)?,
        });
    }

    if !saw_jump_if_false {
        return Err(P6X86_64BaselineSemanticByteEmissionError::MissingReturn);
    }
    let normal_epilogue =
        normal_epilogue.ok_or(P6X86_64BaselineSemanticByteEmissionError::MissingReturn)?;
    let normal_path_end_offset = builder.offset()?;
    let bytecode_branches = builder.finish_bytecode_branches(&instruction_bytes)?;
    let side_exit_return_stubs = builder.finish_side_exit_return_stubs(&instruction_bytes)?;
    for record in &mut instruction_bytes {
        record.bytes =
            p6_arm64_bytes_for_range(builder.bytes(), record.start_offset, record.end_offset)?;
    }

    Ok(P6X86_64SemanticEncodedSelection {
        bytes: builder.bytes().to_vec(),
        terminal_policy: P6X86_64BaselineTerminalPolicyRecord {
            policy: P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueBytecodeBranchesSharedNormalEpilogueThenInlinePayloadStubs,
            return_bytecode_index,
            ret_offset: normal_epilogue.ret_offset,
            normal_path_end_offset,
        },
        callable_prologue: Some(callable_prologue),
        callable_normal_epilogue: Some(normal_epilogue),
        instruction_bytes,
        bytecode_branches,
        side_exit_placeholders: Vec::new(),
        side_exit_return_stubs,
        loop_backedge_safepoint_stubs: Vec::new(),
        runtime_helper_native_exit_stubs: Vec::new(),
        js_call_native_exit_stubs: Vec::new(),
        js_call_owner_post_call_stubs: Vec::new(),
        js_call_owner_post_call_reentry_stubs: Vec::new(),
        property_native_exit_stubs: Vec::new(),
    })
}

fn p6_arm64_builder_emit_callable_prologue(
    builder: &mut control_flow::P6Arm64SemanticByteBuilder,
) -> Result<P6X86_64BaselineCallablePrologueRecord, P6X86_64BaselineSemanticByteEmissionError> {
    let start_offset = builder.offset()?;
    builder.emit_bytes(P6_ARM64_CALLABLE_PROLOGUE_BYTES)?;
    let end_offset = builder.offset()?;
    Ok(P6X86_64BaselineCallablePrologueRecord {
        start_offset,
        end_offset,
        byte_len: end_offset.checked_sub(start_offset).ok_or(
            P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                actual: builder.bytes().len(),
            },
        )?,
        bytes: p6_arm64_bytes_for_range(builder.bytes(), start_offset, end_offset)?,
    })
}

fn p6_arm64_builder_emit_callable_epilogue(
    builder: &mut control_flow::P6Arm64SemanticByteBuilder,
) -> Result<P6X86_64BaselineCallableEpilogueRecord, P6X86_64BaselineSemanticByteEmissionError> {
    let start_offset = builder.offset()?;
    builder.emit_bytes(P6_ARM64_CALLABLE_EPILOGUE_BYTES)?;
    let end_offset = builder.offset()?;
    Ok(P6X86_64BaselineCallableEpilogueRecord {
        start_offset,
        ret_offset: end_offset.checked_sub(4).ok_or(
            P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                actual: builder.bytes().len(),
            },
        )?,
        end_offset,
        byte_len: end_offset.checked_sub(start_offset).ok_or(
            P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                actual: builder.bytes().len(),
            },
        )?,
        bytes: p6_arm64_bytes_for_range(builder.bytes(), start_offset, end_offset)?,
    })
}

fn p6_arm64_builder_emit_materialized_frame_write(
    builder: &mut control_flow::P6Arm64SemanticByteBuilder,
    value_layout: P6X86_64BaselineValueLayoutContract,
    instruction: &P6X86_64BaselineSelectedInstruction,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    let mut bytes = Vec::new();
    let mut known_values = Vec::new();
    p6_arm64_emit_materialize_seed_frame_write(
        &mut bytes,
        value_layout,
        &mut known_values,
        instruction,
    )?;
    builder.emit_bytes(&bytes)
}

fn p6_arm64_builder_emit_return_value(
    builder: &mut control_flow::P6Arm64SemanticByteBuilder,
    bytecode_index: BytecodeIndex,
    value: P6Arm64ReturnSeedValue,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    let mut bytes = Vec::new();
    p6_arm64_emit_return_value(&mut bytes, bytecode_index, value)?;
    builder.emit_bytes(&bytes)
}

fn p6_arm64_seed_source_location(
    instruction: &P6X86_64BaselineSelectedInstruction,
) -> Result<P6X86_64BaselineOperandLocation, P6X86_64BaselineSemanticByteEmissionError> {
    instruction
        .operand_locations
        .iter()
        .find(|record| record.role == P6X86_64BaselineOperandRole::Source)
        .map(|record| record.location)
        .ok_or_else(|| p6_arm64_seed_unsupported_operation(instruction))
}

fn p6_arm64_seed_validate_branch_target_operand(
    instruction: &P6X86_64BaselineSelectedInstruction,
    target: BytecodeIndex,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    let Some(record) = instruction
        .operand_locations
        .iter()
        .find(|record| record.role == P6X86_64BaselineOperandRole::BranchTarget)
    else {
        return Err(p6_arm64_seed_unsupported_operation(instruction));
    };
    match record.location {
        P6X86_64BaselineOperandLocation::Immediate(
            P6X86_64BaselineImmediateOperand::BytecodeIndex(actual),
        ) if actual == target => Ok(()),
        _ => Err(p6_arm64_seed_unsupported_operation(instruction)),
    }
}

fn p6_arm64_unsupported_truthiness_side_exit_label(
    bytecode_index: BytecodeIndex,
) -> P6X86_64BaselineSideExitLabel {
    P6X86_64BaselineSideExitLabel {
        reason: P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
        retained_bytecode_index: bytecode_index,
        destination: P6X86_64BaselineSideExitDestinationEffect::DestinationUnchanged,
        may_throw: false,
        runtime_call: false,
        heap_allocation: false,
        touches_gc_roots: false,
    }
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
    p6_arm64_seed_frame_value_for_role(instruction, P6X86_64BaselineOperandRole::ReturnValue)
}

#[allow(dead_code)]
fn p6_arm64_seed_source_frame_value(
    instruction: &P6X86_64BaselineSelectedInstruction,
) -> Option<P6Arm64ReturnSeedValue> {
    p6_arm64_seed_frame_value_for_role(instruction, P6X86_64BaselineOperandRole::Source)
}

fn p6_arm64_seed_frame_value_for_role(
    instruction: &P6X86_64BaselineSelectedInstruction,
    role: P6X86_64BaselineOperandRole,
) -> Option<P6Arm64ReturnSeedValue> {
    let location = instruction
        .operand_locations
        .iter()
        .find(|record| record.role == role)
        .map(|record| record.location)?;
    match location {
        P6X86_64BaselineOperandLocation::FrameLocal { .. }
        | P6X86_64BaselineOperandLocation::FrameArgument { .. } => {
            Some(P6Arm64ReturnSeedValue::Frame(location))
        }
        _ => None,
    }
}

#[allow(dead_code)]
fn p6_arm64_seed_destination_frame_location(
    instruction: &P6X86_64BaselineSelectedInstruction,
) -> Result<P6X86_64BaselineOperandLocation, P6X86_64BaselineSemanticByteEmissionError> {
    instruction
        .operand_locations
        .iter()
        .find(|record| record.role == P6X86_64BaselineOperandRole::Destination)
        .map(|record| record.location)
        .ok_or_else(|| p6_arm64_seed_unsupported_operation(instruction))
}

#[allow(dead_code)]
fn p6_arm64_emit_materialize_value_to_frame_location(
    bytes: &mut Vec<u8>,
    bytecode_index: BytecodeIndex,
    value: P6Arm64ReturnSeedValue,
    destination: P6X86_64BaselineOperandLocation,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    // C++ JSC emits opcode results with `emitPutVirtualRegister`, which
    // stores the JSValue into `addressFor(dst)`. The current Rust ARM64
    // return seed still rejects public branch-aware returns, so this is dormant
    // frame-write glue for the later branch-aware byte builder.
    match value {
        P6Arm64ReturnSeedValue::Immediate(bits) => {
            p6_arm64_emit_mov_xd_u64(bytes, register_contract::X9, bits)?;
            p6_arm64_emit_str_xd_to_frame_location(
                bytes,
                bytecode_index,
                register_contract::X9,
                destination,
            )
        }
        P6Arm64ReturnSeedValue::Frame(source) => {
            p6_arm64_emit_ldr_xd_from_frame_location(
                bytes,
                bytecode_index,
                source,
                register_contract::X9,
            )?;
            p6_arm64_emit_str_xd_to_frame_location(
                bytes,
                bytecode_index,
                register_contract::X9,
                destination,
            )
        }
        P6Arm64ReturnSeedValue::CalleeValue => p6_arm64_emit_str_xd_to_frame_location(
            bytes,
            bytecode_index,
            register_contract::X2,
            destination,
        ),
    }
}

#[allow(dead_code)]
fn p6_arm64_emit_materialize_seed_frame_write(
    bytes: &mut Vec<u8>,
    value_layout: P6X86_64BaselineValueLayoutContract,
    known_values: &mut Vec<(VirtualRegister, P6Arm64ReturnSeedValue)>,
    instruction: &P6X86_64BaselineSelectedInstruction,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    let bytecode_index = instruction.bytecode_index;
    let (destination_register, value) = match instruction.lowered.operation {
        P6X86_64BaselineLoweredOperation::LoadUndefined { destination } => (
            destination,
            P6Arm64ReturnSeedValue::Immediate(value_layout.immediate_undefined_tag),
        ),
        P6X86_64BaselineLoweredOperation::LoadNull { destination } => (
            destination,
            P6Arm64ReturnSeedValue::Immediate(value_layout.immediate_null_tag),
        ),
        P6X86_64BaselineLoweredOperation::LoadBool {
            destination, value, ..
        } => {
            let bits = if value {
                value_layout.immediate_true_tag
            } else {
                value_layout.immediate_false_tag
            };
            (destination, P6Arm64ReturnSeedValue::Immediate(bits))
        }
        P6X86_64BaselineLoweredOperation::LoadInt32 { destination, value } => {
            // TRANSITIONAL (D1a): this dormant frame-write builder is not yet
            // live-executed (the live return seed rejects branch-aware bodies),
            // so it stays on the low-byte int32 scheme like the x86 byte-emitter.
            // The live return seed uses JSVALUE64 boxInt32 (`number_tag | u32`).
            let bits = ((value as u32 as u64) << value_layout.payload_shift)
                | value_layout.immediate_int32_tag;
            (destination, P6Arm64ReturnSeedValue::Immediate(bits))
        }
        P6X86_64BaselineLoweredOperation::LoadCallee { destination } => {
            (destination, P6Arm64ReturnSeedValue::CalleeValue)
        }
        P6X86_64BaselineLoweredOperation::Move {
            destination,
            source,
        } => {
            let value = p6_arm64_seed_known_value(known_values, source)
                .or_else(|| p6_arm64_seed_source_frame_value(instruction))
                .ok_or_else(|| p6_arm64_seed_unsupported_operation(instruction))?;
            (destination, value)
        }
        _ => return Err(p6_arm64_seed_unsupported_operation(instruction)),
    };
    let destination = p6_arm64_seed_destination_frame_location(instruction)?;
    p6_arm64_emit_materialize_value_to_frame_location(bytes, bytecode_index, value, destination)?;
    let known_value = match value {
        P6Arm64ReturnSeedValue::Frame(_) => P6Arm64ReturnSeedValue::Frame(destination),
        value => value,
    };
    p6_arm64_seed_set_value(known_values, destination_register, known_value);
    Ok(())
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

#[allow(dead_code)]
fn p6_arm64_callable_payload_return_stub_bytes(
    return_value_bits: u64,
) -> Result<Vec<u8>, P6X86_64BaselineSemanticByteEmissionError> {
    let mut bytes = Vec::new();
    p6_arm64_emit_mov_x0_u64(&mut bytes, return_value_bits)?;
    p6_arm64_emit_bytes(&mut bytes, P6_ARM64_CALLABLE_EPILOGUE_BYTES)?;
    Ok(bytes)
}

#[allow(dead_code)]
fn p6_arm64_callable_side_exit_payload_return_stub_bytes(
    encoded_payload: P6X86_64BaselineSideExitReturnPayload,
) -> Result<Vec<u8>, P6X86_64BaselineSemanticByteEmissionError> {
    // C++ Baseline JIT handles `jfalse` fallback through
    // `JITOpcodes.cpp::valueIsFalseyGenerator` and LLInt's
    // `slow_path_jfalse`. This Rust-only native-entry stub does not implement
    // truthiness; it returns the encoded payload through x0 so the VM can
    // leave and reenter through the current rooting/fallback tables.
    p6_arm64_callable_payload_return_stub_bytes(encoded_payload.raw_bits())
}

fn p6_arm64_emit_ldr_x0_from_frame_location(
    bytes: &mut Vec<u8>,
    bytecode_index: BytecodeIndex,
    location: P6X86_64BaselineOperandLocation,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    p6_arm64_emit_ldr_xd_from_frame_location(bytes, bytecode_index, location, register_contract::X0)
}

fn p6_arm64_emit_ldr_xd_from_frame_location(
    bytes: &mut Vec<u8>,
    bytecode_index: BytecodeIndex,
    location: P6X86_64BaselineOperandLocation,
    dest: register_contract::Arm64Gpr,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    let byte_offset = p6_arm64_frame_location_byte_offset(bytecode_index, location)?;
    let Some(instruction) =
        p6_arm64_encode_ldr_unsigned_64(dest, register_contract::CALL_FRAME_REGISTER, byte_offset)
    else {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::FrameOffsetOutOfDisp32 {
                bytecode_index,
                location,
                byte_offset,
            },
        );
    };
    p6_arm64_emit_word(bytes, instruction)
}

fn p6_arm64_emit_str_xd_to_frame_location(
    bytes: &mut Vec<u8>,
    bytecode_index: BytecodeIndex,
    source: register_contract::Arm64Gpr,
    location: P6X86_64BaselineOperandLocation,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    let byte_offset = p6_arm64_frame_location_byte_offset(bytecode_index, location)?;
    let Some(instruction) = p6_arm64_encode_str_unsigned_64(
        source,
        register_contract::CALL_FRAME_REGISTER,
        byte_offset,
    ) else {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::FrameOffsetOutOfDisp32 {
                bytecode_index,
                location,
                byte_offset,
            },
        );
    };
    p6_arm64_emit_word(bytes, instruction)
}

fn p6_arm64_frame_location_byte_offset(
    bytecode_index: BytecodeIndex,
    location: P6X86_64BaselineOperandLocation,
) -> Result<u64, P6X86_64BaselineSemanticByteEmissionError> {
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
    if byte_offset.is_multiple_of(8) && byte_offset / 8 <= 0x0fff {
        Ok(byte_offset)
    } else {
        Err(
            P6X86_64BaselineSemanticByteEmissionError::FrameOffsetOutOfDisp32 {
                bytecode_index,
                location,
                byte_offset,
            },
        )
    }
}

fn p6_arm64_encode_ldr_unsigned_64(
    dest: register_contract::Arm64Gpr,
    base: register_contract::Arm64Gpr,
    byte_offset: u64,
) -> Option<u32> {
    if !byte_offset.is_multiple_of(8) || byte_offset / 8 > 0x0fff {
        return None;
    }
    let imm12 = (byte_offset / 8) as u32;
    Some(0xf940_0000_u32 | (imm12 << 10) | (u32::from(base.index) << 5) | u32::from(dest.index))
}

fn p6_arm64_encode_str_unsigned_64(
    source: register_contract::Arm64Gpr,
    base: register_contract::Arm64Gpr,
    byte_offset: u64,
) -> Option<u32> {
    if !byte_offset.is_multiple_of(8) || byte_offset / 8 > 0x0fff {
        return None;
    }
    let imm12 = (byte_offset / 8) as u32;
    Some(0xf900_0000_u32 | (imm12 << 10) | (u32::from(base.index) << 5) | u32::from(source.index))
}

fn p6_arm64_emit_mov_x0_u64(
    bytes: &mut Vec<u8>,
    value: u64,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    p6_arm64_emit_mov_xd_u64(bytes, register_contract::X0, value)
}

fn p6_arm64_emit_mov_xd_u64(
    bytes: &mut Vec<u8>,
    dest: register_contract::Arm64Gpr,
    value: u64,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    let mut emitted_movz = false;
    for halfword in 0..4_u32 {
        let chunk = ((value >> (halfword * 16)) & 0xffff) as u32;
        if !emitted_movz {
            if chunk == 0 && halfword != 3 && value != 0 {
                continue;
            }
            p6_arm64_emit_word(
                bytes,
                0xd280_0000_u32 | (halfword << 21) | (chunk << 5) | u32::from(dest.index),
            )?;
            emitted_movz = true;
        } else if chunk != 0 {
            p6_arm64_emit_word(
                bytes,
                0xf280_0000_u32 | (halfword << 21) | (chunk << 5) | u32::from(dest.index),
            )?;
        }
    }
    if !emitted_movz {
        p6_arm64_emit_word(bytes, 0xd280_0000 | u32::from(dest.index))?;
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
                physical: register_contract::RETURN_VALUE_GPR.name,
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::Scratch0,
                physical: register_contract::X9.name,
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::Scratch1,
                physical: register_contract::X10.name,
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::Scratch2,
                physical: register_contract::X11.name,
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::PropertyBase,
                physical: register_contract::PROPERTY_BASE_RESERVED_PHYSICAL,
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::PinnedCalleeValue,
                physical: register_contract::X2.name,
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::PinnedCallFrameBase,
                physical: register_contract::PINNED_CALL_FRAME_BASE_PHYSICAL,
            },
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::PinnedVm,
                physical: register_contract::PINNED_VM_RETURN_SEED_PHYSICAL,
            },
            // C++ ARM64 baseline has GPRInfo::jitDataRegister in x26/regCS7.
            // The shared return-seed map has no jitData symbolic register yet,
            // so x26 stays recorded in the local dormant contract instead of
            // widening cross-module emitter state in this metadata-only batch.
            P6X86_64BaselinePhysicalRegisterBinding {
                symbolic: P6X86_64BaselineSymbolicRegister::MetadataTableBase,
                physical: register_contract::METADATA_TABLE_BASE_RESERVED_PHYSICAL,
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

#[cfg(test)]
mod tests {
    use super::control_flow::*;
    use super::*;
    use crate::jit::emitter::{
        P6BaselineNativeReentryTargetRecord, P6X86_64BaselineBytecodeBranchKind,
        P6X86_64BaselineControlFlowBranchContract, P6X86_64BaselineLoweredInstruction,
        P6X86_64BaselineOperandLocationRecord, P6X86_64BaselineSelectedInstructionEffects,
        P6X86_64BaselineSelectedSideExitReason, P6X86_64BaselineSideExitDestinationEffect,
        P6X86_64BaselineSideExitLabel, P6X86_64BaselineValueLayoutContract,
    };

    fn bci(offset: u32) -> BytecodeIndex {
        BytecodeIndex::from_offset(offset)
    }

    fn jump(source_offset: u32, end_offset: u32) -> Arm64BaselineJumpRecord {
        Arm64BaselineJumpRecord {
            source_offset,
            end_offset,
        }
    }

    fn assert_linked_direct_branch(
        linked: Arm64LinkedDirectBranch,
        kind: Arm64DirectBranchKind,
        source_instruction_offset: u32,
        target_offset: u32,
        word_offset: i64,
        instruction_word: u32,
    ) {
        assert_eq!(linked.kind, kind);
        assert_eq!(linked.source_instruction_offset, source_instruction_offset);
        assert_eq!(linked.target_offset, target_offset);
        assert_eq!(
            linked.byte_delta_from_source_instruction,
            target_offset as i64 - source_instruction_offset as i64
        );
        assert_eq!(linked.word_offset, word_offset);
        assert_eq!(linked.instruction_word, instruction_word);
    }

    fn physical_binding(symbolic: P6X86_64BaselineSymbolicRegister) -> &'static str {
        let map = p6_arm64_semantic_physical_register_map();
        map.bindings
            .iter()
            .find(|binding| binding.symbolic == symbolic)
            .unwrap()
            .physical
    }

    fn materialization_for(
        register: register_contract::Arm64Gpr,
    ) -> register_contract::Arm64BaselineMaterializationRequirement {
        register_contract::REQUIRED_MATERIALIZED_REGISTER_STATES
            .iter()
            .copied()
            .find(|requirement| requirement.register == register)
            .unwrap()
    }

    fn assert_no_x86_ud2_bytes(bytes: &[u8]) {
        assert!(!bytes.windows(2).any(|window| window == [0x0f, 0x0b]));
    }

    fn decode_mov_x0_u64_sequence(bytes: &[u8]) -> u64 {
        assert!(!bytes.is_empty());
        assert_eq!(bytes.len() % 4, 0);

        let mut value = 0_u64;
        let mut saw_movz = false;
        for chunk in bytes.chunks_exact(4) {
            let word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let halfword = ((word >> 21) & 0x3) as u64;
            let imm16 = ((word >> 5) & 0xffff) as u64;
            let shift = halfword * 16;

            match word & 0xff80_001f {
                0xd280_0000 => {
                    assert!(!saw_movz, "payload materialization has multiple movz");
                    saw_movz = true;
                    value = imm16 << shift;
                }
                0xf280_0000 => {
                    assert!(saw_movz, "payload materialization starts with movk");
                    value &= !(0xffff_u64 << shift);
                    value |= imm16 << shift;
                }
                _ => panic!("unexpected ARM64 x0 materialization word 0x{word:08x}"),
            }
        }
        assert!(saw_movz, "payload materialization is missing movz");
        value
    }

    #[test]
    fn arm64_baseline_register_contract_matches_gprinfo_jsrinfo_identity() {
        assert_eq!(
            register_contract::RETURN_VALUE_JSR.gpr,
            register_contract::X0
        );
        assert_eq!(
            register_contract::RETURN_VALUE_JSR.gpr,
            register_contract::REG_T0
        );
        assert_eq!(
            register_contract::RETURN_VALUE_GPR,
            register_contract::REG_T0
        );
        assert_eq!(
            register_contract::RETURN_VALUE_GPR2,
            register_contract::REG_T1
        );
        assert_eq!(
            register_contract::CALL_FRAME_REGISTER,
            register_contract::X29
        );
        assert_eq!(register_contract::CALL_FRAME_REGISTER.index, 29);
        assert_eq!(register_contract::CALL_FRAME_REGISTER_NAME, "fp/x29");
        assert_eq!(
            register_contract::ARGUMENT_GPRS,
            [
                register_contract::X0,
                register_contract::X1,
                register_contract::X2,
                register_contract::X3,
                register_contract::X4,
                register_contract::X5,
                register_contract::X6,
                register_contract::X7,
            ]
        );
        assert_eq!(register_contract::TEMPORARY_GPRS[0], register_contract::X0);
        assert_eq!(
            register_contract::TEMPORARY_GPRS[15],
            register_contract::X15
        );
    }

    #[test]
    fn arm64_callable_payload_return_stub_materializes_x0_payload_then_epilogue() {
        let cases = [
            (0_u64, 4_usize),
            (0x1234_5678_u64, 8_usize),
            (0xfedc_ba98_7654_3210_u64, 16_usize),
        ];

        for (payload, expected_mov_byte_len) in cases {
            let stub = p6_arm64_callable_payload_return_stub_bytes(payload).unwrap();
            let mov_byte_len = stub.len() - P6_ARM64_CALLABLE_EPILOGUE_BYTES.len();

            assert_eq!(mov_byte_len, expected_mov_byte_len);
            assert_eq!(&stub[mov_byte_len..], P6_ARM64_CALLABLE_EPILOGUE_BYTES);
            assert_eq!(decode_mov_x0_u64_sequence(&stub[..mov_byte_len]), payload);
            assert_no_x86_ud2_bytes(&stub);
        }
    }

    #[test]
    fn arm64_callable_side_exit_payload_return_stub_preserves_p6_payload_identity() {
        for side_exit_index in [0_u32, 1, u32::MAX] {
            let encoded_payload = P6X86_64BaselineSideExitReturnPayload::encode(side_exit_index);
            let stub =
                p6_arm64_callable_side_exit_payload_return_stub_bytes(encoded_payload).unwrap();
            let mov_byte_len = stub.len() - P6_ARM64_CALLABLE_EPILOGUE_BYTES.len();
            let decoded_bits = decode_mov_x0_u64_sequence(&stub[..mov_byte_len]);

            assert_eq!(&stub[mov_byte_len..], P6_ARM64_CALLABLE_EPILOGUE_BYTES);
            assert_eq!(decoded_bits, encoded_payload.raw_bits());
            assert_eq!(
                P6X86_64BaselineSideExitReturnPayload::decode(decoded_bits),
                Some(encoded_payload)
            );
            assert_eq!(encoded_payload.side_exit_index(), side_exit_index);
            assert_no_x86_ud2_bytes(&stub);
        }
    }

    #[test]
    fn arm64_baseline_metadata_tag_and_callee_save_registers_match_gprinfo() {
        assert_eq!(
            register_contract::METADATA_TABLE_REGISTER,
            register_contract::X25
        );
        assert_eq!(register_contract::JIT_DATA_REGISTER, register_contract::X26);
        assert_eq!(
            register_contract::NUMBER_TAG_REGISTER,
            register_contract::X27
        );
        assert_eq!(
            register_contract::NOT_CELL_MASK_REGISTER,
            register_contract::X28
        );
        assert_eq!(
            register_contract::REG_CS6,
            register_contract::METADATA_TABLE_REGISTER
        );
        assert_eq!(
            register_contract::REG_CS7,
            register_contract::JIT_DATA_REGISTER
        );
        assert_eq!(
            register_contract::REG_CS8,
            register_contract::NUMBER_TAG_REGISTER
        );
        assert_eq!(
            register_contract::REG_CS9,
            register_contract::NOT_CELL_MASK_REGISTER
        );
        assert_eq!(
            register_contract::BASELINE_RESERVED_CALLEE_SAVE_GPRS,
            [
                register_contract::X25,
                register_contract::X26,
                register_contract::X27,
                register_contract::X28,
            ]
        );
    }

    #[test]
    fn arm64_baseline_materialization_contract_is_metadata_only() {
        use register_contract::{
            Arm64BaselineMaterializationEmission as Emission,
            Arm64BaselineMaterializationSource as Source, Arm64BaselineMaterializedState as State,
            Arm64BaselineTagConstant as TagConstant,
        };

        let number_tag = materialization_for(register_contract::NUMBER_TAG_REGISTER);
        let not_cell_mask = materialization_for(register_contract::NOT_CELL_MASK_REGISTER);
        let jit_data = materialization_for(register_contract::JIT_DATA_REGISTER);
        let metadata_table = materialization_for(register_contract::METADATA_TABLE_REGISTER);

        assert_eq!(
            number_tag.required_state,
            State::TagConstant(TagConstant::NumberTag)
        );
        assert_eq!(
            not_cell_mask.required_state,
            State::TagConstant(TagConstant::NotCellMask)
        );
        assert_ne!(number_tag.required_state, not_cell_mask.required_state);
        assert_eq!(jit_data.required_state, State::JitDataFromCodeBlockJitData);
        assert_eq!(
            metadata_table.required_state,
            State::MetadataTableFromCodeBlockMetadataTable
        );
        assert_eq!(number_tag.source, Source::AssemblyHelpersTagCheckRegisters);
        assert_eq!(
            not_cell_mask.source,
            Source::AssemblyHelpersTagCheckRegisters
        );
        assert_eq!(jit_data.source, Source::CodeBlockJitDataAndMetadataTable);
        assert_eq!(
            metadata_table.source,
            Source::CodeBlockJitDataAndMetadataTable
        );
        for requirement in register_contract::REQUIRED_MATERIALIZED_REGISTER_STATES {
            assert_eq!(
                requirement.emission,
                Emission::DormantMetadataOnlyNoInstructions
            );
        }
    }

    #[test]
    fn arm64_return_seed_c_abi_map_is_narrower_than_full_baseline_contract() {
        let map = p6_arm64_semantic_physical_register_map();
        let exact_physical_registers: Vec<_> = map
            .bindings
            .iter()
            .map(|binding| binding.physical)
            .collect();

        assert_eq!(
            physical_binding(P6X86_64BaselineSymbolicRegister::ReturnGpr),
            register_contract::RETURN_VALUE_GPR.name
        );
        assert_eq!(
            physical_binding(P6X86_64BaselineSymbolicRegister::MetadataTableBase),
            register_contract::METADATA_TABLE_BASE_RESERVED_PHYSICAL
        );
        assert_eq!(
            physical_binding(P6X86_64BaselineSymbolicRegister::PinnedVm),
            register_contract::PINNED_VM_RETURN_SEED_PHYSICAL
        );
        assert!(register_contract::REQUIRED_MATERIALIZED_REGISTER_STATES
            .iter()
            .any(|requirement| requirement.register == register_contract::JIT_DATA_REGISTER));
        assert!(!exact_physical_registers.contains(&register_contract::JIT_DATA_REGISTER.name));
        assert!(!exact_physical_registers.contains(&register_contract::NUMBER_TAG_REGISTER.name));
        assert!(!exact_physical_registers.contains(&register_contract::NOT_CELL_MASK_REGISTER.name));
        assert!(
            map.bindings.len()
                < register_contract::TEMPORARY_GPRS.len()
                    + register_contract::BASELINE_RESERVED_CALLEE_SAVE_GPRS.len()
        );
    }

    #[test]
    fn arm64_baseline_bytecode_labels_resolve_and_pending_jumps_keep_append_order() {
        let mut builder = Arm64BaselineControlFlowBuilder::new();
        builder.record_label(bci(8), 0x80).unwrap();
        builder.record_label(bci(0), 0x10).unwrap();
        builder.record_label(bci(4), 0x44).unwrap();

        let mut jump_list = Arm64BaselineJumpList::from_jump(jump(0x20, 0x24));
        jump_list.push(jump(0x30, 0x34));
        builder
            .record_pending_jump(jump(0x10, 0x14), bci(8))
            .unwrap();
        builder
            .record_pending_jump_list(&jump_list, bci(4))
            .unwrap();

        let labels = builder.labels_in_bytecode_order();
        assert_eq!(
            labels,
            vec![
                Arm64BaselineBytecodeLabel {
                    bytecode_index: bci(0),
                    code_offset: 0x10,
                },
                Arm64BaselineBytecodeLabel {
                    bytecode_index: bci(4),
                    code_offset: 0x44,
                },
                Arm64BaselineBytecodeLabel {
                    bytecode_index: bci(8),
                    code_offset: 0x80,
                },
            ]
        );

        let linked = builder.link_pending_jumps().unwrap();
        assert_eq!(linked.len(), 3);
        assert_eq!(linked[0].target_bytecode_index, bci(8));
        assert_eq!(linked[0].target_offset, 0x80);
        assert_eq!(linked[0].byte_displacement_from_source, 0x70);
        assert_eq!(linked[0].byte_displacement_from_end, 0x6c);
        assert_eq!(linked[1].target_bytecode_index, bci(4));
        assert_eq!(linked[1].target_offset, 0x44);
        assert_eq!(linked[1].byte_displacement_from_source, 0x24);
        assert_eq!(linked[1].byte_displacement_from_end, 0x20);
        assert_eq!(linked[2].target_bytecode_index, bci(4));
        assert_eq!(linked[2].target_offset, 0x44);
    }

    #[test]
    fn arm64_baseline_bytecode_label_resolution_rejects_missing_and_duplicate_targets() {
        let mut missing = Arm64BaselineControlFlowBuilder::new();
        missing.record_label(bci(0), 0x10).unwrap();
        missing
            .record_pending_jump(jump(0x20, 0x24), bci(4))
            .unwrap();
        assert_eq!(
            missing.link_pending_jumps(),
            Err(Arm64BaselineControlFlowLinkError::MissingLabel {
                bytecode_index: bci(4),
            })
        );

        let mut duplicate = Arm64BaselineControlFlowBuilder::new();
        duplicate.record_label(bci(2), 0x20).unwrap();
        assert_eq!(
            duplicate.record_label(bci(2), 0x28),
            Err(Arm64BaselineControlFlowLinkError::DuplicateLabel {
                bytecode_index: bci(2),
                existing_offset: 0x20,
                duplicate_offset: 0x28,
            })
        );
    }

    #[test]
    fn arm64_baseline_slow_cases_link_by_bytecode_index_order() {
        let mut builder = Arm64BaselineControlFlowBuilder::new();
        builder
            .record_slow_case_jump(bci(1), jump(0x10, 0x14), Some(0x18))
            .unwrap();
        builder
            .record_slow_case_jump(bci(1), jump(0x20, 0x24), Some(0x28))
            .unwrap();
        builder
            .record_slow_case(Arm64BaselineSlowCaseEntry {
                jump: None,
                bytecode_index: bci(3),
                fast_path_resume_offset: None,
                slow_path_offset: None,
            })
            .unwrap();

        let linked_at_one = builder
            .link_all_slow_cases_up_to_bytecode_index(bci(1), 0x90)
            .unwrap();
        assert_eq!(linked_at_one.len(), 2);
        assert_eq!(linked_at_one[0].entry.bytecode_index, bci(1));
        assert_eq!(linked_at_one[0].entry.fast_path_resume_offset, Some(0x18));
        assert_eq!(linked_at_one[0].entry.slow_path_offset, Some(0x90));
        assert_eq!(
            linked_at_one[0].linked_jump.unwrap(),
            Arm64BaselineLinkedJumpRecord {
                jump: jump(0x10, 0x14),
                target_offset: 0x90,
                byte_displacement_from_source: 0x80,
                byte_displacement_from_end: 0x7c,
            }
        );
        assert_eq!(linked_at_one[1].entry.bytecode_index, bci(1));

        let linked_at_two = builder
            .link_all_slow_cases_up_to_bytecode_index(bci(2), 0xa0)
            .unwrap();
        assert!(linked_at_two.is_empty());

        let linked_at_three = builder
            .link_all_slow_cases_up_to_bytecode_index(bci(3), 0xb0)
            .unwrap();
        assert_eq!(linked_at_three.len(), 1);
        assert_eq!(linked_at_three[0].entry.bytecode_index, bci(3));
        assert_eq!(linked_at_three[0].entry.slow_path_offset, Some(0xb0));
        assert_eq!(linked_at_three[0].linked_jump, None);
    }

    #[test]
    fn arm64_baseline_slow_cases_reject_out_of_order_entries() {
        let mut builder = Arm64BaselineControlFlowBuilder::new();
        builder
            .record_slow_case_jump(bci(4), jump(0x20, 0x24), None)
            .unwrap();
        assert_eq!(
            builder.record_slow_case_jump(bci(2), jump(0x30, 0x34), None),
            Err(Arm64BaselineControlFlowLinkError::SlowCaseOutOfOrder {
                previous: bci(4),
                current: bci(2),
            })
        );
    }

    #[test]
    fn arm64_baseline_x0_jsvalue_return_is_distinct_from_slow_case_control_edge() {
        let normal =
            Arm64BaselineControlFlowBuilder::normal_return_value_jsr(bci(9), 0x70).unwrap();
        let slow_entry = Arm64BaselineSlowCaseEntry {
            jump: Some(jump(0x70, 0x74)),
            bytecode_index: bci(9),
            fast_path_resume_offset: Some(0x78),
            slow_path_offset: None,
        };
        let slow = Arm64BaselineControlEdgeRecord::SlowCaseControlEdge(slow_entry);

        assert_eq!(
            normal,
            Arm64BaselineControlEdgeRecord::NormalReturnValueJSR(
                Arm64BaselineNormalReturnValueRecord {
                    bytecode_index: bci(9),
                    return_value_gpr: "x0",
                    return_value_jsr: "returnValueJSR",
                    value_offset: 0x70,
                }
            )
        );
        assert!(matches!(
            slow,
            Arm64BaselineControlEdgeRecord::SlowCaseControlEdge(_)
        ));
        assert_ne!(normal, slow);
    }

    #[test]
    fn arm64_baseline_jump_list_records_empty_and_appended_jumps() {
        let mut empty = Arm64BaselineJumpList::new();
        assert!(empty.jumps().is_empty());
        empty.push(jump(0x40, 0x44));
        assert_eq!(empty.jumps(), &[jump(0x40, 0x44)]);
    }

    #[test]
    fn arm64_baseline_direct_branch_placeholders_match_arm64_b_and_b_cond() {
        assert_eq!(unconditional_branch_placeholder_word(), 0x1400_0000);
        assert_eq!(
            Arm64DirectBranchPatch::unconditional(0x20).placeholder_word(),
            0x1400_0000
        );
        assert_eq!(
            conditional_branch_placeholder_word(Arm64Condition::Eq),
            0x5400_0000
        );
        assert_eq!(
            conditional_branch_placeholder_word(Arm64Condition::Ne),
            0x5400_0001
        );
        assert_eq!(
            Arm64DirectBranchPatch::conditional(0x24, Arm64Condition::Le).placeholder_word(),
            0x5400_000d
        );
        assert_eq!(
            conditional_branch_placeholder_word(Arm64Condition::Invalid),
            0x5400_000f
        );

        for code in 0..=15 {
            assert_eq!(Arm64Condition::from_code(code).unwrap().code(), code);
        }
        assert_eq!(Arm64Condition::from_code(16), None);
        assert_eq!(ARM64_CONDITION_CS, Arm64Condition::Hs);
        assert_eq!(ARM64_CONDITION_CC, Arm64Condition::Lo);
    }

    fn arm64_word(bytes: &[u8], offset: u32) -> u32 {
        let start = offset as usize;
        u32::from_le_bytes([
            bytes[start],
            bytes[start + 1],
            bytes[start + 2],
            bytes[start + 3],
        ])
    }

    fn instruction_record(
        bytecode_offset: u32,
        start_offset: u32,
    ) -> P6X86_64BaselineInstructionByteRecord {
        P6X86_64BaselineInstructionByteRecord {
            bytecode_index: bci(bytecode_offset),
            start_offset,
            end_offset: start_offset + 4,
            byte_len: 4,
            machine_instruction_count: 1,
            bytes: vec![0x1f, 0x20, 0x03, 0xd5],
        }
    }

    fn branch_contract(
        kind: P6X86_64BaselineBytecodeBranchKind,
        source_bytecode_offset: u32,
        target_bytecode_offset: u32,
    ) -> P6X86_64BaselineControlFlowBranchContract {
        P6X86_64BaselineControlFlowBranchContract {
            kind,
            source_bytecode_index: bci(source_bytecode_offset),
            target_bytecode_index: bci(target_bytecode_offset),
        }
    }

    fn side_exit_label(
        bytecode_offset: u32,
        reason: P6X86_64BaselineSelectedSideExitReason,
    ) -> P6X86_64BaselineSideExitLabel {
        P6X86_64BaselineSideExitLabel {
            reason,
            retained_bytecode_index: bci(bytecode_offset),
            destination: P6X86_64BaselineSideExitDestinationEffect::DestinationUnchanged,
            may_throw: false,
            runtime_call: false,
            heap_allocation: false,
            touches_gc_roots: false,
        }
    }

    fn frame_local_location(byte_offset: u64) -> P6X86_64BaselineOperandLocation {
        P6X86_64BaselineOperandLocation::FrameLocal {
            local_index: 0,
            slot_index: (byte_offset / 8) as u32,
            byte_offset,
        }
    }

    fn frame_argument_location(
        byte_offset_from_frame_base: u64,
    ) -> P6X86_64BaselineOperandLocation {
        P6X86_64BaselineOperandLocation::FrameArgument {
            argument_index_including_this: 0,
            raw_virtual_register: 5,
            byte_offset_from_argument_base: 0,
            byte_offset_from_frame_base,
        }
    }

    fn selected_instruction(
        bytecode_offset: u32,
        operation: P6X86_64BaselineLoweredOperation,
        operand_locations: Vec<P6X86_64BaselineOperandLocationRecord>,
    ) -> P6X86_64BaselineSelectedInstruction {
        P6X86_64BaselineSelectedInstruction {
            bytecode_index: bci(bytecode_offset),
            lowered: P6X86_64BaselineLoweredInstruction {
                bytecode_index: bci(bytecode_offset),
                width: crate::bytecode::OperandWidth::Narrow,
                operation,
            },
            operand_locations,
            effects: P6X86_64BaselineSelectedInstructionEffects {
                may_throw: false,
                runtime_call: false,
                heap_allocation: false,
                touches_gc_roots: false,
            },
            machine_instructions: Vec::new(),
        }
    }

    fn destination_location_record(
        location: P6X86_64BaselineOperandLocation,
    ) -> P6X86_64BaselineOperandLocationRecord {
        P6X86_64BaselineOperandLocationRecord {
            role: P6X86_64BaselineOperandRole::Destination,
            location,
        }
    }

    fn source_location_record(
        location: P6X86_64BaselineOperandLocation,
    ) -> P6X86_64BaselineOperandLocationRecord {
        P6X86_64BaselineOperandLocationRecord {
            role: P6X86_64BaselineOperandRole::Source,
            location,
        }
    }

    fn return_value_location_record(
        location: P6X86_64BaselineOperandLocation,
    ) -> P6X86_64BaselineOperandLocationRecord {
        P6X86_64BaselineOperandLocationRecord {
            role: P6X86_64BaselineOperandRole::ReturnValue,
            location,
        }
    }

    fn branch_target_location_record(
        target: BytecodeIndex,
    ) -> P6X86_64BaselineOperandLocationRecord {
        P6X86_64BaselineOperandLocationRecord {
            role: P6X86_64BaselineOperandRole::BranchTarget,
            location: P6X86_64BaselineOperandLocation::Immediate(
                P6X86_64BaselineImmediateOperand::BytecodeIndex(target),
            ),
        }
    }

    fn rust_low_byte_value_layout() -> P6X86_64BaselineValueLayoutContract {
        // JSVALUE64 immediate VALUES (undefined 0xa / null 0x2 / false 0x6 /
        // true 0x7, runtime/JSCJSValue.h:472-491) plus the live `number_tag`/
        // `double_encode_offset` the arm64 seed uses for boxInt32/boxDouble.
        // int32/double/cell stay on the transitional low-byte scheme that the
        // x86 byte-emitter (and the dormant frame-write helper) still emit.
        P6X86_64BaselineValueLayoutContract {
            layout_name: "rust-jsvalue64-immediates-transitional-cells",
            storage_bits: 64,
            slot_width_bytes: 8,
            tag_mask: 0xff,
            payload_shift: 8,
            immediate_undefined_tag: 0x0a,
            immediate_null_tag: 0x02,
            immediate_false_tag: 0x06,
            immediate_true_tag: 0x07,
            immediate_int32_tag: 0x10,
            immediate_double_tag: 0x30,
            cell_tag: 0x20,
            double_tag: 0x30,
            number_tag: 0xfffe_0000_0000_0000,
            double_encode_offset: 1 << 49,
        }
    }

    #[test]
    fn arm64_branch_if_false_primitive_words_match_macro_assembler_arm64_fast_path() {
        assert_eq!(
            p6_arm64_encode_ldr_unsigned_64(
                register_contract::X9,
                register_contract::CALL_FRAME_REGISTER,
                0
            ),
            Some(0xf940_03a9)
        );
        assert_eq!(
            p6_arm64_encode_ldr_unsigned_64(
                register_contract::X9,
                register_contract::CALL_FRAME_REGISTER,
                16
            ),
            Some(0xf940_0ba9)
        );
        assert_eq!(p6_arm64_encode_and_x10_x9_low_byte_tag_mask(), 0x9240_1d2a);
        assert_eq!(
            p6_arm64_encode_cmp_imm64(register_contract::X10, 0x03),
            0xf100_0d5f
        );
        assert_eq!(
            p6_arm64_encode_cmp_imm64(register_contract::X10, 0x10),
            0xf100_415f
        );
        assert_eq!(
            p6_arm64_encode_cmp_imm64(register_contract::X9, 0x10),
            0xf100_413f
        );
    }

    #[test]
    fn arm64_frame_store_words_match_macro_assembler_arm64_unsigned_immediate_form() {
        assert_eq!(
            p6_arm64_encode_str_unsigned_64(
                register_contract::X9,
                register_contract::CALL_FRAME_REGISTER,
                0
            ),
            Some(0xf900_03a9)
        );
        assert_eq!(
            p6_arm64_encode_str_unsigned_64(
                register_contract::X9,
                register_contract::CALL_FRAME_REGISTER,
                16
            ),
            Some(0xf900_0ba9)
        );
        assert_eq!(
            p6_arm64_encode_str_unsigned_64(
                register_contract::X2,
                register_contract::CALL_FRAME_REGISTER,
                24
            ),
            Some(0xf900_0fa2)
        );
        assert_eq!(
            p6_arm64_encode_str_unsigned_64(
                register_contract::X9,
                register_contract::CALL_FRAME_REGISTER,
                4095 * 8
            ),
            Some(0xf93f_ffa9)
        );
        assert_eq!(
            p6_arm64_encode_str_unsigned_64(
                register_contract::X9,
                register_contract::CALL_FRAME_REGISTER,
                4
            ),
            None
        );
        assert_eq!(
            p6_arm64_encode_str_unsigned_64(
                register_contract::X9,
                register_contract::CALL_FRAME_REGISTER,
                4096 * 8
            ),
            None
        );
    }

    #[test]
    fn arm64_materializes_immediate_jsvalue_bits_to_frame_local_with_scratch_store() {
        let mut bytes = Vec::new();

        p6_arm64_emit_materialize_value_to_frame_location(
            &mut bytes,
            bci(2),
            P6Arm64ReturnSeedValue::Immediate(0x1234_5678),
            frame_local_location(16),
        )
        .unwrap();

        assert_eq!(arm64_word(&bytes, 0), 0xd28a_cf09);
        assert_eq!(arm64_word(&bytes, 4), 0xf2a2_4689);
        assert_eq!(arm64_word(&bytes, 8), 0xf900_0ba9);
    }

    #[test]
    fn arm64_materializes_callee_value_to_frame_argument_slot() {
        let mut bytes = Vec::new();

        p6_arm64_emit_materialize_value_to_frame_location(
            &mut bytes,
            bci(3),
            P6Arm64ReturnSeedValue::CalleeValue,
            frame_argument_location(24),
        )
        .unwrap();

        assert_eq!(bytes.len(), 4);
        assert_eq!(arm64_word(&bytes, 0), 0xf900_0fa2);
    }

    #[test]
    fn arm64_materializes_frame_to_frame_move_through_scratch_load_store() {
        let mut bytes = Vec::new();

        p6_arm64_emit_materialize_value_to_frame_location(
            &mut bytes,
            bci(4),
            P6Arm64ReturnSeedValue::Frame(frame_local_location(8)),
            frame_local_location(16),
        )
        .unwrap();

        assert_eq!(bytes.len(), 8);
        assert_eq!(arm64_word(&bytes, 0), 0xf940_07a9);
        assert_eq!(arm64_word(&bytes, 4), 0xf900_0ba9);
    }

    #[test]
    fn arm64_seed_frame_write_helper_materializes_load_and_move_selected_instructions() {
        let value_layout = rust_low_byte_value_layout();
        let load = selected_instruction(
            0,
            P6X86_64BaselineLoweredOperation::LoadInt32 {
                destination: VirtualRegister::local(0),
                value: 7,
            },
            vec![destination_location_record(frame_local_location(0))],
        );
        let move_from_known = selected_instruction(
            1,
            P6X86_64BaselineLoweredOperation::Move {
                destination: VirtualRegister::local(1),
                source: VirtualRegister::local(0),
            },
            vec![
                destination_location_record(frame_local_location(8)),
                source_location_record(frame_local_location(0)),
            ],
        );
        let move_from_frame = selected_instruction(
            2,
            P6X86_64BaselineLoweredOperation::Move {
                destination: VirtualRegister::local(0),
                source: VirtualRegister::local(1),
            },
            vec![
                destination_location_record(frame_local_location(0)),
                source_location_record(frame_local_location(8)),
            ],
        );
        let mut known_values = Vec::new();
        let mut bytes = Vec::new();

        p6_arm64_emit_materialize_seed_frame_write(
            &mut bytes,
            value_layout,
            &mut known_values,
            &load,
        )
        .unwrap();
        p6_arm64_emit_materialize_seed_frame_write(
            &mut bytes,
            value_layout,
            &mut known_values,
            &move_from_known,
        )
        .unwrap();
        known_values.clear();
        p6_arm64_emit_materialize_seed_frame_write(
            &mut bytes,
            value_layout,
            &mut known_values,
            &move_from_frame,
        )
        .unwrap();

        // TRANSITIONAL (D1a): this dormant frame-write builder still emits the
        // low-byte int32 `(7 << 8) | 0x10` = 0x710 (single MOVZ X9, #0x710); the
        // live return seed uses JSVALUE64 boxInt32 instead.
        assert_eq!(arm64_word(&bytes, 0), 0xd280_e209);
        assert_eq!(arm64_word(&bytes, 4), 0xf900_03a9);
        assert_eq!(arm64_word(&bytes, 8), 0xd280_e209);
        assert_eq!(arm64_word(&bytes, 12), 0xf900_07a9);
        assert_eq!(arm64_word(&bytes, 16), 0xf940_07a9);
        assert_eq!(arm64_word(&bytes, 20), 0xf900_03a9);
    }

    #[test]
    fn arm64_materialization_rejects_non_frame_destination_with_existing_semantic_error() {
        let mut bytes = Vec::new();
        let location = P6X86_64BaselineOperandLocation::Immediate(
            crate::jit::emitter::P6X86_64BaselineImmediateOperand::Null,
        );

        assert_eq!(
            p6_arm64_emit_materialize_value_to_frame_location(
                &mut bytes,
                bci(9),
                P6Arm64ReturnSeedValue::CalleeValue,
                location,
            ),
            Err(
                P6X86_64BaselineSemanticByteEmissionError::UnsupportedOperandLocation {
                    bytecode_index: bci(9),
                    location,
                    reason:
                        P6X86_64BaselineSemanticOperandRejectionReason::ExpectedFrameLocalMemory,
                }
            )
        );
        assert!(bytes.is_empty());
    }

    #[test]
    fn arm64_dormant_branch_aware_callable_encodes_forward_jump_if_false_shared_epilogue() {
        let value_layout = rust_low_byte_value_layout();
        let instructions = vec![
            selected_instruction(
                0,
                P6X86_64BaselineLoweredOperation::LoadBool {
                    destination: VirtualRegister::local(0),
                    raw_immediate: 0,
                    value: false,
                },
                vec![destination_location_record(frame_local_location(0))],
            ),
            selected_instruction(
                1,
                P6X86_64BaselineLoweredOperation::JumpIfFalse {
                    source: VirtualRegister::local(0),
                    target: bci(4),
                },
                vec![
                    source_location_record(frame_local_location(0)),
                    branch_target_location_record(bci(4)),
                ],
            ),
            selected_instruction(
                2,
                P6X86_64BaselineLoweredOperation::LoadInt32 {
                    destination: VirtualRegister::local(1),
                    value: 11,
                },
                vec![destination_location_record(frame_local_location(8))],
            ),
            selected_instruction(
                3,
                P6X86_64BaselineLoweredOperation::Return {
                    source: VirtualRegister::local(1),
                },
                vec![return_value_location_record(frame_local_location(8))],
            ),
            selected_instruction(
                4,
                P6X86_64BaselineLoweredOperation::LoadInt32 {
                    destination: VirtualRegister::local(1),
                    value: 42,
                },
                vec![destination_location_record(frame_local_location(8))],
            ),
            selected_instruction(
                5,
                P6X86_64BaselineLoweredOperation::Return {
                    source: VirtualRegister::local(1),
                },
                vec![return_value_location_record(frame_local_location(8))],
            ),
        ];
        let encoded =
            encode_p6_arm64_dormant_branch_aware_callable_selection(value_layout, &instructions)
                .unwrap();
        let prologue = encoded.callable_prologue.as_ref().unwrap();
        let epilogue = encoded.callable_normal_epilogue.as_ref().unwrap();

        assert_eq!(
            encoded.terminal_policy.policy,
            P6X86_64BaselineTerminalPolicy::CallableCAbiPrologueBytecodeBranchesSharedNormalEpilogueThenInlinePayloadStubs
        );
        assert_eq!(encoded.terminal_policy.return_bytecode_index, bci(5));
        assert_eq!(prologue.start_offset, 0);
        assert_eq!(prologue.end_offset, 8);
        assert_eq!(prologue.bytes, P6_ARM64_CALLABLE_PROLOGUE_BYTES);
        assert_eq!(epilogue.start_offset, 100);
        assert_eq!(epilogue.ret_offset, 104);
        assert_eq!(epilogue.end_offset, 108);
        assert_eq!(epilogue.bytes, P6_ARM64_CALLABLE_EPILOGUE_BYTES);
        assert_eq!(encoded.terminal_policy.ret_offset, epilogue.ret_offset);
        assert_eq!(
            encoded.terminal_policy.normal_path_end_offset,
            epilogue.end_offset
        );
        assert_eq!(encoded.instruction_bytes.len(), instructions.len());
        assert_eq!(encoded.instruction_bytes[4].bytecode_index, bci(4));
        assert_eq!(encoded.instruction_bytes[4].start_offset, 88);

        assert_eq!(encoded.bytecode_branches.len(), 4);
        for (record, expected_offset) in encoded.bytecode_branches.iter().zip([28_u32, 36, 44, 68])
        {
            assert_eq!(record.bytecode_index, bci(1));
            assert_eq!(
                record.kind,
                P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken
            );
            assert_eq!(record.branch_offset, expected_offset);
            assert_eq!(record.rel32_offset, expected_offset);
            assert_eq!(record.branch_end_offset, expected_offset + 4);
            assert_eq!(record.target_bytecode_index, bci(4));
            assert_eq!(record.target_offset, 88);
            assert_eq!(
                arm64_word(&encoded.bytes, expected_offset),
                link_conditional_branch(Arm64Condition::Eq, expected_offset, 88)
                    .unwrap()
                    .instruction_word
            );
        }

        assert_eq!(
            arm64_word(&encoded.bytes, 84),
            link_unconditional_branch(84, epilogue.start_offset)
                .unwrap()
                .instruction_word
        );
        assert_eq!(encoded.side_exit_return_stubs.len(), 1);
        let side_exit = &encoded.side_exit_return_stubs[0];
        let taken_reentry = &encoded.instruction_bytes[4];
        let fallthrough_reentry = &encoded.instruction_bytes[2];
        assert_eq!(side_exit.bytecode_index, bci(1));
        assert_eq!(
            side_exit.reason,
            P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand
        );
        assert_eq!(side_exit.branch_offset, 60);
        assert_eq!(side_exit.branch_end_offset, 64);
        assert_eq!(side_exit.target_offset, epilogue.end_offset);
        assert_eq!(side_exit.resume_bytecode_index, None);
        assert_eq!(side_exit.resume_entry_offset, None);
        assert_eq!(
            side_exit.native_reentry_targets,
            vec![
                P6BaselineNativeReentryTargetRecord {
                    resume_bytecode_index: bci(4),
                    resume_entry_offset: taken_reentry.start_offset,
                },
                P6BaselineNativeReentryTargetRecord {
                    resume_bytecode_index: bci(2),
                    resume_entry_offset: fallthrough_reentry.start_offset,
                },
            ]
        );
        assert_eq!(
            arm64_word(&encoded.bytes, side_exit.branch_offset),
            link_conditional_branch(
                Arm64Condition::Ne,
                side_exit.branch_offset,
                epilogue.end_offset
            )
            .unwrap()
            .instruction_word
        );
        assert_eq!(
            side_exit.bytes,
            p6_arm64_callable_side_exit_payload_return_stub_bytes(side_exit.encoded_payload)
                .unwrap()
        );
    }

    #[test]
    fn arm64_dormant_branch_aware_callable_rejects_unsupported_branch_operations() {
        for operation in [
            P6X86_64BaselineLoweredOperation::Jump { target: bci(2) },
            P6X86_64BaselineLoweredOperation::JumpIfNotNullish {
                source: VirtualRegister::local(0),
                target: bci(2),
            },
        ] {
            let branch = selected_instruction(1, operation, Vec::new());
            let ret = selected_instruction(
                2,
                P6X86_64BaselineLoweredOperation::Return {
                    source: VirtualRegister::local(0),
                },
                vec![return_value_location_record(frame_local_location(0))],
            );
            assert_eq!(
                encode_p6_arm64_dormant_branch_aware_callable_selection(
                    rust_low_byte_value_layout(),
                    &[branch.clone(), ret],
                ),
                Err(p6_arm64_seed_unsupported_operation(&branch))
            );
        }
    }

    #[test]
    fn arm64_dormant_branch_aware_callable_rejects_backward_jump_if_false_and_missing_source() {
        let backward = selected_instruction(
            1,
            P6X86_64BaselineLoweredOperation::JumpIfFalse {
                source: VirtualRegister::local(0),
                target: bci(0),
            },
            vec![
                source_location_record(frame_local_location(0)),
                branch_target_location_record(bci(0)),
            ],
        );
        let missing_source = selected_instruction(
            1,
            P6X86_64BaselineLoweredOperation::JumpIfFalse {
                source: VirtualRegister::local(0),
                target: bci(2),
            },
            vec![branch_target_location_record(bci(2))],
        );
        let ret = selected_instruction(
            2,
            P6X86_64BaselineLoweredOperation::Return {
                source: VirtualRegister::local(0),
            },
            vec![return_value_location_record(frame_local_location(0))],
        );

        for branch in [backward, missing_source] {
            assert_eq!(
                encode_p6_arm64_dormant_branch_aware_callable_selection(
                    rust_low_byte_value_layout(),
                    &[branch.clone(), ret.clone()],
                ),
                Err(p6_arm64_seed_unsupported_operation(&branch))
            );
        }
    }

    #[test]
    fn arm64_semantic_builder_emits_primitive_jump_if_false_fast_path_and_side_exit_stub() {
        let mut builder = P6Arm64SemanticByteBuilder::default();
        let bytecode_index = bci(4);
        let value_layout = rust_low_byte_value_layout();
        let target = branch_contract(P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken, 4, 12);
        let unsupported_exit = side_exit_label(
            4,
            P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
        );

        builder
            .emit_branch_if_false_primitive(
                bytecode_index,
                frame_local_location(16),
                value_layout,
                unsupported_exit,
                target,
                &[bci(12), bci(8)],
            )
            .unwrap();
        let fallthrough_offset = builder.offset().unwrap();
        builder.emit_word(0xd503_201f).unwrap();
        let target_offset = builder.emit_word(0xd503_201f).unwrap();
        let branch_records = builder
            .finish_bytecode_branches(&[instruction_record(12, target_offset)])
            .unwrap();
        let side_exit_records = builder
            .finish_side_exit_return_stubs(&[
                instruction_record(8, fallthrough_offset),
                instruction_record(12, target_offset),
            ])
            .unwrap();

        assert_eq!(fallthrough_offset, 56);
        assert_eq!(target_offset, 60);
        assert_eq!(branch_records.len(), 4);
        assert_eq!(side_exit_records.len(), 1);

        let expected_branch_offsets = [12, 20, 28, 52];
        for (record, expected_branch_offset) in branch_records.iter().zip(expected_branch_offsets) {
            assert_eq!(record.bytecode_index, bytecode_index);
            assert_eq!(
                record.kind,
                P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken
            );
            assert_eq!(record.branch_offset, expected_branch_offset);
            assert_eq!(record.branch_end_offset, expected_branch_offset + 4);
            assert_eq!(record.target_bytecode_index, bci(12));
            assert_eq!(record.target_offset, target_offset);
            assert_eq!(
                arm64_word(builder.bytes(), expected_branch_offset),
                link_conditional_branch(Arm64Condition::Eq, expected_branch_offset, target_offset)
                    .unwrap()
                    .instruction_word
            );
        }

        assert_eq!(arm64_word(builder.bytes(), 0), 0xf940_0ba9);
        assert_eq!(arm64_word(builder.bytes(), 4), 0x9240_1d2a);
        for (tag, cmp_offset, branch_offset) in [
            (value_layout.immediate_undefined_tag, 8, 12),
            (value_layout.immediate_null_tag, 16, 20),
            (value_layout.immediate_false_tag, 24, 28),
        ] {
            assert_eq!(
                arm64_word(builder.bytes(), cmp_offset),
                p6_arm64_encode_cmp_imm64(register_contract::X10, tag as u8)
            );
            assert_eq!(
                arm64_word(builder.bytes(), branch_offset),
                link_conditional_branch(Arm64Condition::Eq, branch_offset, target_offset)
                    .unwrap()
                    .instruction_word
            );
        }
        assert_eq!(
            arm64_word(builder.bytes(), 32),
            p6_arm64_encode_cmp_imm64(
                register_contract::X10,
                value_layout.immediate_true_tag as u8
            )
        );
        assert_eq!(
            arm64_word(builder.bytes(), 36),
            link_conditional_branch(Arm64Condition::Eq, 36, fallthrough_offset)
                .unwrap()
                .instruction_word
        );
        assert_eq!(
            arm64_word(builder.bytes(), 40),
            p6_arm64_encode_cmp_imm64(
                register_contract::X10,
                value_layout.immediate_int32_tag as u8
            )
        );
        assert_eq!(
            arm64_word(builder.bytes(), 48),
            p6_arm64_encode_cmp_imm64(
                register_contract::X9,
                value_layout.immediate_int32_tag as u8
            )
        );
        assert_eq!(
            arm64_word(builder.bytes(), 52),
            link_conditional_branch(Arm64Condition::Eq, 52, target_offset)
                .unwrap()
                .instruction_word
        );
        // JSVALUE64 immediate values: cmp X10,#0xa (undefined), #0x2 (null),
        // #0x6 (false), #0x7 (true). int32 (#0x10) stays transitional low-byte.
        assert_eq!(arm64_word(builder.bytes(), 8), 0xf100_295f);
        assert_eq!(arm64_word(builder.bytes(), 16), 0xf100_095f);
        assert_eq!(arm64_word(builder.bytes(), 24), 0xf100_195f);
        assert_eq!(arm64_word(builder.bytes(), 32), 0xf100_1d5f);
        assert_eq!(arm64_word(builder.bytes(), 36), 0x5400_00a0);
        assert_eq!(arm64_word(builder.bytes(), 40), 0xf100_415f);
        assert_eq!(arm64_word(builder.bytes(), 48), 0xf100_413f);
        assert_eq!(arm64_word(builder.bytes(), 52), 0x5400_0040);
        assert_eq!(arm64_word(builder.bytes(), fallthrough_offset), 0xd503_201f);
        assert_eq!(arm64_word(builder.bytes(), target_offset), 0xd503_201f);

        let side_exit = &side_exit_records[0];
        assert_eq!(side_exit.bytecode_index, bytecode_index);
        assert_eq!(
            side_exit.reason,
            P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand
        );
        assert_eq!(side_exit.branch_offset, 44);
        assert_eq!(side_exit.branch_end_offset, 48);
        assert_eq!(side_exit.target_offset, 64);
        assert_eq!(side_exit.resume_bytecode_index, None);
        assert_eq!(side_exit.resume_entry_offset, None);
        assert_eq!(
            side_exit.native_reentry_targets,
            vec![
                P6BaselineNativeReentryTargetRecord {
                    resume_bytecode_index: bci(12),
                    resume_entry_offset: target_offset,
                },
                P6BaselineNativeReentryTargetRecord {
                    resume_bytecode_index: bci(8),
                    resume_entry_offset: fallthrough_offset,
                },
            ]
        );
        assert_eq!(
            arm64_word(builder.bytes(), side_exit.branch_offset),
            link_conditional_branch(Arm64Condition::Ne, side_exit.branch_offset, 64)
                .unwrap()
                .instruction_word
        );
        assert_eq!(
            side_exit.bytes,
            p6_arm64_callable_side_exit_payload_return_stub_bytes(side_exit.encoded_payload)
                .unwrap()
        );
        assert_eq!(side_exit.encoded_payload.side_exit_index(), 0);
    }

    #[test]
    fn arm64_semantic_builder_patches_forward_unconditional_bytecode_branch() {
        let mut builder = P6Arm64SemanticByteBuilder::default();
        let source = branch_contract(P6X86_64BaselineBytecodeBranchKind::UnconditionalJump, 0, 8);

        builder.emit_unconditional_bytecode_branch(source).unwrap();
        builder.emit_word(0xd503_201f).unwrap();
        let target_offset = builder.emit_word(0xd503_201f).unwrap();
        let records = builder
            .finish_bytecode_branches(&[instruction_record(8, target_offset)])
            .unwrap();
        let expected = link_unconditional_branch(0, target_offset)
            .unwrap()
            .instruction_word;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].bytecode_index, bci(0));
        assert_eq!(
            records[0].kind,
            P6X86_64BaselineBytecodeBranchKind::UnconditionalJump
        );
        assert_eq!(records[0].branch_offset, 0);
        assert_eq!(records[0].rel32_offset, 0);
        assert_eq!(records[0].branch_end_offset, 4);
        assert_eq!(records[0].target_bytecode_index, bci(8));
        assert_eq!(records[0].target_offset, target_offset);
        assert_eq!(arm64_word(builder.bytes(), 0), expected);
    }

    #[test]
    fn arm64_semantic_builder_patches_forward_conditional_bytecode_branch() {
        let mut builder = P6Arm64SemanticByteBuilder::default();
        let source = branch_contract(P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken, 4, 12);

        builder.emit_word(0xd503_201f).unwrap();
        builder
            .emit_conditional_bytecode_branch(source, Arm64Condition::Eq)
            .unwrap();
        let target_offset = builder.emit_word(0xd503_201f).unwrap();
        let records = builder
            .finish_bytecode_branches(&[instruction_record(12, target_offset)])
            .unwrap();
        let expected = link_conditional_branch(Arm64Condition::Eq, 4, target_offset)
            .unwrap()
            .instruction_word;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].bytecode_index, bci(4));
        assert_eq!(
            records[0].kind,
            P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken
        );
        assert_eq!(records[0].branch_offset, 4);
        assert_eq!(records[0].rel32_offset, 4);
        assert_eq!(records[0].branch_end_offset, 8);
        assert_eq!(records[0].target_bytecode_index, bci(12));
        assert_eq!(records[0].target_offset, target_offset);
        assert_eq!(arm64_word(builder.bytes(), 4), expected);
    }

    #[test]
    fn arm64_semantic_builder_reports_missing_bytecode_branch_target() {
        let mut builder = P6Arm64SemanticByteBuilder::default();
        let source = branch_contract(P6X86_64BaselineBytecodeBranchKind::UnconditionalJump, 0, 44);

        builder.emit_unconditional_bytecode_branch(source).unwrap();
        assert_eq!(
            builder.finish_bytecode_branches(&[]),
            Err(
                P6X86_64BaselineSemanticByteEmissionError::BranchTargetMissing {
                    bytecode_index: bci(0),
                    target: bci(44),
                }
            )
        );
    }

    #[test]
    fn arm64_semantic_builder_appends_side_exit_payload_return_stubs_in_order() {
        let mut builder = P6Arm64SemanticByteBuilder::default();
        let first_label = side_exit_label(
            20,
            P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
        );
        let second_label =
            side_exit_label(24, P6X86_64BaselineSelectedSideExitReason::NonInt32Operand);

        builder.emit_word(0xd503_201f).unwrap();
        builder
            .emit_retained_side_exit_direct_branch(
                bci(20),
                first_label,
                Arm64DirectBranchKind::Unconditional,
            )
            .unwrap();
        builder.emit_word(0xd503_201f).unwrap();
        builder
            .emit_retained_side_exit_direct_branch(
                bci(24),
                second_label,
                Arm64DirectBranchKind::Unconditional,
            )
            .unwrap();
        let normal_path_end = builder.offset().unwrap();
        let records = builder.finish_side_exit_return_stubs(&[]).unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].bytecode_index, bci(20));
        assert_eq!(records[0].side_exit_index, 0);
        assert_eq!(records[0].target_offset, normal_path_end);
        assert_eq!(records[0].resume_bytecode_index, None);
        assert_eq!(records[0].resume_entry_offset, None);
        assert!(records[0].native_reentry_targets.is_empty());
        assert_eq!(records[1].bytecode_index, bci(24));
        assert_eq!(records[1].side_exit_index, 1);
        assert_eq!(records[1].target_offset, records[0].stub_end_offset);
        assert_eq!(records[1].resume_bytecode_index, None);
        assert_eq!(records[1].resume_entry_offset, None);
        assert!(records[1].native_reentry_targets.is_empty());

        for record in &records {
            let expected_stub =
                p6_arm64_callable_side_exit_payload_return_stub_bytes(record.encoded_payload)
                    .unwrap();
            let expected_branch =
                link_unconditional_branch(record.branch_offset, record.target_offset)
                    .unwrap()
                    .instruction_word;

            assert_eq!(record.bytes, expected_stub);
            assert_eq!(record.byte_len as usize, expected_stub.len());
            assert_eq!(
                record.stub_end_offset,
                record.target_offset + record.byte_len
            );
            assert_eq!(
                P6X86_64BaselineSideExitReturnPayload::decode(record.encoded_payload.raw_bits()),
                Some(record.encoded_payload)
            );
            assert_eq!(
                record.encoded_payload.side_exit_index(),
                record.side_exit_index
            );
            assert_eq!(
                arm64_word(builder.bytes(), record.branch_offset),
                expected_branch
            );
        }
    }

    #[test]
    fn arm64_baseline_direct_unconditional_links_forward_and_backward() {
        let forward = link_unconditional_branch(0x100, 0x110).unwrap();
        assert_linked_direct_branch(
            forward,
            Arm64DirectBranchKind::Unconditional,
            0x100,
            0x110,
            4,
            0x1400_0004,
        );

        let backward = Arm64DirectBranchPatch::unconditional(0x110)
            .link_to(0x100)
            .unwrap();
        assert_linked_direct_branch(
            backward,
            Arm64DirectBranchKind::Unconditional,
            0x110,
            0x100,
            -4,
            0x17ff_fffc,
        );
    }

    #[test]
    fn arm64_baseline_direct_conditional_links_forward_and_backward() {
        let kind = Arm64DirectBranchKind::Conditional {
            condition: Arm64Condition::Ne,
        };
        let forward = link_conditional_branch(Arm64Condition::Ne, 0x100, 0x110).unwrap();
        assert_linked_direct_branch(forward, kind, 0x100, 0x110, 4, 0x5400_0081);

        let backward = Arm64DirectBranchPatch::conditional(0x110, Arm64Condition::Ne)
            .link_to(0x100)
            .unwrap();
        assert_linked_direct_branch(backward, kind, 0x110, 0x100, -4, 0x54ff_ff81);
    }

    #[test]
    fn arm64_baseline_direct_branch_linking_accepts_signed_immediate_boundaries() {
        let unconditional_max = ((1_u32 << 25) - 1) * 4;
        assert_eq!(
            link_unconditional_branch(0, unconditional_max)
                .unwrap()
                .instruction_word,
            0x15ff_ffff
        );
        assert_eq!(
            link_unconditional_branch(unconditional_max + 4, 0)
                .unwrap()
                .instruction_word,
            0x1600_0000
        );

        let conditional_max = ((1_u32 << 18) - 1) * 4;
        assert_eq!(
            link_conditional_branch(Arm64Condition::Eq, 0, conditional_max)
                .unwrap()
                .instruction_word,
            0x547f_ffe0
        );
        assert_eq!(
            link_conditional_branch(Arm64Condition::Ne, conditional_max + 4, 0)
                .unwrap()
                .instruction_word,
            0x5480_0001
        );
    }

    #[test]
    fn arm64_baseline_direct_branch_linking_rejects_unaligned_offsets() {
        assert_eq!(
            link_unconditional_branch(0x101, 0x104),
            Err(Arm64DirectBranchLinkError::UnalignedOffset {
                role: Arm64DirectBranchOffsetRole::SourceInstruction,
                offset: 0x101,
            })
        );
        assert_eq!(
            link_conditional_branch(Arm64Condition::Eq, 0x100, 0x103),
            Err(Arm64DirectBranchLinkError::UnalignedOffset {
                role: Arm64DirectBranchOffsetRole::Target,
                offset: 0x103,
            })
        );
    }

    #[test]
    fn arm64_baseline_direct_branch_linking_rejects_out_of_range_offsets() {
        let unconditional_target = (1_u32 << 25) * 4;
        assert_eq!(
            link_unconditional_branch(0, unconditional_target),
            Err(Arm64DirectBranchLinkError::OutOfRange {
                kind: Arm64DirectBranchKind::Unconditional,
                source_instruction_offset: 0,
                target_offset: unconditional_target,
                word_offset: 1_i64 << 25,
                signed_word_bits: 26,
            })
        );

        let conditional_target = (1_u32 << 18) * 4;
        assert_eq!(
            link_conditional_branch(Arm64Condition::Eq, 0, conditional_target),
            Err(Arm64DirectBranchLinkError::OutOfRange {
                kind: Arm64DirectBranchKind::Conditional {
                    condition: Arm64Condition::Eq,
                },
                source_instruction_offset: 0,
                target_offset: conditional_target,
                word_offset: 1_i64 << 18,
                signed_word_bits: 19,
            })
        );
    }
}
