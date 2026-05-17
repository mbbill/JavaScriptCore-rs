//! Yarr match input and result contracts.
//!
//! Matching is not implemented here. These types describe how callers, the
//! bytecode interpreter, and the JIT will exchange input bounds and captures.

use crate::runtime::StringId;

/// Thread or context that requested a match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchFrom {
    VmThread,
    CompilerThread,
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

/// Runtime context shared with generated Yarr code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrMatchContext {
    pub state: MatchState,
    pub unicode_aware: bool,
    pub has_indices: bool,
    pub can_call_jit: bool,
}

/// Result returned to the regexp runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchResult {
    pub status: MatchStatus,
    pub overall: Option<MatchRange>,
    pub captures: Vec<Option<MatchRange>>,
}
