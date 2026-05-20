//! Yarr match input and result contracts.
//!
//! Matching is not implemented here. These types describe how callers, the
//! bytecode interpreter, and the JIT will exchange input bounds and captures.

use crate::strings::StringId;

/// Thread or context that requested a match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchFrom {
    VmThread,
    CompilerThread,
}

/// Stack-limit source selected by a match context holder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchStackLimitSource {
    VmSoftStackLimit,
    CurrentThreadRecursionLimit,
}

/// Direction used by parsed terms and bytecode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchDirection {
    Forward,
    Backward,
}

/// Match status compatible with Yarr result categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchStatus {
    Match,
    NoMatch,
    ErrorNoMatch,
    JitCodeFailure,
    ErrorHitLimit,
    ErrorNoMemory,
    ErrorInternal,
}

/// Input string and bounds for a match attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MatchInput {
    pub string: StringId,
    pub start: u32,
    pub length: u32,
    pub from: MatchFrom,
}

/// Inclusive-exclusive range in the input string.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MatchRange {
    pub start: u32,
    pub end: u32,
}

/// Mutable state shape needed by future interpreter or JIT code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchState {
    pub input: MatchInput,
    pub current_position: u32,
    pub remaining_match_limit: u32,
    pub captures: Vec<Option<MatchRange>>,
    pub backtrack_depth: u32,
}

/// Scoped holder that marks VM regexp execution while a match is active.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchingContextHolderDescriptor {
    pub from: MatchFrom,
    pub stack_limit_source: MatchStackLimitSource,
    pub has_free_list: bool,
    pub vm_executing_regexp_is_set: bool,
}

/// Runtime context shared with generated Yarr code.
/// The holder owns VM execution marking. Generated code receives this context
/// as a borrowed boundary and may mutate only match state and output captures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrMatchContext {
    pub state: MatchState,
    pub unicode_aware: bool,
    pub has_indices: bool,
    pub can_call_jit: bool,
    pub holder: Option<MatchingContextHolderDescriptor>,
}

/// Result returned to the regexp runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchResult {
    pub status: MatchStatus,
    pub overall: Option<MatchRange>,
    pub captures: Vec<Option<MatchRange>>,
}

/// Semantic error reported for non-executing match descriptors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchSemanticError {
    InputRangeOverflow,
    CurrentPositionOutOfBounds,
    CaptureOutOfBounds(MatchRange),
    CaptureRangeInverted(MatchRange),
    SuccessfulResultWithoutOverallRange,
    FailedResultWithOverallRange,
    HasIndicesCaptureMismatch { expected: usize, actual: usize },
}

/// Semantic summary of a match state before any engine runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchStateSemanticDescriptor {
    pub input: MatchInput,
    pub input_end: u32,
    pub current_position: u32,
    pub capture_slot_count: usize,
    pub initialized_capture_count: usize,
    pub remaining_match_limit: u32,
    pub backtrack_depth: u32,
    pub unicode_aware: bool,
    pub has_indices: bool,
    pub can_call_jit: bool,
    pub holder_marks_regexp_execution: bool,
}

/// Semantic summary of a result returned by a future matcher.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchResultSemanticDescriptor {
    pub status: MatchStatus,
    pub succeeded: bool,
    pub recoverable_no_match: bool,
    pub fatal_error: bool,
    pub overall: Option<MatchRange>,
    pub capture_slot_count: usize,
    pub initialized_capture_count: usize,
    pub has_indices_capture_slot_count: usize,
}

pub fn describe_match_state_semantics(
    context: &YarrMatchContext,
) -> Result<MatchStateSemanticDescriptor, MatchSemanticError> {
    let input_end = context
        .state
        .input
        .start
        .checked_add(context.state.input.length)
        .ok_or(MatchSemanticError::InputRangeOverflow)?;
    if context.state.current_position < context.state.input.start
        || context.state.current_position > input_end
    {
        return Err(MatchSemanticError::CurrentPositionOutOfBounds);
    }
    for capture in context.state.captures.iter().flatten() {
        validate_match_range(*capture, context.state.input.start, input_end)?;
    }

    Ok(MatchStateSemanticDescriptor {
        input: context.state.input,
        input_end,
        current_position: context.state.current_position,
        capture_slot_count: context.state.captures.len(),
        initialized_capture_count: context
            .state
            .captures
            .iter()
            .filter(|capture| capture.is_some())
            .count(),
        remaining_match_limit: context.state.remaining_match_limit,
        backtrack_depth: context.state.backtrack_depth,
        unicode_aware: context.unicode_aware,
        has_indices: context.has_indices,
        can_call_jit: context.can_call_jit,
        holder_marks_regexp_execution: context
            .holder
            .as_ref()
            .map(|holder| holder.vm_executing_regexp_is_set)
            .unwrap_or(false),
    })
}

pub fn describe_match_result_semantics(
    result: &MatchResult,
    input: MatchInput,
    has_indices: bool,
) -> Result<MatchResultSemanticDescriptor, MatchSemanticError> {
    let input_end = input
        .start
        .checked_add(input.length)
        .ok_or(MatchSemanticError::InputRangeOverflow)?;
    if let Some(overall) = result.overall {
        validate_match_range(overall, input.start, input_end)?;
    }
    for capture in result.captures.iter().flatten() {
        validate_match_range(*capture, input.start, input_end)?;
    }

    let succeeded = result.status == MatchStatus::Match;
    if succeeded && result.overall.is_none() {
        return Err(MatchSemanticError::SuccessfulResultWithoutOverallRange);
    }
    if !succeeded && result.overall.is_some() {
        return Err(MatchSemanticError::FailedResultWithOverallRange);
    }

    let initialized_capture_count = result
        .captures
        .iter()
        .filter(|capture| capture.is_some())
        .count();
    if has_indices && succeeded && initialized_capture_count > result.captures.len() {
        return Err(MatchSemanticError::HasIndicesCaptureMismatch {
            expected: result.captures.len(),
            actual: initialized_capture_count,
        });
    }

    Ok(MatchResultSemanticDescriptor {
        status: result.status,
        succeeded,
        recoverable_no_match: matches!(
            result.status,
            MatchStatus::NoMatch | MatchStatus::ErrorNoMatch
        ),
        fatal_error: matches!(
            result.status,
            MatchStatus::ErrorHitLimit | MatchStatus::ErrorNoMemory | MatchStatus::ErrorInternal
        ),
        overall: result.overall,
        capture_slot_count: result.captures.len(),
        initialized_capture_count,
        has_indices_capture_slot_count: if has_indices {
            result.captures.len()
        } else {
            0
        },
    })
}

fn validate_match_range(
    range: MatchRange,
    input_start: u32,
    input_end: u32,
) -> Result<(), MatchSemanticError> {
    if range.start > range.end {
        return Err(MatchSemanticError::CaptureRangeInverted(range));
    }
    if range.start < input_start || range.end > input_end {
        return Err(MatchSemanticError::CaptureOutOfBounds(range));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> MatchInput {
        MatchInput {
            string: StringId(7),
            start: 2,
            length: 8,
            from: MatchFrom::VmThread,
        }
    }

    #[test]
    fn match_state_semantics_validate_bounds_without_matching() {
        let context = YarrMatchContext {
            state: MatchState {
                input: input(),
                current_position: 4,
                remaining_match_limit: 32,
                captures: vec![Some(MatchRange { start: 2, end: 5 }), None],
                backtrack_depth: 1,
            },
            unicode_aware: true,
            has_indices: true,
            can_call_jit: false,
            holder: Some(MatchingContextHolderDescriptor {
                from: MatchFrom::VmThread,
                stack_limit_source: MatchStackLimitSource::VmSoftStackLimit,
                has_free_list: true,
                vm_executing_regexp_is_set: true,
            }),
        };

        let descriptor = describe_match_state_semantics(&context).unwrap();

        assert_eq!(descriptor.input_end, 10);
        assert_eq!(descriptor.initialized_capture_count, 1);
        assert!(descriptor.holder_marks_regexp_execution);
    }

    #[test]
    fn match_result_semantics_reject_failed_result_with_range() {
        let result = MatchResult {
            status: MatchStatus::NoMatch,
            overall: Some(MatchRange { start: 2, end: 4 }),
            captures: Vec::new(),
        };

        assert_eq!(
            describe_match_result_semantics(&result, input(), false).unwrap_err(),
            MatchSemanticError::FailedResultWithOverallRange
        );
    }
}
