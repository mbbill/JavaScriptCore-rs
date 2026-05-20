use crate::bytecode::code_block::{BytecodeIndex, CodeKind};
use crate::interpreter::{InterpreterFrameKind, InterpreterFrameRecord};
use crate::jit::JitType;
use crate::runtime::CodeBlockId;

use crate::llint::entrypoint::{
    select_llint_entrypoint_kinds, LLIntEntrypoint, LLIntEntrypointKind, LLIntEntrypointTable,
    LLIntEntrypointValidationError,
};

/// Data passed when an interpreter-owned frame enters or re-enters LLInt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LLIntInterpreterEntryHandoff {
    pub frame_kind: InterpreterFrameKind,
    pub code_block: CodeBlockId,
    pub bytecode_index: Option<BytecodeIndex>,
    pub code_kind: CodeKind,
    pub selected_entrypoint: LLIntEntrypoint,
    pub source_tier: JitType,
    pub reason: LLIntInterpreterHandoffReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LLIntInterpreterHandoffReason {
    InitialInterpreterEntry,
    SlowPathResume,
    TierFallback,
    InlineCacheMiss,
    ExceptionResume,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LLIntInterpreterHandoffError {
    MissingFrameKind,
    UnsupportedFrameKind(InterpreterFrameKind),
    MissingCodeBlock,
    MissingEntrypoint(LLIntEntrypointKind),
    EntrypointInvalid(LLIntEntrypointValidationError),
}

impl LLIntInterpreterEntryHandoff {
    pub fn validate(&self) -> Result<(), LLIntInterpreterHandoffError> {
        if !matches!(self.frame_kind, InterpreterFrameKind::JavaScript) {
            return Err(LLIntInterpreterHandoffError::UnsupportedFrameKind(
                self.frame_kind,
            ));
        }
        if self.selected_entrypoint.code.is_none() {
            return Err(LLIntInterpreterHandoffError::EntrypointInvalid(
                LLIntEntrypointValidationError::InstalledWithoutCode,
            ));
        }
        Ok(())
    }
}

pub fn select_llint_entry_for_interpreter_frame(
    frame: &InterpreterFrameRecord,
    code_kind: CodeKind,
    construct_entry: bool,
    available: &LLIntEntrypointTable,
    reason: LLIntInterpreterHandoffReason,
) -> Result<LLIntInterpreterEntryHandoff, LLIntInterpreterHandoffError> {
    let frame_kind = frame
        .kind
        .ok_or(LLIntInterpreterHandoffError::MissingFrameKind)?;
    if !matches!(frame_kind, InterpreterFrameKind::JavaScript) {
        return Err(LLIntInterpreterHandoffError::UnsupportedFrameKind(
            frame_kind,
        ));
    }
    let code_block = frame
        .code_block
        .ok_or(LLIntInterpreterHandoffError::MissingCodeBlock)?;
    let required = select_llint_entrypoint_kinds(code_kind, construct_entry);
    let selected_kind = if construct_entry {
        LLIntEntrypointKind::FunctionForConstruct
    } else {
        required[0]
    };
    let selected_entrypoint = entrypoint_for_kind(available, selected_kind).ok_or(
        LLIntInterpreterHandoffError::MissingEntrypoint(selected_kind),
    )?;
    let handoff = LLIntInterpreterEntryHandoff {
        frame_kind,
        code_block,
        bytecode_index: frame.bytecode_index,
        code_kind,
        selected_entrypoint,
        source_tier: JitType::None,
        reason,
    };
    handoff.validate()?;
    Ok(handoff)
}

fn entrypoint_for_kind(
    table: &LLIntEntrypointTable,
    kind: LLIntEntrypointKind,
) -> Option<LLIntEntrypoint> {
    [
        table.call,
        table.construct,
        table.arity_check_call,
        table.arity_check_construct,
    ]
    .into_iter()
    .flatten()
    .find(|entrypoint| entrypoint.kind == kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::code_block::InterpreterEntrySlot;
    use crate::gc::CellId;
    use crate::llint::LLIntCodePtr;

    #[test]
    fn interpreter_frame_selects_eval_entrypoint() {
        let code_block = CodeBlockId(CellId(11));
        let frame = InterpreterFrameRecord {
            kind: Some(InterpreterFrameKind::JavaScript),
            code_block: Some(code_block),
            bytecode_index: Some(BytecodeIndex::from_offset(4)),
            ..InterpreterFrameRecord::default()
        };
        let table = LLIntEntrypointTable::from_entrypoints([LLIntEntrypoint {
            kind: LLIntEntrypointKind::Eval,
            slot: InterpreterEntrySlot(2),
            code: Some(LLIntCodePtr(0x20)),
            frame_register_count: Some(8),
        }]);

        let handoff = select_llint_entry_for_interpreter_frame(
            &frame,
            CodeKind::Eval,
            false,
            &table,
            LLIntInterpreterHandoffReason::InitialInterpreterEntry,
        )
        .unwrap();

        assert_eq!(handoff.code_block, code_block);
        assert_eq!(handoff.selected_entrypoint.kind, LLIntEntrypointKind::Eval);
        assert_eq!(handoff.bytecode_index, Some(BytecodeIndex::from_offset(4)));
    }

    #[test]
    fn native_frame_cannot_select_llint_entrypoint() {
        let frame = InterpreterFrameRecord {
            kind: Some(InterpreterFrameKind::Native),
            ..InterpreterFrameRecord::default()
        };

        assert_eq!(
            select_llint_entry_for_interpreter_frame(
                &frame,
                CodeKind::Function,
                false,
                &LLIntEntrypointTable::default(),
                LLIntInterpreterHandoffReason::InitialInterpreterEntry
            ),
            Err(LLIntInterpreterHandoffError::UnsupportedFrameKind(
                InterpreterFrameKind::Native
            ))
        );
    }
}
