use crate::bytecode::{BytecodeIndex, CoreOpcode};
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
    // target or the fallthrough `fastPathResumePoint`. Rust keeps this dormant
    // multi-target metadata behind explicit JumpIfFalse/truthiness matching so
    // ARM64 retained side exits remain fallback-only until a native reentry ABI
    // and rooting bridge are admitted.
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
