//! Yarr bytecode descriptors.
//!
//! Bytecode here is a descriptive IR contract. The interpreter and compiler
//! that will execute or emit it are intentionally absent.

use crate::strings::StringId;
use crate::yarr::{
    CharacterClassDescriptor, MatchDirection, PatternAssertion, PatternDisjunction, PatternTerm,
    PatternTermKind, RegexFlags, YarrPattern, YarrPatternId,
};

/// Stable identity for a bytecode pattern.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BytecodePatternId(pub u64);

/// Stable identity for one bytecode term.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BytecodeTermId(pub u32);

/// Quantifier family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuantifierKind {
    FixedCount,
    Greedy,
    NonGreedy,
    Infinite,
}

/// Quantifier bounds. `max` is absent for an unbounded repetition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Quantifier {
    pub kind: QuantifierKind,
    pub min: u32,
    pub max: Option<u32>,
}

/// Bytecode term category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BytecodeTermKind {
    BodyAlternativeBegin,
    BodyAlternativeDisjunction,
    BodyAlternativeEnd,
    AlternativeBegin,
    AlternativeDisjunction,
    AlternativeEnd,
    SubpatternBegin,
    SubpatternEnd,
    AssertionBol,
    AssertionEol,
    AssertionWordBoundary,
    PatternCharacterOnce,
    PatternCharacterFixed,
    PatternCharacterGreedy,
    PatternCharacterNonGreedy,
    PatternCasedCharacterOnce,
    PatternCasedCharacterFixed,
    PatternCasedCharacterGreedy,
    PatternCasedCharacterNonGreedy,
    CharacterClass,
    BackReference,
    ParenthesesSubpattern,
    ParenthesesSubpatternOnceBegin,
    ParenthesesSubpatternOnceEnd,
    ParenthesesSubpatternTerminalBegin,
    ParenthesesSubpatternTerminalEnd,
    ParentheticalAssertionBegin,
    ParentheticalAssertionEnd,
    CheckInput,
    UncheckInput,
    HaveCheckedInput,
    DotStarEnclosure,
}

/// Immutable schema category for data carried by a bytecode term.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BytecodeTermPayloadKind {
    None,
    Character,
    CasedCharacterRange,
    CharacterClass,
    BackReference,
    Subpattern,
    SubpatternRange,
    AlternativeJump,
    InputCheck,
    DotStarEnclosure,
}

/// Component that owns a bytecode schema row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BytecodeSchemaOwner {
    Parser,
    BytecodeGenerator,
    Interpreter,
    Jit,
}

/// Static schema for one Yarr bytecode term kind.
///
/// The bytecode generator owns registry mutation when generated metadata is
/// refreshed. Parser, interpreter, and JIT code may only borrow this table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BytecodeTermSchemaDescriptor {
    pub kind: BytecodeTermKind,
    pub payload: BytecodeTermPayloadKind,
    pub owner: BytecodeSchemaOwner,
    pub may_quantify: bool,
    pub may_capture: bool,
    pub changes_input_position: bool,
}

const BYTECODE_TERM_SCHEMA: &[BytecodeTermSchemaDescriptor] = &[
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::BodyAlternativeBegin,
        payload: BytecodeTermPayloadKind::AlternativeJump,
        owner: BytecodeSchemaOwner::BytecodeGenerator,
        may_quantify: false,
        may_capture: false,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::BodyAlternativeDisjunction,
        payload: BytecodeTermPayloadKind::AlternativeJump,
        owner: BytecodeSchemaOwner::BytecodeGenerator,
        may_quantify: false,
        may_capture: false,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::BodyAlternativeEnd,
        payload: BytecodeTermPayloadKind::AlternativeJump,
        owner: BytecodeSchemaOwner::BytecodeGenerator,
        may_quantify: false,
        may_capture: false,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::AlternativeBegin,
        payload: BytecodeTermPayloadKind::AlternativeJump,
        owner: BytecodeSchemaOwner::BytecodeGenerator,
        may_quantify: false,
        may_capture: false,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::AlternativeDisjunction,
        payload: BytecodeTermPayloadKind::AlternativeJump,
        owner: BytecodeSchemaOwner::BytecodeGenerator,
        may_quantify: false,
        may_capture: false,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::AlternativeEnd,
        payload: BytecodeTermPayloadKind::AlternativeJump,
        owner: BytecodeSchemaOwner::BytecodeGenerator,
        may_quantify: false,
        may_capture: false,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::SubpatternBegin,
        payload: BytecodeTermPayloadKind::Subpattern,
        owner: BytecodeSchemaOwner::Parser,
        may_quantify: false,
        may_capture: true,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::SubpatternEnd,
        payload: BytecodeTermPayloadKind::Subpattern,
        owner: BytecodeSchemaOwner::Parser,
        may_quantify: false,
        may_capture: true,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::AssertionBol,
        payload: BytecodeTermPayloadKind::None,
        owner: BytecodeSchemaOwner::Parser,
        may_quantify: false,
        may_capture: false,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::AssertionEol,
        payload: BytecodeTermPayloadKind::None,
        owner: BytecodeSchemaOwner::Parser,
        may_quantify: false,
        may_capture: false,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::AssertionWordBoundary,
        payload: BytecodeTermPayloadKind::None,
        owner: BytecodeSchemaOwner::Parser,
        may_quantify: false,
        may_capture: false,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::PatternCharacterOnce,
        payload: BytecodeTermPayloadKind::Character,
        owner: BytecodeSchemaOwner::BytecodeGenerator,
        may_quantify: true,
        may_capture: false,
        changes_input_position: true,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::PatternCharacterFixed,
        payload: BytecodeTermPayloadKind::Character,
        owner: BytecodeSchemaOwner::BytecodeGenerator,
        may_quantify: true,
        may_capture: false,
        changes_input_position: true,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::PatternCharacterGreedy,
        payload: BytecodeTermPayloadKind::Character,
        owner: BytecodeSchemaOwner::BytecodeGenerator,
        may_quantify: true,
        may_capture: false,
        changes_input_position: true,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::PatternCharacterNonGreedy,
        payload: BytecodeTermPayloadKind::Character,
        owner: BytecodeSchemaOwner::BytecodeGenerator,
        may_quantify: true,
        may_capture: false,
        changes_input_position: true,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::PatternCasedCharacterOnce,
        payload: BytecodeTermPayloadKind::CasedCharacterRange,
        owner: BytecodeSchemaOwner::BytecodeGenerator,
        may_quantify: true,
        may_capture: false,
        changes_input_position: true,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::PatternCasedCharacterFixed,
        payload: BytecodeTermPayloadKind::CasedCharacterRange,
        owner: BytecodeSchemaOwner::BytecodeGenerator,
        may_quantify: true,
        may_capture: false,
        changes_input_position: true,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::PatternCasedCharacterGreedy,
        payload: BytecodeTermPayloadKind::CasedCharacterRange,
        owner: BytecodeSchemaOwner::BytecodeGenerator,
        may_quantify: true,
        may_capture: false,
        changes_input_position: true,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::PatternCasedCharacterNonGreedy,
        payload: BytecodeTermPayloadKind::CasedCharacterRange,
        owner: BytecodeSchemaOwner::BytecodeGenerator,
        may_quantify: true,
        may_capture: false,
        changes_input_position: true,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::CharacterClass,
        payload: BytecodeTermPayloadKind::CharacterClass,
        owner: BytecodeSchemaOwner::BytecodeGenerator,
        may_quantify: true,
        may_capture: false,
        changes_input_position: true,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::BackReference,
        payload: BytecodeTermPayloadKind::BackReference,
        owner: BytecodeSchemaOwner::Parser,
        may_quantify: true,
        may_capture: false,
        changes_input_position: true,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::ParenthesesSubpattern,
        payload: BytecodeTermPayloadKind::SubpatternRange,
        owner: BytecodeSchemaOwner::Parser,
        may_quantify: true,
        may_capture: true,
        changes_input_position: true,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::ParenthesesSubpatternOnceBegin,
        payload: BytecodeTermPayloadKind::SubpatternRange,
        owner: BytecodeSchemaOwner::Parser,
        may_quantify: true,
        may_capture: true,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::ParenthesesSubpatternOnceEnd,
        payload: BytecodeTermPayloadKind::SubpatternRange,
        owner: BytecodeSchemaOwner::Parser,
        may_quantify: true,
        may_capture: true,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::ParenthesesSubpatternTerminalBegin,
        payload: BytecodeTermPayloadKind::SubpatternRange,
        owner: BytecodeSchemaOwner::Parser,
        may_quantify: true,
        may_capture: true,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::ParenthesesSubpatternTerminalEnd,
        payload: BytecodeTermPayloadKind::SubpatternRange,
        owner: BytecodeSchemaOwner::Parser,
        may_quantify: true,
        may_capture: true,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::ParentheticalAssertionBegin,
        payload: BytecodeTermPayloadKind::SubpatternRange,
        owner: BytecodeSchemaOwner::Parser,
        may_quantify: false,
        may_capture: false,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::ParentheticalAssertionEnd,
        payload: BytecodeTermPayloadKind::SubpatternRange,
        owner: BytecodeSchemaOwner::Parser,
        may_quantify: false,
        may_capture: false,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::CheckInput,
        payload: BytecodeTermPayloadKind::InputCheck,
        owner: BytecodeSchemaOwner::Interpreter,
        may_quantify: false,
        may_capture: false,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::UncheckInput,
        payload: BytecodeTermPayloadKind::InputCheck,
        owner: BytecodeSchemaOwner::Interpreter,
        may_quantify: false,
        may_capture: false,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::HaveCheckedInput,
        payload: BytecodeTermPayloadKind::InputCheck,
        owner: BytecodeSchemaOwner::Interpreter,
        may_quantify: false,
        may_capture: false,
        changes_input_position: false,
    },
    BytecodeTermSchemaDescriptor {
        kind: BytecodeTermKind::DotStarEnclosure,
        payload: BytecodeTermPayloadKind::DotStarEnclosure,
        owner: BytecodeSchemaOwner::Jit,
        may_quantify: false,
        may_capture: false,
        changes_input_position: true,
    },
];

/// Returns the immutable Yarr bytecode term schema table.
pub const fn bytecode_term_schema_table() -> &'static [BytecodeTermSchemaDescriptor] {
    BYTECODE_TERM_SCHEMA
}

/// Returns static schema metadata for one bytecode term kind.
pub fn bytecode_term_schema(
    kind: BytecodeTermKind,
) -> Option<&'static BytecodeTermSchemaDescriptor> {
    BYTECODE_TERM_SCHEMA
        .iter()
        .find(|descriptor| descriptor.kind == kind)
}

/// Alternative jump metadata carried by bytecode terms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BytecodeAlternativeJump {
    pub next: i32,
    pub end: i32,
    pub once_through: bool,
}

/// Parentheses or assertion subpattern range embedded in a bytecode term.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BytecodeSubpatternRange {
    pub first_subpattern_id: u32,
    pub last_subpattern_id: u32,
}

/// Input check count carried by check/uncheck bytecode terms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BytecodeInputCheck {
    pub checked_count: u32,
}

/// Backtracking frame shape reserved for a term.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct YarrBacktrackFrame {
    pub frame_location: u32,
    pub stack_slots: u32,
    pub captures_begin: Option<u32>,
    pub captures_end: Option<u32>,
}

/// One bytecode term descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BytecodeTerm {
    pub id: BytecodeTermId,
    pub kind: BytecodeTermKind,
    pub input_position: u32,
    pub character: Option<char>,
    pub cased_range: Option<(char, char)>,
    pub character_class: Option<CharacterClassDescriptor>,
    pub subpattern_id: Option<u32>,
    pub subpattern_range: Option<BytecodeSubpatternRange>,
    /// Index into `BytecodePattern::parentheses` for the adopted sub-disjunction
    /// of a variable-count `ParenthesesSubpattern` term. Faithful safe mapping of
    /// C++ `ByteTerm::atom.parenthesesDisjunction` (a `ByteDisjunction*`); see
    /// YarrInterpreter.h:62-63 and YarrInterpreter.cpp:2563.
    pub parentheses_disjunction: Option<u32>,
    /// Term-distance between a parentheses begin/end pair. C++
    /// `ByteTerm::atom.parenthesesWidth` (`endTerm - beginTerm`), used by the
    /// interpreter to jump between Once/Terminal/Assertion begin and end terms
    /// (YarrInterpreter.cpp:1183, 1317, 1371).
    pub parentheses_width: Option<u32>,
    /// DotStarEnclosure anchors. C++ `ByteTerm::anchors {m_bol, m_eol}`
    /// (YarrInterpreter.h:75-78, :385-391).
    pub dot_star_anchors: Option<(bool, bool)>,
    pub duplicate_named_group_id: Option<u32>,
    pub name: Option<StringId>,
    pub quantifier: Quantifier,
    pub alternative_jump: Option<BytecodeAlternativeJump>,
    pub input_check: Option<BytecodeInputCheck>,
    pub flags: RegexFlags,
    pub invert: bool,
    pub capture: bool,
    pub direction: MatchDirection,
    pub frame: Option<YarrBacktrackFrame>,
}

/// Alternative compiled to bytecode terms.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BytecodeAlternative {
    pub begin: BytecodeTermId,
    pub end: BytecodeTermId,
    pub once_through: bool,
}

/// Byte disjunction adopted by a bytecode pattern.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ByteDisjunction {
    pub terms: Vec<BytecodeTerm>,
    pub subpattern_count: u32,
    pub frame_size: u32,
}

/// Character-class cache slot retained by compiled bytecode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BytecodeCharacterClassCache {
    Newline,
    Word,
    IgnoreCaseWord,
    UserDefined(u32),
}

/// Output vector layout reserved for numbered and duplicate named captures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BytecodeOffsetVectorLayout {
    pub base_for_named_captures: u32,
    pub offsets_size: u32,
    pub duplicate_named_group_count: u32,
}

/// Bytecode-level pattern descriptor.
/// Bytecode owns adopted disjunctions and user character classes after compile;
/// interpreter and JIT stages may read them but must not mutate parser state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BytecodePattern {
    pub id: BytecodePatternId,
    pub pattern: YarrPatternId,
    pub body: ByteDisjunction,
    pub parentheses: Vec<ByteDisjunction>,
    pub alternatives: Vec<BytecodeAlternative>,
    pub terms: Vec<BytecodeTerm>,
    pub frame_size: u32,
    pub minimum_size: Option<u32>,
    pub contains_bol: bool,
    pub contains_eol: bool,
    pub caches: Vec<BytecodeCharacterClassCache>,
    pub offset_vector: BytecodeOffsetVectorLayout,
    pub duplicate_named_group_for_subpattern: Vec<u32>,
}

/// Complete bytecode program reserved for either interpreter or JIT input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrBytecodeProgram {
    pub pattern: BytecodePattern,
    pub generation: u64,
    pub valid_for_jit: bool,
}

/// Structural error reported by Yarr bytecode builders and validators.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum YarrBytecodeValidationError {
    MissingSchema(BytecodeTermKind),
    InvalidQuantifier {
        term: BytecodeTermId,
        min: u32,
        max: Option<u32>,
    },
    UnexpectedQuantifier(BytecodeTermId),
    UnexpectedCapture(BytecodeTermId),
    PayloadMismatch {
        term: BytecodeTermId,
        expected: BytecodeTermPayloadKind,
    },
    InvalidCharacterRange(BytecodeTermId),
    InvalidSubpatternRange(BytecodeTermId),
    InvalidTermOrder {
        expected: BytecodeTermId,
        actual: BytecodeTermId,
    },
    UnknownTerm(BytecodeTermId),
    InvalidAlternativeRange {
        begin: BytecodeTermId,
        end: BytecodeTermId,
    },
    InvalidFrameSize {
        declared: u32,
        required: u32,
    },
    InvalidOffsetVector {
        offsets_size: u32,
        required: u32,
    },
    DuplicateNamedGroupMapMismatch {
        expected: u32,
        actual: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum YarrBytecodeAssemblyError {
    PatternHasParserError(crate::yarr::YarrErrorCode),
    UnsupportedTerm { index: usize, kind: PatternTermKind },
    MissingPayload { index: usize, kind: PatternTermKind },
    Validation(YarrBytecodeValidationError),
}

impl From<YarrBytecodeValidationError> for YarrBytecodeAssemblyError {
    fn from(error: YarrBytecodeValidationError) -> Self {
        Self::Validation(error)
    }
}

const DEFAULT_QUANTIFIER: Quantifier = Quantifier {
    kind: QuantifierKind::FixedCount,
    min: 1,
    max: Some(1),
};

/// Builder for one Yarr bytecode term descriptor.
#[derive(Clone, Debug)]
pub struct BytecodeTermBuilder {
    term: BytecodeTerm,
}

impl BytecodeTermBuilder {
    pub fn new(id: BytecodeTermId, kind: BytecodeTermKind, flags: RegexFlags) -> Self {
        Self {
            term: BytecodeTerm {
                id,
                kind,
                input_position: 0,
                character: None,
                cased_range: None,
                character_class: None,
                subpattern_id: None,
                subpattern_range: None,
                parentheses_disjunction: None,
                parentheses_width: None,
                dot_star_anchors: None,
                duplicate_named_group_id: None,
                name: None,
                quantifier: DEFAULT_QUANTIFIER,
                alternative_jump: None,
                input_check: None,
                flags,
                invert: false,
                capture: false,
                direction: MatchDirection::Forward,
                frame: None,
            },
        }
    }

    pub fn input_position(mut self, input_position: u32) -> Self {
        self.term.input_position = input_position;
        self
    }

    pub fn character(mut self, character: char) -> Self {
        self.term.character = Some(character);
        self
    }

    pub fn cased_range(mut self, begin: char, end: char) -> Self {
        self.term.cased_range = Some((begin, end));
        self
    }

    pub fn character_class(mut self, character_class: CharacterClassDescriptor) -> Self {
        self.term.character_class = Some(character_class);
        self
    }

    pub fn subpattern_id(mut self, subpattern_id: u32) -> Self {
        self.term.subpattern_id = Some(subpattern_id);
        self
    }

    pub fn subpattern_range(mut self, range: BytecodeSubpatternRange) -> Self {
        self.term.subpattern_range = Some(range);
        self
    }

    pub fn parentheses_disjunction(mut self, index: u32) -> Self {
        self.term.parentheses_disjunction = Some(index);
        self
    }

    pub fn parentheses_width(mut self, width: u32) -> Self {
        self.term.parentheses_width = Some(width);
        self
    }

    pub fn dot_star_anchors(mut self, bol: bool, eol: bool) -> Self {
        self.term.dot_star_anchors = Some((bol, eol));
        self
    }

    pub fn duplicate_named_group_id(mut self, duplicate_named_group_id: u32) -> Self {
        self.term.duplicate_named_group_id = Some(duplicate_named_group_id);
        self
    }

    pub fn name(mut self, name: StringId) -> Self {
        self.term.name = Some(name);
        self
    }

    pub fn name_opt(mut self, name: Option<StringId>) -> Self {
        self.term.name = name;
        self
    }

    pub fn quantifier(mut self, quantifier: Quantifier) -> Self {
        self.term.quantifier = quantifier;
        self
    }

    pub fn alternative_jump(mut self, jump: BytecodeAlternativeJump) -> Self {
        self.term.alternative_jump = Some(jump);
        self
    }

    pub fn input_check(mut self, input_check: BytecodeInputCheck) -> Self {
        self.term.input_check = Some(input_check);
        self
    }

    pub fn capture(mut self, capture: bool) -> Self {
        self.term.capture = capture;
        self
    }

    pub fn invert(mut self, invert: bool) -> Self {
        self.term.invert = invert;
        self
    }

    pub fn direction(mut self, direction: MatchDirection) -> Self {
        self.term.direction = direction;
        self
    }

    pub fn frame(mut self, frame: YarrBacktrackFrame) -> Self {
        self.term.frame = Some(frame);
        self
    }

    pub fn build(self) -> Result<BytecodeTerm, YarrBytecodeValidationError> {
        validate_bytecode_term(&self.term)?;
        Ok(self.term)
    }

    /// Returns the defaulted term without validation, for the ByteCompiler which
    /// fills structural fields (alternative jumps, frame links) after creation and
    /// validates the whole sequence at the end via `validate_term_sequence`.
    pub fn build_unchecked(self) -> BytecodeTerm {
        self.term
    }
}

/// Builder for complete Yarr bytecode pattern descriptors.
#[derive(Clone, Debug)]
pub struct BytecodePatternBuilder {
    pattern: BytecodePattern,
}

impl BytecodePatternBuilder {
    pub fn new(id: BytecodePatternId, pattern: YarrPatternId, body: ByteDisjunction) -> Self {
        Self {
            pattern: BytecodePattern {
                id,
                pattern,
                terms: body.terms.clone(),
                frame_size: body.frame_size,
                body,
                parentheses: Vec::new(),
                alternatives: Vec::new(),
                minimum_size: None,
                contains_bol: false,
                contains_eol: false,
                caches: Vec::new(),
                offset_vector: BytecodeOffsetVectorLayout {
                    base_for_named_captures: 0,
                    offsets_size: 2,
                    duplicate_named_group_count: 0,
                },
                duplicate_named_group_for_subpattern: Vec::new(),
            },
        }
    }

    pub fn parentheses(mut self, disjunction: ByteDisjunction) -> Self {
        self.pattern.parentheses.push(disjunction);
        self
    }

    pub fn alternative(mut self, alternative: BytecodeAlternative) -> Self {
        self.pattern.alternatives.push(alternative);
        self
    }

    pub fn minimum_size(mut self, minimum_size: u32) -> Self {
        self.pattern.minimum_size = Some(minimum_size);
        self
    }

    pub fn contains_bol(mut self, contains_bol: bool) -> Self {
        self.pattern.contains_bol = contains_bol;
        self
    }

    pub fn contains_eol(mut self, contains_eol: bool) -> Self {
        self.pattern.contains_eol = contains_eol;
        self
    }

    pub fn cache(mut self, cache: BytecodeCharacterClassCache) -> Self {
        self.pattern.caches.push(cache);
        self
    }

    pub fn offset_vector(mut self, offset_vector: BytecodeOffsetVectorLayout) -> Self {
        self.pattern.offset_vector = offset_vector;
        self
    }

    pub fn duplicate_named_group_for_subpattern(mut self, duplicate_named_group_id: u32) -> Self {
        self.pattern
            .duplicate_named_group_for_subpattern
            .push(duplicate_named_group_id);
        self
    }

    pub fn build(self) -> Result<BytecodePattern, YarrBytecodeValidationError> {
        validate_bytecode_pattern(&self.pattern)?;
        Ok(self.pattern)
    }
}

/// Builder for a complete bytecode program descriptor.
#[derive(Clone, Debug)]
pub struct YarrBytecodeProgramBuilder {
    program: YarrBytecodeProgram,
}

impl YarrBytecodeProgramBuilder {
    pub fn new(pattern: BytecodePattern) -> Self {
        Self {
            program: YarrBytecodeProgram {
                pattern,
                generation: 0,
                valid_for_jit: false,
            },
        }
    }

    pub fn generation(mut self, generation: u64) -> Self {
        self.program.generation = generation;
        self
    }

    pub fn valid_for_jit(mut self, valid_for_jit: bool) -> Self {
        self.program.valid_for_jit = valid_for_jit;
        self
    }

    pub fn build(self) -> Result<YarrBytecodeProgram, YarrBytecodeValidationError> {
        validate_yarr_bytecode_program(&self.program)?;
        Ok(self.program)
    }
}

pub fn validate_yarr_bytecode_program(
    program: &YarrBytecodeProgram,
) -> Result<(), YarrBytecodeValidationError> {
    validate_bytecode_pattern(&program.pattern)
}

/// Faithful port of `class ByteCompiler` (YarrInterpreter.cpp:2276) for the
/// subset of the parse tree the current `PatternTerm` descriptor can express:
/// flat alternatives of FixedCount-1 atoms (characters, character classes,
/// anchors, word boundaries, dot-star enclosures). It performs the real
/// structural lowering — `regexBegin` wraps the body in
/// `BodyAlternativeBegin/Disjunction/End`, `emitDisjunction` emits a per-
/// alternative `CheckInput(minimumSize)` then the atoms, and
/// `closeBodyAlternative` links the alternative `next`/`end` offsets
/// (YarrInterpreter.cpp:2294-2534, :2683-2768).
///
/// SERIAL COUPLING (B1): the Rust `PatternTerm` does not yet carry
/// `quantityType/quantityMinCount/quantityMaxCount`, a per-term `frameLocation`,
/// or a disjunction pool to resolve nested `parentheses.disjunction`. Until B1
/// supplies them, quantified atoms, back-references with frames, capturing/
/// non-capturing groups, and lookaround cannot be lowered FROM THE TREE and are
/// reported as `UnsupportedTerm`. The interpreter exercises those paths via
/// hand-built `BytecodeTerm` fixtures, which fully express quantifiers and
/// frames.
struct ByteCompiler {
    terms: Vec<BytecodeTerm>,
    current_alternative_index: usize,
    flags: RegexFlags,
    contains_eol: bool,
}

impl ByteCompiler {
    fn new(flags: RegexFlags) -> Self {
        Self {
            terms: Vec::new(),
            current_alternative_index: 0,
            flags,
            contains_eol: false,
        }
    }

    fn push(&mut self, kind: BytecodeTermKind) -> usize {
        let index = self.terms.len();
        let term = BytecodeTermBuilder::new(BytecodeTermId(index as u32), kind, self.flags)
            .build_unchecked();
        self.terms.push(term);
        index
    }

    fn set_alt_jump(&mut self, index: usize, next: i32, end: i32, once_through: bool) {
        self.terms[index].alternative_jump = Some(BytecodeAlternativeJump {
            next,
            end,
            once_through,
        });
    }

    /// regexBegin — YarrInterpreter.cpp:2652.
    fn regex_begin(&mut self, once_through: bool) {
        let idx = self.push(BytecodeTermKind::BodyAlternativeBegin);
        self.set_alt_jump(idx, 0, 0, once_through);
        self.current_alternative_index = 0;
    }

    /// regexEnd — YarrInterpreter.cpp:2660.
    fn regex_end(&mut self) {
        self.close_body_alternative();
    }

    /// alternativeBodyDisjunction — YarrInterpreter.cpp:2665.
    fn alternative_body_disjunction(&mut self, once_through: bool) {
        let new_index = self.terms.len();
        let current = self.current_alternative_index;
        // terms[current].alternative.next = newIndex - current
        let cur_jump = self.terms[current]
            .alternative_jump
            .unwrap_or(EMPTY_ALT_JUMP);
        self.set_alt_jump(
            current,
            (new_index - current) as i32,
            cur_jump.end,
            cur_jump.once_through,
        );
        let idx = self.push(BytecodeTermKind::BodyAlternativeDisjunction);
        self.set_alt_jump(idx, 0, 0, once_through);
        self.current_alternative_index = new_index;
    }

    /// closeBodyAlternative — YarrInterpreter.cpp:2514.
    fn close_body_alternative(&mut self) {
        let mut begin_term = 0usize;
        let orig_begin_term = 0usize;
        let end_index = self.terms.len();
        while self.terms[begin_term]
            .alternative_jump
            .unwrap_or(EMPTY_ALT_JUMP)
            .next
            != 0
        {
            let next = self.terms[begin_term]
                .alternative_jump
                .unwrap_or(EMPTY_ALT_JUMP)
                .next;
            begin_term = (begin_term as i32 + next) as usize;
            // C++ sets only `.end` here; the walked term's existing `.next`
            // (0 for the last disjunction, the chain link otherwise) is preserved.
            let existing = self.terms[begin_term]
                .alternative_jump
                .unwrap_or(EMPTY_ALT_JUMP);
            self.set_alt_jump(
                begin_term,
                existing.next,
                (end_index - begin_term) as i32,
                existing.once_through,
            );
        }
        let once = self.terms[begin_term]
            .alternative_jump
            .unwrap_or(EMPTY_ALT_JUMP)
            .once_through;
        let end = self.terms[begin_term]
            .alternative_jump
            .unwrap_or(EMPTY_ALT_JUMP)
            .end;
        self.set_alt_jump(
            begin_term,
            orig_begin_term as i32 - begin_term as i32,
            end,
            once,
        );
        let end_idx = self.push(BytecodeTermKind::BodyAlternativeEnd);
        self.set_alt_jump(end_idx, 0, 0, false);
    }

    fn check_input(&mut self, count: u32) {
        let idx = self.push(BytecodeTermKind::CheckInput);
        self.terms[idx].input_check = Some(BytecodeInputCheck {
            checked_count: count,
        });
    }

    fn assertion_bol(&mut self, input_position: u32, flags: RegexFlags) {
        let idx = self.push(BytecodeTermKind::AssertionBol);
        self.terms[idx].input_position = input_position;
        self.terms[idx].flags = flags;
    }

    fn assertion_eol(&mut self, input_position: u32, flags: RegexFlags) {
        let idx = self.push(BytecodeTermKind::AssertionEol);
        self.terms[idx].input_position = input_position;
        self.terms[idx].flags = flags;
        self.contains_eol = true;
    }

    fn assertion_word_boundary(&mut self, invert: bool, input_position: u32, flags: RegexFlags) {
        let idx = self.push(BytecodeTermKind::AssertionWordBoundary);
        self.terms[idx].input_position = input_position;
        self.terms[idx].invert = invert;
        self.terms[idx].flags = flags;
    }

    /// atomPatternCharacter — YarrInterpreter.cpp:2346. FixedCount-1 only;
    /// ignoreCase emits a cased-character term when lower != upper.
    fn atom_pattern_character(&mut self, ch: char, input_position: u32, flags: RegexFlags) {
        if flags.ignore_case {
            let lo = simple_lower(ch);
            let hi = simple_upper(ch);
            if lo != hi {
                let idx = self.push(BytecodeTermKind::PatternCasedCharacterOnce);
                self.terms[idx].cased_range = Some((lo, hi));
                self.terms[idx].input_position = input_position;
                self.terms[idx].flags = flags;
                return;
            }
        }
        let idx = self.push(BytecodeTermKind::PatternCharacterOnce);
        self.terms[idx].character = Some(ch);
        self.terms[idx].input_position = input_position;
        self.terms[idx].flags = flags;
    }

    /// atomCharacterClass — YarrInterpreter.cpp:2363. FixedCount-1 only.
    fn atom_character_class(
        &mut self,
        class: CharacterClassDescriptor,
        invert: bool,
        input_position: u32,
        flags: RegexFlags,
    ) {
        let idx = self.push(BytecodeTermKind::CharacterClass);
        self.terms[idx].character_class = Some(class);
        self.terms[idx].invert = invert;
        self.terms[idx].input_position = input_position;
        self.terms[idx].flags = flags;
    }

    /// assertionDotStarEnclosure — YarrInterpreter.cpp:2471.
    fn assertion_dot_star_enclosure(&mut self, bol: bool, eol: bool) {
        let idx = self.push(BytecodeTermKind::DotStarEnclosure);
        self.terms[idx].dot_star_anchors = Some((bol, eol));
    }

    /// emitDisjunction — YarrInterpreter.cpp:2683 (Forward, flat subset).
    fn emit_disjunction(
        &mut self,
        disjunction: &PatternDisjunction,
        parentheses_already_checked: u32,
    ) -> Result<(), YarrBytecodeAssemblyError> {
        for (alt_index, alternative) in disjunction.alternatives.iter().enumerate() {
            if alt_index != 0 {
                if disjunction.is_body {
                    self.alternative_body_disjunction(alternative.once_through);
                } else {
                    // Nested (non-body) disjunctions need the full alternative
                    // machinery + frame allocation; deferred to B1 (see struct doc).
                    return Err(YarrBytecodeAssemblyError::UnsupportedTerm {
                        index: self.terms.len(),
                        kind: PatternTermKind::ParenthesesSubpattern,
                    });
                }
            }

            let minimum_size = alternative.minimum_size.unwrap_or(0);
            let count_to_check = minimum_size.saturating_sub(parentheses_already_checked);
            if count_to_check != 0 {
                self.check_input(count_to_check);
            }
            let current_count = count_to_check + parentheses_already_checked;

            for (term_index, term) in alternative.terms.iter().enumerate() {
                let input_position = current_count.saturating_sub(term.input_position);
                self.emit_term(term, term_index, input_position)?;
            }
        }
        Ok(())
    }

    fn emit_term(
        &mut self,
        term: &PatternTerm,
        index: usize,
        input_position: u32,
    ) -> Result<(), YarrBytecodeAssemblyError> {
        match term.kind {
            PatternTermKind::Assertion(PatternAssertion::Bol) => {
                self.assertion_bol(input_position, term.flags)
            }
            PatternTermKind::Assertion(PatternAssertion::Eol) => {
                self.assertion_eol(input_position, term.flags)
            }
            PatternTermKind::Assertion(PatternAssertion::WordBoundary) => {
                self.assertion_word_boundary(term.invert, input_position, term.flags)
            }
            PatternTermKind::Assertion(PatternAssertion::NotWordBoundary) => {
                self.assertion_word_boundary(true, input_position, term.flags)
            }
            PatternTermKind::PatternCharacter => {
                let ch = term
                    .character
                    .ok_or(YarrBytecodeAssemblyError::MissingPayload {
                        index,
                        kind: term.kind,
                    })?;
                self.atom_pattern_character(ch, input_position, term.flags);
            }
            PatternTermKind::CharacterClass => {
                let class = term.character_class.clone().ok_or(
                    YarrBytecodeAssemblyError::MissingPayload {
                        index,
                        kind: term.kind,
                    },
                )?;
                self.atom_character_class(class, term.invert, input_position, term.flags);
            }
            PatternTermKind::DotStarEnclosure => {
                let (bol, eol) = term
                    .dot_star_anchors
                    .map(|a| (a.bol_anchor, a.eol_anchor))
                    .unwrap_or((false, false));
                self.assertion_dot_star_enclosure(bol, eol);
            }
            // The remaining kinds require B1's quantifier/frame/disjunction-pool
            // support (see ByteCompiler doc). Reported, not silently mis-lowered.
            PatternTermKind::NumberedBackReference
            | PatternTermKind::NamedBackReference
            | PatternTermKind::NumberedForwardReference
            | PatternTermKind::NamedForwardReference
            | PatternTermKind::ParenthesesSubpattern
            | PatternTermKind::ParentheticalAssertion
            | PatternTermKind::Assertion(PatternAssertion::LookAhead)
            | PatternTermKind::Assertion(PatternAssertion::NegativeLookAhead)
            | PatternTermKind::Assertion(PatternAssertion::LookBehind)
            | PatternTermKind::Assertion(PatternAssertion::NegativeLookBehind) => {
                return Err(YarrBytecodeAssemblyError::UnsupportedTerm {
                    index,
                    kind: term.kind,
                });
            }
        }
        Ok(())
    }
}

const EMPTY_ALT_JUMP: BytecodeAlternativeJump = BytecodeAlternativeJump {
    next: 0,
    end: 0,
    once_through: false,
};

fn simple_lower(ch: char) -> char {
    // C++ `u_tolower`; first scalar of Rust's full case fold (exact for BMP
    // single-char foldings, which is the legacy interpreter path).
    ch.to_lowercase().next().unwrap_or(ch)
}

fn simple_upper(ch: char) -> char {
    ch.to_uppercase().next().unwrap_or(ch)
}

/// byteCompile — YarrInterpreter.cpp:2294 / :562. Faithful flat-subset compile of
/// a parsed pattern into a `BytecodePattern` ready for `interpret_bytecode`.
pub fn assemble_yarr_bytecode_plan(
    parsed: &YarrPattern,
    id: BytecodePatternId,
    generation: u64,
) -> Result<YarrBytecodeProgram, YarrBytecodeAssemblyError> {
    if parsed.error != crate::yarr::YarrErrorCode::NoError {
        return Err(YarrBytecodeAssemblyError::PatternHasParserError(
            parsed.error,
        ));
    }

    let once_through = parsed
        .body
        .alternatives
        .first()
        .map(|alt| alt.once_through)
        .unwrap_or(false);

    let mut compiler = ByteCompiler::new(parsed.flags);
    compiler.regex_begin(once_through);
    compiler.emit_disjunction(&parsed.body, 0)?;
    compiler.regex_end();

    let terms = compiler.terms;
    let contains_eol = compiler.contains_eol;
    validate_term_sequence(&terms)?;

    // No frame users in the flat fixed-count-1 subset (only non-body Alternative
    // and quantified/back-reference/parentheses terms reserve frame slots).
    let frame_size = required_frame_size(&terms);
    let body = ByteDisjunction {
        terms: terms.clone(),
        subpattern_count: parsed.capture_count,
        frame_size,
    };

    let mut builder = BytecodePatternBuilder::new(id, parsed.id, body)
        .contains_bol(parsed.contains_bol)
        .contains_eol(contains_eol)
        .offset_vector(BytecodeOffsetVectorLayout {
            // C++ `offsetVectorBaseForNamedCaptures` == (numSubpatterns + 1) * 2:
            // duplicate-named-group slots live AFTER the numbered-capture slots
            // (overall + each group). The prior stub used `capture_count * 2`,
            // which overlapped group N's slots and zeroed them on init.
            base_for_named_captures: (parsed.capture_count + 1).saturating_mul(2),
            offsets_size: (parsed.capture_count + 1).saturating_mul(2)
                + parsed.duplicate_named_capture_count,
            duplicate_named_group_count: parsed.duplicate_named_capture_count,
        });

    if let Some(minimum_size) = parsed.body.minimum_size {
        builder = builder.minimum_size(minimum_size);
    }
    for duplicate in 0..parsed.duplicate_named_capture_count {
        builder = builder.duplicate_named_group_for_subpattern(duplicate);
    }
    if !terms.is_empty() {
        builder = builder.alternative(BytecodeAlternative {
            begin: BytecodeTermId(0),
            end: BytecodeTermId(terms.len() as u32 - 1),
            once_through,
        });
    }

    let pattern = builder.build()?;
    YarrBytecodeProgramBuilder::new(pattern)
        .generation(generation)
        .valid_for_jit(!parsed.contains_lookbehinds)
        .build()
        .map_err(Into::into)
}

fn required_frame_size(terms: &[BytecodeTerm]) -> u32 {
    terms
        .iter()
        .filter_map(|term| term.frame.as_ref())
        .map(|frame| frame.frame_location.saturating_add(frame.stack_slots))
        .max()
        .unwrap_or(0)
}

pub fn validate_bytecode_pattern(
    pattern: &BytecodePattern,
) -> Result<(), YarrBytecodeValidationError> {
    validate_byte_disjunction(&pattern.body)?;

    for disjunction in &pattern.parentheses {
        validate_byte_disjunction(disjunction)?;
    }

    validate_term_sequence(&pattern.terms)?;

    for alternative in &pattern.alternatives {
        validate_term_id_exists(&pattern.terms, alternative.begin)?;
        validate_term_id_exists(&pattern.terms, alternative.end)?;
        if alternative.begin > alternative.end {
            return Err(YarrBytecodeValidationError::InvalidAlternativeRange {
                begin: alternative.begin,
                end: alternative.end,
            });
        }
    }

    if pattern.frame_size < pattern.body.frame_size {
        return Err(YarrBytecodeValidationError::InvalidFrameSize {
            declared: pattern.frame_size,
            required: pattern.body.frame_size,
        });
    }

    let required_offsets = 2 + pattern.offset_vector.duplicate_named_group_count * 2;
    if pattern.offset_vector.offsets_size < required_offsets {
        return Err(YarrBytecodeValidationError::InvalidOffsetVector {
            offsets_size: pattern.offset_vector.offsets_size,
            required: required_offsets,
        });
    }

    if pattern.duplicate_named_group_for_subpattern.len()
        != pattern.offset_vector.duplicate_named_group_count as usize
    {
        return Err(
            YarrBytecodeValidationError::DuplicateNamedGroupMapMismatch {
                expected: pattern.offset_vector.duplicate_named_group_count,
                actual: pattern.duplicate_named_group_for_subpattern.len(),
            },
        );
    }

    Ok(())
}

pub fn validate_byte_disjunction(
    disjunction: &ByteDisjunction,
) -> Result<(), YarrBytecodeValidationError> {
    validate_term_sequence(&disjunction.terms)?;

    let required_frame_size = disjunction
        .terms
        .iter()
        .filter_map(|term| term.frame)
        .map(|frame| frame.frame_location.saturating_add(frame.stack_slots))
        .max()
        .unwrap_or(0);

    if disjunction.frame_size < required_frame_size {
        return Err(YarrBytecodeValidationError::InvalidFrameSize {
            declared: disjunction.frame_size,
            required: required_frame_size,
        });
    }

    Ok(())
}

pub fn validate_bytecode_term(term: &BytecodeTerm) -> Result<(), YarrBytecodeValidationError> {
    let schema = bytecode_term_schema(term.kind)
        .ok_or(YarrBytecodeValidationError::MissingSchema(term.kind))?;

    validate_quantifier(term, schema)?;

    if term.capture && !schema.may_capture {
        return Err(YarrBytecodeValidationError::UnexpectedCapture(term.id));
    }

    match schema.payload {
        BytecodeTermPayloadKind::None => {
            if term.character.is_some()
                || term.cased_range.is_some()
                || term.character_class.is_some()
                || term.subpattern_id.is_some()
                || term.subpattern_range.is_some()
                || term.alternative_jump.is_some()
                || term.input_check.is_some()
            {
                return Err(YarrBytecodeValidationError::PayloadMismatch {
                    term: term.id,
                    expected: schema.payload,
                });
            }
        }
        BytecodeTermPayloadKind::Character => {
            if term.character.is_none()
                || term.cased_range.is_some()
                || term.character_class.is_some()
                || term.subpattern_id.is_some()
                || term.subpattern_range.is_some()
                || term.alternative_jump.is_some()
                || term.input_check.is_some()
            {
                return Err(YarrBytecodeValidationError::PayloadMismatch {
                    term: term.id,
                    expected: schema.payload,
                });
            }
        }
        BytecodeTermPayloadKind::CasedCharacterRange => match term.cased_range {
            Some((begin, end)) if begin <= end => {
                if term.character.is_some()
                    || term.character_class.is_some()
                    || term.subpattern_id.is_some()
                    || term.subpattern_range.is_some()
                    || term.alternative_jump.is_some()
                    || term.input_check.is_some()
                {
                    return Err(YarrBytecodeValidationError::PayloadMismatch {
                        term: term.id,
                        expected: schema.payload,
                    });
                }
            }
            Some(_) => return Err(YarrBytecodeValidationError::InvalidCharacterRange(term.id)),
            None => {
                return Err(YarrBytecodeValidationError::PayloadMismatch {
                    term: term.id,
                    expected: schema.payload,
                });
            }
        },
        BytecodeTermPayloadKind::CharacterClass => {
            if term.character_class.is_none()
                || term.character.is_some()
                || term.cased_range.is_some()
                || term.subpattern_id.is_some()
                || term.subpattern_range.is_some()
                || term.alternative_jump.is_some()
                || term.input_check.is_some()
            {
                return Err(YarrBytecodeValidationError::PayloadMismatch {
                    term: term.id,
                    expected: schema.payload,
                });
            }
        }
        BytecodeTermPayloadKind::BackReference | BytecodeTermPayloadKind::Subpattern => {
            if term.subpattern_id.is_none()
                || term.character.is_some()
                || term.cased_range.is_some()
                || term.character_class.is_some()
                || term.subpattern_range.is_some()
                || term.alternative_jump.is_some()
                || term.input_check.is_some()
            {
                return Err(YarrBytecodeValidationError::PayloadMismatch {
                    term: term.id,
                    expected: schema.payload,
                });
            }
        }
        BytecodeTermPayloadKind::SubpatternRange => match term.subpattern_range {
            Some(range) if range.first_subpattern_id <= range.last_subpattern_id => {
                if term.character.is_some()
                    || term.cased_range.is_some()
                    || term.character_class.is_some()
                    || term.alternative_jump.is_some()
                    || term.input_check.is_some()
                {
                    return Err(YarrBytecodeValidationError::PayloadMismatch {
                        term: term.id,
                        expected: schema.payload,
                    });
                }
            }
            Some(_) => return Err(YarrBytecodeValidationError::InvalidSubpatternRange(term.id)),
            None => {
                return Err(YarrBytecodeValidationError::PayloadMismatch {
                    term: term.id,
                    expected: schema.payload,
                });
            }
        },
        BytecodeTermPayloadKind::AlternativeJump => {
            if term.alternative_jump.is_none()
                || term.character.is_some()
                || term.cased_range.is_some()
                || term.character_class.is_some()
                || term.subpattern_id.is_some()
                || term.subpattern_range.is_some()
                || term.input_check.is_some()
            {
                return Err(YarrBytecodeValidationError::PayloadMismatch {
                    term: term.id,
                    expected: schema.payload,
                });
            }
        }
        BytecodeTermPayloadKind::InputCheck => {
            if term.input_check.is_none()
                || term.character.is_some()
                || term.cased_range.is_some()
                || term.character_class.is_some()
                || term.subpattern_id.is_some()
                || term.subpattern_range.is_some()
                || term.alternative_jump.is_some()
            {
                return Err(YarrBytecodeValidationError::PayloadMismatch {
                    term: term.id,
                    expected: schema.payload,
                });
            }
        }
        BytecodeTermPayloadKind::DotStarEnclosure => {
            if term.character.is_some()
                || term.cased_range.is_some()
                || term.character_class.is_some()
                || term.subpattern_id.is_some()
                || term.subpattern_range.is_some()
                || term.alternative_jump.is_some()
                || term.input_check.is_some()
            {
                return Err(YarrBytecodeValidationError::PayloadMismatch {
                    term: term.id,
                    expected: schema.payload,
                });
            }
        }
    }

    Ok(())
}

fn validate_quantifier(
    term: &BytecodeTerm,
    schema: &BytecodeTermSchemaDescriptor,
) -> Result<(), YarrBytecodeValidationError> {
    if let Some(max) = term.quantifier.max {
        if term.quantifier.min > max {
            return Err(YarrBytecodeValidationError::InvalidQuantifier {
                term: term.id,
                min: term.quantifier.min,
                max: term.quantifier.max,
            });
        }
    }

    if matches!(term.quantifier.kind, QuantifierKind::Infinite) && term.quantifier.max.is_some() {
        return Err(YarrBytecodeValidationError::InvalidQuantifier {
            term: term.id,
            min: term.quantifier.min,
            max: term.quantifier.max,
        });
    }

    if !schema.may_quantify && term.quantifier != DEFAULT_QUANTIFIER {
        return Err(YarrBytecodeValidationError::UnexpectedQuantifier(term.id));
    }

    Ok(())
}

fn validate_term_sequence(terms: &[BytecodeTerm]) -> Result<(), YarrBytecodeValidationError> {
    for (index, term) in terms.iter().enumerate() {
        let expected = BytecodeTermId(index as u32);
        if term.id != expected {
            return Err(YarrBytecodeValidationError::InvalidTermOrder {
                expected,
                actual: term.id,
            });
        }
        validate_bytecode_term(term)?;
    }
    Ok(())
}

fn validate_term_id_exists(
    terms: &[BytecodeTerm],
    id: BytecodeTermId,
) -> Result<(), YarrBytecodeValidationError> {
    terms
        .get(id.0 as usize)
        .filter(|term| term.id == id)
        .map(|_| ())
        .ok_or(YarrBytecodeValidationError::UnknownTerm(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytecode_schema_has_distinct_term_kinds() {
        let table = bytecode_term_schema_table();

        for (index, descriptor) in table.iter().enumerate() {
            assert_eq!(bytecode_term_schema(descriptor.kind), Some(descriptor));

            for other in table.iter().skip(index + 1) {
                assert_ne!(descriptor.kind, other.kind);
            }
        }
    }

    #[test]
    fn bytecode_builder_accepts_structural_character_term() {
        let term = BytecodeTermBuilder::new(
            BytecodeTermId(0),
            BytecodeTermKind::PatternCharacterOnce,
            RegexFlags::default(),
        )
        .character('a')
        .build()
        .unwrap();

        let body = ByteDisjunction {
            terms: vec![term],
            subpattern_count: 0,
            frame_size: 0,
        };

        let pattern = BytecodePatternBuilder::new(BytecodePatternId(1), YarrPatternId(2), body)
            .build()
            .unwrap();

        assert!(validate_bytecode_pattern(&pattern).is_ok());
    }

    #[test]
    fn bytecode_validator_rejects_wrong_payload() {
        let error = BytecodeTermBuilder::new(
            BytecodeTermId(0),
            BytecodeTermKind::PatternCharacterOnce,
            RegexFlags::default(),
        )
        .input_check(BytecodeInputCheck { checked_count: 1 })
        .build()
        .unwrap_err();

        assert_eq!(
            error,
            YarrBytecodeValidationError::PayloadMismatch {
                term: BytecodeTermId(0),
                expected: BytecodeTermPayloadKind::Character,
            }
        );
    }

    #[test]
    fn bytecode_assembly_plan_uses_existing_validators() {
        let pattern = YarrPattern {
            id: YarrPatternId(7),
            source: StringId(1),
            flags: RegexFlags::default(),
            compile_mode: crate::yarr::CompileMode::Legacy,
            body: crate::yarr::PatternDisjunction {
                alternatives: vec![crate::yarr::PatternAlternative {
                    terms: vec![PatternTerm {
                        kind: PatternTermKind::PatternCharacter,
                        input_position: 0,
                        character: Some('a'),
                        character_class: None,
                        parentheses: None,
                        dot_star_anchors: None,
                        capture: false,
                        invert: false,
                        subpattern_id: None,
                        name: None,
                        flags: RegexFlags::default(),
                    }],
                    minimum_size: Some(1),
                    first_subpattern_id: 0,
                    last_subpattern_id: 0,
                    direction: MatchDirection::Forward,
                    once_through: false,
                    has_fixed_size: true,
                    starts_with_bol: false,
                    contains_bol: false,
                    is_last_alternative: true,
                    contains_captures: false,
                }],
                parent_subpattern: None,
                is_body: true,
                minimum_size: Some(1),
                call_frame_size: 0,
                has_fixed_size: true,
            },
            capture_count: 0,
            named_capture_count: 0,
            duplicate_named_capture_count: 0,
            contains_backreferences: false,
            contains_bol: false,
            contains_lookbehinds: false,
            contains_unsigned_length_pattern: false,
            has_copied_parentheses: false,
            save_initial_start_value: false,
            error: crate::yarr::YarrErrorCode::NoError,
        };

        let program = assemble_yarr_bytecode_plan(&pattern, BytecodePatternId(9), 3).unwrap();

        assert_eq!(program.generation, 3);
        // Faithful ByteCompiler wraps the body in BodyAlternativeBegin / CheckInput
        // / atom / BodyAlternativeEnd (YarrInterpreter.cpp:2652-2768), unlike the
        // prior flat stub which emitted a bare atom.
        let kinds: Vec<_> = program.pattern.terms.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                BytecodeTermKind::BodyAlternativeBegin,
                BytecodeTermKind::CheckInput,
                BytecodeTermKind::PatternCharacterOnce,
                BytecodeTermKind::BodyAlternativeEnd,
            ]
        );
        // CheckInput pre-checks minimumSize; the atom's inputPosition is the
        // negative offset minimumSize - term.inputPosition (= 1 - 0).
        assert_eq!(
            program.pattern.terms[1].input_check.unwrap().checked_count,
            1
        );
        assert_eq!(program.pattern.terms[2].input_position, 1);
        assert!(validate_yarr_bytecode_program(&program).is_ok());
    }

    #[test]
    fn bytecode_assembly_plan_rejects_pattern_error() {
        let mut pattern = YarrPattern {
            id: YarrPatternId(7),
            source: StringId(1),
            flags: RegexFlags::default(),
            compile_mode: crate::yarr::CompileMode::Legacy,
            body: crate::yarr::PatternDisjunction {
                alternatives: Vec::new(),
                parent_subpattern: None,
                is_body: true,
                minimum_size: Some(0),
                call_frame_size: 0,
                has_fixed_size: true,
            },
            capture_count: 0,
            named_capture_count: 0,
            duplicate_named_capture_count: 0,
            contains_backreferences: false,
            contains_bol: false,
            contains_lookbehinds: false,
            contains_unsigned_length_pattern: false,
            has_copied_parentheses: false,
            save_initial_start_value: false,
            error: crate::yarr::YarrErrorCode::ParenthesesUnmatched,
        };

        let error = assemble_yarr_bytecode_plan(&pattern, BytecodePatternId(9), 0).unwrap_err();
        assert_eq!(
            error,
            YarrBytecodeAssemblyError::PatternHasParserError(
                crate::yarr::YarrErrorCode::ParenthesesUnmatched
            )
        );

        pattern.error = crate::yarr::YarrErrorCode::NoError;
        assert!(assemble_yarr_bytecode_plan(&pattern, BytecodePatternId(9), 0).is_ok());
    }
}
