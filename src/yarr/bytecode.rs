//! Yarr bytecode descriptors.
//!
//! Bytecode here is a descriptive IR contract. The interpreter and compiler
//! that will execute or emit it are intentionally absent.

use crate::strings::StringId;
use crate::yarr::{
    CharacterClassDescriptor, MatchDirection, PatternAssertion, PatternTermKind, QuantifierType,
    RegexFlags, YarrPattern, YarrPatternId,
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

// Yarr backtrack stack-space constants (Yarr.h:35-45), needed by the ByteCompiler
// to derive each parentheses' `alternativeFrameLocation`.
const YARR_STACK_PARENTHESES_ONCE: u32 = 2;
const YARR_STACK_PARENTHESES_TERMINAL: u32 = 1;
const YARR_STACK_PARENTHETICAL_ASSERTION: u32 = 1;
/// Yarr.h:45 `quantifyInfinite == UINT_MAX`.
const QUANTIFY_INFINITE: u32 = u32::MAX;

fn quantifier_kind(qtype: QuantifierType) -> QuantifierKind {
    match qtype {
        QuantifierType::FixedCount => QuantifierKind::FixedCount,
        QuantifierType::Greedy => QuantifierKind::Greedy,
        QuantifierType::NonGreedy => QuantifierKind::NonGreedy,
    }
}

fn make_quantifier(qtype: QuantifierType, min: u32, max: u32) -> Quantifier {
    Quantifier {
        kind: quantifier_kind(qtype),
        min,
        max: if max == QUANTIFY_INFINITE {
            None
        } else {
            Some(max)
        },
    }
}

/// C++ `ByteTerm(char32_t, ...)` selects the term kind from the quantifier
/// (YarrInterpreter.h:125-150).
fn pattern_character_kind(qtype: QuantifierType, max: u32) -> BytecodeTermKind {
    match qtype {
        QuantifierType::FixedCount => {
            if max == 1 {
                BytecodeTermKind::PatternCharacterOnce
            } else {
                BytecodeTermKind::PatternCharacterFixed
            }
        }
        QuantifierType::Greedy => BytecodeTermKind::PatternCharacterGreedy,
        QuantifierType::NonGreedy => BytecodeTermKind::PatternCharacterNonGreedy,
    }
}

/// C++ `ByteTerm(char32_t lo, char32_t hi, ...)` (YarrInterpreter.h:153-180).
fn cased_character_kind(qtype: QuantifierType, max: u32) -> BytecodeTermKind {
    match qtype {
        QuantifierType::FixedCount => {
            if max == 1 {
                BytecodeTermKind::PatternCasedCharacterOnce
            } else {
                BytecodeTermKind::PatternCasedCharacterFixed
            }
        }
        QuantifierType::Greedy => BytecodeTermKind::PatternCasedCharacterGreedy,
        QuantifierType::NonGreedy => BytecodeTermKind::PatternCasedCharacterNonGreedy,
    }
}

/// Parentheses stack entry — YarrInterpreter.cpp:2277 `ParenthesesStackEntry`.
struct ParenthesesStackEntry {
    begin_term: usize,
    saved_alternative_index: usize,
}

/// Faithful port of `class ByteCompiler` (YarrInterpreter.cpp:2276). Lowers the
/// parsed `YarrPattern` disjunction tree into a flat `BytecodePattern` the
/// interpreter (`matchDisjunction`) executes. `terms` is the in-progress body
/// buffer (`m_bodyDisjunction->terms`); `all_parentheses` holds the extracted
/// variable-count sub-disjunctions (`m_allParenthesesInfo`), referenced by the
/// `ParenthesesSubpattern` term's `parentheses_disjunction` index (C++ raw
/// `ByteDisjunction*`). The parentheses/alternative jump linking
/// (`closeAlternative`/`closeBodyAlternative`) is ported 1:1.
struct ByteCompiler<'a> {
    parsed: &'a YarrPattern,
    flags: RegexFlags,
    terms: Vec<BytecodeTerm>,
    all_parentheses: Vec<ByteDisjunction>,
    current_alternative_index: usize,
    parentheses_stack: Vec<ParenthesesStackEntry>,
    contains_eol: bool,
}

impl<'a> ByteCompiler<'a> {
    fn new(parsed: &'a YarrPattern) -> Self {
        Self {
            parsed,
            flags: parsed.flags,
            terms: Vec::new(),
            all_parentheses: Vec::new(),
            current_alternative_index: 0,
            parentheses_stack: Vec::new(),
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

    /// Sets `frameLocation` on a term (Rust bundles it into the optional
    /// backtrack-frame reservation; `stack_slots` is left 0 because the
    /// authoritative frame size is the disjunction `call_frame_size` from
    /// setupOffsets — the interpreter only reads `frame_location`).
    fn set_frame(&mut self, idx: usize, frame_location: u32) {
        self.terms[idx].frame = Some(YarrBacktrackFrame {
            frame_location,
            stack_slots: 0,
            captures_begin: None,
            captures_end: None,
        });
    }

    fn set_alt_jump(&mut self, idx: usize, jump: BytecodeAlternativeJump) {
        self.terms[idx].alternative_jump = Some(jump);
    }

    fn alt_jump(&self, idx: usize) -> BytecodeAlternativeJump {
        self.terms[idx].alternative_jump.unwrap_or(EMPTY_ALT_JUMP)
    }

    fn frame_location_of(&self, idx: usize) -> u32 {
        self.terms[idx]
            .frame
            .as_ref()
            .map(|f| f.frame_location)
            .unwrap_or(0)
    }

    // ---- input checks (YarrInterpreter.cpp:2316-2329) ----

    fn check_input(&mut self, count: u32) {
        let idx = self.push(BytecodeTermKind::CheckInput);
        self.terms[idx].input_check = Some(BytecodeInputCheck {
            checked_count: count,
        });
    }

    fn uncheck_input(&mut self, count: u32) {
        let idx = self.push(BytecodeTermKind::UncheckInput);
        self.terms[idx].input_check = Some(BytecodeInputCheck {
            checked_count: count,
        });
    }

    fn have_checked_input(&mut self, count: u32) {
        let idx = self.push(BytecodeTermKind::HaveCheckedInput);
        self.terms[idx].input_check = Some(BytecodeInputCheck {
            checked_count: count,
        });
    }

    // ---- assertions (YarrInterpreter.cpp:2331-2344) ----

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

    fn assertion_word_boundary(
        &mut self,
        invert: bool,
        direction: MatchDirection,
        input_position: u32,
        flags: RegexFlags,
    ) {
        let idx = self.push(BytecodeTermKind::AssertionWordBoundary);
        self.terms[idx].input_position = input_position;
        self.terms[idx].invert = invert;
        self.terms[idx].direction = direction;
        self.terms[idx].flags = flags;
    }

    // ---- atoms (YarrInterpreter.cpp:2346-2389) ----

    #[allow(clippy::too_many_arguments)]
    fn atom_pattern_character(
        &mut self,
        ch: char,
        direction: MatchDirection,
        input_position: u32,
        frame_location: u32,
        quantity_max_count: u32,
        quantity_type: QuantifierType,
        flags: RegexFlags,
    ) {
        if flags.ignore_case {
            let lo = simple_lower(ch);
            let hi = simple_upper(ch);
            if lo != hi {
                let kind = cased_character_kind(quantity_type, quantity_max_count);
                let idx = self.push(kind);
                self.terms[idx].cased_range = Some((lo.min(hi), lo.max(hi)));
                self.terms[idx].input_position = input_position;
                self.terms[idx].direction = direction;
                self.terms[idx].flags = flags;
                self.set_frame(idx, frame_location);
                let min = if quantity_type == QuantifierType::FixedCount {
                    quantity_max_count
                } else {
                    0
                };
                self.terms[idx].quantifier =
                    make_quantifier(quantity_type, min, quantity_max_count);
                return;
            }
        }
        let kind = pattern_character_kind(quantity_type, quantity_max_count);
        let idx = self.push(kind);
        self.terms[idx].character = Some(ch);
        self.terms[idx].input_position = input_position;
        self.terms[idx].direction = direction;
        self.terms[idx].flags = flags;
        self.set_frame(idx, frame_location);
        let min = if quantity_type == QuantifierType::FixedCount {
            quantity_max_count
        } else {
            0
        };
        self.terms[idx].quantifier = make_quantifier(quantity_type, min, quantity_max_count);
    }

    #[allow(clippy::too_many_arguments)]
    fn atom_character_class(
        &mut self,
        class: CharacterClassDescriptor,
        invert: bool,
        direction: MatchDirection,
        input_position: u32,
        frame_location: u32,
        quantity_max_count: u32,
        quantity_type: QuantifierType,
        flags: RegexFlags,
    ) {
        let idx = self.push(BytecodeTermKind::CharacterClass);
        self.terms[idx].character_class = Some(class);
        self.terms[idx].invert = invert;
        self.terms[idx].direction = direction;
        self.terms[idx].input_position = input_position;
        self.terms[idx].flags = flags;
        self.set_frame(idx, frame_location);
        // C++ ByteTerm class ctor defaults min=1; atomCharacterClass resets to 0
        // only for non-fixed quantifiers (YarrInterpreter.cpp:2363).
        let min = if quantity_type == QuantifierType::FixedCount {
            1
        } else {
            0
        };
        self.terms[idx].quantifier = make_quantifier(quantity_type, min, quantity_max_count);
    }

    #[allow(clippy::too_many_arguments)]
    fn atom_back_reference(
        &mut self,
        subpattern_id: u32,
        direction: MatchDirection,
        input_position: u32,
        frame_location: u32,
        quantity_min_count: u32,
        quantity_max_count: u32,
        quantity_type: QuantifierType,
        flags: RegexFlags,
    ) {
        let idx = self.push(BytecodeTermKind::BackReference);
        self.terms[idx].subpattern_id = Some(subpattern_id);
        self.terms[idx].direction = direction;
        self.terms[idx].input_position = input_position;
        self.terms[idx].flags = flags;
        self.set_frame(idx, frame_location);
        // Duplicate named-capture groups are out of this unit (no duplicateNamedGroupId).
        self.terms[idx].quantifier =
            make_quantifier(quantity_type, quantity_min_count, quantity_max_count);
    }

    fn assertion_dot_star_enclosure(&mut self, bol: bool, eol: bool) {
        let idx = self.push(BytecodeTermKind::DotStarEnclosure);
        self.terms[idx].dot_star_anchors = Some((bol, eol));
    }

    // ---- parentheses begins (YarrInterpreter.cpp:2392-2447) ----

    #[allow(clippy::too_many_arguments)]
    fn parentheses_begin(
        &mut self,
        kind: BytecodeTermKind,
        subpattern_id: u32,
        match_direction: MatchDirection,
        capture: bool,
        input_position: u32,
        frame_location: u32,
        alternative_frame_location: u32,
    ) {
        let begin_term = self.terms.len();
        let idx = self.push(kind);
        self.terms[idx].subpattern_id = Some(subpattern_id);
        self.terms[idx].capture = capture;
        self.terms[idx].direction = match_direction;
        self.terms[idx].input_position = input_position;
        self.terms[idx].subpattern_range = Some(BytecodeSubpatternRange {
            first_subpattern_id: subpattern_id,
            last_subpattern_id: subpattern_id,
        });
        self.set_frame(idx, frame_location);
        let ab = self.push(BytecodeTermKind::AlternativeBegin);
        self.set_alt_jump(ab, EMPTY_ALT_JUMP);
        self.set_frame(ab, alternative_frame_location);
        self.parentheses_stack.push(ParenthesesStackEntry {
            begin_term,
            saved_alternative_index: self.current_alternative_index,
        });
        self.current_alternative_index = begin_term + 1;
    }

    fn atom_parenthetical_assertion_begin(
        &mut self,
        subpattern_id: u32,
        invert: bool,
        match_direction: MatchDirection,
        frame_location: u32,
        alternative_frame_location: u32,
    ) {
        let begin_term = self.terms.len();
        let idx = self.push(BytecodeTermKind::ParentheticalAssertionBegin);
        self.terms[idx].subpattern_id = Some(subpattern_id);
        self.terms[idx].invert = invert;
        self.terms[idx].direction = match_direction;
        self.terms[idx].subpattern_range = Some(BytecodeSubpatternRange {
            first_subpattern_id: subpattern_id,
            last_subpattern_id: subpattern_id,
        });
        self.set_frame(idx, frame_location);
        let ab = self.push(BytecodeTermKind::AlternativeBegin);
        self.set_alt_jump(ab, EMPTY_ALT_JUMP);
        self.set_frame(ab, alternative_frame_location);
        self.parentheses_stack.push(ParenthesesStackEntry {
            begin_term,
            saved_alternative_index: self.current_alternative_index,
        });
        self.current_alternative_index = begin_term + 1;
    }

    // ---- parentheses ends (YarrInterpreter.cpp:2448-2650) ----

    fn pop_parentheses_stack(&mut self) -> usize {
        let entry = self
            .parentheses_stack
            .pop()
            .expect("parentheses stack non-empty");
        self.current_alternative_index = entry.saved_alternative_index;
        entry.begin_term
    }

    fn atom_parenthetical_assertion_end(&mut self, last_subpattern_id: u32, frame_location: u32) {
        let begin_term = self.pop_parentheses_stack();
        self.close_alternative(begin_term + 1);
        let end_term = self.terms.len();
        let invert = self.terms[begin_term].invert;
        let direction = self.terms[begin_term].direction;
        let subpattern_id = self.terms[begin_term].subpattern_id.unwrap_or(0);

        let idx = self.push(BytecodeTermKind::ParentheticalAssertionEnd);
        self.terms[idx].subpattern_id = Some(subpattern_id);
        self.terms[idx].invert = invert;
        self.terms[idx].direction = direction;
        // first > last encodes "no captures" (C++ containsAnyCaptures()).
        self.terms[idx].subpattern_range = Some(BytecodeSubpatternRange {
            first_subpattern_id: subpattern_id,
            last_subpattern_id,
        });
        let width = (end_term - begin_term) as u32;
        self.terms[begin_term].parentheses_width = Some(width);
        self.terms[idx].parentheses_width = Some(width);
        self.set_frame(idx, frame_location);
        // Quantity stays FixedCount/1 (assertions match at most once) — the default.
    }

    fn atom_parentheses_once_end(
        &mut self,
        input_position: u32,
        frame_location: u32,
        quantity_min_count: u32,
        quantity_max_count: u32,
        quantity_type: QuantifierType,
    ) {
        let begin_term = self.pop_parentheses_stack();
        self.close_alternative(begin_term + 1);
        let end_term = self.terms.len();
        let capture = self.terms[begin_term].capture;
        let subpattern_id = self.terms[begin_term].subpattern_id.unwrap_or(0);
        let begin_direction = self.terms[begin_term].direction;

        let idx = self.push(BytecodeTermKind::ParenthesesSubpatternOnceEnd);
        self.terms[idx].subpattern_id = Some(subpattern_id);
        self.terms[idx].capture = capture;
        self.terms[idx].input_position = input_position;
        self.terms[idx].subpattern_range = Some(BytecodeSubpatternRange {
            first_subpattern_id: subpattern_id,
            last_subpattern_id: subpattern_id,
        });
        if begin_direction == MatchDirection::Backward {
            // Swap input positions for backward captures (YarrInterpreter.cpp:2588).
            let begin_input = self.terms[begin_term].input_position;
            self.terms[idx].input_position = begin_input;
            self.terms[begin_term].input_position = input_position;
        }
        let width = (end_term - begin_term) as u32;
        self.terms[begin_term].parentheses_width = Some(width);
        self.terms[idx].parentheses_width = Some(width);
        self.set_frame(idx, frame_location);
        self.terms[idx].direction = begin_direction;
        self.terms[begin_term].quantifier =
            make_quantifier(quantity_type, quantity_min_count, quantity_max_count);
        self.terms[idx].quantifier =
            make_quantifier(quantity_type, quantity_min_count, quantity_max_count);
    }

    fn atom_parentheses_terminal_end(
        &mut self,
        input_position: u32,
        frame_location: u32,
        quantity_min_count: u32,
        quantity_max_count: u32,
        quantity_type: QuantifierType,
    ) {
        let begin_term = self.pop_parentheses_stack();
        self.close_alternative(begin_term + 1);
        let end_term = self.terms.len();
        let begin_direction = self.terms[begin_term].direction;
        let input_position = if begin_direction == MatchDirection::Backward {
            0
        } else {
            input_position
        };
        let capture = self.terms[begin_term].capture;
        let subpattern_id = self.terms[begin_term].subpattern_id.unwrap_or(0);

        let idx = self.push(BytecodeTermKind::ParenthesesSubpatternTerminalEnd);
        self.terms[idx].subpattern_id = Some(subpattern_id);
        self.terms[idx].capture = capture;
        self.terms[idx].input_position = input_position;
        self.terms[idx].subpattern_range = Some(BytecodeSubpatternRange {
            first_subpattern_id: subpattern_id,
            last_subpattern_id: subpattern_id,
        });
        let width = (end_term - begin_term) as u32;
        self.terms[begin_term].parentheses_width = Some(width);
        self.terms[idx].parentheses_width = Some(width);
        self.set_frame(idx, frame_location);
        self.terms[begin_term].quantifier =
            make_quantifier(quantity_type, quantity_min_count, quantity_max_count);
        self.terms[idx].quantifier =
            make_quantifier(quantity_type, quantity_min_count, quantity_max_count);
    }

    #[allow(clippy::too_many_arguments)]
    fn atom_parentheses_subpattern_end(
        &mut self,
        last_subpattern_id: u32,
        input_position: u32,
        frame_location: u32,
        quantity_min_count: u32,
        quantity_max_count: u32,
        quantity_type: QuantifierType,
        call_frame_size: u32,
    ) {
        let begin_term = self.pop_parentheses_stack();
        self.close_alternative(begin_term + 1);
        let end_term = self.terms.len();
        let parentheses_match_direction = self.terms[begin_term].direction;
        let capture = self.terms[begin_term].capture;
        let subpattern_id = self.terms[begin_term].subpattern_id.unwrap_or(0);
        let num_subpatterns = if last_subpattern_id >= subpattern_id {
            last_subpattern_id - subpattern_id + 1
        } else {
            0
        };

        // Extract begin_term+1 .. end_term into a fresh ByteDisjunction wrapped in
        // SubpatternBegin/End (YarrInterpreter.cpp:2536-2560).
        let mut sub_terms: Vec<BytecodeTerm> = Vec::with_capacity(end_term - begin_term + 1);
        sub_terms
            .push(self.make_subpattern_marker(BytecodeTermKind::SubpatternBegin, subpattern_id));
        for i in (begin_term + 1)..end_term {
            sub_terms.push(self.terms[i].clone());
        }
        sub_terms.push(self.make_subpattern_marker(BytecodeTermKind::SubpatternEnd, subpattern_id));

        self.terms.truncate(begin_term);

        let parentheses_index = self.all_parentheses.len() as u32;
        let pidx = self.push(BytecodeTermKind::ParenthesesSubpattern);
        self.terms[pidx].subpattern_id = Some(subpattern_id);
        self.terms[pidx].parentheses_disjunction = Some(parentheses_index);
        self.terms[pidx].capture = capture;
        self.terms[pidx].input_position = input_position;
        self.terms[pidx].direction = parentheses_match_direction;
        self.terms[pidx].subpattern_range = Some(BytecodeSubpatternRange {
            first_subpattern_id: subpattern_id,
            last_subpattern_id,
        });
        self.set_frame(pidx, frame_location);
        self.terms[pidx].quantifier =
            make_quantifier(quantity_type, quantity_min_count, quantity_max_count);

        self.all_parentheses.push(ByteDisjunction {
            terms: sub_terms,
            subpattern_count: num_subpatterns,
            frame_size: call_frame_size,
        });
    }

    fn make_subpattern_marker(&self, kind: BytecodeTermKind, subpattern_id: u32) -> BytecodeTerm {
        let mut term =
            BytecodeTermBuilder::new(BytecodeTermId(0), kind, self.flags).build_unchecked();
        term.subpattern_id = Some(subpattern_id);
        term
    }

    // ---- alternative jump linking (YarrInterpreter.cpp:2476-2534) ----

    fn close_alternative(&mut self, begin_term: usize) {
        let orig_begin_term = begin_term;
        let end_index = self.terms.len();
        let frame_location = self.frame_location_of(begin_term);
        if self.alt_jump(begin_term).next == 0 {
            self.terms.remove(begin_term);
        } else {
            let mut bt = begin_term;
            while self.alt_jump(bt).next != 0 {
                bt = (bt as i32 + self.alt_jump(bt).next) as usize;
                let mut j = self.alt_jump(bt);
                j.end = end_index as i32 - bt as i32;
                self.set_alt_jump(bt, j);
                self.set_frame(bt, frame_location);
            }
            let mut j = self.alt_jump(bt);
            j.next = orig_begin_term as i32 - bt as i32;
            self.set_alt_jump(bt, j);
            let end_idx = self.push(BytecodeTermKind::AlternativeEnd);
            self.set_alt_jump(end_idx, EMPTY_ALT_JUMP);
            self.set_frame(end_idx, frame_location);
        }
    }

    fn close_body_alternative(&mut self) {
        let begin_term = 0usize;
        let orig_begin_term = 0usize;
        let end_index = self.terms.len();
        let frame_location = self.frame_location_of(begin_term);
        let mut bt = begin_term;
        while self.alt_jump(bt).next != 0 {
            bt = (bt as i32 + self.alt_jump(bt).next) as usize;
            let mut j = self.alt_jump(bt);
            j.end = end_index as i32 - bt as i32;
            self.set_alt_jump(bt, j);
            self.set_frame(bt, frame_location);
        }
        let mut j = self.alt_jump(bt);
        j.next = orig_begin_term as i32 - bt as i32;
        self.set_alt_jump(bt, j);
        let end_idx = self.push(BytecodeTermKind::BodyAlternativeEnd);
        self.set_alt_jump(end_idx, EMPTY_ALT_JUMP);
        self.set_frame(end_idx, frame_location);
    }

    fn regex_begin(&mut self, once_through: bool) {
        let idx = self.push(BytecodeTermKind::BodyAlternativeBegin);
        self.set_alt_jump(
            idx,
            BytecodeAlternativeJump {
                next: 0,
                end: 0,
                once_through,
            },
        );
        self.set_frame(idx, 0);
        self.current_alternative_index = 0;
    }

    fn regex_end(&mut self) {
        self.close_body_alternative();
    }

    fn alternative_body_disjunction(&mut self, once_through: bool) {
        let new_index = self.terms.len();
        let cur = self.current_alternative_index;
        let mut j = self.alt_jump(cur);
        j.next = new_index as i32 - cur as i32;
        self.set_alt_jump(cur, j);
        let idx = self.push(BytecodeTermKind::BodyAlternativeDisjunction);
        self.set_alt_jump(
            idx,
            BytecodeAlternativeJump {
                next: 0,
                end: 0,
                once_through,
            },
        );
        self.current_alternative_index = new_index;
    }

    fn alternative_disjunction(&mut self) {
        let new_index = self.terms.len();
        let cur = self.current_alternative_index;
        let mut j = self.alt_jump(cur);
        j.next = new_index as i32 - cur as i32;
        self.set_alt_jump(cur, j);
        let idx = self.push(BytecodeTermKind::AlternativeDisjunction);
        self.set_alt_jump(idx, EMPTY_ALT_JUMP);
        self.current_alternative_index = new_index;
    }

    // ---- emitDisjunction (YarrInterpreter.cpp:2670-2890) ----

    fn emit_disjunction(
        &mut self,
        d: usize,
        input_count_already_checked: u32,
        parentheses_input_count_already_checked: u32,
        match_direction: MatchDirection,
    ) -> Result<(), YarrBytecodeAssemblyError> {
        let parsed = self.parsed;
        let is_body = parsed.disjunctions[d].is_body;
        let disjunction_minimum_size = parsed.disjunctions[d].minimum_size.unwrap_or(0);
        let nalt = parsed.disjunctions[d].alternatives.len();
        for alt in 0..nalt {
            let mut current_count_already_checked = input_count_already_checked;
            let once_through = parsed.disjunctions[d].alternatives[alt].once_through;
            let minimum_size = parsed.disjunctions[d].alternatives[alt]
                .minimum_size
                .unwrap_or(0);

            if alt != 0 {
                if is_body {
                    self.alternative_body_disjunction(once_through);
                } else {
                    self.alternative_disjunction();
                }
            }

            let mut count_to_check = 0u32;
            let mut backward_uncheck_amount = 0u32;
            if match_direction == MatchDirection::Forward {
                count_to_check =
                    minimum_size.saturating_sub(parentheses_input_count_already_checked);
            } else {
                let min_already_checked =
                    disjunction_minimum_size.min(parentheses_input_count_already_checked);
                if minimum_size > min_already_checked {
                    count_to_check = minimum_size - min_already_checked;
                    let checked_input =
                        count_to_check.saturating_add(current_count_already_checked);
                    self.have_checked_input(checked_input);
                    backward_uncheck_amount = if minimum_size > disjunction_minimum_size {
                        count_to_check
                    } else {
                        minimum_size
                    };
                }
            }
            if count_to_check != 0 {
                if match_direction == MatchDirection::Forward {
                    self.check_input(count_to_check);
                }
                current_count_already_checked =
                    current_count_already_checked.saturating_add(count_to_check);
            }

            let term_count = parsed.disjunctions[d].alternatives[alt].terms.len();
            for i in 0..term_count {
                let term_index = if match_direction == MatchDirection::Forward {
                    i
                } else {
                    term_count - 1 - i
                };
                self.emit_term(
                    d,
                    alt,
                    term_index,
                    current_count_already_checked,
                    match_direction,
                )?;
            }

            if match_direction == MatchDirection::Backward && backward_uncheck_amount != 0 {
                self.uncheck_input(backward_uncheck_amount);
            }
        }
        Ok(())
    }

    fn emit_term(
        &mut self,
        d: usize,
        alt: usize,
        t: usize,
        current_count_already_checked: u32,
        match_direction: MatchDirection,
    ) -> Result<(), YarrBytecodeAssemblyError> {
        let parsed = self.parsed;
        let term = &parsed.disjunctions[d].alternatives[alt].terms[t];
        let current_input_position =
            current_count_already_checked.saturating_sub(term.input_position);
        match term.kind {
            PatternTermKind::Assertion(PatternAssertion::Bol) => {
                self.assertion_bol(current_input_position, term.flags);
            }
            PatternTermKind::Assertion(PatternAssertion::Eol) => {
                self.assertion_eol(current_input_position, term.flags);
            }
            PatternTermKind::Assertion(PatternAssertion::WordBoundary) => {
                self.assertion_word_boundary(
                    term.invert,
                    match_direction,
                    current_input_position,
                    term.flags,
                );
            }
            PatternTermKind::Assertion(PatternAssertion::NotWordBoundary) => {
                self.assertion_word_boundary(
                    true,
                    match_direction,
                    current_input_position,
                    term.flags,
                );
            }
            PatternTermKind::PatternCharacter => {
                let ch = term
                    .character
                    .ok_or(YarrBytecodeAssemblyError::MissingPayload {
                        index: t,
                        kind: term.kind,
                    })?;
                self.atom_pattern_character(
                    ch,
                    match_direction,
                    current_input_position,
                    term.frame_location,
                    term.quantity_max_count,
                    term.quantity_type,
                    term.flags,
                );
            }
            PatternTermKind::CharacterClass => {
                let class = term.character_class.clone().ok_or(
                    YarrBytecodeAssemblyError::MissingPayload {
                        index: t,
                        kind: term.kind,
                    },
                )?;
                self.atom_character_class(
                    class,
                    term.invert,
                    match_direction,
                    current_input_position,
                    term.frame_location,
                    term.quantity_max_count,
                    term.quantity_type,
                    term.flags,
                );
            }
            PatternTermKind::NumberedBackReference | PatternTermKind::NamedBackReference => {
                let sub = term
                    .subpattern_id
                    .ok_or(YarrBytecodeAssemblyError::MissingPayload {
                        index: t,
                        kind: term.kind,
                    })?;
                self.atom_back_reference(
                    sub,
                    match_direction,
                    current_input_position,
                    term.frame_location,
                    term.quantity_min_count,
                    term.quantity_max_count,
                    term.quantity_type,
                    term.flags,
                );
            }
            PatternTermKind::NumberedForwardReference | PatternTermKind::NamedForwardReference => {}
            PatternTermKind::ParenthesesSubpattern => {
                self.emit_parentheses_subpattern(
                    d,
                    alt,
                    t,
                    current_count_already_checked,
                    match_direction,
                )?;
            }
            PatternTermKind::ParentheticalAssertion
            | PatternTermKind::Assertion(PatternAssertion::LookAhead)
            | PatternTermKind::Assertion(PatternAssertion::NegativeLookAhead)
            | PatternTermKind::Assertion(PatternAssertion::LookBehind)
            | PatternTermKind::Assertion(PatternAssertion::NegativeLookBehind) => {
                self.emit_parenthetical_assertion(d, alt, t, current_count_already_checked)?;
            }
            PatternTermKind::DotStarEnclosure => {
                let (bol, eol) = term
                    .dot_star_anchors
                    .map(|a| (a.bol_anchor, a.eol_anchor))
                    .unwrap_or((false, false));
                self.assertion_dot_star_enclosure(bol, eol);
            }
        }
        Ok(())
    }

    fn emit_parentheses_subpattern(
        &mut self,
        d: usize,
        alt: usize,
        t: usize,
        current_count_already_checked: u32,
        match_direction: MatchDirection,
    ) -> Result<(), YarrBytecodeAssemblyError> {
        let parsed = self.parsed;
        let term = &parsed.disjunctions[d].alternatives[alt].terms[t];
        let parens =
            term.parentheses
                .as_ref()
                .ok_or(YarrBytecodeAssemblyError::MissingPayload {
                    index: t,
                    kind: term.kind,
                })?;
        let child = parens
            .disjunction
            .ok_or(YarrBytecodeAssemblyError::MissingPayload {
                index: t,
                kind: term.kind,
            })? as usize;
        let subpattern_id = parens.subpattern_id;
        let last_subpattern_id = parens.last_subpattern_id;
        let is_copy = parens.is_copy;
        let is_terminal = parens.is_terminal;
        let capture = term.capture;
        let frame_location = term.frame_location;
        let input_position = term.input_position;
        let qmin = term.quantity_min_count;
        let qmax = term.quantity_max_count;
        let qtype = term.quantity_type;
        let delegate_end_input_offset =
            current_count_already_checked.saturating_sub(input_position);
        let child_minimum_size = parsed.disjunctions[child].minimum_size.unwrap_or(0);
        let child_call_frame_size = parsed.disjunctions[child].call_frame_size;

        if qmax == 1 && !is_copy {
            let mut alternative_frame_location = frame_location;
            let disjunction_already_checked_count = if qtype == QuantifierType::FixedCount {
                child_minimum_size
            } else {
                alternative_frame_location += YARR_STACK_PARENTHESES_ONCE;
                0
            };
            self.parentheses_begin(
                BytecodeTermKind::ParenthesesSubpatternOnceBegin,
                subpattern_id,
                match_direction,
                capture,
                disjunction_already_checked_count.saturating_add(delegate_end_input_offset),
                frame_location,
                alternative_frame_location,
            );
            self.emit_disjunction(
                child,
                current_count_already_checked,
                disjunction_already_checked_count,
                match_direction,
            )?;
            self.atom_parentheses_once_end(
                delegate_end_input_offset,
                frame_location,
                qmin,
                qmax,
                qtype,
            );
        } else if is_terminal {
            self.parentheses_begin(
                BytecodeTermKind::ParenthesesSubpatternTerminalBegin,
                subpattern_id,
                match_direction,
                capture,
                delegate_end_input_offset,
                frame_location,
                frame_location + YARR_STACK_PARENTHESES_TERMINAL,
            );
            self.emit_disjunction(child, current_count_already_checked, 0, match_direction)?;
            self.atom_parentheses_terminal_end(
                delegate_end_input_offset,
                frame_location,
                qmin,
                qmax,
                qtype,
            );
        } else {
            self.parentheses_begin(
                BytecodeTermKind::ParenthesesSubpatternOnceBegin,
                subpattern_id,
                match_direction,
                capture,
                delegate_end_input_offset,
                frame_location,
                0,
            );
            self.emit_disjunction(child, current_count_already_checked, 0, match_direction)?;
            self.atom_parentheses_subpattern_end(
                last_subpattern_id,
                delegate_end_input_offset,
                frame_location,
                qmin,
                qmax,
                qtype,
                child_call_frame_size,
            );
        }
        Ok(())
    }

    fn emit_parenthetical_assertion(
        &mut self,
        d: usize,
        alt: usize,
        t: usize,
        current_count_already_checked: u32,
    ) -> Result<(), YarrBytecodeAssemblyError> {
        let parsed = self.parsed;
        let term = &parsed.disjunctions[d].alternatives[alt].terms[t];
        let parens =
            term.parentheses
                .as_ref()
                .ok_or(YarrBytecodeAssemblyError::MissingPayload {
                    index: t,
                    kind: term.kind,
                })?;
        let child = parens
            .disjunction
            .ok_or(YarrBytecodeAssemblyError::MissingPayload {
                index: t,
                kind: term.kind,
            })? as usize;
        let subpattern_id = parens.subpattern_id;
        let last_subpattern_id = parens.last_subpattern_id;
        let invert = term.invert;
        let direction = term.match_direction;
        let frame_location = term.frame_location;
        let alternative_frame_location = frame_location + YARR_STACK_PARENTHETICAL_ASSERTION;
        let positive_input_offset =
            current_count_already_checked.saturating_sub(term.input_position);
        let child_minimum_size = parsed.disjunctions[child].minimum_size.unwrap_or(0);

        if direction == MatchDirection::Forward {
            let mut current = current_count_already_checked;
            let mut uncheck_amount = 0u32;
            if positive_input_offset > child_minimum_size {
                uncheck_amount = positive_input_offset - child_minimum_size;
                self.uncheck_input(uncheck_amount);
                current = current.saturating_sub(uncheck_amount);
            }
            self.atom_parenthetical_assertion_begin(
                subpattern_id,
                invert,
                direction,
                frame_location,
                alternative_frame_location,
            );
            self.emit_disjunction(
                child,
                current,
                positive_input_offset - uncheck_amount,
                direction,
            )?;
            self.atom_parenthetical_assertion_end(last_subpattern_id, frame_location);
            if uncheck_amount != 0 {
                self.check_input(uncheck_amount);
            }
        } else {
            let mut checked = positive_input_offset;
            if child_minimum_size != 0 {
                checked = checked.saturating_add(child_minimum_size);
                if checked > current_count_already_checked && !invert {
                    self.have_checked_input(checked);
                }
            }
            self.atom_parenthetical_assertion_begin(
                subpattern_id,
                invert,
                direction,
                frame_location,
                alternative_frame_location,
            );
            self.emit_disjunction(
                child,
                checked,
                positive_input_offset + child_minimum_size,
                direction,
            )?;
            self.atom_parenthetical_assertion_end(last_subpattern_id, frame_location);
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

/// byteCompile — YarrInterpreter.cpp:2294 / :3218 `ByteCompiler::compile`. Lowers
/// a fully constructed `YarrPattern` (nested disjunction tree + setupOffsets) into
/// a `BytecodePattern` ready for `interpret_bytecode`.
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
        .body()
        .alternatives
        .first()
        .map(|alt| alt.once_through)
        .unwrap_or(false);

    let mut compiler = ByteCompiler::new(parsed);
    compiler.regex_begin(once_through);
    compiler.emit_disjunction(0, 0, 0, MatchDirection::Forward)?;
    compiler.regex_end();

    let mut body_terms = compiler.terms;
    let contains_eol = compiler.contains_eol;
    let mut parentheses = compiler.all_parentheses;

    // The C++ ByteTerm has no identity; the begin/extract/shrink churn desyncs the
    // Rust `BytecodeTermId`s, so renumber each disjunction's terms to 0..len before
    // validating (the interpreter dispatches on term index, not id).
    renumber_terms(&mut body_terms);
    for disjunction in &mut parentheses {
        renumber_terms(&mut disjunction.terms);
    }

    validate_term_sequence(&body_terms)?;
    for disjunction in &parentheses {
        validate_byte_disjunction(disjunction)?;
    }

    // Frame size is the body disjunction's call frame from setupOffsets (the global
    // maximum), NOT the per-term frame reservations (YarrPattern.cpp:1981;
    // ByteDisjunction::m_frameSize = m_callFrameSize).
    let frame_size = parsed.body().call_frame_size;
    let body = ByteDisjunction {
        terms: body_terms.clone(),
        subpattern_count: parsed.capture_count,
        frame_size,
    };

    let mut builder = BytecodePatternBuilder::new(id, parsed.id, body)
        .contains_bol(parsed.contains_bol)
        .contains_eol(contains_eol)
        .offset_vector(BytecodeOffsetVectorLayout {
            // C++ `offsetVectorBaseForNamedCaptures` == (numSubpatterns + 1) * 2:
            // duplicate-named-group slots live AFTER the numbered-capture slots.
            base_for_named_captures: (parsed.capture_count + 1).saturating_mul(2),
            offsets_size: (parsed.capture_count + 1).saturating_mul(2)
                + parsed.duplicate_named_capture_count,
            duplicate_named_group_count: parsed.duplicate_named_capture_count,
        });

    for disjunction in parentheses {
        builder = builder.parentheses(disjunction);
    }
    if let Some(minimum_size) = parsed.body().minimum_size {
        builder = builder.minimum_size(minimum_size);
    }
    for duplicate in 0..parsed.duplicate_named_capture_count {
        builder = builder.duplicate_named_group_for_subpattern(duplicate);
    }
    if !body_terms.is_empty() {
        builder = builder.alternative(BytecodeAlternative {
            begin: BytecodeTermId(0),
            end: BytecodeTermId(body_terms.len() as u32 - 1),
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

fn renumber_terms(terms: &mut [BytecodeTerm]) {
    for (index, term) in terms.iter_mut().enumerate() {
        term.id = BytecodeTermId(index as u32);
    }
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
        // C++ encodes "no captures" as lastSubpatternId < firstSubpatternId
        // (ByteTerm::containsAnyCaptures(): last >= first). A parenthesised group
        // or assertion that wraps no capturing groups therefore carries an empty
        // (first > last) range, so we accept any present range and only reject an
        // absent one.
        BytecodeTermPayloadKind::SubpatternRange => match term.subpattern_range {
            Some(_range) => {
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
            disjunctions: vec![crate::yarr::PatternDisjunction {
                alternatives: vec![crate::yarr::PatternAlternative {
                    terms: vec![crate::yarr::PatternTerm {
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
                        quantity_type: QuantifierType::FixedCount,
                        quantity_min_count: 1,
                        quantity_max_count: 1,
                        frame_location: 0,
                        match_direction: MatchDirection::Forward,
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
            }],
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
            disjunctions: vec![crate::yarr::PatternDisjunction {
                alternatives: Vec::new(),
                parent_subpattern: None,
                is_body: true,
                minimum_size: Some(0),
                call_frame_size: 0,
                has_fixed_size: true,
            }],
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
