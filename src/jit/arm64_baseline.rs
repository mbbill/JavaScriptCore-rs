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

#[allow(dead_code)]
pub(crate) mod control_flow {
    use super::BytecodeIndex;

    // C++ JSC map: `AbstractMacroAssembler::Jump`/`JumpList`, `JIT::JumpTable`,
    // and `JIT::SlowCaseEntry` are control-flow records linked to labels after the
    // hot path has been emitted. ARM64 `returnValueJSR` is x0; `JITCall.cpp`
    // `op_ret` loads the normal JSValue return into x0 and jumps ReturnFromBaseline.
    // Rust must not treat x0 as a fallback discriminator. Slow/fallback edges are
    // represented here only as metadata until the out-of-line ARM64 baseline slow
    // path machinery is ported.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) struct Arm64BaselineBytecodeLabel {
        pub(crate) bytecode_index: BytecodeIndex,
        pub(crate) code_offset: u32,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) struct Arm64BaselineJumpRecord {
        pub(crate) source_offset: u32,
        pub(crate) end_offset: u32,
    }

    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub(crate) struct Arm64BaselineJumpList {
        jumps: Vec<Arm64BaselineJumpRecord>,
    }

    impl Arm64BaselineJumpList {
        pub(crate) fn new() -> Self {
            Self { jumps: Vec::new() }
        }

        pub(crate) fn from_jump(jump: Arm64BaselineJumpRecord) -> Self {
            Self { jumps: vec![jump] }
        }

        pub(crate) fn push(&mut self, jump: Arm64BaselineJumpRecord) {
            self.jumps.push(jump);
        }

        pub(crate) fn jumps(&self) -> &[Arm64BaselineJumpRecord] {
            &self.jumps
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) struct Arm64BaselineJumpTableEntry {
        pub(crate) jump: Arm64BaselineJumpRecord,
        pub(crate) target_bytecode_index: BytecodeIndex,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) struct Arm64BaselineLinkedJumpRecord {
        pub(crate) jump: Arm64BaselineJumpRecord,
        pub(crate) target_offset: u32,
        pub(crate) byte_displacement_from_source: i64,
        pub(crate) byte_displacement_from_end: i64,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) struct Arm64BaselineLinkedJumpTableEntry {
        pub(crate) jump: Arm64BaselineJumpRecord,
        pub(crate) target_bytecode_index: BytecodeIndex,
        pub(crate) target_offset: u32,
        pub(crate) byte_displacement_from_source: i64,
        pub(crate) byte_displacement_from_end: i64,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) struct Arm64BaselineSlowCaseEntry {
        pub(crate) jump: Option<Arm64BaselineJumpRecord>,
        pub(crate) bytecode_index: BytecodeIndex,
        pub(crate) fast_path_resume_offset: Option<u32>,
        pub(crate) slow_path_offset: Option<u32>,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) struct Arm64BaselineLinkedSlowCaseEntry {
        pub(crate) entry: Arm64BaselineSlowCaseEntry,
        pub(crate) linked_jump: Option<Arm64BaselineLinkedJumpRecord>,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) struct Arm64BaselineNormalReturnValueRecord {
        pub(crate) bytecode_index: BytecodeIndex,
        pub(crate) return_value_gpr: &'static str,
        pub(crate) return_value_jsr: &'static str,
        pub(crate) value_offset: u32,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Arm64BaselineControlEdgeRecord {
        NormalReturnValueJSR(Arm64BaselineNormalReturnValueRecord),
        SlowCaseControlEdge(Arm64BaselineSlowCaseEntry),
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Arm64BaselineControlFlowLinkError {
        InvalidBytecodeIndex {
            bytecode_index: BytecodeIndex,
        },
        DuplicateLabel {
            bytecode_index: BytecodeIndex,
            existing_offset: u32,
            duplicate_offset: u32,
        },
        MissingLabel {
            bytecode_index: BytecodeIndex,
        },
        InvalidJumpRange {
            source_offset: u32,
            end_offset: u32,
        },
        SlowCaseOutOfOrder {
            previous: BytecodeIndex,
            current: BytecodeIndex,
        },
    }

    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub(crate) struct Arm64BaselineControlFlowBuilder {
        labels: Vec<Arm64BaselineBytecodeLabel>,
        jump_table: Vec<Arm64BaselineJumpTableEntry>,
        slow_cases: Vec<Arm64BaselineSlowCaseEntry>,
        next_slow_case_to_link: usize,
    }

    impl Arm64BaselineControlFlowBuilder {
        pub(crate) fn new() -> Self {
            Self::default()
        }

        pub(crate) fn record_label(
            &mut self,
            bytecode_index: BytecodeIndex,
            code_offset: u32,
        ) -> Result<Arm64BaselineBytecodeLabel, Arm64BaselineControlFlowLinkError> {
            validate_arm64_baseline_bytecode_index(bytecode_index)?;
            if let Some(existing) = self
                .labels
                .iter()
                .find(|label| label.bytecode_index == bytecode_index)
            {
                return Err(Arm64BaselineControlFlowLinkError::DuplicateLabel {
                    bytecode_index,
                    existing_offset: existing.code_offset,
                    duplicate_offset: code_offset,
                });
            }
            let label = Arm64BaselineBytecodeLabel {
                bytecode_index,
                code_offset,
            };
            self.labels.push(label);
            Ok(label)
        }

        pub(crate) fn labels_in_bytecode_order(&self) -> Vec<Arm64BaselineBytecodeLabel> {
            let mut labels = self.labels.clone();
            labels.sort_by_key(|label| label.bytecode_index);
            labels
        }

        pub(crate) fn record_pending_jump(
            &mut self,
            jump: Arm64BaselineJumpRecord,
            target_bytecode_index: BytecodeIndex,
        ) -> Result<Arm64BaselineJumpTableEntry, Arm64BaselineControlFlowLinkError> {
            validate_arm64_baseline_jump(jump)?;
            validate_arm64_baseline_bytecode_index(target_bytecode_index)?;
            let entry = Arm64BaselineJumpTableEntry {
                jump,
                target_bytecode_index,
            };
            self.jump_table.push(entry);
            Ok(entry)
        }

        pub(crate) fn record_pending_jump_list(
            &mut self,
            jumps: &Arm64BaselineJumpList,
            target_bytecode_index: BytecodeIndex,
        ) -> Result<Vec<Arm64BaselineJumpTableEntry>, Arm64BaselineControlFlowLinkError> {
            jumps
                .jumps()
                .iter()
                .copied()
                .map(|jump| self.record_pending_jump(jump, target_bytecode_index))
                .collect()
        }

        pub(crate) fn link_pending_jumps(
            &self,
        ) -> Result<Vec<Arm64BaselineLinkedJumpTableEntry>, Arm64BaselineControlFlowLinkError>
        {
            self.jump_table
                .iter()
                .copied()
                .map(|entry| {
                    let target_offset = self.label_offset(entry.target_bytecode_index)?;
                    let linked = resolve_arm64_baseline_jump(entry.jump, target_offset)?;
                    Ok(Arm64BaselineLinkedJumpTableEntry {
                        jump: entry.jump,
                        target_bytecode_index: entry.target_bytecode_index,
                        target_offset,
                        byte_displacement_from_source: linked.byte_displacement_from_source,
                        byte_displacement_from_end: linked.byte_displacement_from_end,
                    })
                })
                .collect()
        }

        pub(crate) fn record_slow_case(
            &mut self,
            entry: Arm64BaselineSlowCaseEntry,
        ) -> Result<Arm64BaselineSlowCaseEntry, Arm64BaselineControlFlowLinkError> {
            validate_arm64_baseline_bytecode_index(entry.bytecode_index)?;
            if let Some(jump) = entry.jump {
                validate_arm64_baseline_jump(jump)?;
            }
            if let Some(previous) = self.slow_cases.last() {
                if previous.bytecode_index > entry.bytecode_index {
                    return Err(Arm64BaselineControlFlowLinkError::SlowCaseOutOfOrder {
                        previous: previous.bytecode_index,
                        current: entry.bytecode_index,
                    });
                }
            }
            self.slow_cases.push(entry);
            Ok(entry)
        }

        pub(crate) fn record_slow_case_jump(
            &mut self,
            bytecode_index: BytecodeIndex,
            jump: Arm64BaselineJumpRecord,
            fast_path_resume_offset: Option<u32>,
        ) -> Result<Arm64BaselineSlowCaseEntry, Arm64BaselineControlFlowLinkError> {
            self.record_slow_case(Arm64BaselineSlowCaseEntry {
                jump: Some(jump),
                bytecode_index,
                fast_path_resume_offset,
                slow_path_offset: None,
            })
        }

        pub(crate) fn link_all_slow_cases_up_to_bytecode_index(
            &mut self,
            bytecode_index: BytecodeIndex,
            slow_path_offset: u32,
        ) -> Result<Vec<Arm64BaselineLinkedSlowCaseEntry>, Arm64BaselineControlFlowLinkError>
        {
            validate_arm64_baseline_bytecode_index(bytecode_index)?;
            let mut linked = Vec::new();
            while let Some(entry) = self.slow_cases.get(self.next_slow_case_to_link).copied() {
                if entry.bytecode_index > bytecode_index {
                    break;
                }
                let entry = Arm64BaselineSlowCaseEntry {
                    slow_path_offset: Some(slow_path_offset),
                    ..entry
                };
                let linked_jump = entry
                    .jump
                    .map(|jump| resolve_arm64_baseline_jump(jump, slow_path_offset))
                    .transpose()?;
                linked.push(Arm64BaselineLinkedSlowCaseEntry { entry, linked_jump });
                self.next_slow_case_to_link += 1;
            }
            Ok(linked)
        }

        pub(crate) fn normal_return_value_jsr(
            bytecode_index: BytecodeIndex,
            value_offset: u32,
        ) -> Result<Arm64BaselineControlEdgeRecord, Arm64BaselineControlFlowLinkError> {
            validate_arm64_baseline_bytecode_index(bytecode_index)?;
            Ok(Arm64BaselineControlEdgeRecord::NormalReturnValueJSR(
                Arm64BaselineNormalReturnValueRecord {
                    bytecode_index,
                    return_value_gpr: "x0",
                    return_value_jsr: "returnValueJSR",
                    value_offset,
                },
            ))
        }

        fn label_offset(
            &self,
            bytecode_index: BytecodeIndex,
        ) -> Result<u32, Arm64BaselineControlFlowLinkError> {
            self.labels
                .iter()
                .find(|label| label.bytecode_index == bytecode_index)
                .map(|label| label.code_offset)
                .ok_or(Arm64BaselineControlFlowLinkError::MissingLabel { bytecode_index })
        }
    }

    fn validate_arm64_baseline_bytecode_index(
        bytecode_index: BytecodeIndex,
    ) -> Result<(), Arm64BaselineControlFlowLinkError> {
        if bytecode_index.is_valid() {
            Ok(())
        } else {
            Err(Arm64BaselineControlFlowLinkError::InvalidBytecodeIndex { bytecode_index })
        }
    }

    fn validate_arm64_baseline_jump(
        jump: Arm64BaselineJumpRecord,
    ) -> Result<(), Arm64BaselineControlFlowLinkError> {
        if jump.source_offset <= jump.end_offset {
            Ok(())
        } else {
            Err(Arm64BaselineControlFlowLinkError::InvalidJumpRange {
                source_offset: jump.source_offset,
                end_offset: jump.end_offset,
            })
        }
    }

    fn resolve_arm64_baseline_jump(
        jump: Arm64BaselineJumpRecord,
        target_offset: u32,
    ) -> Result<Arm64BaselineLinkedJumpRecord, Arm64BaselineControlFlowLinkError> {
        validate_arm64_baseline_jump(jump)?;
        Ok(Arm64BaselineLinkedJumpRecord {
            jump,
            target_offset,
            byte_displacement_from_source: target_offset as i64 - jump.source_offset as i64,
            byte_displacement_from_end: target_offset as i64 - jump.end_offset as i64,
        })
    }
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

#[cfg(test)]
mod tests {
    use super::control_flow::*;
    use super::*;

    fn bci(offset: u32) -> BytecodeIndex {
        BytecodeIndex::from_offset(offset)
    }

    fn jump(source_offset: u32, end_offset: u32) -> Arm64BaselineJumpRecord {
        Arm64BaselineJumpRecord {
            source_offset,
            end_offset,
        }
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
}
