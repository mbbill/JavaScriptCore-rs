//! RegExp execution boundary records.
//!
//! These records describe how runtime code enters and leaves a compiled RegExp
//! program, and provide the executable entry that runs the Yarr bytecode
//! interpreter over a subject (mirroring C++ `Yarr::interpret`,
//! YarrInterpreter.cpp:3221) and lifts its raw output vector into a `MatchResult`.

use crate::gc::{RootKind, RootSetMutationAuthority};
use crate::runtime::{ObjectId, RegExpMatchMode, RegExpProgramId};
use crate::yarr::{
    describe_match_result_semantics, describe_match_state_semantics, interpret_bytecode,
    BytecodePattern, BytecodePatternId, JSRegExpResult, MatchInput, MatchRange, MatchResult,
    MatchResultSemanticDescriptor, MatchSemanticError, MatchStateSemanticDescriptor, MatchStatus,
    YarrMatchContext, YarrPatternId, YARR_OFFSET_NO_MATCH,
};

/// Storage backing selected for a RegExp program invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegExpProgramBody {
    ParsedPattern(YarrPatternId),
    Bytecode(BytecodePatternId),
    Jit(BytecodePatternId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegExpRootBoundaryKind {
    RegExpObject,
    InputString,
    MatchResultArray,
    NamedCaptureGroups,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegExpRootBoundaryRecord {
    pub kind: RegExpRootBoundaryKind,
    pub root_kind: RootKind,
    pub mutation_authority: RootSetMutationAuthority,
    pub object: Option<ObjectId>,
    pub precise: bool,
}

/// Runtime entry record for invoking an already-compiled RegExp program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegExpProgramInvocationRecord {
    pub program: RegExpProgramId,
    pub regexp_object: Option<ObjectId>,
    pub body: Option<RegExpProgramBody>,
    pub mode: RegExpMatchMode,
    pub context: YarrMatchContext,
    pub expected_capture_slots: usize,
    pub match_only: bool,
    pub root_boundaries: Vec<RegExpRootBoundaryRecord>,
}

/// Runtime exit record for a RegExp program invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegExpProgramResultRecord {
    pub program: RegExpProgramId,
    pub input: MatchInput,
    pub result: MatchResult,
    pub used_jit: bool,
    pub fell_back_to_interpreter: bool,
    pub remaining_match_limit: u32,
    pub has_indices: bool,
}

/// Boundary validation error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegExpExecutionBoundaryError {
    MatchState(MatchSemanticError),
    MatchResult(MatchSemanticError),
    ProgramMismatch {
        expected: RegExpProgramId,
        actual: RegExpProgramId,
    },
    InputMismatch {
        expected: MatchInput,
        actual: MatchInput,
    },
    CaptureSlotCountMismatch {
        expected: usize,
        actual: usize,
    },
    MatchOnlyResultHasCaptures,
    JitFallbackWithoutJit,
    RootBoundaryAuthorityMismatch {
        kind: RegExpRootBoundaryKind,
        root_kind: RootKind,
        authority: RootSetMutationAuthority,
    },
}

/// Descriptor consumed by a future VM entry path before invoking a matcher.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegExpProgramInvocationDescriptor {
    pub program: RegExpProgramId,
    pub regexp_object: Option<ObjectId>,
    pub body: Option<RegExpProgramBody>,
    pub mode: RegExpMatchMode,
    pub state: MatchStateSemanticDescriptor,
    pub expected_capture_slots: usize,
    pub match_only: bool,
    pub can_enter_jit: bool,
    pub requires_vm_regexp_execution_mark: bool,
    pub root_boundary_count: usize,
}

/// Descriptor consumed by a future VM exit path after a matcher returns.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegExpProgramResultDescriptor {
    pub program: RegExpProgramId,
    pub input: MatchInput,
    pub result: MatchResultSemanticDescriptor,
    pub used_jit: bool,
    pub fell_back_to_interpreter: bool,
    pub remaining_match_limit: u32,
}

pub fn describe_regexp_program_invocation(
    record: &RegExpProgramInvocationRecord,
) -> Result<RegExpProgramInvocationDescriptor, RegExpExecutionBoundaryError> {
    let state = describe_match_state_semantics(&record.context)
        .map_err(RegExpExecutionBoundaryError::MatchState)?;
    if state.capture_slot_count != record.expected_capture_slots {
        return Err(RegExpExecutionBoundaryError::CaptureSlotCountMismatch {
            expected: record.expected_capture_slots,
            actual: state.capture_slot_count,
        });
    }
    if record.match_only && state.capture_slot_count != 0 {
        return Err(RegExpExecutionBoundaryError::MatchOnlyResultHasCaptures);
    }
    for boundary in &record.root_boundaries {
        if !regexp_root_boundary_authority_is_valid(boundary.root_kind, boundary.mutation_authority)
        {
            return Err(
                RegExpExecutionBoundaryError::RootBoundaryAuthorityMismatch {
                    kind: boundary.kind,
                    root_kind: boundary.root_kind,
                    authority: boundary.mutation_authority,
                },
            );
        }
    }

    Ok(RegExpProgramInvocationDescriptor {
        program: record.program,
        regexp_object: record.regexp_object,
        body: record.body,
        mode: record.mode,
        expected_capture_slots: record.expected_capture_slots,
        match_only: record.match_only,
        can_enter_jit: state.can_call_jit && matches!(record.body, Some(RegExpProgramBody::Jit(_))),
        requires_vm_regexp_execution_mark: record.context.state.input.from
            == crate::yarr::MatchFrom::VmThread,
        root_boundary_count: record.root_boundaries.len(),
        state,
    })
}

const fn regexp_root_boundary_authority_is_valid(
    root_kind: RootKind,
    authority: RootSetMutationAuthority,
) -> bool {
    matches!(
        (root_kind, authority),
        (
            RootKind::VMRegister,
            RootSetMutationAuthority::VmRegisterFile
        ) | (
            RootKind::ExplicitRoot,
            RootSetMutationAuthority::ExplicitRootRegistry
        ) | (RootKind::Handle, RootSetMutationAuthority::HandleScope)
            | (RootKind::Host, RootSetMutationAuthority::HostIntegration)
    )
}

pub fn describe_regexp_program_result(
    invocation: &RegExpProgramInvocationRecord,
    record: &RegExpProgramResultRecord,
) -> Result<RegExpProgramResultDescriptor, RegExpExecutionBoundaryError> {
    let invocation_descriptor = describe_regexp_program_invocation(invocation)?;
    if invocation.program != record.program {
        return Err(RegExpExecutionBoundaryError::ProgramMismatch {
            expected: invocation.program,
            actual: record.program,
        });
    }
    if invocation_descriptor.state.input != record.input {
        return Err(RegExpExecutionBoundaryError::InputMismatch {
            expected: invocation_descriptor.state.input,
            actual: record.input,
        });
    }
    let result = describe_match_result_semantics(&record.result, record.input, record.has_indices)
        .map_err(RegExpExecutionBoundaryError::MatchResult)?;

    if result.capture_slot_count != invocation.expected_capture_slots {
        return Err(RegExpExecutionBoundaryError::CaptureSlotCountMismatch {
            expected: invocation.expected_capture_slots,
            actual: result.capture_slot_count,
        });
    }
    if invocation.match_only && result.capture_slot_count != 0 {
        return Err(RegExpExecutionBoundaryError::MatchOnlyResultHasCaptures);
    }
    if record.fell_back_to_interpreter && !record.used_jit {
        return Err(RegExpExecutionBoundaryError::JitFallbackWithoutJit);
    }

    Ok(RegExpProgramResultDescriptor {
        program: record.program,
        input: invocation_descriptor.state.input,
        result,
        used_jit: record.used_jit,
        fell_back_to_interpreter: record.fell_back_to_interpreter,
        remaining_match_limit: record.remaining_match_limit,
    })
}

/// Executable RegExp entry: runs the Yarr bytecode interpreter over a UTF-16
/// code-unit subject and lifts the raw output offset vector into a `MatchResult`.
/// Mirrors the C++ exit path that reads `output[0]`/`output[1]` for the overall
/// span and `output[2i]`/`output[2i+1]` for each capture (YarrInterpreter.cpp:
/// 2234-2245; RegExp::matchInline lifts the same vector into capture ranges).
pub fn execute_regexp_bytecode(
    pattern: &BytecodePattern,
    input: &[u16],
    start: u32,
) -> MatchResult {
    let outcome = interpret_bytecode(pattern, input, start);
    let status = match outcome.result {
        JSRegExpResult::Match => MatchStatus::Match,
        JSRegExpResult::NoMatch => MatchStatus::NoMatch,
        JSRegExpResult::HitLimit => MatchStatus::ErrorHitLimit,
    };

    if outcome.result != JSRegExpResult::Match {
        return MatchResult {
            status,
            overall: None,
            captures: Vec::new(),
        };
    }

    let overall = Some(MatchRange {
        start: outcome.output[0],
        end: outcome.output[1],
    });

    // Capture groups 1..=numSubpatterns; an unset begin (offsetNoMatch) means the
    // group did not participate in the match (C++ leaves it as offsetNoMatch).
    let num_subpatterns = pattern.body.subpattern_count;
    let mut captures = Vec::with_capacity(num_subpatterns as usize);
    for i in 1..=num_subpatterns {
        let begin = outcome.output[(i << 1) as usize];
        let end = outcome.output[((i << 1) + 1) as usize];
        if begin == YARR_OFFSET_NO_MATCH || end == YARR_OFFSET_NO_MATCH {
            captures.push(None);
        } else {
            captures.push(Some(MatchRange { start: begin, end }));
        }
    }

    MatchResult {
        status,
        overall,
        captures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::StringId;
    use crate::yarr::{
        MatchFrom, MatchRange, MatchStackLimitSource, MatchState, MatchStatus,
        MatchingContextHolderDescriptor,
    };

    fn invocation(captures: Vec<Option<MatchRange>>) -> RegExpProgramInvocationRecord {
        RegExpProgramInvocationRecord {
            program: RegExpProgramId(17),
            regexp_object: None,
            body: Some(RegExpProgramBody::Bytecode(BytecodePatternId(3))),
            mode: RegExpMatchMode::Exec,
            context: YarrMatchContext {
                state: MatchState {
                    input: MatchInput {
                        string: StringId(9),
                        start: 1,
                        length: 6,
                        from: MatchFrom::VmThread,
                    },
                    current_position: 1,
                    remaining_match_limit: 128,
                    captures,
                    backtrack_depth: 0,
                },
                unicode_aware: true,
                has_indices: false,
                can_call_jit: false,
                holder: Some(MatchingContextHolderDescriptor {
                    from: MatchFrom::VmThread,
                    stack_limit_source: MatchStackLimitSource::VmSoftStackLimit,
                    has_free_list: false,
                    vm_executing_regexp_is_set: true,
                }),
            },
            expected_capture_slots: 2,
            match_only: false,
            root_boundaries: vec![RegExpRootBoundaryRecord {
                kind: RegExpRootBoundaryKind::InputString,
                root_kind: RootKind::VMRegister,
                mutation_authority: RootSetMutationAuthority::VmRegisterFile,
                object: None,
                precise: true,
            }],
        }
    }

    #[test]
    fn regexp_invocation_consumes_match_state_semantics() {
        let record = invocation(vec![None, None]);

        let descriptor = describe_regexp_program_invocation(&record).unwrap();

        assert_eq!(descriptor.program, RegExpProgramId(17));
        assert_eq!(descriptor.state.input_end, 7);
        assert!(descriptor.requires_vm_regexp_execution_mark);
        assert!(!descriptor.can_enter_jit);
        assert_eq!(descriptor.root_boundary_count, 1);
    }

    #[test]
    fn regexp_result_consumes_match_result_semantics() {
        let invocation = invocation(vec![None, None]);
        let result = RegExpProgramResultRecord {
            program: RegExpProgramId(17),
            input: invocation.context.state.input,
            result: MatchResult {
                status: MatchStatus::Match,
                overall: Some(MatchRange { start: 1, end: 4 }),
                captures: vec![Some(MatchRange { start: 1, end: 4 }), None],
            },
            used_jit: false,
            fell_back_to_interpreter: false,
            remaining_match_limit: 120,
            has_indices: false,
        };

        let descriptor = describe_regexp_program_result(&invocation, &result).unwrap();

        assert!(descriptor.result.succeeded);
        assert_eq!(descriptor.result.initialized_capture_count, 1);
        assert_eq!(descriptor.remaining_match_limit, 120);
    }

    #[test]
    fn regexp_boundary_rejects_capture_shape_mismatch() {
        let invocation = invocation(vec![None]);

        assert_eq!(
            describe_regexp_program_invocation(&invocation).unwrap_err(),
            RegExpExecutionBoundaryError::CaptureSlotCountMismatch {
                expected: 2,
                actual: 1,
            }
        );
    }
}
