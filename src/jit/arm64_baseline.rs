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
    P6X86_64BaselineSideExitReturnPayload, P6X86_64BaselineSymbolicRegister,
    P6X86_64BaselineTerminalPolicy, P6X86_64BaselineTerminalPolicyRecord,
    P6X86_64SemanticEncodedSelection, P6X86_64SemanticTerminalSelection,
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

#[allow(dead_code)]
pub(crate) mod direct_branch {
    const ARM64_INSTRUCTION_SIZE_BYTES: u32 = 4;
    const UNCONDITIONAL_BRANCH_BASE: u32 = 0x1400_0000;
    const CONDITIONAL_BRANCH_BASE: u32 = 0x5400_0000;
    const UNCONDITIONAL_BRANCH_IMM26_MASK: u32 = 0x03ff_ffff;
    const CONDITIONAL_BRANCH_IMM19_MASK: u32 = 0x0007_ffff;
    const UNCONDITIONAL_BRANCH_SIGNED_WORD_BITS: u8 = 26;
    const CONDITIONAL_BRANCH_SIGNED_WORD_BITS: u8 = 19;

    // C++ JSC map: `MacroAssemblerARM64::jump()` records the label before
    // `ARM64Assembler::b()`, while conditional helpers return
    // `Jump(makeBranch(cond))` after `ARM64Assembler::b_cond(cond)`.
    // `ARM64Assembler::linkJumpOrCall()` and `linkConditionalBranch()` compute
    // PC-relative immediates in instruction words from `fromInstruction`. For
    // non-fixed conditional jumps, C++ stores the public `Jump` label after
    // `b.cond` and normalizes it by subtracting one instruction in
    // `ARM64Assembler::link()` before patching; this Rust record stores the
    // already-normalized branch-instruction offset. This Rust module is dormant
    // encoding metadata only: it emits no executable control flow and exists to
    // keep the future ARM64 baseline emitter's patch records shaped like the JSC
    // assembler layer.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) struct Arm64DirectBranchPatch {
        pub(crate) kind: Arm64DirectBranchKind,
        pub(crate) source_instruction_offset: u32,
    }

    impl Arm64DirectBranchPatch {
        pub(crate) fn unconditional(source_instruction_offset: u32) -> Self {
            Self {
                kind: Arm64DirectBranchKind::Unconditional,
                source_instruction_offset,
            }
        }

        pub(crate) fn conditional(
            source_instruction_offset: u32,
            condition: Arm64Condition,
        ) -> Self {
            Self {
                kind: Arm64DirectBranchKind::Conditional { condition },
                source_instruction_offset,
            }
        }

        pub(crate) fn placeholder_word(self) -> u32 {
            self.kind.placeholder_word()
        }

        pub(crate) fn link_to(
            self,
            target_offset: u32,
        ) -> Result<Arm64LinkedDirectBranch, Arm64DirectBranchLinkError> {
            link_direct_branch(self.kind, self.source_instruction_offset, target_offset)
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Arm64DirectBranchKind {
        Unconditional,
        Conditional { condition: Arm64Condition },
    }

    impl Arm64DirectBranchKind {
        pub(crate) fn placeholder_word(self) -> u32 {
            match self {
                Self::Unconditional => encode_unconditional_branch_word(0),
                Self::Conditional { condition } => encode_conditional_branch_word(0, condition),
            }
        }

        fn signed_word_bits(self) -> u8 {
            match self {
                Self::Unconditional => UNCONDITIONAL_BRANCH_SIGNED_WORD_BITS,
                Self::Conditional { .. } => CONDITIONAL_BRANCH_SIGNED_WORD_BITS,
            }
        }
    }

    #[repr(u8)]
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Arm64Condition {
        Eq = 0,
        Ne = 1,
        Hs = 2,
        Lo = 3,
        Mi = 4,
        Pl = 5,
        Vs = 6,
        Vc = 7,
        Hi = 8,
        Ls = 9,
        Ge = 10,
        Lt = 11,
        Gt = 12,
        Le = 13,
        Al = 14,
        Invalid = 15,
    }

    impl Arm64Condition {
        pub(crate) fn from_code(code: u8) -> Option<Self> {
            match code {
                0 => Some(Self::Eq),
                1 => Some(Self::Ne),
                2 => Some(Self::Hs),
                3 => Some(Self::Lo),
                4 => Some(Self::Mi),
                5 => Some(Self::Pl),
                6 => Some(Self::Vs),
                7 => Some(Self::Vc),
                8 => Some(Self::Hi),
                9 => Some(Self::Ls),
                10 => Some(Self::Ge),
                11 => Some(Self::Lt),
                12 => Some(Self::Gt),
                13 => Some(Self::Le),
                14 => Some(Self::Al),
                15 => Some(Self::Invalid),
                _ => None,
            }
        }

        pub(crate) fn code(self) -> u8 {
            self as u8
        }
    }

    pub(crate) const ARM64_CONDITION_CS: Arm64Condition = Arm64Condition::Hs;
    pub(crate) const ARM64_CONDITION_CC: Arm64Condition = Arm64Condition::Lo;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Arm64DirectBranchOffsetRole {
        SourceInstruction,
        Target,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Arm64DirectBranchLinkError {
        UnalignedOffset {
            role: Arm64DirectBranchOffsetRole,
            offset: u32,
        },
        OutOfRange {
            kind: Arm64DirectBranchKind,
            source_instruction_offset: u32,
            target_offset: u32,
            word_offset: i64,
            signed_word_bits: u8,
        },
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) struct Arm64LinkedDirectBranch {
        pub(crate) kind: Arm64DirectBranchKind,
        pub(crate) source_instruction_offset: u32,
        pub(crate) target_offset: u32,
        pub(crate) byte_delta_from_source_instruction: i64,
        pub(crate) word_offset: i64,
        pub(crate) instruction_word: u32,
    }

    pub(crate) fn unconditional_branch_placeholder_word() -> u32 {
        Arm64DirectBranchKind::Unconditional.placeholder_word()
    }

    pub(crate) fn conditional_branch_placeholder_word(condition: Arm64Condition) -> u32 {
        Arm64DirectBranchKind::Conditional { condition }.placeholder_word()
    }

    pub(crate) fn link_unconditional_branch(
        source_instruction_offset: u32,
        target_offset: u32,
    ) -> Result<Arm64LinkedDirectBranch, Arm64DirectBranchLinkError> {
        link_direct_branch(
            Arm64DirectBranchKind::Unconditional,
            source_instruction_offset,
            target_offset,
        )
    }

    pub(crate) fn link_conditional_branch(
        condition: Arm64Condition,
        source_instruction_offset: u32,
        target_offset: u32,
    ) -> Result<Arm64LinkedDirectBranch, Arm64DirectBranchLinkError> {
        link_direct_branch(
            Arm64DirectBranchKind::Conditional { condition },
            source_instruction_offset,
            target_offset,
        )
    }

    fn link_direct_branch(
        kind: Arm64DirectBranchKind,
        source_instruction_offset: u32,
        target_offset: u32,
    ) -> Result<Arm64LinkedDirectBranch, Arm64DirectBranchLinkError> {
        validate_offset_alignment(
            Arm64DirectBranchOffsetRole::SourceInstruction,
            source_instruction_offset,
        )?;
        validate_offset_alignment(Arm64DirectBranchOffsetRole::Target, target_offset)?;

        let byte_delta_from_source_instruction =
            target_offset as i64 - source_instruction_offset as i64;
        let word_offset = byte_delta_from_source_instruction / ARM64_INSTRUCTION_SIZE_BYTES as i64;
        let signed_word_bits = kind.signed_word_bits();
        if !is_signed_n_bit_word_offset(word_offset, signed_word_bits) {
            // C++ `ARM64Assembler::linkConditionalBranch()` can fall back to an
            // inverted conditional branch plus direct unconditional branch pair
            // when the target is out of the direct b.cond range. This dormant
            // Rust skeleton records/rejects out-of-range direct branches until
            // full executable branch-pair emission is ported.
            return Err(Arm64DirectBranchLinkError::OutOfRange {
                kind,
                source_instruction_offset,
                target_offset,
                word_offset,
                signed_word_bits,
            });
        }

        let instruction_word = match kind {
            Arm64DirectBranchKind::Unconditional => encode_unconditional_branch_word(word_offset),
            Arm64DirectBranchKind::Conditional { condition } => {
                encode_conditional_branch_word(word_offset, condition)
            }
        };

        Ok(Arm64LinkedDirectBranch {
            kind,
            source_instruction_offset,
            target_offset,
            byte_delta_from_source_instruction,
            word_offset,
            instruction_word,
        })
    }

    fn validate_offset_alignment(
        role: Arm64DirectBranchOffsetRole,
        offset: u32,
    ) -> Result<(), Arm64DirectBranchLinkError> {
        if offset % ARM64_INSTRUCTION_SIZE_BYTES == 0 {
            Ok(())
        } else {
            Err(Arm64DirectBranchLinkError::UnalignedOffset { role, offset })
        }
    }

    fn is_signed_n_bit_word_offset(word_offset: i64, bits: u8) -> bool {
        let min = -(1_i64 << (bits - 1));
        let max = (1_i64 << (bits - 1)) - 1;
        (min..=max).contains(&word_offset)
    }

    fn encode_unconditional_branch_word(word_offset: i64) -> u32 {
        UNCONDITIONAL_BRANCH_BASE | ((word_offset as u32) & UNCONDITIONAL_BRANCH_IMM26_MASK)
    }

    fn encode_conditional_branch_word(word_offset: i64, condition: Arm64Condition) -> u32 {
        CONDITIONAL_BRANCH_BASE
            | (((word_offset as u32) & CONDITIONAL_BRANCH_IMM19_MASK) << 5)
            | condition.code() as u32
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
    use super::direct_branch::*;
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
