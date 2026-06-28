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

// =============================================================================
// Yarr flat backtracking interpreter.
//
// Faithful port of `class Interpreter` and `matchDisjunction` in
// JavaScriptCore/yarr/YarrInterpreter.cpp:79-2274. The interpreter is a flat
// goto-driven dispatch machine over compiled `ByteTerm`s (here `BytecodeTerm`)
// and `ByteDisjunction`s rather than recursive per-term calls
// (mcts_mem yarr/interpreter-dispatch.md; YarrInterpreter.cpp:1698-2190).
//
// CODE-UNIT REP (language-forced divergence, per the frozen IR contract):
// C++ specializes `Interpreter<CharType>` over Latin1Character/char16_t and the
// `InputStream` indexes raw 8/16-bit code units. We operate over a UTF-16
// code-unit view (`&[u16]`) so capture and lastIndex offsets live in the SAME
// code-unit space C++ uses; we do NOT use UTF-8 byte offsets. Surrogate-pair
// decoding is only enabled in Unicode compile mode (`decode_surrogate_pairs`),
// matching C++ `pattern.eitherUnicode()`.
// =============================================================================

use crate::yarr::{
    ByteDisjunction, BytecodeAlternativeJump, BytecodePattern, BytecodeTerm, BytecodeTermKind,
    CharacterClassDescriptor, QuantifierKind,
};

/// YarrInterpreter result enum. C++ `JSRegExpResult` (Yarr.h) carries
/// Match/NoMatch/ErrorNoMatch/ErrorHitLimit/ErrorNoMemory/ErrorInternal; the
/// frozen IR contract pre-decided a 3-valued result (bool-return is the rejected
/// alt). The C++ error variants (no-memory / recursion-overflow / internal) are
/// folded into `HitLimit` here — all bail out of matching without a thrown
/// exception, which the runtime boundary then surfaces. See
/// YarrInterpreter.cpp:1745 (`!--remainingMatchCount`) and
/// mcts_mem yarr/interpreter-dispatch.md (728dc3f9: bool-return replaced by the
/// 3-valued enum so HitLimit propagates up the call tree).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JSRegExpResult {
    Match,
    NoMatch,
    HitLimit,
}

/// `offsetNoMatch` — Yarr.h:47 (`std::numeric_limits<unsigned>::max()`).
pub const YARR_OFFSET_NO_MATCH: u32 = u32::MAX;

/// `matchLimit` — Yarr.h:51. Bounds pathological backtracking; when the
/// per-`matchDisjunction` decrementing counter reaches zero the interpreter
/// returns `HitLimit`.
pub const YARR_MATCH_LIMIT: u32 = 100_000_000;

/// Safe-Rust proxy for C++ `StackCheck::isSafeToRecurse()`
/// (YarrInterpreter.cpp:2264): a native-stack guard that returns ErrorNoMemory.
/// We cannot inspect the native stack in safe Rust, so we cap recursion depth
/// explicitly. Exceeding it folds into `HitLimit` (see `JSRegExpResult`).
const YARR_MAX_RECURSION_DEPTH: u32 = 4096;

const ERROR_CODE_POINT: u32 = 0xFFFF_FFFF; // YarrInterpreter.cpp:82 errorCodePoint

/// Quantifier family as the interpreter dispatches on it. Mirrors C++
/// `QuantifierType` (FixedCount/Greedy/NonGreedy). The descriptor's
/// `QuantifierKind::Infinite` (unbounded repetition) is treated as `Greedy`
/// with an unbounded max, matching how C++ encodes `{n,}` greedy repeats.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QType {
    FixedCount,
    Greedy,
    NonGreedy,
}

fn quantity_type(term: &BytecodeTerm) -> QType {
    match term.quantifier.kind {
        QuantifierKind::FixedCount => QType::FixedCount,
        QuantifierKind::Greedy | QuantifierKind::Infinite => QType::Greedy,
        QuantifierKind::NonGreedy => QType::NonGreedy,
    }
}

#[inline]
fn quantity_max(term: &BytecodeTerm) -> u32 {
    // C++ `atom.quantityMaxCount`; `quantifyInfinite` == UINT_MAX. The descriptor
    // stores `max: None` for unbounded repetition.
    term.quantifier.max.unwrap_or(u32::MAX)
}

#[inline]
fn dir_index(direction: MatchDirection) -> u32 {
    // C++ uses `term.matchDirection()` (Forward==0, Backward==1) directly as an
    // output-vector index offset (YarrInterpreter.cpp:1133-1134).
    match direction {
        MatchDirection::Forward => 0,
        MatchDirection::Backward => 1,
    }
}

#[inline]
fn u16_length(ch: u32) -> u32 {
    // ICU `U16_LENGTH`: 1 for BMP, 2 for supplementary code points.
    if ch <= 0xFFFF {
        1
    } else {
        2
    }
}

/// JSC `newlineCharacterClass` membership (YarrPattern newline class): the four
/// ECMAScript line terminators. Used by BOL/EOL (multiline) and DotStar.
fn is_newline(ch: u32) -> bool {
    matches!(ch, 0x0A | 0x0D | 0x2028 | 0x2029)
}

/// JSC `wordcharCharacterClass` membership: `[A-Za-z0-9_]`. In Unicode +
/// ignoreCase mode JSC also folds U+017F and U+212A into the word class
/// (`ignoreCaseWordcharCharacterClass`); we add those when `ignore_case_unicode`.
fn is_word_char(ch: u32, ignore_case_unicode: bool) -> bool {
    let base = matches!(ch, 0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A) || ch == 0x5F;
    if ignore_case_unicode {
        base || ch == 0x017F || ch == 0x212A
    } else {
        base
    }
}

/// testCharacterClass — YarrInterpreter.cpp:485. Membership in a character class,
/// WITHOUT applying the term-level `m_invert` (the caller applies that). We reuse
/// `character_class_contains`, which already folds `descriptor.inverted`; fixtures
/// therefore keep `descriptor.inverted == false` and express inversion via the
/// term, exactly as JSC's interpreter splits class membership from `term.invert()`.
fn test_character_class(class: &CharacterClassDescriptor, ch: u32) -> bool {
    match char::from_u32(ch) {
        Some(c) => crate::yarr::character_class_contains(class, c).unwrap_or(false),
        None => false,
    }
}

/// ignoreCase membership: C++ pre-folds the `CharacterClass` at construction
/// (`Yarr::CharacterClassConstructor` adds each member's canonical equivalents
/// when `ignoreCase`). We fold at the membership test instead, for the legacy/BMP
/// single-char-fold subset (a-z<->A-Z and the Latin-1 case pairs). Full UCS2/
/// Unicode canonicalization-table fidelity (multi-char folds like `ß`, and the
/// Unicode-mode `k`/Kelvin, `s`/`ſ` groups) is deferred to the Unicode unit;
/// JSC legacy does not fold `ß`, matching the single-char guard below.
fn test_character_class_ci(class: &CharacterClassDescriptor, ch: u32, ignore_case: bool) -> bool {
    if test_character_class(class, ch) {
        return true;
    }
    if !ignore_case {
        return false;
    }
    let Some(c) = char::from_u32(ch) else {
        return false;
    };
    let mut upper = c.to_uppercase();
    if let (Some(u), None) = (upper.next(), upper.next()) {
        if u != c && test_character_class(class, u as u32) {
            return true;
        }
    }
    let mut lower = c.to_lowercase();
    if let (Some(l), None) = (lower.next(), lower.next()) {
        if l != c && test_character_class(class, l as u32) {
            return true;
        }
    }
    false
}

/// `InputStream` — YarrInterpreter.cpp:273. Code-unit cursor over the subject.
struct InputStream {
    input: Vec<u16>,
    pos: u32,
    length: u32,
    decode_surrogate_pairs: bool,
}

// `at_start()` and `try_uncheck_input()` are faithful InputStream methods
// (YarrInterpreter.cpp:406, :436) used by the Backward/lookbehind handlers that
// are deferred with the rest of the Backward machinery (see serial-coupling
// notes); kept here so the port mirrors the C++ InputStream surface.
#[allow(dead_code)]
impl InputStream {
    fn new(input: Vec<u16>, start: u32, decode_surrogate_pairs: bool) -> Self {
        let length = input.len() as u32;
        Self {
            input,
            pos: start,
            length,
            decode_surrogate_pairs,
        }
    }

    #[inline]
    fn unit(&self, p: u32) -> u16 {
        self.input[p as usize]
    }

    fn next(&mut self) {
        self.pos += 1;
    }

    fn rewind(&mut self, amount: u32) {
        self.pos = self.pos.saturating_sub(amount);
    }

    fn read(&self) -> u32 {
        if self.pos < self.length {
            self.unit(self.pos) as u32
        } else {
            ERROR_CODE_POINT
        }
    }

    /// readChecked — YarrInterpreter.cpp:302. Reads at `pos - neg`; in surrogate
    /// mode a lead surrogate advances `pos` to consume the trailing unit.
    fn read_checked(&mut self, neg: u32) -> u32 {
        if self.pos < neg {
            return ERROR_CODE_POINT;
        }
        let p = self.pos - neg;
        if p >= self.length {
            return ERROR_CODE_POINT;
        }
        let result = self.unit(p) as u32;
        if self.decode_surrogate_pairs
            && is_u16_lead(result)
            && p + 1 < self.length
            && is_u16_trail(self.unit(p + 1) as u32)
        {
            if self.at_end() {
                return ERROR_CODE_POINT;
            }
            self.next();
            return u16_supplementary(result, self.unit(p + 1) as u32);
        } else if self.decode_surrogate_pairs
            && p > 0
            && is_u16_trail(result)
            && is_u16_lead(self.unit(p - 1) as u32)
        {
            return ERROR_CODE_POINT;
        }
        result
    }

    /// readCheckedDontAdvance — YarrInterpreter.cpp:318.
    fn read_checked_dont_advance(&self, neg: u32) -> u32 {
        if self.pos < neg {
            return ERROR_CODE_POINT;
        }
        let p = self.pos - neg;
        if p >= self.length {
            return ERROR_CODE_POINT;
        }
        let result = self.unit(p) as u32;
        if self.decode_surrogate_pairs
            && is_u16_lead(result)
            && p + 1 < self.length
            && is_u16_trail(self.unit(p + 1) as u32)
        {
            return u16_supplementary(result, self.unit(p + 1) as u32);
        }
        if self.decode_surrogate_pairs
            && is_u16_trail(result)
            && p > 0
            && is_u16_lead(self.unit(p - 1) as u32)
        {
            return ERROR_CODE_POINT;
        }
        result
    }

    /// tryReadBackward — YarrInterpreter.cpp:347.
    fn try_read_backward(&mut self, neg: u32) -> u32 {
        if self.pos < neg {
            return ERROR_CODE_POINT;
        }
        let p = self.pos - neg;
        if p >= self.length {
            return ERROR_CODE_POINT;
        }
        let result = self.unit(p) as u32;
        if self.decode_surrogate_pairs
            && is_u16_trail(result)
            && p > 0
            && is_u16_lead(self.unit(p - 1) as u32)
        {
            self.rewind(1);
            return u16_supplementary(self.unit(p - 1) as u32, result);
        }
        result
    }

    /// readSurrogatePairChecked — YarrInterpreter.cpp:361.
    fn read_surrogate_pair_checked(&self, neg: u32) -> u32 {
        if self.pos < neg {
            return ERROR_CODE_POINT;
        }
        let p = self.pos - neg;
        if p + 1 >= self.length {
            return ERROR_CODE_POINT;
        }
        let first = self.unit(p) as u32;
        let second = self.unit(p + 1) as u32;
        if is_u16_lead(first) && is_u16_trail(second) {
            u16_supplementary(first, second)
        } else {
            ERROR_CODE_POINT
        }
    }

    /// reread — YarrInterpreter.cpp:375. Absolute read (used by back-references
    /// and DotStar) from index `from`.
    fn reread(&self, from: u32) -> u32 {
        if from >= self.length {
            return ERROR_CODE_POINT;
        }
        let result = self.unit(from) as u32;
        if self.decode_surrogate_pairs && from + 1 < self.length {
            if is_u16_lead(result) && is_u16_trail(self.unit(from + 1) as u32) {
                return u16_supplementary(result, self.unit(from + 1) as u32);
            }
            if is_u16_trail(result) && is_u16_lead(self.unit(from + 1) as u32) {
                return ERROR_CODE_POINT;
            }
        }
        result
    }

    fn get_pos(&self) -> u32 {
        self.pos
    }

    fn set_pos(&mut self, p: u32) {
        self.pos = p;
    }

    fn at_start(&self) -> bool {
        self.pos == 0
    }

    fn at_start_offset(&self, neg: u32) -> bool {
        self.pos == neg
    }

    fn at_end(&self) -> bool {
        self.pos == self.length
    }

    fn at_end_offset(&self, neg: u32) -> bool {
        self.pos >= neg && (self.pos - neg) == self.length
    }

    fn end(&self) -> u32 {
        self.length
    }

    fn check_input(&mut self, count: u32) -> bool {
        if let Some(np) = self.pos.checked_add(count) {
            if np <= self.length {
                self.pos = np;
                return true;
            }
        }
        false
    }

    fn uncheck_input(&mut self, count: u32) {
        self.pos = self.pos.saturating_sub(count);
    }

    fn try_uncheck_input(&mut self, count: u32) -> bool {
        if count > self.pos {
            return false;
        }
        self.pos -= count;
        true
    }

    fn is_available_input(&self, offset: u32) -> bool {
        match self.pos.checked_add(offset) {
            Some(np) => np <= self.length,
            None => false,
        }
    }

    fn is_valid_negative_input_offset(&self, offset: u32) -> bool {
        self.pos >= offset && (self.pos - offset) < self.length
    }
}

/// C++ `toASCIIUpper` (YarrInterpreter.cpp:678): uppercases an ASCII letter,
/// leaving every other code point unchanged.
#[inline]
fn ascii_upper(ch: u32) -> u32 {
    if (0x61..=0x7A).contains(&ch) {
        ch - 0x20
    } else {
        ch
    }
}

#[inline]
fn is_u16_lead(ch: u32) -> bool {
    (0xD800..=0xDBFF).contains(&ch)
}

#[inline]
fn is_u16_trail(ch: u32) -> bool {
    (0xDC00..=0xDFFF).contains(&ch)
}

#[inline]
fn u16_supplementary(lead: u32, trail: u32) -> u32 {
    0x10000 + ((lead - 0xD800) << 10) + (trail - 0xDC00)
}

/// One cell of a `DisjunctionContext` backtracking frame. C++ uses a flat
/// `uintptr_t frame[]` reinterpret-cast to typed `BackTrackInfo*` structs at each
/// term's `frameLocation` (YarrInterpreter.cpp:732 etc.). Safe Rust cannot
/// reinterpret raw memory, so each `uintptr_t` slot is a `Num`, except the
/// variable-count parentheses slot which holds the owned context list
/// (`BackTrackInfoParentheses`, YarrInterpreter.cpp:86). `frameLocation`
/// arithmetic and `frame_size` slot counts stay identical to C++.
#[derive(Clone, Debug)]
enum FrameCell {
    Empty,
    Num(u32),
    Paren(BackTrackInfoParentheses),
}

/// DisjunctionContext — YarrInterpreter.cpp:107. Per-disjunction backtracking
/// state: current term index, the match span, and the precomputed frame.
#[derive(Clone, Debug)]
struct DisjunctionContext {
    term: i32,
    match_begin: u32,
    match_end: u32,
    frame: Vec<FrameCell>,
}

impl DisjunctionContext {
    fn new(frame_size: u32) -> Self {
        Self {
            term: 0,
            match_begin: 0,
            match_end: 0,
            frame: vec![FrameCell::Empty; frame_size as usize],
        }
    }

    fn num(&self, loc: u32) -> u32 {
        match self.frame.get(loc as usize) {
            Some(FrameCell::Num(v)) => *v,
            _ => 0,
        }
    }

    fn set_num(&mut self, loc: u32, value: u32) {
        if let Some(slot) = self.frame.get_mut(loc as usize) {
            *slot = FrameCell::Num(value);
        }
    }

    fn paren_mut(&mut self, loc: u32) -> &mut BackTrackInfoParentheses {
        let slot = &mut self.frame[loc as usize];
        if !matches!(slot, FrameCell::Paren(_)) {
            *slot = FrameCell::Paren(BackTrackInfoParentheses::default());
        }
        match slot {
            FrameCell::Paren(p) => p,
            _ => unreachable!(),
        }
    }
}

/// BackTrackInfoParentheses — YarrInterpreter.cpp:86. `{begin, matchAmount,
/// lastContext}`; the C++ `lastContext` linked list of
/// `ParenthesesDisjunctionContext` is modeled here as an owned stack
/// (`contexts.last()` == `lastContext`).
#[derive(Clone, Debug, Default)]
struct BackTrackInfoParentheses {
    begin: u32,
    match_amount: u32,
    contexts: Vec<ParenthesesDisjunctionContext>,
}

/// ParenthesesDisjunctionContext — YarrInterpreter.cpp:155. Saves the captures
/// overwritten by one parentheses iteration plus that iteration's nested
/// `DisjunctionContext`.
#[derive(Clone, Debug)]
struct ParenthesesDisjunctionContext {
    saved_output: Vec<u32>,
    first_subpattern_id: u32,
    num_nested_subpatterns: u32,
    sub: DisjunctionContext,
}

/// The flat interpreter — YarrInterpreter.cpp:79 `class Interpreter`.
struct Interpreter<'a> {
    pattern: &'a BytecodePattern,
    output: Vec<u32>,
    input: InputStream,
    /// Recovered pattern flags. C++ reads `BytecodePattern::m_flags`; the Rust
    /// descriptor has no pattern-level flags field, so we recover them from the
    /// per-term `flags` the ByteCompiler stamps onto every emitted term.
    flags: RegexFlagsForMatch,
    start_offset: u32,
    remaining_match_count: u32,
    recursion_depth: u32,
}

#[derive(Clone, Copy)]
struct RegexFlagsForMatch {
    sticky: bool,
    either_unicode: bool,
}

/// Outcome of running the interpreter: the 3-valued result plus the output
/// offset vector (output[0],[1] = overall span; output[2i],[2i+1] = capture i).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrInterpretOutcome {
    pub result: JSRegExpResult,
    pub output: Vec<u32>,
}

/// `interpret(BytecodePattern*, StringView, start, output)` —
/// YarrInterpreter.cpp:3221 / :2209. Runs the bytecode interpreter over a UTF-16
/// code-unit subject.
pub fn interpret_bytecode(
    pattern: &BytecodePattern,
    input: &[u16],
    start: u32,
) -> YarrInterpretOutcome {
    interpret_bytecode_with_limit(pattern, input, start, YARR_MATCH_LIMIT)
}

/// Variant exposing the match limit so HitLimit is reachable in tests without a
/// 100M-iteration loop (the limit constant itself mirrors C++ exactly).
pub fn interpret_bytecode_with_limit(
    pattern: &BytecodePattern,
    input: &[u16],
    start: u32,
    match_limit: u32,
) -> YarrInterpretOutcome {
    let flags = recover_match_flags(pattern);
    let mut interp = Interpreter {
        pattern,
        output: vec![YARR_OFFSET_NO_MATCH; pattern.offset_vector.offsets_size.max(2) as usize],
        input: InputStream::new(input.to_vec(), start, flags.either_unicode),
        flags,
        start_offset: start,
        remaining_match_count: match_limit,
        recursion_depth: 0,
    };
    let result = interp.run();
    YarrInterpretOutcome {
        result,
        output: interp.output,
    }
}

fn recover_match_flags(pattern: &BytecodePattern) -> RegexFlagsForMatch {
    let f = pattern
        .body
        .terms
        .first()
        .map(|t| t.flags)
        .unwrap_or_default();
    RegexFlagsForMatch {
        sticky: f.sticky,
        either_unicode: f.unicode || f.unicode_sets,
    }
}

impl<'a> Interpreter<'a> {
    /// interpret() — YarrInterpreter.cpp:2209. Initializes the output vector and
    /// runs `matchDisjunction` over the body.
    fn run(&mut self) -> JSRegExpResult {
        if !self.input.is_available_input(0) {
            return JSRegExpResult::NoMatch;
        }

        // C++ inits only the begin offsets to offsetNoMatch (the ends are written
        // whenever a begin is). We initialize the whole vector to offsetNoMatch
        // (a safe superset) then zero the named-capture region, avoiding any read
        // of an uninitialized end slot in safe Rust.
        for v in self.output.iter_mut() {
            *v = YARR_OFFSET_NO_MATCH;
        }
        let base = self.pattern.offset_vector.base_for_named_captures as usize;
        let size = self.pattern.offset_vector.offsets_size as usize;
        for i in base..size.min(self.output.len()) {
            self.output[i] = 0;
        }

        let pattern = self.pattern;
        let body = &pattern.body;
        let mut context = DisjunctionContext::new(body.frame_size);
        let result = self.match_disjunction(body, &mut context, false);
        if result == JSRegExpResult::Match {
            self.output[0] = context.match_begin;
            self.output[1] = context.match_end;
        }
        result
    }

    // ---- character / class / assertion primitives -------------------------

    fn check_character(&mut self, term: &BytecodeTerm, neg: u32) -> bool {
        let ch = if term.direction == MatchDirection::Forward {
            self.input.read_checked(neg)
        } else {
            self.input.try_read_backward(neg)
        };
        term.character.map(|c| c as u32) == Some(ch)
    }

    fn check_surrogate_pair(&mut self, term: &BytecodeTerm, neg: u32) -> bool {
        let ch = self.input.read_surrogate_pair_checked(neg);
        term.character.map(|c| c as u32) == Some(ch)
    }

    fn check_cased_character(&mut self, term: &BytecodeTerm, neg: u32) -> bool {
        let ch = if term.direction == MatchDirection::Forward {
            self.input.read_checked(neg)
        } else {
            self.input.try_read_backward(neg)
        };
        if let Some((lo, hi)) = term.cased_range {
            (lo as u32 == ch) || (hi as u32 == ch)
        } else {
            false
        }
    }

    fn check_character_class(&mut self, term: &BytecodeTerm, neg: u32) -> bool {
        let ch = if term.direction == MatchDirection::Forward {
            self.input.read_checked(neg)
        } else {
            self.input.try_read_backward(neg)
        };
        if ch == ERROR_CODE_POINT {
            return false;
        }
        let class = match &term.character_class {
            Some(c) => c,
            None => return false,
        };
        let m = test_character_class_ci(class, ch, term.flags.ignore_case);
        if term.invert {
            !m
        } else {
            m
        }
    }

    /// checkCharacterClassDontAdvanceInputForNonBMP — YarrInterpreter.cpp:624.
    /// Legacy (BMP) path; in Unicode mode reads a surrogate pair without
    /// advancing for non-BMP-only classes.
    fn check_character_class_dont_advance(&mut self, term: &BytecodeTerm, neg: u32) -> bool {
        let class = match &term.character_class {
            Some(c) => c,
            None => return false,
        };
        if term.direction == MatchDirection::Backward && neg > self.input.get_pos() {
            return false;
        }
        let only_non_bmp = class_has_only_non_bmp(class);
        let read = if only_non_bmp {
            self.input.read_surrogate_pair_checked(neg)
        } else {
            self.input.read_checked(neg)
        };
        if read == ERROR_CODE_POINT {
            return false;
        }
        let class = term.character_class.as_ref().unwrap();
        test_character_class_ci(class, read, term.flags.ignore_case)
    }

    fn match_assertion_bol(&self, term: &BytecodeTerm) -> bool {
        self.input.at_start_offset(term.input_position)
            || (term.flags.multiline
                && is_newline(
                    self.input
                        .read_checked_dont_advance(term.input_position + 1),
                ))
    }

    fn match_assertion_eol(&self, term: &BytecodeTerm) -> bool {
        if term.input_position != 0 {
            self.input.at_end_offset(term.input_position)
                || (term.flags.multiline
                    && is_newline(self.input.read_checked_dont_advance(term.input_position)))
        } else {
            self.input.at_end() || (term.flags.multiline && is_newline(self.input.read()))
        }
    }

    fn match_assertion_word_boundary(&self, term: &BytecodeTerm) -> bool {
        let offset = term.input_position;
        let icu = term.flags.ignore_case && self.flags.either_unicode;
        let prev_is_word = !self.input.at_start_offset(offset)
            && is_word_char(self.input.read_checked_dont_advance(offset + 1), icu);
        let read_is_word = if offset != 0 {
            !self.input.at_end_offset(offset)
                && is_word_char(self.input.read_checked_dont_advance(offset), icu)
        } else {
            !self.input.at_end() && is_word_char(self.input.read(), icu)
        };
        let word_boundary = prev_is_word != read_is_word;
        if term.invert {
            !word_boundary
        } else {
            word_boundary
        }
    }

    /// tryConsumeBackReference — YarrInterpreter.cpp:640. Forward-only path
    /// (lookbehind back-references are deferred with the rest of the Backward
    /// machinery; see serial-coupling notes).
    fn try_consume_back_reference(
        &mut self,
        match_begin: u32,
        match_end: u32,
        term: &BytecodeTerm,
    ) -> bool {
        let match_size = match_end - match_begin;
        if !self.input.check_input(match_size) {
            return false;
        }
        for i in 0..match_size {
            // YarrInterpreter.cpp:651: the read offset includes the term's
            // inputPosition (the negative distance of the back-reference END from
            // the current cursor). Omitting it mis-positions a back-reference that
            // is followed by further fixed terms (e.g. `(a)\1b`).
            let neg = term.input_position + match_size - i;
            let old_ch = self.input.reread(match_begin + i);
            let ch = self.input.read_checked_dont_advance(neg);
            if old_ch == ERROR_CODE_POINT || ch == ERROR_CODE_POINT {
                self.input.uncheck_input(match_size);
                return false;
            }
            if old_ch == ch {
                continue;
            }
            // YarrInterpreter.cpp:674 ignoreCase canonicalization. The legacy
            // (non-Unicode) ASCII path is faithful; full Unicode canonical
            // equivalence for non-ASCII back-references is deferred (Unicode unit).
            if term.flags.ignore_case
                && (old_ch < 0x80 || ch < 0x80)
                && ascii_upper(old_ch) == ascii_upper(ch)
            {
                continue;
            }
            self.input.uncheck_input(match_size);
            return false;
        }
        true
    }

    // ---- character / class backtrack handlers -----------------------------

    /// backtrackPatternCharacter — YarrInterpreter.cpp:730 (Forward path).
    fn backtrack_pattern_character(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) -> bool {
        let loc = frame_loc(term);
        match quantity_type(term) {
            QType::FixedCount => false,
            QType::Greedy => {
                let amount = ctx.num(loc + 1);
                if amount != 0 {
                    ctx.set_num(loc + 1, amount - 1);
                    self.input
                        .uncheck_input(u16_length(term.character.map(|c| c as u32).unwrap_or(0)));
                    true
                } else {
                    false
                }
            }
            QType::NonGreedy => {
                let amount = ctx.num(loc + 1);
                if amount < quantity_max(term) && self.input.check_input(1) {
                    ctx.set_num(loc + 1, amount + 1);
                    if self.check_character(term, term.input_position + 1) {
                        return true;
                    }
                }
                self.input.set_pos(ctx.num(loc)); // begin
                false
            }
        }
    }

    /// backtrackPatternCasedCharacter — YarrInterpreter.cpp:780 (Forward path).
    fn backtrack_pattern_cased_character(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) -> bool {
        let loc = frame_loc(term);
        match quantity_type(term) {
            QType::FixedCount => false,
            QType::Greedy => {
                let amount = ctx.num(loc + 1);
                if amount != 0 {
                    ctx.set_num(loc + 1, amount - 1);
                    self.input.uncheck_input(1);
                    true
                } else {
                    false
                }
            }
            QType::NonGreedy => {
                let amount = ctx.num(loc + 1);
                if amount < quantity_max(term) && self.input.check_input(1) {
                    ctx.set_num(loc + 1, amount + 1);
                    if self.check_cased_character(term, term.input_position + 1) {
                        return true;
                    }
                }
                self.input.uncheck_input(ctx.num(loc + 1));
                false
            }
        }
    }

    /// matchCharacterClass — YarrInterpreter.cpp:829 (Forward path; legacy and
    /// Unicode FixedCount).
    fn match_character_class(&mut self, term: &BytecodeTerm, ctx: &mut DisjunctionContext) -> bool {
        let loc = frame_loc(term);
        match quantity_type(term) {
            QType::FixedCount => {
                if self.flags.either_unicode {
                    let begin = self.input.get_pos();
                    ctx.set_num(loc, begin); // begin
                    let only_non_bmp = term
                        .character_class
                        .as_ref()
                        .map(class_has_only_non_bmp)
                        .unwrap_or(false);
                    for match_amount in 0..quantity_max(term) {
                        if term.invert {
                            if !self.check_character_class(term, term.input_position - match_amount)
                            {
                                self.input.set_pos(begin);
                                return false;
                            }
                        } else {
                            let off = match_amount * if only_non_bmp { 2 } else { 1 };
                            if !self
                                .check_character_class_dont_advance(term, term.input_position - off)
                            {
                                self.input.set_pos(begin);
                                return false;
                            }
                        }
                    }
                    return true;
                }
                for match_amount in 0..quantity_max(term) {
                    if !self.check_character_class(term, term.input_position - match_amount) {
                        return false;
                    }
                }
                true
            }
            QType::Greedy => {
                let mut position = self.input.get_pos();
                let mut match_amount = 0;
                while match_amount < quantity_max(term) && self.input.check_input(1) {
                    if !self.check_character_class(term, term.input_position + 1) {
                        self.input.set_pos(position);
                        break;
                    }
                    match_amount += 1;
                    position = self.input.get_pos();
                }
                ctx.set_num(loc + 1, match_amount);
                true
            }
            QType::NonGreedy => {
                ctx.set_num(loc, self.input.get_pos());
                ctx.set_num(loc + 1, 0);
                true
            }
        }
    }

    /// backtrackCharacterClass — YarrInterpreter.cpp:939 (Forward path).
    fn backtrack_character_class(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) -> bool {
        let loc = frame_loc(term);
        match quantity_type(term) {
            QType::FixedCount => {
                if self.flags.either_unicode {
                    self.input.set_pos(ctx.num(loc));
                }
                false
            }
            QType::Greedy => {
                let amount = ctx.num(loc + 1);
                if amount != 0 {
                    if self.flags.either_unicode {
                        ctx.set_num(loc + 1, amount - 1);
                        self.input.uncheck_input(1);
                        self.input.try_read_backward(term.input_position);
                        return true;
                    }
                    ctx.set_num(loc + 1, amount - 1);
                    self.input.uncheck_input(1);
                    return true;
                }
                false
            }
            QType::NonGreedy => {
                let amount = ctx.num(loc + 1);
                if amount < quantity_max(term) && self.input.check_input(1) {
                    ctx.set_num(loc + 1, amount + 1);
                    if self.check_character_class(term, term.input_position + 1) {
                        return true;
                    }
                }
                self.input.set_pos(ctx.num(loc));
                false
            }
        }
    }

    /// matchBackReference — YarrInterpreter.cpp:998 (Forward path).
    fn match_back_reference(&mut self, term: &BytecodeTerm, ctx: &mut DisjunctionContext) -> bool {
        let loc = frame_loc(term);
        match quantity_type(term) {
            QType::NonGreedy => {
                ctx.set_num(loc + 1, 0);
                ctx.set_num(loc, self.input.get_pos());
            }
            QType::FixedCount => {
                ctx.set_num(loc, self.input.get_pos());
            }
            QType::Greedy => {
                ctx.set_num(loc + 1, 0);
            }
        }

        let subpattern_id = term.subpattern_id.unwrap_or(0);
        let match_begin = self.output[(subpattern_id << 1) as usize];
        let match_end = self.output[((subpattern_id << 1) + 1) as usize];

        if match_end == YARR_OFFSET_NO_MATCH || match_begin == YARR_OFFSET_NO_MATCH {
            return true;
        }
        if match_begin == match_end {
            return true;
        }

        match quantity_type(term) {
            QType::FixedCount => {
                for _ in 0..quantity_max(term) {
                    if !self.try_consume_back_reference(match_begin, match_end, term) {
                        self.input.set_pos(ctx.num(loc));
                        return false;
                    }
                }
                true
            }
            QType::Greedy => {
                let mut match_amount = 0;
                while match_amount < quantity_max(term)
                    && self.try_consume_back_reference(match_begin, match_end, term)
                {
                    match_amount += 1;
                }
                ctx.set_num(loc + 1, match_amount);
                true
            }
            QType::NonGreedy => true,
        }
    }

    /// backtrackBackReference — YarrInterpreter.cpp:1073 (Forward path).
    fn backtrack_back_reference(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) -> bool {
        let loc = frame_loc(term);
        let subpattern_id = term.subpattern_id.unwrap_or(0);
        let match_begin = self.output[(subpattern_id << 1) as usize];
        let match_end = self.output[((subpattern_id << 1) + 1) as usize];

        if match_begin == YARR_OFFSET_NO_MATCH || match_end == YARR_OFFSET_NO_MATCH {
            return false;
        }
        if match_begin == match_end {
            return false;
        }

        match quantity_type(term) {
            QType::FixedCount => {
                self.input.set_pos(ctx.num(loc));
                false
            }
            QType::Greedy => {
                let amount = ctx.num(loc + 1);
                if amount != 0 {
                    ctx.set_num(loc + 1, amount - 1);
                    self.input.rewind(match_end - match_begin);
                    return true;
                }
                false
            }
            QType::NonGreedy => {
                let amount = ctx.num(loc + 1);
                if amount < quantity_max(term)
                    && self.try_consume_back_reference(match_begin, match_end, term)
                {
                    ctx.set_num(loc + 1, amount + 1);
                    return true;
                }
                self.input.set_pos(ctx.num(loc));
                false
            }
        }
    }

    // ---- parentheses-once / terminal / assertion handlers -----------------

    /// matchParenthesesOnceBegin — YarrInterpreter.cpp:1168. Returns the relative
    /// term advance to apply on success (parenthesesWidth for a NonGreedy
    /// speculative skip; 0 otherwise), or i32::MIN for failure (BACKTRACK).
    fn match_parentheses_once_begin(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) -> i32 {
        let loc = frame_loc(term);
        match quantity_type(term) {
            QType::Greedy => {
                ctx.set_num(loc, self.input.get_pos()); // begin
            }
            QType::NonGreedy => {
                ctx.set_num(loc, YARR_OFFSET_NO_MATCH); // begin = notFound
                return parentheses_width(term) as i32;
            }
            QType::FixedCount => {}
        }
        if term.capture {
            let subpattern_id = term.subpattern_id.unwrap_or(0);
            self.output[((subpattern_id << 1) + dir_index(term.direction)) as usize] =
                self.input.get_pos() - term.input_position;
        }
        0
    }

    /// matchParenthesesOnceEnd — YarrInterpreter.cpp:1199.
    fn match_parentheses_once_end(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) -> bool {
        if term.capture {
            let subpattern_id = term.subpattern_id.unwrap_or(0);
            self.output[((subpattern_id << 1) + 1 - dir_index(term.direction)) as usize] =
                self.input.get_pos() - term.input_position;
        }
        if quantity_type(term) == QType::FixedCount {
            return true;
        }
        let loc = frame_loc(term);
        ctx.num(loc) != self.input.get_pos()
    }

    /// backtrackParenthesesOnceBegin — YarrInterpreter.cpp:1222. Returns
    /// parenthesesWidth advance on success, or i32::MIN on failure.
    fn backtrack_parentheses_once_begin(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) -> i32 {
        if term.capture {
            let subpattern_id = term.subpattern_id.unwrap_or(0);
            self.output[(subpattern_id << 1) as usize] = YARR_OFFSET_NO_MATCH;
            self.output[((subpattern_id << 1) + 1) as usize] = YARR_OFFSET_NO_MATCH;
        }
        match quantity_type(term) {
            QType::Greedy => {
                let loc = frame_loc(term);
                ctx.set_num(loc, YARR_OFFSET_NO_MATCH); // begin = notFound
                parentheses_width(term) as i32
            }
            QType::NonGreedy | QType::FixedCount => i32::MIN,
        }
    }

    /// backtrackParenthesesOnceEnd — YarrInterpreter.cpp:1257. Returns a coded
    /// term action (see the dispatcher): positive => succeed and step back
    /// `parenthesesWidth`; <= -2 => fail and step back `-(code+2)`; i32::MIN =>
    /// plain failure.
    fn backtrack_parentheses_once_end(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) -> i32 {
        let loc = frame_loc(term);
        let width = parentheses_width(term) as i32;
        match quantity_type(term) {
            QType::Greedy => {
                if ctx.num(loc) == YARR_OFFSET_NO_MATCH {
                    return -width - 2; // sentinel: term -= width, return false
                }
                self.backtrack_once_end_try_nothing(term, ctx, width)
            }
            QType::NonGreedy => self.backtrack_once_end_try_nothing(term, ctx, width),
            QType::FixedCount => i32::MIN,
        }
    }

    fn backtrack_once_end_try_nothing(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
        width: i32,
    ) -> i32 {
        let loc = frame_loc(term);
        if ctx.num(loc) == YARR_OFFSET_NO_MATCH {
            ctx.set_num(loc, self.input.get_pos());
            if term.capture {
                let subpattern_id = term.subpattern_id.unwrap_or(0);
                self.output[((subpattern_id << 1) + dir_index(term.direction)) as usize] =
                    self.input.get_pos() - term.input_position;
            }
            return width; // term -= width, return true
        }
        i32::MIN
    }

    /// matchParenthesesTerminalBegin — YarrInterpreter.cpp:1295.
    fn match_parentheses_terminal_begin(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) {
        let loc = frame_loc(term);
        ctx.set_num(loc, self.input.get_pos());
    }

    /// matchParenthesesTerminalEnd — YarrInterpreter.cpp:1307. Returns the term
    /// rewind (parenthesesWidth+1) on success, or i32::MIN on empty-match failure.
    fn match_parentheses_terminal_end(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) -> i32 {
        let loc = frame_loc(term);
        if ctx.num(loc) == self.input.get_pos() {
            return i32::MIN;
        }
        parentheses_width(term) as i32 + 1
    }

    /// matchParentheticalAssertionBegin — YarrInterpreter.cpp:1342.
    fn match_parenthetical_assertion_begin(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) {
        let loc = frame_loc(term);
        ctx.set_num(loc, self.input.get_pos());
    }

    /// matchParentheticalAssertionEnd — YarrInterpreter.cpp:1354. Returns the
    /// term rewind (parenthesesWidth) on inverted failure, or i32::MAX on success.
    fn match_parenthetical_assertion_end(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) -> i32 {
        let loc = frame_loc(term);
        self.input.set_pos(ctx.num(loc));
        if term.invert {
            self.clear_assertion_captures(term);
            return parentheses_width(term) as i32; // term -= width, return false
        }
        i32::MAX
    }

    /// backtrackParentheticalAssertionBegin — YarrInterpreter.cpp:1378. Returns
    /// parenthesesWidth advance on inverted success, or i32::MIN on failure.
    fn backtrack_parenthetical_assertion_begin(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) -> i32 {
        if term.direction == MatchDirection::Backward {
            let loc = frame_loc(term);
            self.input.set_pos(ctx.num(loc));
        }
        if term.invert {
            return parentheses_width(term) as i32;
        }
        i32::MIN
    }

    /// backtrackParentheticalAssertionEnd — YarrInterpreter.cpp:1397. Always
    /// fails, stepping the term back parenthesesWidth.
    fn backtrack_parenthetical_assertion_end(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) -> i32 {
        let loc = frame_loc(term);
        self.input.set_pos(ctx.num(loc));
        self.clear_assertion_captures(term);
        parentheses_width(term) as i32
    }

    fn clear_assertion_captures(&mut self, term: &BytecodeTerm) {
        // term.containsAnyCaptures(): lastSubpatternId >= firstSubpatternId.
        if let Some(range) = term.subpattern_range {
            if range.last_subpattern_id >= range.first_subpattern_id {
                for sub in range.first_subpattern_id..=range.last_subpattern_id {
                    self.output[(sub << 1) as usize] = YARR_OFFSET_NO_MATCH;
                    self.output[((sub << 1) + 1) as usize] = YARR_OFFSET_NO_MATCH;
                }
            }
        }
    }

    /// matchDotStarEnclosure — YarrInterpreter.cpp:1659.
    fn match_dot_star_enclosure(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) -> bool {
        if term.flags.dot_all {
            ctx.match_begin = self.start_offset;
            ctx.match_end = self.input.end();
            return true;
        }
        let mut match_begin = ctx.match_begin;
        if match_begin > self.start_offset {
            match_begin -= 1;
            loop {
                if is_newline(self.input.reread(match_begin)) {
                    match_begin += 1;
                    break;
                }
                if match_begin == self.start_offset {
                    break;
                }
                match_begin -= 1;
            }
        }
        let mut match_end = self.input.get_pos();
        while match_end != self.input.end() && !is_newline(self.input.reread(match_end)) {
            match_end += 1;
        }
        let bol = match_begin != 0 && term.dot_star_bol();
        let eol = match_end != self.input.end() && term.dot_star_eol();
        if (bol || eol) && !term.flags.multiline {
            return false;
        }
        ctx.match_begin = match_begin;
        ctx.match_end = match_end;
        true
    }

    // ---- variable-count parentheses (matchParentheses) --------------------

    /// allocParenthesesDisjunctionContext — YarrInterpreter.cpp:237 +
    /// ParenthesesDisjunctionContext ctor (:157). Saves and clears the nested
    /// subpattern outputs for one iteration.
    fn alloc_paren_context(
        &mut self,
        sub_disjunction: &ByteDisjunction,
        term: &BytecodeTerm,
    ) -> ParenthesesDisjunctionContext {
        let num_nested = sub_disjunction.subpattern_count;
        let first_subpattern_id = term.subpattern_id.unwrap_or(0);
        let mut saved = Vec::with_capacity((num_nested << 1) as usize);
        for i in 0..(num_nested << 1) {
            let idx = ((first_subpattern_id << 1) + i) as usize;
            saved.push(self.output[idx]);
            self.output[idx] = YARR_OFFSET_NO_MATCH;
        }
        ParenthesesDisjunctionContext {
            saved_output: saved,
            first_subpattern_id,
            num_nested_subpatterns: num_nested,
            sub: DisjunctionContext::new(sub_disjunction.frame_size),
        }
    }

    fn restore_output(&mut self, pctx: &ParenthesesDisjunctionContext) {
        for i in 0..(pctx.num_nested_subpatterns << 1) {
            let idx = ((pctx.first_subpattern_id << 1) + i) as usize;
            self.output[idx] = pctx.saved_output[i as usize];
        }
    }

    /// recordParenthesesMatch — YarrInterpreter.cpp:1128.
    fn record_parentheses_match(
        &mut self,
        term: &BytecodeTerm,
        pctx: &ParenthesesDisjunctionContext,
    ) {
        if term.capture {
            let subpattern_id = term.subpattern_id.unwrap_or(0);
            self.output[((subpattern_id << 1) + dir_index(term.direction)) as usize] =
                pctx.sub.match_begin - term.input_position;
            self.output[((subpattern_id << 1) + 1 - dir_index(term.direction)) as usize] =
                pctx.sub.match_end - term.input_position;
        }
    }

    /// parenthesesDoBacktrack — YarrInterpreter.cpp:1148.
    fn parentheses_do_backtrack(
        &mut self,
        _term: &BytecodeTerm,
        sub: &ByteDisjunction,
        loc: u32,
        ctx: &mut DisjunctionContext,
    ) -> JSRegExpResult {
        loop {
            let amount = ctx.paren_mut(loc).match_amount;
            if amount == 0 {
                break;
            }
            // lastContext == contexts.last()
            let mut context = ctx.paren_mut(loc).contexts.last().cloned().unwrap();
            let result = self.match_disjunction(sub, &mut context.sub, true);
            if result == JSRegExpResult::Match {
                *ctx.paren_mut(loc).contexts.last_mut().unwrap() = context;
                return JSRegExpResult::Match;
            }
            self.restore_output(&context);
            ctx.paren_mut(loc).contexts.pop();
            ctx.paren_mut(loc).match_amount -= 1;
            if result != JSRegExpResult::NoMatch {
                return result;
            }
        }
        JSRegExpResult::NoMatch
    }

    /// matchParentheses — YarrInterpreter.cpp:1417.
    fn match_parentheses(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) -> JSRegExpResult {
        let loc = frame_loc(term);
        let sub_index = term.parentheses_disjunction.unwrap_or(0) as usize;
        let pattern = self.pattern;
        let sub = &pattern.parentheses[sub_index];

        {
            let bt = ctx.paren_mut(loc);
            bt.begin = self.input.get_pos();
            bt.match_amount = 0;
            bt.contexts.clear();
        }

        let minimum_match_count = term.quantifier.min;

        if minimum_match_count != 0 {
            while ctx.paren_mut(loc).match_amount < minimum_match_count {
                let mut context = self.alloc_paren_context(sub, term);
                let fixed = self.match_disjunction(sub, &mut context.sub, false);
                if fixed == JSRegExpResult::Match {
                    let bt = ctx.paren_mut(loc);
                    bt.contexts.push(context);
                    bt.match_amount += 1;
                } else {
                    self.restore_output(&context);
                    if fixed != JSRegExpResult::NoMatch {
                        return fixed;
                    }
                    let backtrack = self.parentheses_do_backtrack(term, sub, loc, ctx);
                    if backtrack != JSRegExpResult::Match {
                        return backtrack;
                    }
                }
            }
            let context = ctx.paren_mut(loc).contexts.last().cloned().unwrap();
            self.record_parentheses_match(term, &context);
        }

        match quantity_type(term) {
            QType::FixedCount => JSRegExpResult::Match,
            QType::Greedy => {
                while ctx.paren_mut(loc).match_amount < quantity_max(term) {
                    let mut context = self.alloc_paren_context(sub, term);
                    let result = self.match_non_zero_disjunction(sub, &mut context.sub, false);
                    if result == JSRegExpResult::Match {
                        let bt = ctx.paren_mut(loc);
                        bt.contexts.push(context);
                        bt.match_amount += 1;
                    } else {
                        self.restore_output(&context);
                        if result != JSRegExpResult::NoMatch {
                            return result;
                        }
                        break;
                    }
                }
                if ctx.paren_mut(loc).match_amount != 0 {
                    let context = ctx.paren_mut(loc).contexts.last().cloned().unwrap();
                    self.record_parentheses_match(term, &context);
                }
                JSRegExpResult::Match
            }
            QType::NonGreedy => JSRegExpResult::Match,
        }
    }

    /// backtrackParentheses — YarrInterpreter.cpp:1511.
    fn backtrack_parentheses(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) -> JSRegExpResult {
        let loc = frame_loc(term);
        let sub_index = term.parentheses_disjunction.unwrap_or(0) as usize;
        let pattern = self.pattern;
        let sub = &pattern.parentheses[sub_index];

        match quantity_type(term) {
            QType::FixedCount => {
                let result = self.parentheses_do_backtrack(term, sub, loc, ctx);
                if result != JSRegExpResult::Match {
                    return result;
                }
                while ctx.paren_mut(loc).match_amount < quantity_max(term) {
                    let mut context = self.alloc_paren_context(sub, term);
                    let result = self.match_disjunction(sub, &mut context.sub, false);
                    if result == JSRegExpResult::Match {
                        let bt = ctx.paren_mut(loc);
                        bt.contexts.push(context);
                        bt.match_amount += 1;
                    } else {
                        self.restore_output(&context);
                        if result != JSRegExpResult::NoMatch {
                            return result;
                        }
                        let backtrack = self.parentheses_do_backtrack(term, sub, loc, ctx);
                        if backtrack != JSRegExpResult::Match {
                            return backtrack;
                        }
                    }
                }
                let context = ctx.paren_mut(loc).contexts.last().cloned().unwrap();
                self.record_parentheses_match(term, &context);
                JSRegExpResult::Match
            }
            QType::Greedy => {
                if ctx.paren_mut(loc).match_amount == 0 {
                    return JSRegExpResult::NoMatch;
                }
                let mut context = ctx.paren_mut(loc).contexts.last().cloned().unwrap();
                let result = self.match_non_zero_disjunction(sub, &mut context.sub, true);
                if result == JSRegExpResult::Match {
                    *ctx.paren_mut(loc).contexts.last_mut().unwrap() = context;
                    while ctx.paren_mut(loc).match_amount < quantity_max(term) {
                        let mut c2 = self.alloc_paren_context(sub, term);
                        let r2 = self.match_non_zero_disjunction(sub, &mut c2.sub, false);
                        if r2 == JSRegExpResult::Match {
                            let bt = ctx.paren_mut(loc);
                            bt.contexts.push(c2);
                            bt.match_amount += 1;
                        } else {
                            self.restore_output(&c2);
                            if r2 != JSRegExpResult::NoMatch {
                                return r2;
                            }
                            break;
                        }
                    }
                } else {
                    self.restore_output(&context);
                    ctx.paren_mut(loc).contexts.pop();
                    ctx.paren_mut(loc).match_amount -= 1;
                    if ctx.paren_mut(loc).match_amount < term.quantifier.min {
                        while ctx.paren_mut(loc).match_amount != 0 {
                            let c = ctx.paren_mut(loc).contexts.last().cloned().unwrap();
                            self.restore_output(&c);
                            ctx.paren_mut(loc).contexts.pop();
                            ctx.paren_mut(loc).match_amount -= 1;
                        }
                        let begin = ctx.paren_mut(loc).begin;
                        self.input.set_pos(begin);
                        return result;
                    }
                    if result != JSRegExpResult::NoMatch {
                        return result;
                    }
                }
                if ctx.paren_mut(loc).match_amount != 0 {
                    let context = ctx.paren_mut(loc).contexts.last().cloned().unwrap();
                    self.record_parentheses_match(term, &context);
                }
                JSRegExpResult::Match
            }
            QType::NonGreedy => {
                if ctx.paren_mut(loc).match_amount < quantity_max(term) {
                    let mut context = self.alloc_paren_context(sub, term);
                    let result = self.match_non_zero_disjunction(sub, &mut context.sub, false);
                    if result == JSRegExpResult::Match {
                        let bt = ctx.paren_mut(loc);
                        bt.contexts.push(context.clone());
                        bt.match_amount += 1;
                        self.record_parentheses_match(term, &context);
                        return JSRegExpResult::Match;
                    }
                    self.restore_output(&context);
                    if result != JSRegExpResult::NoMatch {
                        return result;
                    }
                }
                while ctx.paren_mut(loc).match_amount != 0 {
                    let mut context = ctx.paren_mut(loc).contexts.last().cloned().unwrap();
                    let result = self.match_non_zero_disjunction(sub, &mut context.sub, true);
                    if result == JSRegExpResult::Match {
                        *ctx.paren_mut(loc).contexts.last_mut().unwrap() = context.clone();
                        self.record_parentheses_match(term, &context);
                        return JSRegExpResult::Match;
                    }
                    self.restore_output(&context);
                    ctx.paren_mut(loc).contexts.pop();
                    ctx.paren_mut(loc).match_amount -= 1;
                    if result != JSRegExpResult::NoMatch {
                        return result;
                    }
                }
                JSRegExpResult::NoMatch
            }
        }
    }

    /// matchNonZeroDisjunction — YarrInterpreter.cpp:2192.
    fn match_non_zero_disjunction(
        &mut self,
        disjunction: &ByteDisjunction,
        ctx: &mut DisjunctionContext,
        btrack: bool,
    ) -> JSRegExpResult {
        let mut result = self.match_disjunction(disjunction, ctx, btrack);
        if result == JSRegExpResult::Match {
            while ctx.match_begin == ctx.match_end {
                result = self.match_disjunction(disjunction, ctx, true);
                if result != JSRegExpResult::Match {
                    return result;
                }
            }
            return JSRegExpResult::Match;
        }
        result
    }

    // ---- the flat dispatch machine (matchDisjunction) ---------------------

    /// matchDisjunction — YarrInterpreter.cpp:1740. Goto-driven MATCH_NEXT()/
    /// BACKTRACK() state machine, translated into a `Phase` loop.
    fn match_disjunction(
        &mut self,
        disjunction: &ByteDisjunction,
        ctx: &mut DisjunctionContext,
        btrack: bool,
    ) -> JSRegExpResult {
        self.recursion_depth += 1;
        if self.recursion_depth > YARR_MAX_RECURSION_DEPTH {
            self.recursion_depth -= 1;
            return JSRegExpResult::HitLimit; // C++ ErrorNoMemory (folded)
        }
        // `if (!--remainingMatchCount) return ErrorHitLimit;`
        self.remaining_match_count = self.remaining_match_count.saturating_sub(1);
        if self.remaining_match_count == 0 {
            self.recursion_depth -= 1;
            return JSRegExpResult::HitLimit;
        }
        let result = self.run_state_machine(disjunction, ctx, btrack);
        self.recursion_depth -= 1;
        result
    }

    fn run_state_machine(
        &mut self,
        disjunction: &ByteDisjunction,
        ctx: &mut DisjunctionContext,
        btrack: bool,
    ) -> JSRegExpResult {
        #[derive(Clone, Copy, PartialEq)]
        enum Phase {
            Match,
            Backtrack,
        }

        let mut phase = if btrack {
            ctx.term -= 1;
            Phase::Backtrack
        } else {
            ctx.match_begin = self.input.get_pos();
            ctx.term = 0;
            Phase::Match
        };

        loop {
            let idx = ctx.term;
            if idx < 0 || idx as usize >= disjunction.terms.len() {
                // C++ RELEASE_ASSERT_NOT_REACHED on out-of-range term.
                return JSRegExpResult::HitLimit;
            }
            let term = &disjunction.terms[idx as usize];
            let kind = term.kind;

            match phase {
                Phase::Match => {
                    use BytecodeTermKind as K;
                    match kind {
                        K::SubpatternBegin => {
                            ctx.term += 1;
                            continue;
                        }
                        K::SubpatternEnd => {
                            ctx.match_end = self.input.get_pos();
                            return JSRegExpResult::Match;
                        }
                        K::BodyAlternativeBegin => {
                            ctx.term += 1;
                            continue;
                        }
                        K::BodyAlternativeDisjunction | K::BodyAlternativeEnd => {
                            ctx.match_end = self.input.get_pos();
                            return JSRegExpResult::Match;
                        }
                        K::AlternativeBegin => {
                            ctx.term += 1;
                            continue;
                        }
                        K::AlternativeDisjunction | K::AlternativeEnd => {
                            let jump = alt_jump(term);
                            let offset = jump.end;
                            ctx.set_num(frame_loc(term), offset as u32);
                            ctx.term += offset;
                            ctx.term += 1;
                            continue;
                        }
                        K::AssertionBol => {
                            if self.match_assertion_bol(term) {
                                ctx.term += 1;
                            } else {
                                ctx.term -= 1;
                                phase = Phase::Backtrack;
                            }
                            continue;
                        }
                        K::AssertionEol => {
                            if self.match_assertion_eol(term) {
                                ctx.term += 1;
                            } else {
                                ctx.term -= 1;
                                phase = Phase::Backtrack;
                            }
                            continue;
                        }
                        K::AssertionWordBoundary => {
                            if self.match_assertion_word_boundary(term) {
                                ctx.term += 1;
                            } else {
                                ctx.term -= 1;
                                phase = Phase::Backtrack;
                            }
                            continue;
                        }
                        K::PatternCharacterOnce | K::PatternCharacterFixed => {
                            let term = term.clone();
                            if self.match_pattern_character_fixed(&term) {
                                ctx.term += 1;
                            } else {
                                ctx.term -= 1;
                                phase = Phase::Backtrack;
                            }
                            continue;
                        }
                        K::PatternCharacterGreedy => {
                            let term = term.clone();
                            self.match_pattern_character_greedy(&term, ctx);
                            ctx.term += 1;
                            continue;
                        }
                        K::PatternCharacterNonGreedy | K::PatternCasedCharacterNonGreedy => {
                            let loc = frame_loc(term);
                            ctx.set_num(loc, self.input.get_pos()); // begin
                            ctx.set_num(loc + 1, 0); // matchAmount
                            ctx.term += 1;
                            continue;
                        }
                        K::PatternCasedCharacterOnce | K::PatternCasedCharacterFixed => {
                            let term = term.clone();
                            if self.match_pattern_cased_character_fixed(&term) {
                                ctx.term += 1;
                            } else {
                                ctx.term -= 1;
                                phase = Phase::Backtrack;
                            }
                            continue;
                        }
                        K::PatternCasedCharacterGreedy => {
                            let term = term.clone();
                            self.match_pattern_cased_character_greedy(&term, ctx);
                            ctx.term += 1;
                            continue;
                        }
                        K::CharacterClass => {
                            let term = term.clone();
                            if self.match_character_class(&term, ctx) {
                                ctx.term += 1;
                            } else {
                                ctx.term -= 1;
                                phase = Phase::Backtrack;
                            }
                            continue;
                        }
                        K::BackReference => {
                            let term = term.clone();
                            if self.match_back_reference(&term, ctx) {
                                ctx.term += 1;
                            } else {
                                ctx.term -= 1;
                                phase = Phase::Backtrack;
                            }
                            continue;
                        }
                        K::ParenthesesSubpattern => {
                            let term = term.clone();
                            let result = self.match_parentheses(&term, ctx);
                            if result == JSRegExpResult::Match {
                                ctx.term += 1;
                                continue;
                            } else if result != JSRegExpResult::NoMatch {
                                return result;
                            }
                            ctx.term -= 1;
                            phase = Phase::Backtrack;
                            continue;
                        }
                        K::ParenthesesSubpatternOnceBegin => {
                            let term = term.clone();
                            let adv = self.match_parentheses_once_begin(&term, ctx);
                            if adv == i32::MIN {
                                ctx.term -= 1;
                                phase = Phase::Backtrack;
                            } else {
                                ctx.term += adv;
                                ctx.term += 1;
                            }
                            continue;
                        }
                        K::ParenthesesSubpatternOnceEnd => {
                            let term = term.clone();
                            if self.match_parentheses_once_end(&term, ctx) {
                                ctx.term += 1;
                            } else {
                                ctx.term -= 1;
                                phase = Phase::Backtrack;
                            }
                            continue;
                        }
                        K::ParenthesesSubpatternTerminalBegin => {
                            let term = term.clone();
                            self.match_parentheses_terminal_begin(&term, ctx);
                            ctx.term += 1;
                            continue;
                        }
                        K::ParenthesesSubpatternTerminalEnd => {
                            let term = term.clone();
                            let rewind = self.match_parentheses_terminal_end(&term, ctx);
                            if rewind == i32::MIN {
                                ctx.term -= 1;
                                phase = Phase::Backtrack;
                            } else {
                                ctx.term -= rewind;
                                ctx.term += 1;
                            }
                            continue;
                        }
                        K::ParentheticalAssertionBegin => {
                            let term = term.clone();
                            self.match_parenthetical_assertion_begin(&term, ctx);
                            ctx.term += 1;
                            continue;
                        }
                        K::ParentheticalAssertionEnd => {
                            let term = term.clone();
                            let adv = self.match_parenthetical_assertion_end(&term, ctx);
                            if adv == i32::MAX {
                                ctx.term += 1;
                            } else {
                                ctx.term -= adv;
                                ctx.term -= 1;
                                phase = Phase::Backtrack;
                            }
                            continue;
                        }
                        K::CheckInput => {
                            let count = term.input_check.map(|c| c.checked_count).unwrap_or(0);
                            if self.input.check_input(count) {
                                ctx.term += 1;
                            } else {
                                ctx.term -= 1;
                                phase = Phase::Backtrack;
                            }
                            continue;
                        }
                        K::UncheckInput => {
                            let count = term.input_check.map(|c| c.checked_count).unwrap_or(0);
                            self.input.uncheck_input(count);
                            ctx.term += 1;
                            continue;
                        }
                        K::HaveCheckedInput => {
                            let count = term.input_check.map(|c| c.checked_count).unwrap_or(0);
                            if self.input.is_valid_negative_input_offset(count) {
                                ctx.term += 1;
                            } else {
                                ctx.term -= 1;
                                phase = Phase::Backtrack;
                            }
                            continue;
                        }
                        K::DotStarEnclosure => {
                            let term = term.clone();
                            if self.match_dot_star_enclosure(&term, ctx) {
                                return JSRegExpResult::Match;
                            }
                            ctx.term -= 1;
                            phase = Phase::Backtrack;
                            continue;
                        }
                    }
                }
                Phase::Backtrack => {
                    use BytecodeTermKind as K;
                    match kind {
                        K::SubpatternBegin => return JSRegExpResult::NoMatch,
                        K::SubpatternEnd => return JSRegExpResult::HitLimit, // NOT_REACHED
                        K::BodyAlternativeBegin | K::BodyAlternativeDisjunction => {
                            let jump = alt_jump(term);
                            let offset = jump.next;
                            ctx.term += offset;
                            if offset > 0 {
                                ctx.term += 1;
                                phase = Phase::Match;
                                continue;
                            }
                            if self.input.at_end() || self.flags.sticky {
                                return JSRegExpResult::NoMatch;
                            }
                            self.input.next();
                            ctx.match_begin = self.input.get_pos();
                            let cur = &disjunction.terms[ctx.term as usize];
                            let cur_jump = alt_jump(cur);
                            if cur_jump.once_through {
                                ctx.term += cur_jump.next;
                            }
                            ctx.term += 1;
                            phase = Phase::Match;
                            continue;
                        }
                        K::BodyAlternativeEnd => return JSRegExpResult::HitLimit, // NOT_REACHED
                        K::AlternativeBegin | K::AlternativeDisjunction => {
                            let jump = alt_jump(term);
                            let offset = jump.next;
                            ctx.term += offset;
                            if offset > 0 {
                                ctx.term += 1;
                                phase = Phase::Match;
                            } else {
                                ctx.term -= 1;
                            }
                            continue;
                        }
                        K::AlternativeEnd => {
                            let offset = ctx.num(frame_loc(term));
                            ctx.term -= offset as i32;
                            ctx.term -= 1;
                            continue;
                        }
                        K::AssertionBol | K::AssertionEol | K::AssertionWordBoundary => {
                            ctx.term -= 1;
                            continue;
                        }
                        K::PatternCharacterOnce
                        | K::PatternCharacterFixed
                        | K::PatternCharacterGreedy
                        | K::PatternCharacterNonGreedy => {
                            let term = term.clone();
                            if self.backtrack_pattern_character(&term, ctx) {
                                ctx.term += 1;
                                phase = Phase::Match;
                            } else {
                                ctx.term -= 1;
                            }
                            continue;
                        }
                        K::PatternCasedCharacterOnce
                        | K::PatternCasedCharacterFixed
                        | K::PatternCasedCharacterGreedy
                        | K::PatternCasedCharacterNonGreedy => {
                            let term = term.clone();
                            if self.backtrack_pattern_cased_character(&term, ctx) {
                                ctx.term += 1;
                                phase = Phase::Match;
                            } else {
                                ctx.term -= 1;
                            }
                            continue;
                        }
                        K::CharacterClass => {
                            let term = term.clone();
                            if self.backtrack_character_class(&term, ctx) {
                                ctx.term += 1;
                                phase = Phase::Match;
                            } else {
                                ctx.term -= 1;
                            }
                            continue;
                        }
                        K::BackReference => {
                            let term = term.clone();
                            if self.backtrack_back_reference(&term, ctx) {
                                ctx.term += 1;
                                phase = Phase::Match;
                            } else {
                                ctx.term -= 1;
                            }
                            continue;
                        }
                        K::ParenthesesSubpattern => {
                            let term = term.clone();
                            let result = self.backtrack_parentheses(&term, ctx);
                            if result == JSRegExpResult::Match {
                                ctx.term += 1;
                                phase = Phase::Match;
                                continue;
                            } else if result != JSRegExpResult::NoMatch {
                                return result;
                            }
                            ctx.term -= 1;
                            continue;
                        }
                        K::ParenthesesSubpatternOnceBegin => {
                            let term = term.clone();
                            let adv = self.backtrack_parentheses_once_begin(&term, ctx);
                            if adv == i32::MIN {
                                ctx.term -= 1;
                            } else {
                                ctx.term += adv;
                                ctx.term += 1;
                                phase = Phase::Match;
                            }
                            continue;
                        }
                        K::ParenthesesSubpatternOnceEnd => {
                            let term = term.clone();
                            let code = self.backtrack_parentheses_once_end(&term, ctx);
                            if code == i32::MIN {
                                ctx.term -= 1;
                            } else if code <= -2 {
                                // Greedy notFound: term -= width, return false.
                                let width = -(code + 2);
                                ctx.term -= width;
                                ctx.term -= 1;
                            } else {
                                // success: term -= width, return true.
                                ctx.term -= code;
                                ctx.term += 1;
                                phase = Phase::Match;
                            }
                            continue;
                        }
                        K::ParenthesesSubpatternTerminalBegin => {
                            // backtrackParenthesesTerminalBegin — :1321. Always
                            // "succeeds" as a match: term += parenthesesWidth.
                            let width = parentheses_width(term) as i32;
                            ctx.term += width;
                            ctx.term += 1;
                            phase = Phase::Match;
                            continue;
                        }
                        K::ParenthesesSubpatternTerminalEnd => {
                            return JSRegExpResult::HitLimit; // NOT_REACHED
                        }
                        K::ParentheticalAssertionBegin => {
                            let term = term.clone();
                            let adv = self.backtrack_parenthetical_assertion_begin(&term, ctx);
                            if adv == i32::MIN {
                                ctx.term -= 1;
                            } else {
                                ctx.term += adv;
                                ctx.term += 1;
                                phase = Phase::Match;
                            }
                            continue;
                        }
                        K::ParentheticalAssertionEnd => {
                            let term = term.clone();
                            let adv = self.backtrack_parenthetical_assertion_end(&term, ctx);
                            ctx.term -= adv;
                            ctx.term -= 1;
                            continue;
                        }
                        K::CheckInput => {
                            let count = term.input_check.map(|c| c.checked_count).unwrap_or(0);
                            self.input.uncheck_input(count);
                            ctx.term -= 1;
                            continue;
                        }
                        K::UncheckInput => {
                            let count = term.input_check.map(|c| c.checked_count).unwrap_or(0);
                            self.input.check_input(count);
                            ctx.term -= 1;
                            continue;
                        }
                        K::HaveCheckedInput => {
                            ctx.term -= 1;
                            continue;
                        }
                        K::DotStarEnclosure => return JSRegExpResult::HitLimit, // NOT_REACHED
                    }
                }
            }
        }
    }

    /// PatternCharacterOnce/Fixed match arm — YarrInterpreter.cpp:1804 (Forward,
    /// legacy/BMP path; Unicode surrogate atoms handled via checkSurrogatePair).
    fn match_pattern_character_fixed(&mut self, term: &BytecodeTerm) -> bool {
        if term.direction == MatchDirection::Forward {
            if self.flags.either_unicode {
                if let Some(c) = term.character {
                    if (c as u32) > 0xFFFF {
                        for match_amount in 0..quantity_max(term) {
                            if !self
                                .check_surrogate_pair(term, term.input_position - 2 * match_amount)
                            {
                                return false;
                            }
                        }
                        return true;
                    }
                }
            }
            let position = self.input.get_pos();
            for match_amount in 0..quantity_max(term) {
                if !self.check_character(term, term.input_position - match_amount) {
                    self.input.set_pos(position);
                    return false;
                }
            }
            true
        } else {
            // Backward (lookbehind) char atoms — deferred with the Backward
            // machinery; see serial-coupling notes.
            if self.input.get_pos() < term.input_position {
                return false;
            }
            let position = self.input.get_pos();
            let qmax = quantity_max(term);
            for match_amount in 0..qmax {
                if !self.check_character(term, term.input_position + match_amount + 1 - qmax) {
                    self.input.set_pos(position);
                    return false;
                }
            }
            true
        }
    }

    /// PatternCharacterGreedy match arm — YarrInterpreter.cpp:1856 (Forward).
    fn match_pattern_character_greedy(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) {
        let loc = frame_loc(term);
        let mut match_amount = 0;
        let mut position = self.input.get_pos();
        while match_amount < quantity_max(term) && self.input.check_input(1) {
            if !self.check_character(term, term.input_position + 1) {
                self.input.set_pos(position);
                break;
            }
            match_amount += 1;
            position = self.input.get_pos();
        }
        ctx.set_num(loc + 1, match_amount);
    }

    /// PatternCasedCharacterOnce/Fixed match arm — YarrInterpreter.cpp:1901
    /// (legacy path; Unicode case-insensitive folds into CharacterClass).
    fn match_pattern_cased_character_fixed(&mut self, term: &BytecodeTerm) -> bool {
        for match_amount in 0..quantity_max(term) {
            if !self.check_cased_character(term, term.input_position - match_amount) {
                return false;
            }
        }
        true
    }

    /// PatternCasedCharacterGreedy match arm — YarrInterpreter.cpp:1939 (Forward).
    fn match_pattern_cased_character_greedy(
        &mut self,
        term: &BytecodeTerm,
        ctx: &mut DisjunctionContext,
    ) {
        let loc = frame_loc(term);
        let mut match_amount = 0;
        while match_amount < quantity_max(term) && self.input.check_input(1) {
            if !self.check_cased_character(term, term.input_position + 1) {
                self.input.uncheck_input(1);
                break;
            }
            match_amount += 1;
        }
        ctx.set_num(loc + 1, match_amount);
    }
}

#[inline]
fn frame_loc(term: &BytecodeTerm) -> u32 {
    // C++ `term.frameLocation`; the descriptor carries it inside the optional
    // backtracking-frame reservation `YarrBacktrackFrame`.
    term.frame.as_ref().map(|f| f.frame_location).unwrap_or(0)
}

#[inline]
fn alt_jump(term: &BytecodeTerm) -> BytecodeAlternativeJump {
    term.alternative_jump.unwrap_or(BytecodeAlternativeJump {
        next: 0,
        end: 0,
        once_through: false,
    })
}

#[inline]
fn parentheses_width(term: &BytecodeTerm) -> u32 {
    // C++ `atom.parenthesesWidth` (endTerm - beginTerm).
    term.parentheses_width.unwrap_or(0)
}

trait DotStarAnchors {
    fn dot_star_bol(&self) -> bool;
    fn dot_star_eol(&self) -> bool;
}

impl DotStarAnchors for BytecodeTerm {
    fn dot_star_bol(&self) -> bool {
        self.dot_star_anchors.map(|(b, _)| b).unwrap_or(false)
    }
    fn dot_star_eol(&self) -> bool {
        self.dot_star_anchors.map(|(_, e)| e).unwrap_or(false)
    }
}

#[inline]
fn class_has_only_non_bmp(class: &CharacterClassDescriptor) -> bool {
    matches!(class.width, crate::yarr::CharacterClassWidth::NonBmpOnly)
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

#[cfg(test)]
mod interp_tests {
    //! Faithful-behavior tests for the flat Yarr interpreter and the
    //! ByteCompiler. Hand-built `BytecodeTerm` fixtures pin the C++ matchAgain /
    //! backtrack handlers exactly; each case is cross-checked against the C++
    //! `jsc` oracle (output recorded in the commit message), and every fixture
    //! cites the YarrInterpreter.{h,cpp} lines it exercises.
    use super::*;
    use crate::yarr::{
        assemble_yarr_bytecode_plan, execute_regexp_bytecode, ByteDisjunction,
        BytecodeAlternativeJump, BytecodeInputCheck, BytecodeOffsetVectorLayout, BytecodePattern,
        BytecodePatternId, BytecodeSubpatternRange, BytecodeTerm, BytecodeTermBuilder,
        BytecodeTermId, BytecodeTermKind as K, CharacterClassDescriptor, CharacterClassWidth,
        CharacterRange, CompileMode, PatternAlternative, PatternAssertion, PatternDisjunction,
        PatternTerm, PatternTermKind, Quantifier, QuantifierKind, RegexFlags, YarrErrorCode,
        YarrPattern, YarrPatternId,
    };

    fn u16s(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn mk(kind: BytecodeTermKind) -> BytecodeTerm {
        BytecodeTermBuilder::new(BytecodeTermId(0), kind, RegexFlags::default()).build_unchecked()
    }

    fn fixed1() -> Quantifier {
        Quantifier {
            kind: QuantifierKind::FixedCount,
            min: 1,
            max: Some(1),
        }
    }
    fn greedy_inf() -> Quantifier {
        Quantifier {
            kind: QuantifierKind::Greedy,
            min: 0,
            max: None,
        }
    }
    fn nongreedy_inf() -> Quantifier {
        Quantifier {
            kind: QuantifierKind::NonGreedy,
            min: 0,
            max: None,
        }
    }

    fn body_begin() -> BytecodeTerm {
        let mut t = mk(K::BodyAlternativeBegin);
        t.alternative_jump = Some(BytecodeAlternativeJump {
            next: 0,
            end: 0,
            once_through: false,
        });
        t
    }
    fn body_end() -> BytecodeTerm {
        let mut t = mk(K::BodyAlternativeEnd);
        t.alternative_jump = Some(BytecodeAlternativeJump {
            next: 0,
            end: 0,
            once_through: false,
        });
        t
    }
    fn check_input(n: u32) -> BytecodeTerm {
        let mut t = mk(K::CheckInput);
        t.input_check = Some(BytecodeInputCheck { checked_count: n });
        t
    }
    fn char_once(ch: char, input_pos: u32) -> BytecodeTerm {
        let mut t = mk(K::PatternCharacterOnce);
        t.character = Some(ch);
        t.input_position = input_pos;
        t.quantifier = fixed1();
        t
    }
    fn char_greedy(ch: char, input_pos: u32) -> BytecodeTerm {
        let mut t = mk(K::PatternCharacterGreedy);
        t.character = Some(ch);
        t.input_position = input_pos;
        t.quantifier = greedy_inf();
        t
    }
    fn char_nongreedy(ch: char, input_pos: u32) -> BytecodeTerm {
        let mut t = mk(K::PatternCharacterNonGreedy);
        t.character = Some(ch);
        t.input_position = input_pos;
        t.quantifier = nongreedy_inf();
        t
    }
    fn class_chars(chars: &[char]) -> CharacterClassDescriptor {
        CharacterClassDescriptor {
            built_in: None,
            matches: chars.to_vec(),
            ranges: Vec::new(),
            unicode_matches: Vec::new(),
            unicode_ranges: Vec::new(),
            strings: Vec::new(),
            inverted: false,
            table_inverted: false,
            any_character: false,
            width: CharacterClassWidth::BmpOnly,
            operation: None,
            in_canonical_form: false,
        }
    }
    fn class_greedy(chars: &[char], input_pos: u32) -> BytecodeTerm {
        let mut t = mk(K::CharacterClass);
        t.character_class = Some(class_chars(chars));
        t.input_position = input_pos;
        t.quantifier = greedy_inf();
        t
    }

    fn make_pattern(
        mut terms: Vec<BytecodeTerm>,
        frame_size: u32,
        subpattern_count: u32,
        parentheses: Vec<ByteDisjunction>,
    ) -> BytecodePattern {
        for (i, term) in terms.iter_mut().enumerate() {
            term.id = BytecodeTermId(i as u32);
        }
        let offsets_size = (subpattern_count + 1) * 2;
        BytecodePattern {
            id: BytecodePatternId(1),
            pattern: YarrPatternId(1),
            body: ByteDisjunction {
                terms: terms.clone(),
                subpattern_count,
                frame_size,
            },
            parentheses,
            alternatives: Vec::new(),
            terms,
            frame_size,
            minimum_size: None,
            contains_bol: false,
            contains_eol: false,
            caches: Vec::new(),
            offset_vector: BytecodeOffsetVectorLayout {
                base_for_named_captures: offsets_size,
                offsets_size,
                duplicate_named_group_count: 0,
            },
            duplicate_named_group_for_subpattern: Vec::new(),
        }
    }

    // ---- greedy vs lazy quantifiers (YarrInterpreter.cpp:1856, :1893, :730) ----

    #[test]
    fn greedy_star_matches_maximal_then_zero() {
        // /a*/ : PatternCharacterGreedy, no checkInput, inputPosition 0.
        let p = make_pattern(
            vec![body_begin(), char_greedy('a', 0), body_end()],
            2,
            0,
            vec![],
        );
        let r = interpret_bytecode(&p, &u16s("aaa"), 0);
        assert_eq!(r.result, JSRegExpResult::Match);
        assert_eq!((r.output[0], r.output[1]), (0, 3)); // oracle: "aaa"
        let r2 = interpret_bytecode(&p, &u16s("baa"), 0);
        assert_eq!((r2.output[0], r2.output[1]), (0, 0)); // greedy matches zero a's
    }

    #[test]
    fn lazy_star_matches_minimal() {
        // /a*?/ : PatternCharacterNonGreedy. Lazy => zero-length match.
        let p = make_pattern(
            vec![body_begin(), char_nongreedy('a', 0), body_end()],
            2,
            0,
            vec![],
        );
        let r = interpret_bytecode(&p, &u16s("aaa"), 0);
        assert_eq!(r.result, JSRegExpResult::Match);
        assert_eq!((r.output[0], r.output[1]), (0, 0)); // oracle: ""
    }

    #[test]
    fn lazy_star_grows_on_backtrack() {
        // /a*?b/ : NonGreedy 'a' grows one code unit per backtrack until 'b'
        // matches (backtrackPatternCharacter NonGreedy, YarrInterpreter.cpp:751).
        let p = make_pattern(
            vec![
                body_begin(),
                check_input(1),
                char_nongreedy('a', 1),
                char_once('b', 1),
                body_end(),
            ],
            2,
            0,
            vec![],
        );
        assert_eq!(
            span(&interpret_bytecode(&p, &u16s("aab"), 0)),
            Some((0, 3)) // oracle: "aab"
        );
        assert_eq!(
            span(&interpret_bytecode(&p, &u16s("ab"), 0)),
            Some((0, 2)) // oracle: "ab"
        );
    }

    #[test]
    fn greedy_class_gives_back_on_backtrack() {
        // /[ab]*ab/ on "abb" : greedy [ab] over-matches then backtracks one so the
        // trailing "ab" matches (backtrackCharacterClass Greedy + matchAgain).
        let p = make_pattern(
            vec![
                body_begin(),
                check_input(2),
                class_greedy(&['a', 'b'], 2),
                char_once('a', 2),
                char_once('b', 1),
                body_end(),
            ],
            2,
            0,
            vec![],
        );
        assert_eq!(
            span(&interpret_bytecode(&p, &u16s("abb"), 0)),
            Some((0, 2)) // oracle: "ab"
        );
        assert_eq!(
            span(&interpret_bytecode(&p, &u16s("abab"), 0)),
            Some((0, 4)) // oracle: "abab"
        );
    }

    fn span(o: &YarrInterpretOutcome) -> Option<(u32, u32)> {
        if o.result == JSRegExpResult::Match {
            Some((o.output[0], o.output[1]))
        } else {
            None
        }
    }

    // ---- capturing parentheses (ParenthesesSubpatternOnce, :1168/:1199) ----

    #[test]
    fn capturing_once_records_group_offsets() {
        // /(a)/ : ParenthesesSubpatternOnceBegin/End (FixedCount, capture). The
        // single-alternative group elides AlternativeBegin/End.
        let mut once_begin = mk(K::ParenthesesSubpatternOnceBegin);
        once_begin.subpattern_id = Some(1);
        once_begin.capture = true;
        once_begin.input_position = 1;
        once_begin.parentheses_width = Some(2);
        once_begin.quantifier = fixed1();
        let mut once_end = mk(K::ParenthesesSubpatternOnceEnd);
        once_end.subpattern_id = Some(1);
        once_end.capture = true;
        once_end.input_position = 0;
        once_end.parentheses_width = Some(2);
        once_end.quantifier = fixed1();
        let p = make_pattern(
            vec![
                body_begin(),
                check_input(1),
                once_begin,
                char_once('a', 1),
                once_end,
                body_end(),
            ],
            0,
            1,
            vec![],
        );
        let r = interpret_bytecode(&p, &u16s("a"), 0);
        assert_eq!(r.result, JSRegExpResult::Match);
        assert_eq!((r.output[0], r.output[1]), (0, 1)); // overall "a"
        assert_eq!((r.output[2], r.output[3]), (0, 1)); // group 1 "a"
    }

    // ---- back-reference (matchBackReference, YarrInterpreter.cpp:998) ----

    #[test]
    fn backreference_matches_captured_text() {
        // /(a)\1/ on "aa" : the group captures "a"; \1 consumes the next "a"
        // forward via tryConsumeBackReference.
        let mut once_begin = mk(K::ParenthesesSubpatternOnceBegin);
        once_begin.subpattern_id = Some(1);
        once_begin.capture = true;
        once_begin.input_position = 1;
        once_begin.parentheses_width = Some(2);
        once_begin.quantifier = fixed1();
        let mut once_end = mk(K::ParenthesesSubpatternOnceEnd);
        once_end.subpattern_id = Some(1);
        once_end.capture = true;
        once_end.input_position = 0;
        once_end.parentheses_width = Some(2);
        once_end.quantifier = fixed1();
        let mut backref = mk(K::BackReference);
        backref.subpattern_id = Some(1);
        backref.input_position = 0;
        backref.quantifier = fixed1();
        let p = make_pattern(
            vec![
                body_begin(),
                check_input(1),
                once_begin,
                char_once('a', 1),
                once_end,
                backref,
                body_end(),
            ],
            3,
            1,
            vec![],
        );
        let r = interpret_bytecode(&p, &u16s("aa"), 0);
        assert_eq!(r.result, JSRegExpResult::Match);
        assert_eq!((r.output[0], r.output[1]), (0, 2)); // overall "aa"
        assert_eq!((r.output[2], r.output[3]), (0, 1)); // group 1 "a"
                                                        // \1 against "ab" cannot consume "a" at position 1 -> no match.
        assert_eq!(span(&interpret_bytecode(&p, &u16s("ab"), 0)), None);
    }

    // ---- lookahead (ParentheticalAssertion, YarrInterpreter.cpp:1342/:1354) ----

    fn lookahead_pattern(invert: bool) -> BytecodePattern {
        // /a(?=b)/ or /a(?!b)/. Assertion content = checkInput(1) + 'b'@inputPos1.
        let mut a_begin = mk(K::ParentheticalAssertionBegin);
        a_begin.parentheses_width = Some(3);
        a_begin.invert = invert;
        a_begin.quantifier = fixed1();
        let mut a_end = mk(K::ParentheticalAssertionEnd);
        a_end.parentheses_width = Some(3);
        a_end.invert = invert;
        a_end.quantifier = fixed1();
        make_pattern(
            vec![
                body_begin(),
                check_input(1),
                char_once('a', 1),
                a_begin,
                check_input(1),
                char_once('b', 1),
                a_end,
                body_end(),
            ],
            1,
            0,
            vec![],
        )
    }

    #[test]
    fn positive_lookahead() {
        let p = lookahead_pattern(false);
        assert_eq!(span(&interpret_bytecode(&p, &u16s("ab"), 0)), Some((0, 1)));
        assert_eq!(span(&interpret_bytecode(&p, &u16s("ac"), 0)), None);
    }

    #[test]
    fn negative_lookahead() {
        let p = lookahead_pattern(true);
        assert_eq!(span(&interpret_bytecode(&p, &u16s("ac"), 0)), Some((0, 1)));
        assert_eq!(span(&interpret_bytecode(&p, &u16s("ab"), 0)), None);
    }

    // ---- HitLimit (YarrInterpreter.cpp:1745 !--remainingMatchCount) ----

    #[test]
    fn match_limit_returns_hit_limit() {
        let p = make_pattern(
            vec![body_begin(), char_once('a', 0), body_end()],
            0,
            0,
            vec![],
        );
        // First matchDisjunction entry decrements the limit to zero.
        let r = interpret_bytecode_with_limit(&p, &u16s("a"), 0, 1);
        assert_eq!(r.result, JSRegExpResult::HitLimit);
    }

    // ---- end-to-end: ByteCompiler -> interpreter ----

    fn flags() -> RegexFlags {
        RegexFlags::default()
    }

    fn pterm(kind: PatternTermKind, character: Option<char>, input_position: u32) -> PatternTerm {
        PatternTerm {
            kind,
            input_position,
            character,
            character_class: None,
            parentheses: None,
            dot_star_anchors: None,
            capture: false,
            invert: false,
            subpattern_id: None,
            name: None,
            flags: flags(),
            quantity_type: crate::yarr::QuantifierType::FixedCount,
            quantity_min_count: 1,
            quantity_max_count: 1,
            frame_location: 0,
            match_direction: MatchDirection::Forward,
        }
    }

    fn alt(terms: Vec<PatternTerm>, minimum_size: u32, once_through: bool) -> PatternAlternative {
        PatternAlternative {
            terms,
            minimum_size: Some(minimum_size),
            first_subpattern_id: 0,
            last_subpattern_id: 0,
            direction: MatchDirection::Forward,
            once_through,
            has_fixed_size: true,
            starts_with_bol: false,
            contains_bol: false,
            is_last_alternative: true,
            contains_captures: false,
        }
    }

    fn yarr_pattern(
        alts: Vec<PatternAlternative>,
        minimum_size: u32,
        contains_bol: bool,
    ) -> YarrPattern {
        YarrPattern {
            id: YarrPatternId(1),
            source: crate::strings::StringId(1),
            flags: flags(),
            compile_mode: CompileMode::Legacy,
            disjunctions: vec![PatternDisjunction {
                alternatives: alts,
                parent_subpattern: None,
                is_body: true,
                minimum_size: Some(minimum_size),
                call_frame_size: 0,
                has_fixed_size: true,
            }],
            capture_count: 0,
            named_capture_count: 0,
            duplicate_named_capture_count: 0,
            contains_backreferences: false,
            contains_bol,
            contains_lookbehinds: false,
            contains_unsigned_length_pattern: false,
            has_copied_parentheses: false,
            save_initial_start_value: false,
            error: YarrErrorCode::NoError,
        }
    }

    fn compile(pattern: &YarrPattern) -> BytecodePattern {
        assemble_yarr_bytecode_plan(pattern, BytecodePatternId(1), 0)
            .unwrap()
            .pattern
    }

    #[test]
    fn compiled_literal_searches_for_match() {
        // /abc/ on "zabc" : BodyAlternativeBegin search advances start until "abc"
        // matches at index 1 (matchAgain BodyAlternative retry, :2068).
        let pattern = yarr_pattern(
            vec![alt(
                vec![
                    pterm(PatternTermKind::PatternCharacter, Some('a'), 0),
                    pterm(PatternTermKind::PatternCharacter, Some('b'), 1),
                    pterm(PatternTermKind::PatternCharacter, Some('c'), 2),
                ],
                3,
                false,
            )],
            3,
            false,
        );
        let p = compile(&pattern);
        assert_eq!(
            span(&interpret_bytecode(&p, &u16s("zabc"), 0)),
            Some((1, 4))
        );
        assert_eq!(span(&interpret_bytecode(&p, &u16s("zzz"), 0)), None);
    }

    #[test]
    fn compiled_alternation() {
        // /a|bc/ : two body alternatives linked by closeBodyAlternative.
        let pattern = yarr_pattern(
            vec![
                alt(
                    vec![pterm(PatternTermKind::PatternCharacter, Some('a'), 0)],
                    1,
                    false,
                ),
                alt(
                    vec![
                        pterm(PatternTermKind::PatternCharacter, Some('b'), 0),
                        pterm(PatternTermKind::PatternCharacter, Some('c'), 1),
                    ],
                    2,
                    false,
                ),
            ],
            1,
            false,
        );
        let p = compile(&pattern);
        assert_eq!(span(&interpret_bytecode(&p, &u16s("bc"), 0)), Some((0, 2)));
        assert_eq!(span(&interpret_bytecode(&p, &u16s("za"), 0)), Some((1, 2)));
    }

    #[test]
    fn compiled_anchors() {
        // /^a/ matches only at start; /a$/ matches a at end.
        let bol = yarr_pattern(
            vec![alt(
                vec![
                    pterm(PatternTermKind::Assertion(PatternAssertion::Bol), None, 0),
                    pterm(PatternTermKind::PatternCharacter, Some('a'), 0),
                ],
                1,
                false,
            )],
            1,
            true,
        );
        let p = compile(&bol);
        assert_eq!(span(&interpret_bytecode(&p, &u16s("a"), 0)), Some((0, 1)));
        assert_eq!(span(&interpret_bytecode(&p, &u16s("ba"), 0)), None);

        let eol = yarr_pattern(
            vec![alt(
                vec![
                    pterm(PatternTermKind::PatternCharacter, Some('a'), 0),
                    pterm(PatternTermKind::Assertion(PatternAssertion::Eol), None, 1),
                ],
                1,
                false,
            )],
            1,
            false,
        );
        let p = compile(&eol);
        assert_eq!(span(&interpret_bytecode(&p, &u16s("ba"), 0)), Some((1, 2)));
    }

    #[test]
    fn compiled_word_boundary() {
        // /a\b/ : matches 'a' only when followed by a non-word boundary.
        let pattern = yarr_pattern(
            vec![alt(
                vec![
                    pterm(PatternTermKind::PatternCharacter, Some('a'), 0),
                    pterm(
                        PatternTermKind::Assertion(PatternAssertion::WordBoundary),
                        None,
                        1,
                    ),
                ],
                1,
                false,
            )],
            1,
            false,
        );
        let p = compile(&pattern);
        assert_eq!(span(&interpret_bytecode(&p, &u16s("a b"), 0)), Some((0, 1)));
        assert_eq!(span(&interpret_bytecode(&p, &u16s("ab"), 0)), None);
    }

    #[test]
    fn compiled_character_class_range() {
        // /[a-z]/ on "5x" : single FixedCount class, found at index 1.
        let mut class_term = pterm(PatternTermKind::CharacterClass, None, 0);
        class_term.character_class = Some(CharacterClassDescriptor {
            built_in: None,
            matches: Vec::new(),
            ranges: vec![CharacterRange {
                begin: 'a',
                end: 'z',
            }],
            unicode_matches: Vec::new(),
            unicode_ranges: Vec::new(),
            strings: Vec::new(),
            inverted: false,
            table_inverted: false,
            any_character: false,
            width: CharacterClassWidth::BmpOnly,
            operation: None,
            in_canonical_form: false,
        });
        let pattern = yarr_pattern(vec![alt(vec![class_term], 1, false)], 1, false);
        let p = compile(&pattern);
        assert_eq!(span(&interpret_bytecode(&p, &u16s("5x"), 0)), Some((1, 2)));
    }

    #[test]
    fn execute_bridge_lifts_capture_ranges() {
        // execute_regexp_bytecode lifts the raw output vector into a MatchResult.
        let mut once_begin = mk(K::ParenthesesSubpatternOnceBegin);
        once_begin.subpattern_id = Some(1);
        once_begin.capture = true;
        once_begin.input_position = 1;
        once_begin.parentheses_width = Some(2);
        once_begin.quantifier = fixed1();
        let mut once_end = mk(K::ParenthesesSubpatternOnceEnd);
        once_end.subpattern_id = Some(1);
        once_end.capture = true;
        once_end.input_position = 0;
        once_end.parentheses_width = Some(2);
        once_end.quantifier = fixed1();
        let p = make_pattern(
            vec![
                body_begin(),
                check_input(1),
                once_begin,
                char_once('a', 1),
                once_end,
                body_end(),
            ],
            0,
            1,
            vec![],
        );
        let result = execute_regexp_bytecode(&p, &u16s("a"), 0);
        assert_eq!(result.status, MatchStatus::Match);
        assert_eq!(result.overall, Some(MatchRange { start: 0, end: 1 }));
        assert_eq!(result.captures, vec![Some(MatchRange { start: 0, end: 1 })]);
    }
}
