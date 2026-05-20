//! Yarr parser and pattern descriptors.
//!
//! The parser contract names pattern-tree data and non-executing parse plans
//! produced from a regexp source. It does not match strings or build executable
//! bytecode.

use crate::strings::StringId;
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

/// Individual flag keys as ordered by `YarrFlags.h`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegexFlagKind {
    HasIndices,
    Global,
    IgnoreCase,
    Multiline,
    DotAll,
    Unicode,
    UnicodeSets,
    Sticky,
}

/// RegExp modifier flag accepted inside parenthetical modifier groups.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegexModifierFlagKind {
    IgnoreCase,
    Multiline,
    DotAll,
}

/// Semantic error reported while interpreting RegExp flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegexFlagSemanticError {
    DuplicateFlag(RegexFlagKind),
    IncompatibleUnicodeModes,
    InvalidFlag(char),
}

/// Non-executing interpretation of RegExp flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegexFlagSemanticDescriptor {
    pub flags: RegexFlags,
    pub compile_mode: CompileMode,
    pub unicode_aware: bool,
    pub unicode_sets: bool,
    pub case_insensitive: bool,
    pub multiline_anchors: bool,
    pub dot_matches_line_terminators: bool,
    pub has_indices: bool,
    pub stateful_last_index: bool,
    pub advances_by_code_point: bool,
    pub allows_class_strings: bool,
}

/// Parse or syntax error category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum YarrErrorCode {
    NoError,
    PatternTooLarge,
    QuantifierWithoutAtom,
    QuantifierTooLarge,
    QuantifierIncomplete,
    CantQuantifyAtom,
    MissingParentheses,
    BracketUnmatched,
    ParenthesesUnmatched,
    ParenthesesTypeInvalid,
    InvalidGroupName,
    DuplicateGroupName,
    CharacterClassUnmatched,
    CharacterClassRangeInvalid,
    CharacterClassOutOfOrder,
    ClassStringDisjunctionUnmatched,
    EscapeUnterminated,
    QuantifierOutOfOrder,
    InvalidBackReference,
    InvalidNamedCapture,
    InvalidUnicodeEscape,
    InvalidUnicodeCodePointEscape,
    InvalidUnicodeProperty,
    InvalidIdentityEscape,
    InvalidOctalEscape,
    InvalidControlLetterEscape,
    OffsetTooLarge,
    InvalidRegularExpressionFlags,
    InvalidClassSetOperation,
    NegatedClassSetMayContainStrings,
    InvalidClassSetCharacter,
    InvalidRegularExpressionModifier,
    TooManyDisjunctions,
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
    Default,
    Union,
    Intersection,
    Subtraction,
}

/// Character class construction state used by the parser delegate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CharacterClassConstructionState {
    Empty,
    CachedCharacter,
    AfterCharacterClass,
    CachedCharacterHyphen,
    Poisoned,
}

/// Character class descriptor. Tables are represented by IDs rather than data.
/// Parser construction owns mutation of the ranges and string set; later
/// bytecode and JIT stages may cache lookup tables but must preserve the full
/// declarative contents.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharacterClassDescriptor {
    pub built_in: Option<BuiltInCharacterClassId>,
    pub matches: Vec<char>,
    pub ranges: Vec<CharacterRange>,
    pub unicode_matches: Vec<char>,
    pub unicode_ranges: Vec<CharacterRange>,
    pub strings: Vec<StringId>,
    pub inverted: bool,
    pub table_inverted: bool,
    pub any_character: bool,
    pub width: CharacterClassWidth,
    pub operation: Option<CharacterClassSetOperation>,
    pub in_canonical_form: bool,
}

/// Parser-only context for interpreting Unicode escapes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnicodeParseContext {
    PatternCodePoint,
    GroupName,
}

/// Parser-only escape interpretation mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseEscapeMode {
    Normal,
    CharacterClass,
    ClassSet,
    ClassStringDisjunction,
}

/// Token category returned to the parser delegate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParserTokenKind {
    NotAtom,
    Atom,
    Lookbehind,
    SetDisjunction,
    SetDisjunctionMayContainStrings,
}

/// Why a new disjunction node is being created.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreateDisjunctionPurpose {
    NotForNextAlternative,
    ForNextAlternative,
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
    NumberedForwardReference,
    NamedForwardReference,
    ParenthesesSubpattern,
    ParentheticalAssertion,
    DotStarEnclosure,
}

/// Parenthesized subpattern metadata carried by parsed terms.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatternParenthesesDescriptor {
    pub disjunction: Option<u32>,
    pub subpattern_id: u32,
    pub last_subpattern_id: u32,
    pub is_copy: bool,
    pub is_terminal: bool,
    pub is_string_list: bool,
    pub is_eol_string_list: bool,
}

/// Anchors carried by dot-star enclosure terms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DotStarEnclosureAnchors {
    pub bol_anchor: bool,
    pub eol_anchor: bool,
}

/// Parsed pattern term.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatternTerm {
    pub kind: PatternTermKind,
    pub input_position: u32,
    pub character: Option<char>,
    pub character_class: Option<CharacterClassDescriptor>,
    pub parentheses: Option<PatternParenthesesDescriptor>,
    pub dot_star_anchors: Option<DotStarEnclosureAnchors>,
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
    pub first_subpattern_id: u32,
    pub last_subpattern_id: u32,
    pub direction: crate::yarr::MatchDirection,
    pub once_through: bool,
    pub has_fixed_size: bool,
    pub starts_with_bol: bool,
    pub contains_bol: bool,
    pub is_last_alternative: bool,
    pub contains_captures: bool,
}

/// Disjunction tree node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatternDisjunction {
    pub alternatives: Vec<PatternAlternative>,
    pub parent_subpattern: Option<u32>,
    pub is_body: bool,
    pub minimum_size: Option<u32>,
    pub call_frame_size: u32,
    pub has_fixed_size: bool,
}

/// Named capture bookkeeping owned by parsing and reset before reparsing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamedCaptureGroupState {
    pub all_names: Vec<StringId>,
    pub nested_alternative_names: Vec<Vec<StringId>>,
    pub active_alternative_names: Vec<Vec<StringId>>,
    pub duplicate_group_count: u32,
}

/// Syntax delegate boundary used by parser and syntax checker contracts.
pub trait YarrSyntaxDelegate {
    fn abort_error_code(&self) -> YarrErrorCode;
    fn reset_for_reparsing(&mut self);
}

/// Parsed pattern descriptor.
/// The parser is the sole mutation authority for disjunctions, capture maps,
/// and cached character classes until bytecode adopts them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrPattern {
    pub id: YarrPatternId,
    pub source: StringId,
    pub flags: RegexFlags,
    pub compile_mode: CompileMode,
    pub body: PatternDisjunction,
    pub capture_count: u32,
    pub named_capture_count: u32,
    pub duplicate_named_capture_count: u32,
    pub contains_backreferences: bool,
    pub contains_bol: bool,
    pub contains_lookbehinds: bool,
    pub contains_unsigned_length_pattern: bool,
    pub has_copied_parentheses: bool,
    pub save_initial_start_value: bool,
    pub error: YarrErrorCode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct YarrParseError {
    pub code: YarrErrorCode,
    pub offset: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum YarrParsePlanAtomKind {
    Literal,
    BuiltInCharacterClass(BuiltInCharacterClassId),
    CharacterClass,
    Assertion(PatternAssertion),
    CaptureGroup,
    NonCaptureGroup,
    Lookaround(PatternAssertion),
    BackReference,
    Dot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct YarrParsePlanAtom {
    pub kind: YarrParsePlanAtomKind,
    pub offset: u32,
    pub minimum_size: u32,
    pub quantified: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrParsePlan {
    pub flags: RegexFlags,
    pub compile_mode: CompileMode,
    pub atoms: Vec<YarrParsePlanAtom>,
    pub capture_count: u32,
    pub named_capture_count: u32,
    pub disjunction_count: u32,
    pub character_class_count: u32,
    pub max_group_depth: u32,
    pub minimum_size: u32,
    pub contains_backreferences: bool,
    pub contains_bol: bool,
    pub contains_lookbehinds: bool,
}

/// Semantic summary for a parsed RegExp artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegExpParseSemanticDescriptor {
    pub flags: RegexFlagSemanticDescriptor,
    pub atom_count: usize,
    pub capture_count: u32,
    pub named_capture_count: u32,
    pub disjunction_count: u32,
    pub character_class_count: u32,
    pub max_group_depth: u32,
    pub minimum_input_length: u32,
    pub may_match_empty_input: bool,
    pub contains_backreferences: bool,
    pub contains_lookbehinds: bool,
    pub contains_line_start_assertions: bool,
    pub contains_boundary_assertions: bool,
    pub contains_unicode_sensitive_atoms: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GroupFrame {
    assertion: Option<PatternAssertion>,
}

pub fn compile_mode_for_flags(flags: RegexFlags) -> CompileMode {
    if flags.unicode_sets {
        CompileMode::UnicodeSets
    } else if flags.unicode {
        CompileMode::Unicode
    } else {
        CompileMode::Legacy
    }
}

pub fn parse_regex_flags(flags: &str) -> Result<RegexFlags, RegexFlagSemanticError> {
    let mut parsed = RegexFlags::default();
    for flag in flags.chars() {
        let kind = match flag {
            'd' => RegexFlagKind::HasIndices,
            'g' => RegexFlagKind::Global,
            'i' => RegexFlagKind::IgnoreCase,
            'm' => RegexFlagKind::Multiline,
            's' => RegexFlagKind::DotAll,
            'u' => RegexFlagKind::Unicode,
            'v' => RegexFlagKind::UnicodeSets,
            'y' => RegexFlagKind::Sticky,
            other => return Err(RegexFlagSemanticError::InvalidFlag(other)),
        };
        if regex_flags_contain(parsed, kind) {
            return Err(RegexFlagSemanticError::DuplicateFlag(kind));
        }
        set_regex_flag(&mut parsed, kind);
    }

    validate_regex_flag_semantics(parsed)?;
    Ok(parsed)
}

pub fn validate_regex_flag_semantics(flags: RegexFlags) -> Result<(), RegexFlagSemanticError> {
    if flags.unicode && flags.unicode_sets {
        return Err(RegexFlagSemanticError::IncompatibleUnicodeModes);
    }
    Ok(())
}

pub fn describe_regex_flag_semantics(
    flags: RegexFlags,
) -> Result<RegexFlagSemanticDescriptor, RegexFlagSemanticError> {
    validate_regex_flag_semantics(flags)?;
    let compile_mode = compile_mode_for_flags(flags);
    Ok(RegexFlagSemanticDescriptor {
        flags,
        compile_mode,
        unicode_aware: flags.unicode || flags.unicode_sets,
        unicode_sets: flags.unicode_sets,
        case_insensitive: flags.ignore_case,
        multiline_anchors: flags.multiline,
        dot_matches_line_terminators: flags.dot_all,
        has_indices: flags.has_indices,
        stateful_last_index: flags.global || flags.sticky,
        advances_by_code_point: flags.unicode || flags.unicode_sets,
        allows_class_strings: flags.unicode_sets,
    })
}

pub fn describe_yarr_parse_semantics(
    plan: &YarrParsePlan,
) -> Result<RegExpParseSemanticDescriptor, RegexFlagSemanticError> {
    let flags = describe_regex_flag_semantics(plan.flags)?;
    let contains_boundary_assertions = plan.atoms.iter().any(|atom| {
        matches!(
            atom.kind,
            YarrParsePlanAtomKind::Assertion(PatternAssertion::WordBoundary)
                | YarrParsePlanAtomKind::Assertion(PatternAssertion::NotWordBoundary)
        )
    });
    let contains_unicode_sensitive_atoms = flags.unicode_aware
        && plan.atoms.iter().any(|atom| {
            matches!(
                atom.kind,
                YarrParsePlanAtomKind::BuiltInCharacterClass(_)
                    | YarrParsePlanAtomKind::CharacterClass
                    | YarrParsePlanAtomKind::Dot
            )
        });

    Ok(RegExpParseSemanticDescriptor {
        flags,
        atom_count: plan.atoms.len(),
        capture_count: plan.capture_count,
        named_capture_count: plan.named_capture_count,
        disjunction_count: plan.disjunction_count,
        character_class_count: plan.character_class_count,
        max_group_depth: plan.max_group_depth,
        minimum_input_length: plan.minimum_size,
        may_match_empty_input: plan.minimum_size == 0,
        contains_backreferences: plan.contains_backreferences,
        contains_lookbehinds: plan.contains_lookbehinds,
        contains_line_start_assertions: plan.contains_bol,
        contains_boundary_assertions,
        contains_unicode_sensitive_atoms,
    })
}

fn regex_flags_contain(flags: RegexFlags, kind: RegexFlagKind) -> bool {
    match kind {
        RegexFlagKind::HasIndices => flags.has_indices,
        RegexFlagKind::Global => flags.global,
        RegexFlagKind::IgnoreCase => flags.ignore_case,
        RegexFlagKind::Multiline => flags.multiline,
        RegexFlagKind::DotAll => flags.dot_all,
        RegexFlagKind::Unicode => flags.unicode,
        RegexFlagKind::UnicodeSets => flags.unicode_sets,
        RegexFlagKind::Sticky => flags.sticky,
    }
}

fn set_regex_flag(flags: &mut RegexFlags, kind: RegexFlagKind) {
    match kind {
        RegexFlagKind::HasIndices => flags.has_indices = true,
        RegexFlagKind::Global => flags.global = true,
        RegexFlagKind::IgnoreCase => flags.ignore_case = true,
        RegexFlagKind::Multiline => flags.multiline = true,
        RegexFlagKind::DotAll => flags.dot_all = true,
        RegexFlagKind::Unicode => flags.unicode = true,
        RegexFlagKind::UnicodeSets => flags.unicode_sets = true,
        RegexFlagKind::Sticky => flags.sticky = true,
    }
}

pub fn plan_yarr_parse(source: &str, flags: RegexFlags) -> Result<YarrParsePlan, YarrParseError> {
    let parser = ParsePlanner {
        source,
        bytes: source.as_bytes(),
        flags,
        offset: 0,
        atoms: Vec::new(),
        groups: Vec::new(),
        capture_count: 0,
        named_capture_count: 0,
        disjunction_count: 1,
        character_class_count: 0,
        max_group_depth: 0,
        minimum_size: 0,
        contains_backreferences: false,
        contains_bol: false,
        contains_lookbehinds: false,
        last_atom: None,
    };
    parser.parse()
}

struct ParsePlanner<'a> {
    source: &'a str,
    bytes: &'a [u8],
    flags: RegexFlags,
    offset: usize,
    atoms: Vec<YarrParsePlanAtom>,
    groups: Vec<GroupFrame>,
    capture_count: u32,
    named_capture_count: u32,
    disjunction_count: u32,
    character_class_count: u32,
    max_group_depth: u32,
    minimum_size: u32,
    contains_backreferences: bool,
    contains_bol: bool,
    contains_lookbehinds: bool,
    last_atom: Option<usize>,
}

impl<'a> ParsePlanner<'a> {
    fn parse(mut self) -> Result<YarrParsePlan, YarrParseError> {
        while self.offset < self.bytes.len() {
            let start = self.offset;
            let byte = self.bytes[self.offset];
            match byte {
                b'\\' => self.parse_escape(false)?,
                b'[' => self.parse_character_class()?,
                b'(' => self.parse_group()?,
                b')' => self.close_group(start)?,
                b'|' => {
                    self.disjunction_count = self.disjunction_count.saturating_add(1);
                    self.last_atom = None;
                    self.offset += 1;
                }
                b'*' | b'+' | b'?' => self.parse_simple_quantifier(start)?,
                b'{' => {
                    if self.try_parse_braced_quantifier(start)? {
                        continue;
                    }
                    self.push_atom(YarrParsePlanAtomKind::Literal, start, 1);
                    self.offset += 1;
                }
                b'^' => {
                    self.contains_bol = true;
                    self.push_atom(
                        YarrParsePlanAtomKind::Assertion(PatternAssertion::Bol),
                        start,
                        0,
                    );
                    self.offset += 1;
                }
                b'$' => {
                    self.push_atom(
                        YarrParsePlanAtomKind::Assertion(PatternAssertion::Eol),
                        start,
                        0,
                    );
                    self.offset += 1;
                }
                b'.' => {
                    self.push_atom(YarrParsePlanAtomKind::Dot, start, 1);
                    self.offset += 1;
                }
                _ => {
                    let Some((_, width)) = self.source[self.offset..].char_indices().next() else {
                        return self.error(YarrErrorCode::PatternTooLarge, start);
                    };
                    self.push_atom(YarrParsePlanAtomKind::Literal, start, 1);
                    self.offset += width.len_utf8();
                }
            }
        }

        if !self.groups.is_empty() {
            return self.error(YarrErrorCode::ParenthesesUnmatched, self.bytes.len());
        }

        Ok(YarrParsePlan {
            flags: self.flags,
            compile_mode: compile_mode_for_flags(self.flags),
            atoms: self.atoms,
            capture_count: self.capture_count,
            named_capture_count: self.named_capture_count,
            disjunction_count: self.disjunction_count,
            character_class_count: self.character_class_count,
            max_group_depth: self.max_group_depth,
            minimum_size: self.minimum_size,
            contains_backreferences: self.contains_backreferences,
            contains_bol: self.contains_bol,
            contains_lookbehinds: self.contains_lookbehinds,
        })
    }

    fn parse_escape(&mut self, in_class: bool) -> Result<(), YarrParseError> {
        let start = self.offset;
        self.offset += 1;
        if self.offset >= self.bytes.len() {
            return self.error(YarrErrorCode::EscapeUnterminated, start);
        }

        let byte = self.bytes[self.offset];
        if in_class {
            self.offset += 1;
            return Ok(());
        }

        match byte {
            b'd' | b'D' => {
                self.push_atom(
                    YarrParsePlanAtomKind::BuiltInCharacterClass(BuiltInCharacterClassId::Digit),
                    start,
                    1,
                );
                self.offset += 1;
            }
            b's' | b'S' => {
                self.push_atom(
                    YarrParsePlanAtomKind::BuiltInCharacterClass(BuiltInCharacterClassId::Space),
                    start,
                    1,
                );
                self.offset += 1;
            }
            b'w' | b'W' => {
                self.push_atom(
                    YarrParsePlanAtomKind::BuiltInCharacterClass(BuiltInCharacterClassId::Word),
                    start,
                    1,
                );
                self.offset += 1;
            }
            b'b' => {
                self.push_atom(
                    YarrParsePlanAtomKind::Assertion(PatternAssertion::WordBoundary),
                    start,
                    0,
                );
                self.offset += 1;
            }
            b'B' => {
                self.push_atom(
                    YarrParsePlanAtomKind::Assertion(PatternAssertion::NotWordBoundary),
                    start,
                    0,
                );
                self.offset += 1;
            }
            b'u' => self.parse_unicode_escape(start)?,
            b'1'..=b'9' => {
                self.contains_backreferences = true;
                while self.offset < self.bytes.len() && self.bytes[self.offset].is_ascii_digit() {
                    self.offset += 1;
                }
                self.push_atom(YarrParsePlanAtomKind::BackReference, start, 0);
            }
            _ => {
                self.offset += 1;
                self.push_atom(YarrParsePlanAtomKind::Literal, start, 1);
            }
        }
        Ok(())
    }

    fn parse_unicode_escape(&mut self, start: usize) -> Result<(), YarrParseError> {
        self.offset += 1;
        if self.offset < self.bytes.len() && self.bytes[self.offset] == b'{' {
            self.offset += 1;
            let digits_start = self.offset;
            let mut value = 0u32;
            while self.offset < self.bytes.len() && self.bytes[self.offset] != b'}' {
                let Some(digit) = (self.bytes[self.offset] as char).to_digit(16) else {
                    return self.error(YarrErrorCode::InvalidUnicodeCodePointEscape, self.offset);
                };
                value = value.saturating_mul(16).saturating_add(digit);
                self.offset += 1;
            }
            if self.offset == digits_start
                || self.offset >= self.bytes.len()
                || self.bytes[self.offset] != b'}'
                || char::from_u32(value).is_none()
            {
                return self.error(YarrErrorCode::InvalidUnicodeCodePointEscape, start);
            }
            self.offset += 1;
        } else {
            if self.offset + 4 > self.bytes.len() {
                return self.error(YarrErrorCode::InvalidUnicodeEscape, start);
            }
            for index in self.offset..self.offset + 4 {
                if !(self.bytes[index] as char).is_ascii_hexdigit() {
                    return self.error(YarrErrorCode::InvalidUnicodeEscape, index);
                }
            }
            self.offset += 4;
        }
        self.push_atom(YarrParsePlanAtomKind::Literal, start, 1);
        Ok(())
    }

    fn parse_character_class(&mut self) -> Result<(), YarrParseError> {
        let start = self.offset;
        self.offset += 1;
        if self.offset < self.bytes.len() && self.bytes[self.offset] == b'^' {
            self.offset += 1;
        }

        let mut previous_range_start = None;
        let mut saw_member = false;
        while self.offset < self.bytes.len() {
            match self.bytes[self.offset] {
                b']' if saw_member => {
                    self.offset += 1;
                    self.character_class_count = self.character_class_count.saturating_add(1);
                    self.push_atom(YarrParsePlanAtomKind::CharacterClass, start, 1);
                    return Ok(());
                }
                b'\\' => {
                    self.parse_escape(true)?;
                    saw_member = true;
                    previous_range_start = None;
                }
                b'-' if previous_range_start.is_some()
                    && self.offset + 1 < self.bytes.len()
                    && self.bytes[self.offset + 1] != b']' =>
                {
                    let begin = previous_range_start.take().unwrap_or_default();
                    self.offset += 1;
                    let end = self.read_class_scalar()?;
                    if begin > end {
                        return self.error(YarrErrorCode::CharacterClassRangeInvalid, self.offset);
                    }
                    saw_member = true;
                }
                _ => {
                    previous_range_start = Some(self.read_class_scalar()?);
                    saw_member = true;
                }
            }
        }

        self.error(YarrErrorCode::CharacterClassUnmatched, start)
    }

    fn read_class_scalar(&mut self) -> Result<char, YarrParseError> {
        if self.offset >= self.bytes.len() {
            return self.error(YarrErrorCode::CharacterClassUnmatched, self.offset);
        }
        if self.bytes[self.offset] == b'\\' {
            self.offset += 1;
            if self.offset >= self.bytes.len() {
                return self.error(YarrErrorCode::EscapeUnterminated, self.offset - 1);
            }
        }
        let Some((_, character)) = self.source[self.offset..].char_indices().next() else {
            return self.error(YarrErrorCode::CharacterClassUnmatched, self.offset);
        };
        self.offset += character.len_utf8();
        Ok(character)
    }

    fn parse_group(&mut self) -> Result<(), YarrParseError> {
        let start = self.offset;
        self.offset += 1;
        let mut frame = GroupFrame { assertion: None };
        let atom_kind = if self.offset < self.bytes.len() && self.bytes[self.offset] == b'?' {
            self.offset += 1;
            match self.bytes.get(self.offset).copied() {
                Some(b':') => {
                    self.offset += 1;
                    YarrParsePlanAtomKind::NonCaptureGroup
                }
                Some(b'=') => {
                    self.offset += 1;
                    frame.assertion = Some(PatternAssertion::LookAhead);
                    YarrParsePlanAtomKind::Lookaround(PatternAssertion::LookAhead)
                }
                Some(b'!') => {
                    self.offset += 1;
                    frame.assertion = Some(PatternAssertion::NegativeLookAhead);
                    YarrParsePlanAtomKind::Lookaround(PatternAssertion::NegativeLookAhead)
                }
                Some(b'<') => match self.bytes.get(self.offset + 1).copied() {
                    Some(b'=') => {
                        self.offset += 2;
                        self.contains_lookbehinds = true;
                        frame.assertion = Some(PatternAssertion::LookBehind);
                        YarrParsePlanAtomKind::Lookaround(PatternAssertion::LookBehind)
                    }
                    Some(b'!') => {
                        self.offset += 2;
                        self.contains_lookbehinds = true;
                        frame.assertion = Some(PatternAssertion::NegativeLookBehind);
                        YarrParsePlanAtomKind::Lookaround(PatternAssertion::NegativeLookBehind)
                    }
                    _ => {
                        self.parse_group_name()?;
                        self.capture_count = self.capture_count.saturating_add(1);
                        self.named_capture_count = self.named_capture_count.saturating_add(1);
                        YarrParsePlanAtomKind::CaptureGroup
                    }
                },
                _ => return self.error(YarrErrorCode::ParenthesesTypeInvalid, start),
            }
        } else {
            self.capture_count = self.capture_count.saturating_add(1);
            YarrParsePlanAtomKind::CaptureGroup
        };

        self.groups.push(frame);
        self.max_group_depth = self.max_group_depth.max(self.groups.len() as u32);
        self.push_atom(atom_kind, start, 0);
        Ok(())
    }

    fn parse_group_name(&mut self) -> Result<(), YarrParseError> {
        self.offset += 1;
        let name_start = self.offset;
        while self.offset < self.bytes.len() && self.bytes[self.offset] != b'>' {
            self.offset += 1;
        }
        if self.offset == name_start || self.offset >= self.bytes.len() {
            return self.error(YarrErrorCode::InvalidGroupName, name_start);
        }
        self.offset += 1;
        Ok(())
    }

    fn close_group(&mut self, start: usize) -> Result<(), YarrParseError> {
        if self.groups.pop().is_none() {
            return self.error(YarrErrorCode::ParenthesesUnmatched, start);
        }
        self.offset += 1;
        self.last_atom = Some(self.atoms.len().saturating_sub(1));
        Ok(())
    }

    fn parse_simple_quantifier(&mut self, start: usize) -> Result<(), YarrParseError> {
        self.quantify_last(start)?;
        self.offset += 1;
        if self.offset < self.bytes.len() && self.bytes[self.offset] == b'?' {
            self.offset += 1;
        }
        Ok(())
    }

    fn try_parse_braced_quantifier(&mut self, start: usize) -> Result<bool, YarrParseError> {
        let mut index = start + 1;
        if index >= self.bytes.len() || !self.bytes[index].is_ascii_digit() {
            return Ok(false);
        }
        let min = self.read_decimal_at(&mut index)?;
        let max = if index < self.bytes.len() && self.bytes[index] == b',' {
            index += 1;
            if index < self.bytes.len() && self.bytes[index].is_ascii_digit() {
                Some(self.read_decimal_at(&mut index)?)
            } else {
                None
            }
        } else {
            Some(min)
        };
        if index >= self.bytes.len() || self.bytes[index] != b'}' {
            return self.error(YarrErrorCode::QuantifierIncomplete, start);
        }
        if max.map(|value| min > value).unwrap_or(false) {
            return self.error(YarrErrorCode::QuantifierOutOfOrder, start);
        }
        self.quantify_last(start)?;
        self.offset = index + 1;
        if self.offset < self.bytes.len() && self.bytes[self.offset] == b'?' {
            self.offset += 1;
        }
        Ok(true)
    }

    fn read_decimal_at(&self, index: &mut usize) -> Result<u32, YarrParseError> {
        let mut value = 0u32;
        while *index < self.bytes.len() && self.bytes[*index].is_ascii_digit() {
            value = value
                .checked_mul(10)
                .and_then(|current| current.checked_add((self.bytes[*index] - b'0') as u32))
                .ok_or(YarrParseError {
                    code: YarrErrorCode::QuantifierTooLarge,
                    offset: *index as u32,
                })?;
            *index += 1;
        }
        Ok(value)
    }

    fn quantify_last(&mut self, start: usize) -> Result<(), YarrParseError> {
        let Some(index) = self.last_atom else {
            return self.error(YarrErrorCode::QuantifierWithoutAtom, start);
        };
        if self.atoms[index].quantified || self.atoms[index].minimum_size == 0 {
            return self.error(YarrErrorCode::CantQuantifyAtom, start);
        }
        self.atoms[index].quantified = true;
        Ok(())
    }

    fn push_atom(&mut self, kind: YarrParsePlanAtomKind, offset: usize, minimum_size: u32) {
        if minimum_size > 0 {
            self.minimum_size = self.minimum_size.saturating_add(minimum_size);
        }
        self.atoms.push(YarrParsePlanAtom {
            kind,
            offset: offset as u32,
            minimum_size,
            quantified: false,
        });
        self.last_atom = (minimum_size > 0).then_some(self.atoms.len() - 1);
    }

    fn error<T>(&self, code: YarrErrorCode, offset: usize) -> Result<T, YarrParseError> {
        Err(YarrParseError {
            code,
            offset: offset as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plan_counts_groups_classes_and_disjunctions() {
        let plan = plan_yarr_parse(r"^(?<word>\w+)|[a-c]{2}$", RegexFlags::default()).unwrap();

        assert_eq!(plan.compile_mode, CompileMode::Legacy);
        assert_eq!(plan.capture_count, 1);
        assert_eq!(plan.named_capture_count, 1);
        assert_eq!(plan.disjunction_count, 2);
        assert_eq!(plan.character_class_count, 1);
        assert!(plan.contains_bol);
        assert!(plan.minimum_size >= 2);
    }

    #[test]
    fn parse_plan_rejects_quantifier_without_atom() {
        let error = plan_yarr_parse("*abc", RegexFlags::default()).unwrap_err();

        assert_eq!(error.code, YarrErrorCode::QuantifierWithoutAtom);
        assert_eq!(error.offset, 0);
    }

    #[test]
    fn parse_plan_rejects_unmatched_character_class() {
        let error = plan_yarr_parse("[abc", RegexFlags::default()).unwrap_err();

        assert_eq!(error.code, YarrErrorCode::CharacterClassUnmatched);
    }

    #[test]
    fn parse_plan_selects_unicode_sets_mode() {
        let flags = RegexFlags {
            unicode: true,
            unicode_sets: true,
            ..RegexFlags::default()
        };
        let plan = plan_yarr_parse(r"\u{41}", flags).unwrap();

        assert_eq!(plan.compile_mode, CompileMode::UnicodeSets);
    }

    #[test]
    fn regex_flag_semantics_reject_duplicate_and_incompatible_modes() {
        assert_eq!(
            parse_regex_flags("gg").unwrap_err(),
            RegexFlagSemanticError::DuplicateFlag(RegexFlagKind::Global)
        );
        assert_eq!(
            parse_regex_flags("uv").unwrap_err(),
            RegexFlagSemanticError::IncompatibleUnicodeModes
        );
    }

    #[test]
    fn parse_semantics_describe_stateful_unicode_pattern() {
        let flags = parse_regex_flags("dgv").unwrap();
        let plan = plan_yarr_parse(r"\w+", flags).unwrap();
        let descriptor = describe_yarr_parse_semantics(&plan).unwrap();

        assert!(descriptor.flags.stateful_last_index);
        assert!(descriptor.flags.has_indices);
        assert!(descriptor.flags.allows_class_strings);
        assert!(descriptor.contains_unicode_sensitive_atoms);
        assert!(!descriptor.may_match_empty_input);
    }
}
