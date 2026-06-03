//! ARM64 baseline control-flow and direct branch patch records.
//!
//! C++ JSC map: this module mirrors the `MacroAssemblerARM64`/
//! `ARM64Assembler` branch-linking layer (`jump`, `makeBranch`,
//! `linkJumpOrCall`, and `linkConditionalBranch`) plus the baseline JIT
//! `Jump`, `JumpList`, `JumpTable`, and `SlowCaseEntry` bookkeeping used by
//! `JIT::addJump`/`addSlowCase`. The dormant semantic byte builder keeps those
//! records close to their ARM64 byte patch sites.
//!
//! Rust-only glue: `P6Arm64SemanticByteBuilder` owns its byte vector and pending
//! branch/side-exit lists so later patching can stay borrow-checked without
//! sharing mutable buffers across the parent return-seed emitter. It does not
//! add opcode admission, VM routing, or executable control-flow behavior.

#![allow(dead_code)]

use super::{
    p6_arm64_bytes_for_range, p6_arm64_callable_side_exit_payload_return_stub_bytes,
    p6_arm64_emit_bytes, p6_arm64_emit_ldr_xd_from_frame_location, p6_arm64_emit_word,
    register_contract,
};
use crate::bytecode::BytecodeIndex;
use crate::jit::emitter::{
    p6_x86_64_checked_byte_len, validate_p6_x86_64_semantic_value_layout,
    P6X86_64BaselineBytecodeBranchKind, P6X86_64BaselineBytecodeBranchRecord,
    P6X86_64BaselineControlFlowBranchContract, P6X86_64BaselineInstructionByteRecord,
    P6X86_64BaselineMachineInstruction, P6X86_64BaselineOperandLocation,
    P6X86_64BaselineSemanticByteEmissionError, P6X86_64BaselineSideExitDestinationEffect,
    P6X86_64BaselineSideExitLabel, P6X86_64BaselineSideExitReturnPayload,
    P6X86_64BaselineSideExitReturnStubRecord, P6X86_64BaselineSymbolicRegister,
    P6X86_64BaselineValueLayoutContract,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Arm64BaselineBytecodeLabel {
    pub(super) bytecode_index: BytecodeIndex,
    pub(super) code_offset: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Arm64BaselineJumpRecord {
    pub(super) source_offset: u32,
    pub(super) end_offset: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Arm64BaselineJumpList {
    jumps: Vec<Arm64BaselineJumpRecord>,
}

impl Arm64BaselineJumpList {
    pub(super) fn new() -> Self {
        Self { jumps: Vec::new() }
    }

    pub(super) fn from_jump(jump: Arm64BaselineJumpRecord) -> Self {
        Self { jumps: vec![jump] }
    }

    pub(super) fn push(&mut self, jump: Arm64BaselineJumpRecord) {
        self.jumps.push(jump);
    }

    pub(super) fn jumps(&self) -> &[Arm64BaselineJumpRecord] {
        &self.jumps
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Arm64BaselineJumpTableEntry {
    pub(super) jump: Arm64BaselineJumpRecord,
    pub(super) target_bytecode_index: BytecodeIndex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Arm64BaselineLinkedJumpRecord {
    pub(super) jump: Arm64BaselineJumpRecord,
    pub(super) target_offset: u32,
    pub(super) byte_displacement_from_source: i64,
    pub(super) byte_displacement_from_end: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Arm64BaselineLinkedJumpTableEntry {
    pub(super) jump: Arm64BaselineJumpRecord,
    pub(super) target_bytecode_index: BytecodeIndex,
    pub(super) target_offset: u32,
    pub(super) byte_displacement_from_source: i64,
    pub(super) byte_displacement_from_end: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Arm64BaselineSlowCaseEntry {
    pub(super) jump: Option<Arm64BaselineJumpRecord>,
    pub(super) bytecode_index: BytecodeIndex,
    pub(super) fast_path_resume_offset: Option<u32>,
    pub(super) slow_path_offset: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Arm64BaselineLinkedSlowCaseEntry {
    pub(super) entry: Arm64BaselineSlowCaseEntry,
    pub(super) linked_jump: Option<Arm64BaselineLinkedJumpRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Arm64BaselineNormalReturnValueRecord {
    pub(super) bytecode_index: BytecodeIndex,
    pub(super) return_value_gpr: &'static str,
    pub(super) return_value_jsr: &'static str,
    pub(super) value_offset: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Arm64BaselineControlEdgeRecord {
    NormalReturnValueJSR(Arm64BaselineNormalReturnValueRecord),
    SlowCaseControlEdge(Arm64BaselineSlowCaseEntry),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Arm64BaselineControlFlowLinkError {
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
pub(super) struct Arm64BaselineControlFlowBuilder {
    labels: Vec<Arm64BaselineBytecodeLabel>,
    jump_table: Vec<Arm64BaselineJumpTableEntry>,
    slow_cases: Vec<Arm64BaselineSlowCaseEntry>,
    next_slow_case_to_link: usize,
}

impl Arm64BaselineControlFlowBuilder {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn record_label(
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

    pub(super) fn labels_in_bytecode_order(&self) -> Vec<Arm64BaselineBytecodeLabel> {
        let mut labels = self.labels.clone();
        labels.sort_by_key(|label| label.bytecode_index);
        labels
    }

    pub(super) fn record_pending_jump(
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

    pub(super) fn record_pending_jump_list(
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

    pub(super) fn link_pending_jumps(
        &self,
    ) -> Result<Vec<Arm64BaselineLinkedJumpTableEntry>, Arm64BaselineControlFlowLinkError> {
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

    pub(super) fn record_slow_case(
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

    pub(super) fn record_slow_case_jump(
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

    pub(super) fn link_all_slow_cases_up_to_bytecode_index(
        &mut self,
        bytecode_index: BytecodeIndex,
        slow_path_offset: u32,
    ) -> Result<Vec<Arm64BaselineLinkedSlowCaseEntry>, Arm64BaselineControlFlowLinkError> {
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

    pub(super) fn normal_return_value_jsr(
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

mod direct_branch {
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

#[allow(unused_imports)]
pub(super) use direct_branch::{
    conditional_branch_placeholder_word, link_conditional_branch, link_unconditional_branch,
    unconditional_branch_placeholder_word, Arm64Condition, Arm64DirectBranchKind,
    Arm64DirectBranchLinkError, Arm64DirectBranchOffsetRole, Arm64DirectBranchPatch,
    Arm64LinkedDirectBranch, ARM64_CONDITION_CC, ARM64_CONDITION_CS,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct P6Arm64PendingBytecodeBranch {
    source: P6X86_64BaselineControlFlowBranchContract,
    kind: Arm64DirectBranchKind,
    source_instruction_offset: u32,
    branch_end_offset: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct P6Arm64PendingSideExitBranch {
    label: P6X86_64BaselineSideExitLabel,
    kind: Arm64DirectBranchKind,
    branch_offset: u32,
    branch_end_offset: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct P6Arm64PendingInternalBranch {
    kind: Arm64DirectBranchKind,
    branch_offset: u32,
    branch_end_offset: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct P6Arm64SemanticByteBuilder {
    bytes: Vec<u8>,
    pending_bytecode_branches: Vec<P6Arm64PendingBytecodeBranch>,
    pending_side_exits: Vec<P6Arm64PendingSideExitBranch>,
}

impl P6Arm64SemanticByteBuilder {
    pub(super) fn offset(&self) -> Result<u32, P6X86_64BaselineSemanticByteEmissionError> {
        p6_x86_64_checked_byte_len(self.bytes.len())
    }

    pub(super) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(super) fn emit_bytes(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        p6_arm64_emit_bytes(&mut self.bytes, bytes)
    }

    pub(super) fn emit_word(
        &mut self,
        word: u32,
    ) -> Result<u32, P6X86_64BaselineSemanticByteEmissionError> {
        let offset = self.offset()?;
        p6_arm64_emit_word(&mut self.bytes, word)?;
        Ok(offset)
    }

    pub(super) fn emit_unconditional_bytecode_branch(
        &mut self,
        source: P6X86_64BaselineControlFlowBranchContract,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        self.emit_direct_bytecode_branch(source, Arm64DirectBranchKind::Unconditional)
    }

    pub(super) fn emit_conditional_bytecode_branch(
        &mut self,
        source: P6X86_64BaselineControlFlowBranchContract,
        condition: Arm64Condition,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        self.emit_direct_bytecode_branch(source, Arm64DirectBranchKind::Conditional { condition })
    }

    pub(super) fn emit_unconditional_internal_branch(
        &mut self,
    ) -> Result<P6Arm64PendingInternalBranch, P6X86_64BaselineSemanticByteEmissionError> {
        let kind = Arm64DirectBranchKind::Unconditional;
        let branch_offset = self.emit_word(kind.placeholder_word())?;
        let branch_end_offset = self.offset()?;
        Ok(P6Arm64PendingInternalBranch {
            kind,
            branch_offset,
            branch_end_offset,
        })
    }

    fn emit_conditional_internal_branch(
        &mut self,
        condition: Arm64Condition,
    ) -> Result<P6Arm64PendingInternalBranch, P6X86_64BaselineSemanticByteEmissionError> {
        let kind = Arm64DirectBranchKind::Conditional { condition };
        let branch_offset = self.emit_word(kind.placeholder_word())?;
        let branch_end_offset = self.offset()?;
        Ok(P6Arm64PendingInternalBranch {
            kind,
            branch_offset,
            branch_end_offset,
        })
    }

    fn patch_internal_branch_to_current(
        &mut self,
        branch: P6Arm64PendingInternalBranch,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        let target_offset = self.offset()?;
        self.patch_internal_branch_to_target(branch, target_offset)
    }

    pub(super) fn patch_internal_branch_to_target(
        &mut self,
        branch: P6Arm64PendingInternalBranch,
        target_offset: u32,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        let linked = Arm64DirectBranchPatch {
            kind: branch.kind,
            source_instruction_offset: branch.branch_offset,
        }
        .link_to(target_offset)
        .map_err(|error| {
            p6_arm64_direct_branch_link_error(
                error,
                branch.branch_offset,
                branch.branch_end_offset,
                target_offset,
            )
        })?;
        self.patch_direct_branch_word(branch.branch_offset, linked.instruction_word)
    }

    fn emit_direct_bytecode_branch(
        &mut self,
        source: P6X86_64BaselineControlFlowBranchContract,
        kind: Arm64DirectBranchKind,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        let source_instruction_offset = self.emit_word(kind.placeholder_word())?;
        let branch_end_offset = self.offset()?;
        self.pending_bytecode_branches
            .push(P6Arm64PendingBytecodeBranch {
                source,
                kind,
                source_instruction_offset,
                branch_end_offset,
            });
        Ok(())
    }

    pub(super) fn emit_retained_side_exit_direct_branch(
        &mut self,
        bytecode_index: BytecodeIndex,
        label: P6X86_64BaselineSideExitLabel,
        kind: Arm64DirectBranchKind,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        validate_p6_arm64_semantic_side_exit_label(bytecode_index, label)?;
        let branch_offset = self.emit_word(kind.placeholder_word())?;
        let branch_end_offset = self.offset()?;
        self.pending_side_exits.push(P6Arm64PendingSideExitBranch {
            label,
            kind,
            branch_offset,
            branch_end_offset,
        });
        Ok(())
    }

    pub(super) fn emit_branch_if_false_primitive(
        &mut self,
        bytecode_index: BytecodeIndex,
        source: P6X86_64BaselineOperandLocation,
        value_layout: P6X86_64BaselineValueLayoutContract,
        unsupported_exit: P6X86_64BaselineSideExitLabel,
        target: P6X86_64BaselineControlFlowBranchContract,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        validate_p6_x86_64_semantic_value_layout(value_layout)?;
        let machine_instruction = P6X86_64BaselineMachineInstruction::BranchIfFalsePrimitive {
            value: P6X86_64BaselineSymbolicRegister::Scratch0,
            undefined_tag: value_layout.immediate_undefined_tag,
            null_tag: value_layout.immediate_null_tag,
            false_tag: value_layout.immediate_false_tag,
            true_tag: value_layout.immediate_true_tag,
            int32_tag: value_layout.immediate_int32_tag,
            unsupported_exit,
            target,
        };
        if target.kind != P6X86_64BaselineBytecodeBranchKind::JumpIfFalseTaken
            || target.source_bytecode_index != bytecode_index
        {
            return Err(
                P6X86_64BaselineSemanticByteEmissionError::UnsupportedMachineInstruction {
                    bytecode_index,
                    instruction: machine_instruction,
                },
            );
        }

        let undefined_tag =
            p6_arm64_semantic_u8_tag(bytecode_index, value_layout.immediate_undefined_tag)?;
        let null_tag = p6_arm64_semantic_u8_tag(bytecode_index, value_layout.immediate_null_tag)?;
        let false_tag = p6_arm64_semantic_u8_tag(bytecode_index, value_layout.immediate_false_tag)?;
        let true_tag = p6_arm64_semantic_u8_tag(bytecode_index, value_layout.immediate_true_tag)?;
        let int32_tag = p6_arm64_semantic_u8_tag(bytecode_index, value_layout.immediate_int32_tag)?;

        p6_arm64_emit_ldr_x9_from_frame_location(&mut self.bytes, bytecode_index, source)?;
        self.emit_word(p6_arm64_encode_and_x10_x9_low_byte_tag_mask())?;

        // C++ JSC `emit_op_jfalse` first handles JSValue64 booleans/int32/Other
        // with `branchIfNotBoolean`, `branchIfNotInt32`, and `branchIfOther`,
        // then calls `valueIsFalseyGenerator` for full truthiness. This dormant
        // Rust ARM64 slice is intentionally narrower: Rust values currently use
        // a low-byte tag + payload-shift layout, so it masks the low byte and
        // only recognizes false, null, undefined, int32(0), true, and nonzero
        // int32. Double/cell/unknown tags branch to the retained side-exit
        // payload stub instead of claiming C++ `valueIsFalsey`/ToBoolean parity.
        self.emit_word(p6_arm64_encode_cmp_imm64(
            register_contract::X10,
            undefined_tag,
        ))?;
        self.emit_conditional_bytecode_branch(target, Arm64Condition::Eq)?;
        self.emit_word(p6_arm64_encode_cmp_imm64(register_contract::X10, null_tag))?;
        self.emit_conditional_bytecode_branch(target, Arm64Condition::Eq)?;
        self.emit_word(p6_arm64_encode_cmp_imm64(register_contract::X10, false_tag))?;
        self.emit_conditional_bytecode_branch(target, Arm64Condition::Eq)?;
        self.emit_word(p6_arm64_encode_cmp_imm64(register_contract::X10, true_tag))?;
        let true_fallthrough = self.emit_conditional_internal_branch(Arm64Condition::Eq)?;
        self.emit_word(p6_arm64_encode_cmp_imm64(register_contract::X10, int32_tag))?;
        self.emit_retained_side_exit_direct_branch(
            bytecode_index,
            unsupported_exit,
            Arm64DirectBranchKind::Conditional {
                condition: Arm64Condition::Ne,
            },
        )?;
        self.emit_word(p6_arm64_encode_cmp_imm64(register_contract::X9, int32_tag))?;
        self.emit_conditional_bytecode_branch(target, Arm64Condition::Eq)?;
        self.patch_internal_branch_to_current(true_fallthrough)
    }

    pub(super) fn finish_bytecode_branches(
        &mut self,
        instruction_bytes: &[P6X86_64BaselineInstructionByteRecord],
    ) -> Result<Vec<P6X86_64BaselineBytecodeBranchRecord>, P6X86_64BaselineSemanticByteEmissionError>
    {
        let pending = self.pending_bytecode_branches.clone();
        let mut records = Vec::with_capacity(pending.len());
        for branch in pending {
            let target = instruction_bytes
                .iter()
                .find(|record| record.bytecode_index == branch.source.target_bytecode_index)
                .ok_or(
                    P6X86_64BaselineSemanticByteEmissionError::BranchTargetMissing {
                        bytecode_index: branch.source.source_bytecode_index,
                        target: branch.source.target_bytecode_index,
                    },
                )?;
            let target_offset = target.start_offset;
            let linked = Arm64DirectBranchPatch {
                kind: branch.kind,
                source_instruction_offset: branch.source_instruction_offset,
            }
            .link_to(target_offset)
            .map_err(|error| {
                p6_arm64_direct_branch_link_error(
                    error,
                    branch.source_instruction_offset,
                    branch.branch_end_offset,
                    target_offset,
                )
            })?;
            self.patch_direct_branch_word(
                branch.source_instruction_offset,
                linked.instruction_word,
            )?;
            records.push(P6X86_64BaselineBytecodeBranchRecord {
                bytecode_index: branch.source.source_bytecode_index,
                kind: branch.source.kind,
                branch_offset: branch.source_instruction_offset,
                // Shared P6 branch records are still x86-named. For ARM64 this
                // field stores the branch instruction word patch offset until a
                // cross-backend branch-patch record replaces `rel32_offset`.
                rel32_offset: branch.source_instruction_offset,
                branch_end_offset: branch.branch_end_offset,
                target_bytecode_index: branch.source.target_bytecode_index,
                target_offset,
            });
        }
        self.pending_bytecode_branches.clear();
        Ok(records)
    }

    pub(super) fn finish_side_exit_return_stubs(
        &mut self,
    ) -> Result<
        Vec<P6X86_64BaselineSideExitReturnStubRecord>,
        P6X86_64BaselineSemanticByteEmissionError,
    > {
        let pending = self.pending_side_exits.clone();
        let mut records = Vec::with_capacity(pending.len());
        for (side_exit_index, branch) in pending.into_iter().enumerate() {
            let side_exit_index = p6_arm64_checked_side_exit_index(side_exit_index)?;
            let encoded_payload = P6X86_64BaselineSideExitReturnPayload::encode(side_exit_index);
            let stub_bytes =
                p6_arm64_callable_side_exit_payload_return_stub_bytes(encoded_payload)?;
            let target_offset = self.offset()?;
            self.emit_bytes(&stub_bytes)?;
            let stub_end_offset = self.offset()?;
            let linked = Arm64DirectBranchPatch {
                kind: branch.kind,
                source_instruction_offset: branch.branch_offset,
            }
            .link_to(target_offset)
            .map_err(|error| {
                p6_arm64_direct_branch_link_error(
                    error,
                    branch.branch_offset,
                    branch.branch_end_offset,
                    target_offset,
                )
            })?;
            self.patch_direct_branch_word(branch.branch_offset, linked.instruction_word)?;
            records.push(P6X86_64BaselineSideExitReturnStubRecord {
                bytecode_index: branch.label.retained_bytecode_index,
                reason: branch.label.reason,
                side_exit_index,
                branch_offset: branch.branch_offset,
                branch_end_offset: branch.branch_end_offset,
                target_offset,
                stub_end_offset,
                byte_len: stub_end_offset.checked_sub(target_offset).ok_or(
                    P6X86_64BaselineSemanticByteEmissionError::ImageLengthExceedsU32 {
                        actual: self.bytes.len(),
                    },
                )?,
                // C++ `jfalse` fallback branches to `valueIsFalsey`/slow path.
                // This dormant ARM64 native-entry bridge only returns retained
                // payloads for the current Rust fallback ABI; reentry offsets stay
                // empty until executable ARM64 branch bodies are ported.
                resume_bytecode_index: None,
                resume_entry_offset: None,
                encoded_payload,
                bytes: p6_arm64_bytes_for_range(&self.bytes, target_offset, stub_end_offset)?,
            });
        }
        self.pending_side_exits.clear();
        Ok(records)
    }

    fn patch_direct_branch_word(
        &mut self,
        branch_offset: u32,
        instruction_word: u32,
    ) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
        let start = branch_offset as usize;
        let Some(end) = start.checked_add(4) else {
            return Err(
                P6X86_64BaselineSemanticByteEmissionError::BranchPatchOutOfRange { branch_offset },
            );
        };
        let Some(slot) = self.bytes.get_mut(start..end) else {
            return Err(
                P6X86_64BaselineSemanticByteEmissionError::BranchPatchOutOfRange { branch_offset },
            );
        };
        slot.copy_from_slice(&instruction_word.to_le_bytes());
        Ok(())
    }
}

fn validate_p6_arm64_semantic_side_exit_label(
    bytecode_index: BytecodeIndex,
    label: P6X86_64BaselineSideExitLabel,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    if label.retained_bytecode_index != bytecode_index
        || label.destination != P6X86_64BaselineSideExitDestinationEffect::DestinationUnchanged
        || label.may_throw
        || label.runtime_call
        || label.heap_allocation
        || label.touches_gc_roots
    {
        return Err(
            P6X86_64BaselineSemanticByteEmissionError::UnexpectedSideExitEffects {
                bytecode_index,
                reason: label.reason,
            },
        );
    }
    Ok(())
}

fn p6_arm64_checked_side_exit_index(
    side_exit_index: usize,
) -> Result<u32, P6X86_64BaselineSemanticByteEmissionError> {
    if side_exit_index <= u32::MAX as usize {
        Ok(side_exit_index as u32)
    } else {
        Err(
            P6X86_64BaselineSemanticByteEmissionError::SideExitIndexExceedsPayloadCapacity {
                side_exit_index,
            },
        )
    }
}

fn p6_arm64_direct_branch_link_error(
    error: Arm64DirectBranchLinkError,
    branch_offset: u32,
    branch_end_offset: u32,
    target_offset: u32,
) -> P6X86_64BaselineSemanticByteEmissionError {
    match error {
        Arm64DirectBranchLinkError::UnalignedOffset { offset, .. } => {
            P6X86_64BaselineSemanticByteEmissionError::BranchPatchOutOfRange {
                branch_offset: offset,
            }
        }
        Arm64DirectBranchLinkError::OutOfRange { .. } => {
            P6X86_64BaselineSemanticByteEmissionError::BranchDisplacementOutOfRange {
                branch_offset,
                branch_end_offset,
                target_offset,
            }
        }
    }
}

fn p6_arm64_semantic_u8_tag(
    bytecode_index: BytecodeIndex,
    tag: u64,
) -> Result<u8, P6X86_64BaselineSemanticByteEmissionError> {
    if tag <= u64::from(u8::MAX) {
        Ok(tag as u8)
    } else {
        Err(
            P6X86_64BaselineSemanticByteEmissionError::UnsupportedImmediateTag {
                bytecode_index,
                tag,
            },
        )
    }
}

fn p6_arm64_emit_ldr_x9_from_frame_location(
    bytes: &mut Vec<u8>,
    bytecode_index: BytecodeIndex,
    location: P6X86_64BaselineOperandLocation,
) -> Result<(), P6X86_64BaselineSemanticByteEmissionError> {
    p6_arm64_emit_ldr_xd_from_frame_location(bytes, bytecode_index, location, register_contract::X9)
}

pub(super) fn p6_arm64_encode_and_x10_x9_low_byte_tag_mask() -> u32 {
    // ARM64Assembler::and_<64>(x10, x9, LogicalImmediate::create64(0xff)).
    p6_arm64_encode_and_imm64(register_contract::X10, register_contract::X9, 0x1007)
}

fn p6_arm64_encode_and_imm64(
    dest: register_contract::Arm64Gpr,
    source: register_contract::Arm64Gpr,
    logical_immediate_13: u16,
) -> u32 {
    0x9200_0000_u32
        | (u32::from(logical_immediate_13) << 10)
        | (u32::from(source.index) << 5)
        | u32::from(dest.index)
}

pub(super) fn p6_arm64_encode_cmp_imm64(source: register_contract::Arm64Gpr, imm12: u8) -> u32 {
    // ARM64Assembler::cmp<64>(rn, UInt12(imm)) lowers to subs xzr, rn, #imm.
    0xf100_001f_u32 | (u32::from(imm12) << 10) | (u32::from(source.index) << 5)
}
