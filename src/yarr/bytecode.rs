//! Yarr bytecode descriptors.
//!
//! Bytecode here is a descriptive IR contract. The interpreter and compiler
//! that will execute or emit it are intentionally absent.

use crate::runtime::StringId;
use crate::yarr::{CharacterClassDescriptor, MatchDirection, RegexFlags, YarrPatternId};

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
    PatternCharacter,
    PatternCasedCharacter,
    CharacterClass,
    BackReference,
    ParenthesesSubpattern,
    ParentheticalAssertion,
    CheckInput,
    UncheckInput,
    DotStarEnclosure,
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
    pub duplicate_named_group_id: Option<u32>,
    pub name: Option<StringId>,
    pub quantifier: Quantifier,
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

/// Bytecode-level pattern descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BytecodePattern {
    pub id: BytecodePatternId,
    pub pattern: YarrPatternId,
    pub alternatives: Vec<BytecodeAlternative>,
    pub terms: Vec<BytecodeTerm>,
    pub frame_size: u32,
    pub minimum_size: Option<u32>,
    pub contains_bol: bool,
    pub contains_eol: bool,
}

/// Complete bytecode program reserved for either interpreter or JIT input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrBytecodeProgram {
    pub pattern: BytecodePattern,
    pub generation: u64,
    pub valid_for_jit: bool,
}
