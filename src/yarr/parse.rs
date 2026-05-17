//! Yarr parser and pattern descriptors.
//!
//! The parser contract names pattern-tree data produced from a regexp source.
//! It does not tokenize, validate, canonicalize, or build executable bytecode.

use crate::runtime::StringId;
use crate::yarr::{BuiltInCharacterClassId, CharacterRange};

/// RegExp compile mode selected by flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompileMode {
    Legacy,
    Unicode,
    UnicodeSets,
}

/// RegExp flags tracked by Yarr.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegexFlags {
    pub global: bool,
    pub ignore_case: bool,
    pub multiline: bool,
    pub dot_all: bool,
    pub unicode: bool,
    pub unicode_sets: bool,
    pub sticky: bool,
    pub has_indices: bool,
}

/// Parse or syntax error category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum YarrErrorCode {
    NoError,
    PatternTooLarge,
    ParenthesesUnmatched,
    ParenthesesTypeInvalid,
    CharacterClassUnmatched,
    CharacterClassOutOfOrder,
    QuantifierOutOfOrder,
    InvalidBackReference,
    InvalidNamedCapture,
    InvalidUnicodeEscape,
    InvalidUnicodeProperty,
    InvalidClassSetOperation,
}

/// Stable identity for a parsed pattern.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct YarrPatternId(pub u64);

/// Width categories for character classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CharacterClassWidth {
    Unknown,
    BmpOnly,
    NonBmpOnly,
    BmpAndNonBmp,
}

/// Operation used in Unicode set mode character classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CharacterClassSetOperation {
    Union,
    Intersection,
    Subtraction,
}

/// Character class descriptor. Tables are represented by IDs rather than data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharacterClassDescriptor {
    pub built_in: Option<BuiltInCharacterClassId>,
    pub ranges: Vec<CharacterRange>,
    pub strings: Vec<StringId>,
    pub inverted: bool,
    pub width: CharacterClassWidth,
    pub operation: Option<CharacterClassSetOperation>,
    pub in_canonical_form: bool,
}

/// Assertion kind in a parsed pattern.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatternAssertion {
    Bol,
    Eol,
    WordBoundary,
    NotWordBoundary,
    LookAhead,
    NegativeLookAhead,
    LookBehind,
    NegativeLookBehind,
}

/// Parsed term category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatternTermKind {
    Assertion(PatternAssertion),
    PatternCharacter,
    CharacterClass,
    NumberedBackReference,
    NamedBackReference,
    ParenthesesSubpattern,
    ParentheticalAssertion,
    DotStarEnclosure,
}

/// Parsed pattern term.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatternTerm {
    pub kind: PatternTermKind,
    pub input_position: u32,
    pub character: Option<char>,
    pub character_class: Option<CharacterClassDescriptor>,
    pub capture: bool,
    pub invert: bool,
    pub subpattern_id: Option<u32>,
    pub name: Option<StringId>,
    pub flags: RegexFlags,
}

/// One alternative inside a disjunction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatternAlternative {
    pub terms: Vec<PatternTerm>,
    pub minimum_size: Option<u32>,
    pub contains_captures: bool,
}

/// Disjunction tree node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatternDisjunction {
    pub alternatives: Vec<PatternAlternative>,
    pub parent_subpattern: Option<u32>,
    pub is_body: bool,
}

/// Parsed pattern descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrPattern {
    pub id: YarrPatternId,
    pub source: StringId,
    pub flags: RegexFlags,
    pub compile_mode: CompileMode,
    pub body: PatternDisjunction,
    pub capture_count: u32,
    pub named_capture_count: u32,
    pub error: YarrErrorCode,
}
