use crate::bytecode::{BytecodeIndex, Checkpoint, CodeBlock, CoreOpcode, DecodedInstruction};
use crate::interpreter::SingleDispatchOutcome;
use crate::jit::{
    P6BaselineNativeReentryTargetRecord, P6X86_64BaselineSelectedSideExitReason,
    P6X86_64BaselineSideExitReturnPayload,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct P6X86_64CallableSideExitReturnSite {
    pub(super) bytecode_index: BytecodeIndex,
    pub(super) reason: P6X86_64BaselineSelectedSideExitReason,
    pub(super) side_exit_index: u32,
    pub(super) resume_bytecode_index: Option<BytecodeIndex>,
    pub(super) resume_entry_offset: Option<u32>,
    // C++ JSC publishes bytecode->native labels through
    // `fastPathResumePoint`/`JITCodeMapBuilder`. This retained VM table mirrors
    // that metadata for P6 side exits while backend/rooting-specific code
    // decides whether native reentry is callable.
    pub(super) native_reentry_targets: Vec<P6BaselineNativeReentryTargetRecord>,
    pub(super) encoded_payload: P6X86_64BaselineSideExitReturnPayload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct P6CallableSideExitNativeReentryInvocation {
    // C++ JSC maps bytecode labels to native PCs through
    // `fastPathResumePoint`/`JITCodeMapBuilder`. This carries the resolved
    // allocation-relative native-PC metadata after a P6 side-exit fallback; it
    // is not authority for ARM64 or any other backend to execute native reentry.
    pub(super) entry_offset: u32,
}

pub(super) fn p6_side_exit_native_reentry_target_for_single_dispatch_outcome(
    side_exit: &P6X86_64CallableSideExitReturnSite,
    opcode: Option<CoreOpcode>,
    outcome: &SingleDispatchOutcome,
) -> Option<P6BaselineNativeReentryTargetRecord> {
    if let SingleDispatchOutcome::Continue(Some(resume_bytecode_index)) = outcome {
        if side_exit.resume_bytecode_index == Some(*resume_bytecode_index) {
            return Some(P6BaselineNativeReentryTargetRecord {
                resume_bytecode_index: *resume_bytecode_index,
                resume_entry_offset: side_exit.resume_entry_offset?,
            });
        }
    }

    if side_exit.reason != P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand
        || opcode != Some(CoreOpcode::JumpIfFalse)
    {
        return None;
    }

    // C++ `emit_op_jfalse` branches from `valueIsFalsey` to either the taken
    // target or the fallthrough `fastPathResumePoint`. Rust keeps this
    // multi-target metadata behind explicit JumpIfFalse/truthiness matching;
    // public ARM64 admission still requires a backend ABI/rooting proof before
    // this resolved label can authorize native reentry.
    let resume_bytecode_index = match outcome {
        SingleDispatchOutcome::Jump(target) | SingleDispatchOutcome::Continue(Some(target)) => {
            *target
        }
        _ => return None,
    };
    side_exit
        .native_reentry_targets
        .iter()
        .copied()
        .find(|target| target.resume_bytecode_index == resume_bytecode_index)
}

pub(super) fn p6_jump_if_false_truthiness_side_exit_single_dispatch_native_resume_allowed(
    code_block: &CodeBlock,
    side_exit: &P6X86_64CallableSideExitReturnSite,
) -> bool {
    // C++ `jfalse` can resume from `valueIsFalsey` at either the taken target or
    // the next bytecode. Rust admits x86 private native reentry only when the
    // retained side-exit metadata published both labels and left the legacy
    // single-target fields empty.
    if side_exit.resume_bytecode_index.is_some()
        || side_exit.resume_entry_offset.is_some()
        || side_exit.native_reentry_targets.len() != 2
    {
        return false;
    }
    let Ok(instruction) = code_block.decoded_instruction_at(side_exit.bytecode_index) else {
        return false;
    };
    if !p6_jump_if_false_truthiness_side_exit_site_allowed_with_instruction(
        code_block,
        side_exit,
        &instruction,
    ) {
        return false;
    }
    let Ok(taken_target) = instruction.bytecode_index_operand(1) else {
        return false;
    };
    let Some(fallthrough_target) =
        p6_next_decoded_bytecode_index_after(code_block, side_exit.bytecode_index)
    else {
        return false;
    };
    if taken_target == fallthrough_target
        || code_block
            .decoded_instruction_at(fallthrough_target)
            .is_err()
    {
        return false;
    }
    [taken_target, fallthrough_target]
        .into_iter()
        .all(|resume_bytecode_index| {
            side_exit
                .native_reentry_targets
                .iter()
                .any(|target| target.resume_bytecode_index == resume_bytecode_index)
        })
}

fn p6_jump_if_false_truthiness_side_exit_site_allowed_with_instruction(
    code_block: &CodeBlock,
    site: &P6X86_64CallableSideExitReturnSite,
    instruction: &DecodedInstruction<'_>,
) -> bool {
    if !site.bytecode_index.is_valid()
        || site.bytecode_index.checkpoint() != Checkpoint::NONE
        || instruction.bytecode_index != site.bytecode_index
        || site.reason != P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand
        || CoreOpcode::from_opcode(instruction.opcode) != Some(CoreOpcode::JumpIfFalse)
        || instruction.register_operand(0).is_err()
    {
        return false;
    }
    let Ok(target) = instruction.bytecode_index_operand(1) else {
        return false;
    };
    target.is_valid()
        && target.checkpoint() == Checkpoint::NONE
        && target != site.bytecode_index
        && code_block.decoded_instruction_at(target).is_ok()
}

fn p6_next_decoded_bytecode_index_after(
    code_block: &CodeBlock,
    bytecode_index: BytecodeIndex,
) -> Option<BytecodeIndex> {
    let mut matched = false;
    for instruction in code_block.unlinked().instructions().decoded_instructions() {
        let instruction = instruction.ok()?;
        if matched {
            return Some(instruction.bytecode_index);
        }
        if instruction.bytecode_index == bytecode_index {
            matched = true;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{
        CodeBlockEntrypoints, CodeBlockLifecycleState, CodeKind, InterpreterEntrySlot, LinkContext,
        Operand, OperandWidth, PackedInstructionStream, RegisterFrameShape, TypedInstruction,
        UnlinkedCodeBlock, VirtualRegister,
    };

    fn bci(offset: u32) -> BytecodeIndex {
        BytecodeIndex::from_offset(offset)
    }

    fn record(
        resume_bytecode_index: BytecodeIndex,
        resume_entry_offset: u32,
    ) -> P6BaselineNativeReentryTargetRecord {
        P6BaselineNativeReentryTargetRecord {
            resume_bytecode_index,
            resume_entry_offset,
        }
    }

    fn side_exit(
        reason: P6X86_64BaselineSelectedSideExitReason,
        resume_bytecode_index: Option<BytecodeIndex>,
        resume_entry_offset: Option<u32>,
        native_reentry_targets: Vec<P6BaselineNativeReentryTargetRecord>,
    ) -> P6X86_64CallableSideExitReturnSite {
        P6X86_64CallableSideExitReturnSite {
            bytecode_index: bci(1),
            reason,
            side_exit_index: 0,
            resume_bytecode_index,
            resume_entry_offset,
            native_reentry_targets,
            encoded_payload: P6X86_64BaselineSideExitReturnPayload::encode(0),
        }
    }

    fn local(index: u32) -> VirtualRegister {
        VirtualRegister::local(index)
    }

    fn typed_core_instruction_with_operands(
        offset: u32,
        opcode: CoreOpcode,
        operands: Vec<Operand>,
    ) -> TypedInstruction {
        TypedInstruction {
            opcode: opcode.opcode(),
            width: OperandWidth::Narrow,
            operands,
            schema: None,
            bytecode_index: Some(bci(offset)),
        }
    }

    fn jump_if_false_code_block(taken_target: u32) -> CodeBlock {
        CodeBlock::from_unlinked(
            UnlinkedCodeBlock::new(
                CodeKind::Program,
                PackedInstructionStream::from_typed_placeholder(vec![
                    typed_core_instruction_with_operands(
                        0,
                        CoreOpcode::LoadBool,
                        vec![Operand::Register(local(0)), Operand::UnsignedImmediate(0)],
                    ),
                    typed_core_instruction_with_operands(
                        1,
                        CoreOpcode::JumpIfFalse,
                        vec![
                            Operand::Register(local(0)),
                            Operand::BytecodeIndex(bci(taken_target)),
                        ],
                    ),
                    typed_core_instruction_with_operands(
                        2,
                        CoreOpcode::Return,
                        vec![Operand::Register(local(0))],
                    ),
                    typed_core_instruction_with_operands(
                        3,
                        CoreOpcode::LoadBool,
                        vec![Operand::Register(local(0)), Operand::UnsignedImmediate(1)],
                    ),
                    typed_core_instruction_with_operands(
                        4,
                        CoreOpcode::Return,
                        vec![Operand::Register(local(0))],
                    ),
                ]),
            )
            .with_frame(RegisterFrameShape {
                num_parameters_including_this: 1,
                num_vars: 1,
                num_callee_locals: 0,
                num_temporaries: 0,
                special: Default::default(),
            }),
            LinkContext::default(),
        )
        .with_entrypoints(CodeBlockEntrypoints {
            interpreter: Some(InterpreterEntrySlot(0)),
            ..CodeBlockEntrypoints::default()
        })
        .with_lifecycle(CodeBlockLifecycleState::LinkedInterpreter)
    }

    #[test]
    fn resolves_x86_single_target_on_matching_continue() {
        let site = side_exit(
            P6X86_64BaselineSelectedSideExitReason::UnsupportedToNumberOperand,
            Some(bci(7)),
            Some(88),
            Vec::new(),
        );

        assert_eq!(
            p6_side_exit_native_reentry_target_for_single_dispatch_outcome(
                &site,
                Some(CoreOpcode::ToNumber),
                &SingleDispatchOutcome::Continue(Some(bci(7))),
            ),
            Some(record(bci(7), 88))
        );
    }

    #[test]
    fn nonmatching_single_target_outcomes_return_no_reentry() {
        let site = side_exit(
            P6X86_64BaselineSelectedSideExitReason::UnsupportedToNumberOperand,
            Some(bci(7)),
            Some(88),
            Vec::new(),
        );

        assert_eq!(
            p6_side_exit_native_reentry_target_for_single_dispatch_outcome(
                &site,
                Some(CoreOpcode::ToNumber),
                &SingleDispatchOutcome::Continue(Some(bci(8))),
            ),
            None
        );
        assert_eq!(
            p6_side_exit_native_reentry_target_for_single_dispatch_outcome(
                &site,
                Some(CoreOpcode::ToNumber),
                &SingleDispatchOutcome::Jump(bci(7)),
            ),
            None
        );
        assert_eq!(
            p6_side_exit_native_reentry_target_for_single_dispatch_outcome(
                &site,
                Some(CoreOpcode::ToNumber),
                &SingleDispatchOutcome::Continue(None),
            ),
            None
        );
    }

    #[test]
    fn jump_if_false_jump_resolves_taken_target() {
        let site = side_exit(
            P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
            None,
            None,
            vec![record(bci(4), 44), record(bci(2), 22)],
        );

        assert_eq!(
            p6_side_exit_native_reentry_target_for_single_dispatch_outcome(
                &site,
                Some(CoreOpcode::JumpIfFalse),
                &SingleDispatchOutcome::Jump(bci(4)),
            ),
            Some(record(bci(4), 44))
        );
    }

    #[test]
    fn jump_if_false_continue_resolves_fallthrough_target() {
        let site = side_exit(
            P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
            None,
            None,
            vec![record(bci(4), 44), record(bci(2), 22)],
        );

        assert_eq!(
            p6_side_exit_native_reentry_target_for_single_dispatch_outcome(
                &site,
                Some(CoreOpcode::JumpIfFalse),
                &SingleDispatchOutcome::Continue(Some(bci(2))),
            ),
            Some(record(bci(2), 22))
        );
    }

    #[test]
    fn wrong_reason_or_opcode_does_not_use_multi_target_records() {
        let targets = vec![record(bci(4), 44), record(bci(2), 22)];
        let wrong_reason = side_exit(
            P6X86_64BaselineSelectedSideExitReason::NonInt32Operand,
            None,
            None,
            targets.clone(),
        );
        let wrong_opcode = side_exit(
            P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
            None,
            None,
            targets,
        );

        assert_eq!(
            p6_side_exit_native_reentry_target_for_single_dispatch_outcome(
                &wrong_reason,
                Some(CoreOpcode::JumpIfFalse),
                &SingleDispatchOutcome::Jump(bci(4)),
            ),
            None
        );
        assert_eq!(
            p6_side_exit_native_reentry_target_for_single_dispatch_outcome(
                &wrong_opcode,
                Some(CoreOpcode::AddInt32),
                &SingleDispatchOutcome::Continue(Some(bci(2))),
            ),
            None
        );
    }

    #[test]
    fn jump_if_false_truthiness_native_resume_admission_requires_exact_two_labels() {
        let code_block = jump_if_false_code_block(4);
        let valid = side_exit(
            P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
            None,
            None,
            vec![record(bci(4), 44), record(bci(2), 22)],
        );
        assert!(
            p6_jump_if_false_truthiness_side_exit_single_dispatch_native_resume_allowed(
                &code_block,
                &valid
            )
        );

        let mut legacy_single_target_shape = valid.clone();
        legacy_single_target_shape.resume_bytecode_index = Some(bci(2));
        legacy_single_target_shape.resume_entry_offset = Some(22);
        assert!(
            !p6_jump_if_false_truthiness_side_exit_single_dispatch_native_resume_allowed(
                &code_block,
                &legacy_single_target_shape
            )
        );

        let missing_fallthrough = side_exit(
            P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
            None,
            None,
            vec![record(bci(4), 44), record(bci(8), 88)],
        );
        assert!(
            !p6_jump_if_false_truthiness_side_exit_single_dispatch_native_resume_allowed(
                &code_block,
                &missing_fallthrough
            )
        );

        let wrong_reason = side_exit(
            P6X86_64BaselineSelectedSideExitReason::NonInt32Operand,
            None,
            None,
            vec![record(bci(4), 44), record(bci(2), 22)],
        );
        assert!(
            !p6_jump_if_false_truthiness_side_exit_single_dispatch_native_resume_allowed(
                &code_block,
                &wrong_reason
            )
        );

        let degenerate_target_code_block = jump_if_false_code_block(2);
        let duplicate_target = side_exit(
            P6X86_64BaselineSelectedSideExitReason::UnsupportedTruthinessOperand,
            None,
            None,
            vec![record(bci(2), 22), record(bci(2), 33)],
        );
        assert!(
            !p6_jump_if_false_truthiness_side_exit_single_dispatch_native_resume_allowed(
                &degenerate_target_code_block,
                &duplicate_target
            )
        );
    }
}
