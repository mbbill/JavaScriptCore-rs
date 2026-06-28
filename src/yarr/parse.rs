//! Yarr parser and pattern descriptors.
//!
//! The parser contract names pattern-tree data and non-executing parse plans
//! produced from a regexp source. It does not match strings or build executable
//! bytecode.

use crate::strings::StringId;
use crate::yarr::{BuiltInCharacterClassId, CharacterRange, MatchDirection};

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
    // C++ YarrErrorCode.h:81 FrameTooLarge is a hard error emitted by
    // setupAlternativeOffsets when the per-disjunction backtrack call frame
    // overflows. Placed before TooManyDisjunctions to match the C++ ordering.
    FrameTooLarge,
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

/// Quantifier family carried by a parsed term. Faithful to C++
/// `QuantifierType` (YarrPattern.h:195): `FixedCount` / `Greedy` / `NonGreedy`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuantifierType {
    FixedCount,
    Greedy,
    NonGreedy,
}

/// Parsed pattern term.
///
/// Mirrors C++ `struct PatternTerm` (YarrPattern.h:227-249): besides the atom
/// payload it carries the quantifier (`quantityType`/`quantityMinCount`/
/// `quantityMaxCount`), the backtrack-frame slot (`frameLocation`) assigned by
/// `setupOffsets`, and the match direction (`m_matchDirection`). These were
/// previously held in a side `term_info` arena while the type was frozen; they
/// now live on the term exactly as C++ stores them.
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
    /// YarrPattern.h:235-237 `quantityType`/`quantityMinCount`/`quantityMaxCount`.
    pub quantity_type: QuantifierType,
    pub quantity_min_count: u32,
    pub quantity_max_count: u32,
    /// YarrPattern.h:249 `frameLocation`, assigned by `setupOffsets`.
    pub frame_location: u32,
    /// YarrPattern.h:233 `m_matchDirection`.
    pub match_direction: crate::yarr::MatchDirection,
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
///
/// Faithful to C++ `class YarrPattern` (YarrPattern.h:769-770): the pattern owns
/// the `m_disjunctions` arena (`disjunctions`, index 0 == `m_body`); nested
/// parentheses/lookaround disjunctions are referenced by
/// `PatternParenthesesDescriptor.disjunction` indices into this arena. The flat
/// `plan_yarr_parse` validator does not build this tree; `construct_yarr_pattern`
/// does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrPattern {
    pub id: YarrPatternId,
    pub source: StringId,
    pub flags: RegexFlags,
    pub compile_mode: CompileMode,
    /// C++ `m_disjunctions` arena (YarrPattern.h:770). `disjunctions[0]` is the
    /// body (`m_body`, YarrPattern.h:769).
    pub disjunctions: Vec<PatternDisjunction>,
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

impl YarrPattern {
    /// Body disjunction (C++ `m_body`, YarrPattern.h:769 == `m_disjunctions[0]`).
    pub fn body(&self) -> &PatternDisjunction {
        &self.disjunctions[0]
    }
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
    atom_index: usize,
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
        let mut assertion = None;
        let atom_kind = if self.offset < self.bytes.len() && self.bytes[self.offset] == b'?' {
            self.offset += 1;
            match self.bytes.get(self.offset).copied() {
                Some(b':') => {
                    self.offset += 1;
                    YarrParsePlanAtomKind::NonCaptureGroup
                }
                Some(b'=') => {
                    self.offset += 1;
                    assertion = Some(PatternAssertion::LookAhead);
                    YarrParsePlanAtomKind::Lookaround(PatternAssertion::LookAhead)
                }
                Some(b'!') => {
                    self.offset += 1;
                    assertion = Some(PatternAssertion::NegativeLookAhead);
                    YarrParsePlanAtomKind::Lookaround(PatternAssertion::NegativeLookAhead)
                }
                Some(b'<') => match self.bytes.get(self.offset + 1).copied() {
                    Some(b'=') => {
                        self.offset += 2;
                        self.contains_lookbehinds = true;
                        assertion = Some(PatternAssertion::LookBehind);
                        YarrParsePlanAtomKind::Lookaround(PatternAssertion::LookBehind)
                    }
                    Some(b'!') => {
                        self.offset += 2;
                        self.contains_lookbehinds = true;
                        assertion = Some(PatternAssertion::NegativeLookBehind);
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

        let atom_index = self.push_atom(atom_kind, start, 0);
        self.last_atom = None;
        let frame = GroupFrame {
            assertion,
            atom_index,
        };
        self.groups.push(frame);
        self.max_group_depth = self.max_group_depth.max(self.groups.len() as u32);
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
        let Some(frame) = self.groups.pop() else {
            return self.error(YarrErrorCode::ParenthesesUnmatched, start);
        };
        self.offset += 1;
        self.last_atom = yarr_parse_plan_atom_can_be_quantified(self.atoms[frame.atom_index].kind)
            .then_some(frame.atom_index);
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
        if self.atoms[index].quantified
            || !yarr_parse_plan_atom_can_be_quantified(self.atoms[index].kind)
        {
            return self.error(YarrErrorCode::CantQuantifyAtom, start);
        }
        self.atoms[index].quantified = true;
        Ok(())
    }

    fn push_atom(
        &mut self,
        kind: YarrParsePlanAtomKind,
        offset: usize,
        minimum_size: u32,
    ) -> usize {
        if minimum_size > 0 {
            self.minimum_size = self.minimum_size.saturating_add(minimum_size);
        }
        let index = self.atoms.len();
        self.atoms.push(YarrParsePlanAtom {
            kind,
            offset: offset as u32,
            minimum_size,
            quantified: false,
        });
        self.last_atom =
            (minimum_size > 0 || yarr_parse_plan_atom_can_be_quantified(kind)).then_some(index);
        index
    }

    fn error<T>(&self, code: YarrErrorCode, offset: usize) -> Result<T, YarrParseError> {
        Err(YarrParseError {
            code,
            offset: offset as u32,
        })
    }
}

fn yarr_parse_plan_atom_can_be_quantified(kind: YarrParsePlanAtomKind) -> bool {
    !matches!(
        kind,
        YarrParsePlanAtomKind::Assertion(_) | YarrParsePlanAtomKind::Lookaround(_)
    )
}

// =====================================================================
// Faithful YarrParser + YarrPatternConstructor (Yarr B1)
//
// C++ truth: yarr/YarrParser.h:84 `Parser` template fires delegate callbacks;
// yarr/YarrPattern.cpp:1097 `YarrPatternConstructor` builds the
// PatternDisjunction/PatternAlternative/PatternTerm tree and
// setupOffsets()/setupDisjunctionOffsets()/setupAlternativeOffsets()
// (YarrPattern.cpp:1981/1938/1792) compute inputPosition / m_minimumSize /
// m_callFrameSize, which the interpreter requires for frame allocation.
//
// This replaces the use of `plan_yarr_parse` (a flat syntax validator) as the
// pattern IR: it constructs the real nested disjunction tree and runs
// setupOffsets. `plan_yarr_parse` is retained only for its older callers/tests.
//
// The PatternTerm/YarrPattern types are now UNFROZEN (the serial coupling is
// resolved): PatternTerm carries quantityType/quantityMinCount/quantityMaxCount/
// frameLocation/m_matchDirection on the term (YarrPattern.h:227-249), and
// YarrPattern owns the `m_disjunctions` arena (YarrPattern.h:769-770), so
// `construct_yarr_pattern` returns a `YarrPattern` directly and the ByteCompiler
// navigates `parentheses.disjunction` indices into `YarrPattern::disjunctions`.
//
// SCOPE: the legacy / non-Unicode subset sufficient for the Octane regexp
// benchmark. Unicode/UnicodeSets canonicalization & class-set `[[..]]`,
// inline modifiers `(?i:..)`, dot-star-enclosure & BOL unrolling optimizations,
// duplicate named-capture groups, lookbehind forward-reference conversion,
// `\p{}` property escapes, and specific-pattern extraction are out of this unit
// and noted at their sites.
// =====================================================================

// yarr/Yarr.h:35-45 backtrack-frame stack-space constants.
const YARR_STACK_PATTERN_CHARACTER: u32 = 2;
const YARR_STACK_CHARACTER_CLASS: u32 = 2;
const YARR_STACK_BACK_REFERENCE: u32 = 3;
const YARR_STACK_ALTERNATIVE: u32 = 1;
const YARR_STACK_PARENTHETICAL_ASSERTION: u32 = 1;
const YARR_STACK_PARENTHESES_ONCE: u32 = 2;
const YARR_STACK_PARENTHESES_TERMINAL: u32 = 1;
const YARR_STACK_PARENTHESES: u32 = 4;
/// yarr/Yarr.h:45 `quantifyInfinite = UINT_MAX`.
const QUANTIFY_INFINITE: u32 = u32::MAX;

/// YarrParser.h:2204 `ParenthesesType`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParenthesesType {
    Subpattern,
    Assertion,
    LookbehindAssertion,
}

/// YarrParser.h token classification used to validate quantifiers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TokenType {
    NotAtom,
    Atom,
    Lookbehind,
}

/// Escape parsing mode (subset of YarrParser.h `ParseEscapeMode`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EscapeMode {
    Normal,
    CharacterClass,
}

/// YarrPattern.cpp:2484 `ParenthesisContext` frame (invert + match direction).
/// Modifier-flag handling is out of this unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ParenthesisContextFrame {
    invert: bool,
    match_direction: MatchDirection,
}

impl Default for ParenthesisContextFrame {
    fn default() -> Self {
        ParenthesisContextFrame {
            invert: false,
            match_direction: MatchDirection::Forward,
        }
    }
}

fn make_term(kind: PatternTermKind, flags: RegexFlags) -> PatternTerm {
    PatternTerm {
        kind,
        input_position: 0,
        character: None,
        character_class: None,
        parentheses: None,
        dot_star_anchors: None,
        capture: false,
        invert: false,
        subpattern_id: None,
        name: None,
        flags,
        // C++ PatternTerm ctor defaults: FixedCount, quantity 1, frame 0, Forward
        // (YarrPattern.h:255-262).
        quantity_type: QuantifierType::FixedCount,
        quantity_min_count: 1,
        quantity_max_count: 1,
        frame_location: 0,
        match_direction: MatchDirection::Forward,
    }
}

fn character_range(begin: char, end: char) -> CharacterRange {
    CharacterRange { begin, end }
}

/// YarrPatternConstructor (YarrPattern.cpp:1097) merged with the Parser
/// (YarrParser.h:84). The C++ Parser is a template parameterized by a delegate;
/// in Rust the parser borrows the constructor and the two are co-located, with
/// method names kept 1:1 with the C++ callbacks. The constructor owns the
/// disjunction arena.
struct YarrPatternConstructor {
    flags: RegexFlags,
    compile_mode: CompileMode,
    // C++ YarrPattern.m_disjunctions arena (index 0 == body).
    disjunctions: Vec<PatternDisjunction>,
    // Navigation stack of (disjunction, alternative); top == current.
    // Models C++ `m_alternative` pointer chasing without parent back-pointers.
    nav: Vec<(usize, usize)>,
    // ParenthesisContext (YarrPattern.cpp:2484): current frame + backing stack.
    paren_current: ParenthesisContextFrame,
    paren_backing: Vec<ParenthesisContextFrame>,
    paren_depth: u32,
    num_subpatterns: u32,
    named_capture_count: u32,
    // name -> first subpatternId (duplicate named groups are out of this unit).
    named_group_to_id: Vec<(String, u32)>,
    contains_backreferences: bool,
    contains_bol: bool,
    contains_lookbehinds: bool,
    contains_unsigned_length_pattern: bool,
    has_copied_parentheses: bool,
    has_named_capture_groups: bool,
    save_initial_start_value: bool,
    error: YarrErrorCode,
}

impl YarrPatternConstructor {
    fn new(flags: RegexFlags, compile_mode: CompileMode) -> Self {
        let mut c = YarrPatternConstructor {
            flags,
            compile_mode,
            disjunctions: Vec::new(),
            nav: Vec::new(),
            paren_current: ParenthesisContextFrame::default(),
            paren_backing: Vec::new(),
            paren_depth: 0,
            num_subpatterns: 0,
            named_capture_count: 0,
            named_group_to_id: Vec::new(),
            contains_backreferences: false,
            contains_bol: false,
            contains_lookbehinds: false,
            contains_unsigned_length_pattern: false,
            has_copied_parentheses: false,
            has_named_capture_groups: false,
            save_initial_start_value: false,
            error: YarrErrorCode::NoError,
        };
        // YarrPattern.cpp:1142 ctor: body disjunction + first alternative.
        let body = c.add_disjunction(None, true);
        let alt = c.add_alternative(body, 1, MatchDirection::Forward);
        c.nav.push((body, alt));
        c
    }

    // YarrPattern.cpp:1155 resetForReparsing.
    fn reset_for_reparsing(&mut self) {
        self.disjunctions.clear();
        self.nav.clear();
        self.paren_current = ParenthesisContextFrame::default();
        self.paren_backing.clear();
        self.paren_depth = 0;
        self.num_subpatterns = 0;
        self.named_capture_count = 0;
        self.named_group_to_id.clear();
        self.contains_backreferences = false;
        self.contains_bol = false;
        self.contains_lookbehinds = false;
        self.contains_unsigned_length_pattern = false;
        self.has_copied_parentheses = false;
        self.has_named_capture_groups = false;
        self.save_initial_start_value = false;
        self.error = YarrErrorCode::NoError;
        let body = self.add_disjunction(None, true);
        let alt = self.add_alternative(body, 1, MatchDirection::Forward);
        self.nav.push((body, alt));
    }

    fn cur(&self) -> (usize, usize) {
        *self
            .nav
            .last()
            .expect("nav stack non-empty during construction")
    }

    fn add_disjunction(&mut self, parent_subpattern: Option<u32>, is_body: bool) -> usize {
        let idx = self.disjunctions.len();
        self.disjunctions.push(PatternDisjunction {
            alternatives: Vec::new(),
            parent_subpattern,
            is_body,
            minimum_size: None,
            call_frame_size: 0,
            has_fixed_size: false,
        });
        idx
    }

    fn add_alternative(
        &mut self,
        disjunction: usize,
        first_subpattern_id: u32,
        direction: MatchDirection,
    ) -> usize {
        let alt = self.disjunctions[disjunction].alternatives.len();
        self.disjunctions[disjunction]
            .alternatives
            .push(PatternAlternative {
                terms: Vec::new(),
                minimum_size: None,
                first_subpattern_id,
                last_subpattern_id: 0,
                direction,
                once_through: false,
                has_fixed_size: false,
                starts_with_bol: false,
                contains_bol: false,
                is_last_alternative: false,
                contains_captures: false,
            });
        alt
    }

    fn append_term(&mut self, term: PatternTerm) {
        let (d, a) = self.cur();
        self.disjunctions[d].alternatives[a].terms.push(term);
    }

    fn remove_last_term(&mut self) {
        let (d, a) = self.cur();
        self.disjunctions[d].alternatives[a].terms.pop();
    }

    // ParenthesisContext push/pop (YarrPattern.cpp:2516/2527).
    fn push_parenthesis_context(&mut self) {
        if self.paren_depth > 0 {
            self.paren_backing.push(self.paren_current);
        }
        self.paren_depth += 1;
    }

    fn pop_parenthesis_context(&mut self) {
        self.paren_depth -= 1;
        if self.paren_depth > 0 {
            self.paren_current = self.paren_backing.pop().unwrap_or_default();
        } else {
            self.paren_current = ParenthesisContextFrame::default();
        }
    }

    fn parenthesis_invert(&self) -> bool {
        self.paren_current.invert
    }

    fn parenthesis_match_direction(&self) -> MatchDirection {
        self.paren_current.match_direction
    }

    fn set_parenthesis_invert(&mut self, invert: bool) {
        self.paren_current.invert = invert;
    }

    fn set_parenthesis_match_direction(&mut self, direction: MatchDirection) {
        self.paren_current.match_direction = direction;
    }

    // ---- delegate callbacks (YarrPattern.cpp:1223-1780) ----

    fn assertion_bol(&mut self) {
        let (d, a) = self.cur();
        if self.disjunctions[d].alternatives[a].terms.is_empty()
            && !self.parenthesis_invert()
            && self.parenthesis_match_direction() == MatchDirection::Forward
        {
            self.disjunctions[d].alternatives[a].starts_with_bol = true;
            self.disjunctions[d].alternatives[a].contains_bol = true;
            self.contains_bol = true;
        }
        let term = make_term(
            PatternTermKind::Assertion(PatternAssertion::Bol),
            self.flags,
        );
        self.append_term(term);
    }

    fn assertion_eol(&mut self) {
        let term = make_term(
            PatternTermKind::Assertion(PatternAssertion::Eol),
            self.flags,
        );
        self.append_term(term);
    }

    fn assertion_word_boundary(&mut self, invert: bool) {
        let kind = if invert {
            PatternAssertion::NotWordBoundary
        } else {
            PatternAssertion::WordBoundary
        };
        let mut term = make_term(PatternTermKind::Assertion(kind), self.flags);
        term.invert = invert;
        self.append_term(term);
    }

    fn atom_pattern_character(&mut self, ch: char) {
        // YarrPattern.cpp:1244: in the legacy/ASCII subset a PatternCharacter is
        // kept and case-insensitivity is applied at match time via the flags.
        // Unicode case-folding of a non-ASCII character into a CharacterClass
        // (YarrPattern.cpp:1253) is out of this unit.
        let mut term = make_term(PatternTermKind::PatternCharacter, self.flags);
        term.character = Some(ch);
        self.append_term(term);
    }

    fn atom_built_in_character_class(&mut self, class_id: BuiltInCharacterClassId, invert: bool) {
        // YarrPattern.cpp:1265 atomBuiltInCharacterClass. Dot keeps its
        // BuiltInCharacterClassId::Dot identity and is resolved against dotAll at
        // match time (C++ lowers it to newline-inverted / anychar at parse).
        let term_invert = match class_id {
            BuiltInCharacterClassId::Dot => false,
            _ => invert,
        };
        let descriptor = CharacterClassDescriptor {
            built_in: Some(class_id),
            matches: Vec::new(),
            ranges: Vec::new(),
            unicode_matches: Vec::new(),
            unicode_ranges: Vec::new(),
            strings: Vec::new(),
            inverted: false,
            table_inverted: false,
            any_character: matches!(class_id, BuiltInCharacterClassId::Dot) && self.flags.dot_all,
            width: CharacterClassWidth::Unknown,
            operation: None,
            in_canonical_form: false,
        };
        let mut term = make_term(PatternTermKind::CharacterClass, self.flags);
        term.character_class = Some(descriptor);
        term.invert = term_invert;
        self.append_term(term);
    }

    // Character class `[...]` construction (YarrPattern.cpp:1402 atomCharacterClassEnd).
    // Full CharacterClassConstructor canonicalization/coalescing is a separate
    // unit (mcts_mem character-classes); here we collect literal members + fold
    // the legacy built-in members so the term carries representative contents.
    fn atom_character_class_end(&mut self, invert: bool, builder: ClassBuilder) {
        let mut matches = builder.matches;
        let mut ranges = builder.ranges;
        let single_builtin = matches.is_empty() && ranges.is_empty() && builder.builtins.len() == 1;
        let mut built_in = None;
        let mut term_invert = invert;
        if single_builtin {
            // e.g. [\d] / [^\d] / [\D]: keep built-in identity, fold negations.
            let (id, builtin_invert) = builder.builtins[0];
            built_in = Some(id);
            term_invert = invert ^ builtin_invert;
        } else {
            // Mixed class: fold non-inverted built-in members. Inverted built-ins
            // inside a mixed class (rare in the Octane subset) are not folded here;
            // full handling belongs to the CharacterClassConstructor unit.
            for (id, builtin_invert) in &builder.builtins {
                if !builtin_invert {
                    let (m, r) = legacy_built_in_members(*id);
                    matches.extend(m);
                    ranges.extend(r);
                }
            }
        }
        let descriptor = CharacterClassDescriptor {
            built_in,
            matches,
            ranges,
            unicode_matches: Vec::new(),
            unicode_ranges: Vec::new(),
            strings: Vec::new(),
            inverted: false,
            table_inverted: false,
            any_character: false,
            width: CharacterClassWidth::Unknown,
            operation: None,
            in_canonical_form: false,
        };
        let mut term = make_term(PatternTermKind::CharacterClass, self.flags);
        term.character_class = Some(descriptor);
        term.invert = term_invert;
        self.append_term(term);
    }

    fn parentheses_descriptor(
        disjunction: usize,
        subpattern_id: u32,
    ) -> PatternParenthesesDescriptor {
        PatternParenthesesDescriptor {
            disjunction: Some(disjunction as u32),
            subpattern_id,
            last_subpattern_id: 0,
            is_copy: false,
            is_terminal: false,
            is_string_list: false,
            is_eol_string_list: false,
        }
    }

    fn atom_parentheses_subpattern_begin(&mut self, capture: bool, group_name: Option<String>) {
        // YarrPattern.cpp:1457 atomParenthesesSubpatternBegin.
        let subpattern_id = self.num_subpatterns + 1;
        if capture {
            self.num_subpatterns += 1;
            if let Some(name) = group_name {
                self.has_named_capture_groups = true;
                self.named_capture_count += 1;
                self.named_group_to_id.push((name, subpattern_id));
            }
        }
        let child = self.add_disjunction(Some(subpattern_id), false);
        let (d, a) = self.cur();
        let mut term = make_term(PatternTermKind::ParenthesesSubpattern, self.flags);
        term.capture = capture;
        term.match_direction = self.parenthesis_match_direction();
        term.parentheses = Some(Self::parentheses_descriptor(child, subpattern_id));
        self.disjunctions[d].alternatives[a].terms.push(term);
        if capture {
            self.disjunctions[d].alternatives[a].contains_captures = true;
        }
        let child_alt = self.add_alternative(
            child,
            self.num_subpatterns,
            self.parenthesis_match_direction(),
        );
        self.nav.push((child, child_alt));
        self.push_parenthesis_context();
    }

    fn atom_parenthetical_assertion_begin(
        &mut self,
        kind: PatternAssertion,
        invert: bool,
        direction: MatchDirection,
    ) {
        // YarrPattern.cpp:1475 atomParentheticalAssertionBegin. The look-around is
        // a PatternTermKind::Assertion(LookAhead/Negative.../LookBehind/...) with a
        // child disjunction; negation is the kind variant, direction is the kind's
        // look direction (C++ stores both on PatternTerm via m_invert/m_matchDirection).
        let subpattern_id = self.num_subpatterns + 1;
        let child = self.add_disjunction(Some(subpattern_id), false);
        let (d, a) = self.cur();
        let mut term = make_term(PatternTermKind::Assertion(kind), self.flags);
        term.invert = invert;
        term.match_direction = direction;
        term.parentheses = Some(Self::parentheses_descriptor(child, subpattern_id));
        self.disjunctions[d].alternatives[a].terms.push(term);
        let child_alt = self.add_alternative(child, self.num_subpatterns, direction);
        self.nav.push((child, child_alt));
        self.push_parenthesis_context();
        self.set_parenthesis_invert(invert);
        self.set_parenthesis_match_direction(direction);
        if direction == MatchDirection::Backward {
            self.contains_lookbehinds = true;
        }
    }

    fn atom_parentheses_end(&mut self) {
        // YarrPattern.cpp:1505 atomParenthesesEnd. Pop back to the alternative that
        // owns the parentheses term (C++ m_alternative = m_parent->m_parent).
        self.nav.pop();
        let (pd, pa) = self.cur();
        let parens_t = self.disjunctions[pd].alternatives[pa].terms.len() - 1;
        let child = self.disjunctions[pd].alternatives[pa].terms[parens_t]
            .parentheses
            .as_ref()
            .and_then(|p| p.disjunction)
            .expect("parentheses term has a child disjunction") as usize;

        let nalt = self.disjunctions[child].alternatives.len();
        let mut num_bol = 0;
        for i in 0..nalt {
            if self.disjunctions[child].alternatives[i].starts_with_bol {
                num_bol += 1;
            }
        }
        self.disjunctions[child].alternatives[nalt - 1].is_last_alternative = true;
        if num_bol > 0 {
            self.disjunctions[pd].alternatives[pa].contains_bol = true;
            if num_bol == nalt {
                self.disjunctions[pd].alternatives[pa].starts_with_bol = true;
            }
        }
        if let Some(p) = self.disjunctions[pd].alternatives[pa].terms[parens_t]
            .parentheses
            .as_mut()
        {
            p.last_subpattern_id = self.num_subpatterns;
        }
        // Modifier-flag restore and lookbehind forward-reference conversion are
        // out of this unit.
        self.pop_parenthesis_context();
    }

    fn back_reference_in_open_capture(&self, subpattern_id: u32) -> bool {
        // YarrPattern.cpp:1570 ancestor walk: a back reference to a still-open
        // enclosing capturing group is a forward reference.
        for k in (1..self.nav.len()).rev() {
            let (pd, pa) = self.nav[k - 1];
            if let Some(term) = self.disjunctions[pd].alternatives[pa].terms.last() {
                if term.kind == PatternTermKind::ParenthesesSubpattern && term.capture {
                    if let Some(p) = &term.parentheses {
                        if p.subpattern_id == subpattern_id {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    fn atom_back_reference(&mut self, subpattern_id: u32) {
        // YarrPattern.cpp:1550 atomBackReference (forward-matching legacy path).
        if subpattern_id > self.num_subpatterns
            || self.back_reference_in_open_capture(subpattern_id)
        {
            let mut term = make_term(PatternTermKind::NumberedForwardReference, self.flags);
            term.subpattern_id = Some(0);
            self.append_term(term);
            return;
        }
        let mut term = make_term(PatternTermKind::NumberedBackReference, self.flags);
        term.subpattern_id = Some(subpattern_id);
        self.append_term(term);
        self.contains_backreferences = true;
    }

    fn atom_named_back_reference(&mut self, name: &str) {
        // YarrPattern.cpp:1584 atomNamedBackReference (simple, non-duplicate path).
        let id = self
            .named_group_to_id
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id);
        match id {
            Some(id) => {
                let mut term = make_term(PatternTermKind::NamedBackReference, self.flags);
                term.subpattern_id = Some(id);
                self.append_term(term);
                self.contains_backreferences = true;
            }
            None => {
                let mut term = make_term(PatternTermKind::NamedForwardReference, self.flags);
                term.subpattern_id = Some(0);
                self.append_term(term);
            }
        }
    }

    fn atom_named_forward_reference(&mut self) {
        // YarrPattern.cpp:1630 atomNamedForwardReference (forward-matching path).
        let mut term = make_term(PatternTermKind::NamedForwardReference, self.flags);
        term.subpattern_id = Some(0);
        self.append_term(term);
    }

    fn disjunction(&mut self, purpose: CreateDisjunctionPurpose) {
        // YarrPattern.cpp:1772 disjunction.
        let (d, a) = self.cur();
        if purpose == CreateDisjunctionPurpose::ForNextAlternative && self.disjunctions[d].is_body {
            self.disjunctions[d].alternatives[a].last_subpattern_id = self.num_subpatterns;
        }
        let new_alt =
            self.add_alternative(d, self.num_subpatterns, self.parenthesis_match_direction());
        *self.nav.last_mut().unwrap() = (d, new_alt);
    }

    fn quantify_atom(&mut self, min: u32, max: u32, greedy: bool) {
        // YarrPattern.cpp:1700 quantifyAtom.
        let (d, a) = self.cur();
        debug_assert!(!self.disjunctions[d].alternatives[a].terms.is_empty());

        if max == 0 {
            // {0}: elide the atom (lookbehind forward-reference popping is out of scope).
            self.remove_last_term();
            return;
        }

        let t = self.disjunctions[d].alternatives[a].terms.len() - 1;
        let kind = self.disjunctions[d].alternatives[a].terms[t].kind;

        if Self::is_parenthetical_assertion(kind) {
            // YarrPattern.cpp:1722: a quantified assertion runs at most once; a
            // zero-minimum quantified assertion is removed.
            if min == 0 {
                self.remove_last_term();
            }
            return;
        }

        let greedy_type = if greedy {
            QuantifierType::Greedy
        } else {
            QuantifierType::NonGreedy
        };
        if min == max {
            self.set_term_quantity(d, a, t, QuantifierType::FixedCount, min, max);
        } else if min == 0
            || (kind == PatternTermKind::ParenthesesSubpattern && self.has_copied_parentheses)
        {
            self.set_term_quantity(d, a, t, greedy_type, min, max);
        } else {
            // YarrPattern.cpp:1746 (forward-direction split): a {min} fixed copy
            // followed by a greedy/lazy {0, max-min} copy. Backward-direction split
            // (lookbehind bodies) is out of this unit.
            self.set_term_quantity(d, a, t, QuantifierType::FixedCount, min, min);
            let copied = self.copy_term(d, a, t);
            self.append_term(copied);
            let new_t = self.disjunctions[d].alternatives[a].terms.len() - 1;
            let new_max = if max == QUANTIFY_INFINITE {
                QUANTIFY_INFINITE
            } else {
                max - min
            };
            self.set_term_quantity(d, a, new_t, greedy_type, 0, new_max);
            if self.disjunctions[d].alternatives[a].terms[new_t].kind
                == PatternTermKind::ParenthesesSubpattern
            {
                if let Some(p) = self.disjunctions[d].alternatives[a].terms[new_t]
                    .parentheses
                    .as_mut()
                {
                    p.is_copy = true;
                }
            }
        }
    }

    fn set_term_quantity(
        &mut self,
        d: usize,
        a: usize,
        t: usize,
        quantity_type: QuantifierType,
        min: u32,
        max: u32,
    ) {
        let term = &mut self.disjunctions[d].alternatives[a].terms[t];
        term.quantity_type = quantity_type;
        term.quantity_min_count = min;
        term.quantity_max_count = max;
    }

    fn is_parenthetical_assertion(kind: PatternTermKind) -> bool {
        matches!(
            kind,
            PatternTermKind::ParentheticalAssertion
                | PatternTermKind::Assertion(PatternAssertion::LookAhead)
                | PatternTermKind::Assertion(PatternAssertion::NegativeLookAhead)
                | PatternTermKind::Assertion(PatternAssertion::LookBehind)
                | PatternTermKind::Assertion(PatternAssertion::NegativeLookBehind)
        )
    }

    // YarrPattern.cpp:1681 copyTerm / :1643 copyDisjunction.
    fn copy_term(&mut self, d: usize, a: usize, t: usize) -> PatternTerm {
        let mut term = self.disjunctions[d].alternatives[a].terms[t].clone();
        let is_paren = matches!(
            term.kind,
            PatternTermKind::ParenthesesSubpattern | PatternTermKind::ParentheticalAssertion
        ) || Self::is_parenthetical_assertion(term.kind);
        if is_paren {
            if let Some(child) = term.parentheses.as_ref().and_then(|p| p.disjunction) {
                let new_child = self.copy_disjunction(child as usize);
                if let Some(p) = term.parentheses.as_mut() {
                    p.disjunction = Some(new_child as u32);
                }
                self.has_copied_parentheses = true;
            }
        }
        term
    }

    fn copy_disjunction(&mut self, src: usize) -> usize {
        let parent_subpattern = self.disjunctions[src].parent_subpattern;
        let is_body = self.disjunctions[src].is_body;
        let new_d = self.add_disjunction(parent_subpattern, is_body);
        let nalt = self.disjunctions[src].alternatives.len();
        for alt in 0..nalt {
            let first = self.disjunctions[src].alternatives[alt].first_subpattern_id;
            let dir = self.disjunctions[src].alternatives[alt].direction;
            let last = self.disjunctions[src].alternatives[alt].last_subpattern_id;
            let new_a = self.add_alternative(new_d, first, dir);
            self.disjunctions[new_d].alternatives[new_a].last_subpattern_id = last;
            let nterms = self.disjunctions[src].alternatives[alt].terms.len();
            for t in 0..nterms {
                let term = self.copy_term(src, alt, t);
                self.disjunctions[new_d].alternatives[new_a]
                    .terms
                    .push(term);
            }
        }
        new_d
    }

    // YarrPattern.cpp:2001 checkForTerminalParentheses (terminal-paren part).
    fn check_for_terminal_parentheses(&mut self) {
        if self.num_subpatterns != 0 {
            return;
        }
        let body = 0;
        let nalt = self.disjunctions[body].alternatives.len();
        if nalt == 0 {
            return;
        }
        self.disjunctions[body].alternatives[nalt - 1].is_last_alternative = true;
        // String-list detection (an additional matching optimization) is omitted.
        for alt in 0..nalt {
            let nterms = self.disjunctions[body].alternatives[alt].terms.len();
            if nterms == 0 {
                continue;
            }
            let t = nterms - 1;
            let term = &self.disjunctions[body].alternatives[alt].terms[t];
            let kind = term.kind;
            let captures = term.capture;
            if kind == PatternTermKind::ParenthesesSubpattern
                && term.quantity_type == QuantifierType::Greedy
                && term.quantity_min_count == 0
                && term.quantity_max_count == QUANTIFY_INFINITE
                && !captures
            {
                if let Some(p) = self.disjunctions[body].alternatives[alt].terms[t]
                    .parentheses
                    .as_mut()
                {
                    p.is_terminal = true;
                }
            }
        }
    }

    // YarrPattern.cpp:1981 setupOffsets -> :1938 setupDisjunctionOffsets -> :1792
    // setupAlternativeOffsets.
    fn setup_offsets(&mut self) -> Result<(), YarrErrorCode> {
        self.setup_disjunction_offsets(0, 0, 0)?;
        Ok(())
    }

    fn setup_disjunction_offsets(
        &mut self,
        d: usize,
        initial_call_frame: u32,
        initial_input: u32,
    ) -> Result<u32, YarrErrorCode> {
        let mut initial = initial_call_frame;
        if !self.disjunctions[d].is_body && self.disjunctions[d].alternatives.len() > 1 {
            initial = initial
                .checked_add(YARR_STACK_ALTERNATIVE)
                .ok_or(YarrErrorCode::FrameTooLarge)?;
        }
        let share_offsets = self.disjunctions[d].is_body;

        let mut minimum_input = u32::MAX;
        let mut maximum_call_frame = 0u32;
        let mut has_fixed_size = true;
        let mut per_alternative_initial = initial;

        let nalt = self.disjunctions[d].alternatives.len();
        for alt in 0..nalt {
            let call_frame =
                self.setup_alternative_offsets(d, alt, per_alternative_initial, initial_input)?;
            let alt_min = self.disjunctions[d].alternatives[alt]
                .minimum_size
                .unwrap_or(0);
            minimum_input = minimum_input.min(alt_min);
            maximum_call_frame = maximum_call_frame.max(call_frame);
            has_fixed_size &= self.disjunctions[d].alternatives[alt].has_fixed_size;
            if alt_min > i32::MAX as u32 {
                self.contains_unsigned_length_pattern = true;
            }
            if !share_offsets {
                per_alternative_initial = call_frame;
            }
        }

        self.disjunctions[d].has_fixed_size = has_fixed_size;
        self.disjunctions[d].minimum_size = Some(if nalt == 0 { 0 } else { minimum_input });
        self.disjunctions[d].call_frame_size = maximum_call_frame;
        Ok(maximum_call_frame)
    }

    fn setup_alternative_offsets(
        &mut self,
        d: usize,
        a: usize,
        current_call_frame: u32,
        initial_input: u32,
    ) -> Result<u32, YarrErrorCode> {
        let mut current_call_frame = current_call_frame;
        let mut current_input = initial_input;
        self.disjunctions[d].alternatives[a].has_fixed_size = true;

        let nterms = self.disjunctions[d].alternatives[a].terms.len();
        for t in 0..nterms {
            let kind = self.disjunctions[d].alternatives[a].terms[t].kind;
            let term_quantity_type = self.disjunctions[d].alternatives[a].terms[t].quantity_type;
            let term_max_count = self.disjunctions[d].alternatives[a].terms[t].quantity_max_count;
            match kind {
                PatternTermKind::Assertion(PatternAssertion::Bol)
                | PatternTermKind::Assertion(PatternAssertion::Eol)
                | PatternTermKind::Assertion(PatternAssertion::WordBoundary)
                | PatternTermKind::Assertion(PatternAssertion::NotWordBoundary) => {
                    self.disjunctions[d].alternatives[a].terms[t].input_position = current_input;
                }

                PatternTermKind::NumberedBackReference | PatternTermKind::NamedBackReference => {
                    self.disjunctions[d].alternatives[a].terms[t].input_position = current_input;
                    self.disjunctions[d].alternatives[a].terms[t].frame_location =
                        current_call_frame;
                    current_call_frame = current_call_frame
                        .checked_add(YARR_STACK_BACK_REFERENCE)
                        .ok_or(YarrErrorCode::FrameTooLarge)?;
                    self.disjunctions[d].alternatives[a].has_fixed_size = false;
                }

                PatternTermKind::NumberedForwardReference
                | PatternTermKind::NamedForwardReference => {}

                PatternTermKind::PatternCharacter => {
                    self.disjunctions[d].alternatives[a].terms[t].input_position = current_input;
                    if term_quantity_type != QuantifierType::FixedCount {
                        self.disjunctions[d].alternatives[a].terms[t].frame_location =
                            current_call_frame;
                        current_call_frame = current_call_frame
                            .checked_add(YARR_STACK_PATTERN_CHARACTER)
                            .ok_or(YarrErrorCode::FrameTooLarge)?;
                        self.disjunctions[d].alternatives[a].has_fixed_size = false;
                    } else {
                        // Unicode multi-unit advance (YarrPattern.cpp:1833) is out of
                        // this unit; the legacy subset advances by max_count.
                        current_input = current_input
                            .checked_add(term_max_count)
                            .ok_or(YarrErrorCode::OffsetTooLarge)?;
                    }
                }

                PatternTermKind::CharacterClass => {
                    self.disjunctions[d].alternatives[a].terms[t].input_position = current_input;
                    if term_quantity_type != QuantifierType::FixedCount {
                        self.disjunctions[d].alternatives[a].terms[t].frame_location =
                            current_call_frame;
                        current_call_frame = current_call_frame
                            .checked_add(YARR_STACK_CHARACTER_CLASS)
                            .ok_or(YarrErrorCode::FrameTooLarge)?;
                        self.disjunctions[d].alternatives[a].has_fixed_size = false;
                    } else {
                        // Unicode fixed-count class frame (YarrPattern.cpp:1851) is
                        // out of this unit; the legacy subset advances by max_count.
                        current_input = current_input
                            .checked_add(term_max_count)
                            .ok_or(YarrErrorCode::OffsetTooLarge)?;
                    }
                }

                PatternTermKind::ParenthesesSubpattern => {
                    self.disjunctions[d].alternatives[a].terms[t].frame_location =
                        current_call_frame;
                    let child = self.disjunctions[d].alternatives[a].terms[t]
                        .parentheses
                        .as_ref()
                        .and_then(|p| p.disjunction)
                        .expect("parentheses term has a child disjunction")
                        as usize;
                    let is_copy = self.disjunctions[d].alternatives[a].terms[t]
                        .parentheses
                        .as_ref()
                        .map(|p| p.is_copy)
                        .unwrap_or(false);
                    let is_terminal = self.disjunctions[d].alternatives[a].terms[t]
                        .parentheses
                        .as_ref()
                        .map(|p| p.is_terminal)
                        .unwrap_or(false);

                    if term_max_count == 1 && !is_copy {
                        current_call_frame = current_call_frame
                            .checked_add(YARR_STACK_PARENTHESES_ONCE)
                            .ok_or(YarrErrorCode::FrameTooLarge)?;
                        current_call_frame = self.setup_disjunction_offsets(
                            child,
                            current_call_frame,
                            current_input,
                        )?;
                        if term_quantity_type == QuantifierType::FixedCount {
                            let child_min = self.disjunctions[child].minimum_size.unwrap_or(0);
                            current_input = current_input
                                .checked_add(child_min)
                                .ok_or(YarrErrorCode::OffsetTooLarge)?;
                        }
                        self.disjunctions[d].alternatives[a].terms[t].input_position =
                            current_input;
                    } else if is_terminal {
                        current_call_frame = current_call_frame
                            .checked_add(YARR_STACK_PARENTHESES_TERMINAL)
                            .ok_or(YarrErrorCode::FrameTooLarge)?;
                        current_call_frame = self.setup_disjunction_offsets(
                            child,
                            current_call_frame,
                            current_input,
                        )?;
                        self.disjunctions[d].alternatives[a].terms[t].input_position =
                            current_input;
                    } else {
                        self.disjunctions[d].alternatives[a].terms[t].input_position =
                            current_input;
                        current_call_frame = current_call_frame
                            .checked_add(YARR_STACK_PARENTHESES)
                            .ok_or(YarrErrorCode::FrameTooLarge)?;
                        current_call_frame = self.setup_disjunction_offsets(
                            child,
                            current_call_frame,
                            current_input,
                        )?;
                    }
                    self.disjunctions[d].alternatives[a].has_fixed_size = false;
                }

                PatternTermKind::Assertion(PatternAssertion::LookAhead)
                | PatternTermKind::Assertion(PatternAssertion::NegativeLookAhead)
                | PatternTermKind::Assertion(PatternAssertion::LookBehind)
                | PatternTermKind::Assertion(PatternAssertion::NegativeLookBehind)
                | PatternTermKind::ParentheticalAssertion => {
                    // YarrPattern.cpp:1905 ParentheticalAssertion. Backward (lookbehind)
                    // bodies start at input position 0.
                    let backward = matches!(
                        kind,
                        PatternTermKind::Assertion(PatternAssertion::LookBehind)
                            | PatternTermKind::Assertion(PatternAssertion::NegativeLookBehind)
                    );
                    let disjunction_initial = if backward { 0 } else { current_input };
                    let child = self.disjunctions[d].alternatives[a].terms[t]
                        .parentheses
                        .as_ref()
                        .and_then(|p| p.disjunction)
                        .expect("parenthetical assertion has a child disjunction")
                        as usize;
                    self.disjunctions[d].alternatives[a].terms[t].input_position = current_input;
                    self.disjunctions[d].alternatives[a].terms[t].frame_location =
                        current_call_frame;
                    current_call_frame = current_call_frame
                        .checked_add(YARR_STACK_PARENTHETICAL_ASSERTION)
                        .ok_or(YarrErrorCode::FrameTooLarge)?;
                    current_call_frame = self.setup_disjunction_offsets(
                        child,
                        current_call_frame,
                        disjunction_initial,
                    )?;
                }

                PatternTermKind::DotStarEnclosure => {
                    // Dot-star enclosure optimization is out of this unit.
                    self.disjunctions[d].alternatives[a].terms[t].input_position = initial_input;
                    self.disjunctions[d].alternatives[a].has_fixed_size = false;
                }
            }
        }

        self.disjunctions[d].alternatives[a].minimum_size = Some(current_input - initial_input);
        Ok(current_call_frame)
    }

    fn into_pattern(self, id: YarrPatternId, source: StringId) -> YarrPattern {
        YarrPattern {
            id,
            source,
            flags: self.flags,
            compile_mode: self.compile_mode,
            disjunctions: self.disjunctions,
            capture_count: self.num_subpatterns,
            named_capture_count: self.named_capture_count,
            // Duplicate named-capture groups are out of this unit (see module note).
            duplicate_named_capture_count: 0,
            contains_backreferences: self.contains_backreferences,
            contains_bol: self.contains_bol,
            contains_lookbehinds: self.contains_lookbehinds,
            contains_unsigned_length_pattern: self.contains_unsigned_length_pattern,
            has_copied_parentheses: self.has_copied_parentheses,
            save_initial_start_value: self.save_initial_start_value,
            error: self.error,
        }
    }
}

/// Buffered character-class members during `[...]` parsing
/// (YarrParser.h:221 CharacterClassParserDelegate's collected output).
struct ClassBuilder {
    matches: Vec<char>,
    ranges: Vec<CharacterRange>,
    builtins: Vec<(BuiltInCharacterClassId, bool)>,
}

impl ClassBuilder {
    fn new() -> Self {
        ClassBuilder {
            matches: Vec::new(),
            ranges: Vec::new(),
            builtins: Vec::new(),
        }
    }
}

/// YarrParser.h:375 CharacterClassConstructionState (range state machine).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClassState {
    Empty,
    CachedCharacter,
    CachedCharacterHyphen,
    AfterCharacterClass,
    AfterCharacterClassHyphen,
}

/// Legacy (non-Unicode) members of a built-in character class, used to fold
/// built-ins appearing inside `[...]`. Mirrors digitsCreate/spacesCreate/
/// wordcharCreate (JS WhiteSpace + LineTerminator for `\s`).
fn legacy_built_in_members(id: BuiltInCharacterClassId) -> (Vec<char>, Vec<CharacterRange>) {
    match id {
        BuiltInCharacterClassId::Digit => (Vec::new(), vec![character_range('0', '9')]),
        BuiltInCharacterClassId::Word => (
            vec!['_'],
            vec![
                character_range('0', '9'),
                character_range('A', 'Z'),
                character_range('a', 'z'),
            ],
        ),
        BuiltInCharacterClassId::Space => (
            vec![
                '\u{9}', '\u{A}', '\u{B}', '\u{C}', '\u{D}', '\u{20}', '\u{A0}', '\u{1680}',
                '\u{2028}', '\u{2029}', '\u{202F}', '\u{205F}', '\u{3000}', '\u{FEFF}',
            ],
            vec![character_range('\u{2000}', '\u{200A}')],
        ),
        BuiltInCharacterClassId::Dot | BuiltInCharacterClassId::UnicodeProperty(_) => {
            (Vec::new(), Vec::new())
        }
    }
}

/// YarrParser.h:84 `Parser`. Borrows the constructor and drives the callbacks.
struct YarrParser<'a> {
    constructor: &'a mut YarrPatternConstructor,
    data: &'a [char],
    index: usize,
    compile_mode: CompileMode,
    num_subpatterns: u32,
    max_seen_back_reference: u32,
    back_reference_limit: u32,
    error: YarrErrorCode,
    error_offset: usize,
    paren_stack: Vec<ParenthesesType>,
    named_capture_names: Vec<String>,
    forward_reference_names: Vec<String>,
    is_named_forward_reference_allowed: bool,
}

const MAX_PATTERN_SIZE: usize = 1024 * 1024;

impl<'a> YarrParser<'a> {
    fn new(
        constructor: &'a mut YarrPatternConstructor,
        data: &'a [char],
        compile_mode: CompileMode,
    ) -> Self {
        YarrParser {
            constructor,
            data,
            index: 0,
            compile_mode,
            num_subpatterns: 0,
            max_seen_back_reference: 0,
            back_reference_limit: QUANTIFY_INFINITE,
            error: YarrErrorCode::NoError,
            error_offset: 0,
            paren_stack: Vec::new(),
            named_capture_names: Vec::new(),
            forward_reference_names: Vec::new(),
            is_named_forward_reference_allowed: true,
        }
    }

    fn is_either_unicode(&self) -> bool {
        matches!(
            self.compile_mode,
            CompileMode::Unicode | CompileMode::UnicodeSets
        )
    }

    fn is_legacy(&self) -> bool {
        self.compile_mode == CompileMode::Legacy
    }

    fn fail(&mut self, code: YarrErrorCode) {
        if self.error == YarrErrorCode::NoError {
            self.error = code;
            self.error_offset = self.index;
        }
    }

    fn at_end(&self) -> bool {
        self.index >= self.data.len()
    }

    fn peek(&self) -> char {
        self.data[self.index]
    }

    fn consume(&mut self) -> char {
        let ch = self.data[self.index];
        self.index += 1;
        ch
    }

    fn peek_is_digit(&self) -> bool {
        !self.at_end() && self.peek().is_ascii_digit()
    }

    fn try_consume(&mut self, ch: char) -> bool {
        if self.at_end() || self.data[self.index] != ch {
            return false;
        }
        self.index += 1;
        true
    }

    fn consume_digit(&mut self) -> u32 {
        (self.consume() as u32) - ('0' as u32)
    }

    fn consume_number(&mut self) -> u32 {
        // YarrParser.h:2061 consumeNumber (overflow -> quantifyInfinite).
        let mut n: u32 = self.consume_digit();
        while self.peek_is_digit() {
            n = n
                .checked_mul(10)
                .and_then(|v| v.checked_add(self.consume_digit()))
                .unwrap_or(QUANTIFY_INFINITE);
            if n == QUANTIFY_INFINITE {
                while self.peek_is_digit() {
                    self.consume();
                }
                break;
            }
        }
        n
    }

    fn consume_number64(&mut self) -> u64 {
        let mut n: u64 = self.consume_digit() as u64;
        while self.peek_is_digit() {
            n = n
                .saturating_mul(10)
                .saturating_add(self.consume_digit() as u64);
        }
        n
    }

    fn consume_octal(&mut self, count: u32) -> char {
        // YarrParser.h:2078 consumeOctal.
        let mut octal: u32 = 0;
        let mut remaining = count;
        while remaining > 0 && octal < 32 && !self.at_end() && ('0'..='7').contains(&self.peek()) {
            octal = octal * 8 + self.consume_digit();
            remaining -= 1;
        }
        char::from_u32(octal).unwrap_or('\u{0}')
    }

    fn try_consume_hex(&mut self, count: u32) -> Option<char> {
        // YarrParser.h:2094 tryConsumeHex.
        let state = self.index;
        let mut n: u32 = 0;
        let mut remaining = count;
        while remaining > 0 {
            if self.at_end() || !self.peek().is_ascii_hexdigit() {
                self.index = state;
                return None;
            }
            n = (n << 4) | self.consume().to_digit(16).unwrap();
            remaining -= 1;
        }
        char::from_u32(n)
    }

    fn try_consume_unicode_escape(&mut self) -> Option<char> {
        // YarrParser.h:1965 tryConsumeUnicodeEscape (subset). Caller has consumed
        // the backslash; this consumes 'u' and the digits.
        if !self.try_consume('u') || self.at_end() {
            if self.is_either_unicode() {
                self.fail(YarrErrorCode::InvalidUnicodeEscape);
            }
            return None;
        }
        if self.is_either_unicode() && self.try_consume('{') {
            let mut code_point: u32 = 0;
            loop {
                if self.at_end() || !self.peek().is_ascii_hexdigit() {
                    self.fail(YarrErrorCode::InvalidUnicodeCodePointEscape);
                    return None;
                }
                code_point = (code_point << 4) | self.consume().to_digit(16).unwrap();
                if code_point > 0x10FFFF {
                    self.fail(YarrErrorCode::InvalidUnicodeCodePointEscape);
                    return None;
                }
                if !self.at_end() && self.peek() == '}' {
                    break;
                }
            }
            if !self.try_consume('}') {
                self.fail(YarrErrorCode::InvalidUnicodeCodePointEscape);
                return None;
            }
            return char::from_u32(code_point);
        }
        match self.try_consume_hex(4) {
            Some(ch) => Some(ch),
            None => {
                if self.is_either_unicode() {
                    self.fail(YarrErrorCode::InvalidUnicodeEscape);
                }
                None
            }
        }
    }

    fn is_identifier_start(ch: char) -> bool {
        ch.is_ascii_alphabetic() || ch == '_' || ch == '$'
    }

    fn is_identifier_part(ch: char) -> bool {
        ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'
    }

    fn try_consume_group_name(&mut self) -> Option<String> {
        // YarrParser.h:2109 tryConsumeGroupName (legacy/ASCII subset). Returns the
        // name and consumes through the closing '>', or restores and returns None.
        let state = self.index;
        if self.at_end() || !Self::is_identifier_start(self.peek()) {
            self.index = state;
            return None;
        }
        let mut name = String::new();
        name.push(self.consume());
        while !self.at_end() && self.peek() != '>' {
            let ch = self.peek();
            if !Self::is_identifier_part(ch) {
                self.index = state;
                return None;
            }
            name.push(self.consume());
        }
        if !self.try_consume('>') {
            self.index = state;
            return None;
        }
        Some(name)
    }

    fn parse(&mut self) -> YarrErrorCode {
        // YarrParser.h:102 parse().
        if self.data.len() > MAX_PATTERN_SIZE {
            return YarrErrorCode::PatternTooLarge;
        }
        self.parse_tokens();
        if self.error == YarrErrorCode::NoError && self.constructor.error == YarrErrorCode::NoError
        {
            self.handle_illegal_references();
        }
        self.error
    }

    fn handle_illegal_references(&mut self) {
        // YarrParser.h:1854 handleIllegalReferences (legacy numeric-backreference path).
        let mut should_reparse = false;
        if self.max_seen_back_reference > self.num_subpatterns {
            if self.is_either_unicode() {
                self.fail(YarrErrorCode::InvalidBackReference);
                return;
            }
            self.back_reference_limit = self.num_subpatterns;
            should_reparse = true;
        }
        if !self.forward_reference_names.is_empty()
            && self.contains_illegal_named_forward_reference()
        {
            if self.is_either_unicode() || !self.named_capture_names.is_empty() {
                self.fail(YarrErrorCode::InvalidNamedCapture);
                return;
            }
            self.is_named_forward_reference_allowed = false;
            should_reparse = true;
        }
        if should_reparse {
            self.reset_for_reparsing();
            self.parse_tokens();
        }
    }

    fn contains_illegal_named_forward_reference(&self) -> bool {
        if self.forward_reference_names.is_empty() {
            return false;
        }
        if self.named_capture_names.is_empty() {
            return true;
        }
        self.forward_reference_names
            .iter()
            .any(|name| !self.named_capture_names.contains(name))
    }

    fn reset_for_reparsing(&mut self) {
        self.constructor.reset_for_reparsing();
        self.index = 0;
        self.num_subpatterns = 0;
        self.max_seen_back_reference = 0;
        self.paren_stack.clear();
        self.named_capture_names.clear();
        self.forward_reference_names.clear();
    }

    fn aborted(&self) -> bool {
        self.error != YarrErrorCode::NoError || self.constructor.error != YarrErrorCode::NoError
    }

    fn parse_tokens(&mut self) {
        // YarrParser.h:1720 parseTokens.
        let mut last_token_type = TokenType::NotAtom;
        while !self.at_end() {
            match self.peek() {
                '|' => {
                    self.consume();
                    self.constructor
                        .disjunction(CreateDisjunctionPurpose::ForNextAlternative);
                    last_token_type = TokenType::NotAtom;
                }
                '(' => {
                    self.parse_parentheses_begin();
                    last_token_type = TokenType::NotAtom;
                }
                ')' => {
                    last_token_type = self.parse_parentheses_end();
                }
                '^' => {
                    self.consume();
                    self.constructor.assertion_bol();
                    last_token_type = TokenType::NotAtom;
                }
                '$' => {
                    self.consume();
                    self.constructor.assertion_eol();
                    last_token_type = TokenType::NotAtom;
                }
                '.' => {
                    self.consume();
                    self.constructor
                        .atom_built_in_character_class(BuiltInCharacterClassId::Dot, false);
                    last_token_type = TokenType::Atom;
                }
                '[' => {
                    self.parse_character_class();
                    last_token_type = TokenType::Atom;
                }
                ']' | '}' => {
                    if self.is_either_unicode() {
                        self.fail(YarrErrorCode::BracketUnmatched);
                    } else {
                        let ch = self.consume();
                        self.constructor.atom_pattern_character(ch);
                        last_token_type = TokenType::Atom;
                    }
                }
                '\\' => {
                    last_token_type = self.parse_atom_escape();
                }
                '*' => {
                    self.consume();
                    self.parse_quantifier(last_token_type, 0, QUANTIFY_INFINITE);
                    last_token_type = TokenType::NotAtom;
                }
                '+' => {
                    self.consume();
                    self.parse_quantifier(last_token_type, 1, QUANTIFY_INFINITE);
                    last_token_type = TokenType::NotAtom;
                }
                '?' => {
                    self.consume();
                    self.parse_quantifier(last_token_type, 0, 1);
                    last_token_type = TokenType::NotAtom;
                }
                '{' => {
                    let state = self.index;
                    self.consume();
                    let mut handled = false;
                    if self.peek_is_digit() {
                        let min = self.consume_number64();
                        let mut max = min;
                        if self.try_consume(',') {
                            max = if self.peek_is_digit() {
                                self.consume_number64()
                            } else {
                                u64::MAX
                            };
                        }
                        if self.try_consume('}') {
                            let infinite = QUANTIFY_INFINITE as u64;
                            if min == u64::MAX {
                                self.fail(YarrErrorCode::QuantifierTooLarge);
                            } else if min <= max {
                                let min = min.min(infinite) as u32;
                                let max = max.min(infinite) as u32;
                                self.parse_quantifier(last_token_type, min, max);
                            } else {
                                self.fail(YarrErrorCode::QuantifierOutOfOrder);
                            }
                            last_token_type = TokenType::NotAtom;
                            handled = true;
                        }
                    }
                    if !handled {
                        if self.is_either_unicode() {
                            self.fail(YarrErrorCode::QuantifierIncomplete);
                        } else {
                            // Legacy: a bare '{' is a literal character.
                            self.index = state;
                            let ch = self.consume();
                            self.constructor.atom_pattern_character(ch);
                            last_token_type = TokenType::Atom;
                        }
                    }
                }
                _ => {
                    let ch = self.consume();
                    self.constructor.atom_pattern_character(ch);
                    last_token_type = TokenType::Atom;
                }
            }

            if self.aborted() {
                if self.error == YarrErrorCode::NoError {
                    self.error = self.constructor.error;
                    self.error_offset = self.index;
                }
                return;
            }
        }

        if !self.paren_stack.is_empty() {
            self.fail(YarrErrorCode::MissingParentheses);
        }
    }

    fn parse_parentheses_begin(&mut self) {
        // YarrParser.h:1512 parseParenthesesBegin (legacy subset).
        self.consume(); // '('
        let mut paren_type = ParenthesesType::Subpattern;
        let mut is_non_capturing = false;

        if self.try_consume('?') {
            if self.at_end() {
                self.fail(YarrErrorCode::ParenthesesTypeInvalid);
                return;
            }
            match self.peek() {
                ':' => {
                    self.consume();
                    self.constructor
                        .atom_parentheses_subpattern_begin(false, None);
                    is_non_capturing = true;
                }
                '=' => {
                    self.consume();
                    self.constructor.atom_parenthetical_assertion_begin(
                        PatternAssertion::LookAhead,
                        false,
                        MatchDirection::Forward,
                    );
                    paren_type = ParenthesesType::Assertion;
                }
                '!' => {
                    self.consume();
                    self.constructor.atom_parenthetical_assertion_begin(
                        PatternAssertion::NegativeLookAhead,
                        true,
                        MatchDirection::Forward,
                    );
                    paren_type = ParenthesesType::Assertion;
                }
                '<' => {
                    self.consume();
                    // Lookbehind first: '=' / '!' are not identifier starts.
                    if self.try_consume('=') {
                        self.constructor.atom_parenthetical_assertion_begin(
                            PatternAssertion::LookBehind,
                            false,
                            MatchDirection::Backward,
                        );
                        paren_type = ParenthesesType::LookbehindAssertion;
                    } else if self.try_consume('!') {
                        self.constructor.atom_parenthetical_assertion_begin(
                            PatternAssertion::NegativeLookBehind,
                            true,
                            MatchDirection::Backward,
                        );
                        paren_type = ParenthesesType::LookbehindAssertion;
                    } else if let Some(name) = self.try_consume_group_name() {
                        if self.named_capture_names.contains(&name) {
                            self.fail(YarrErrorCode::DuplicateGroupName);
                        } else {
                            self.named_capture_names.push(name.clone());
                            self.constructor
                                .atom_parentheses_subpattern_begin(true, Some(name));
                            self.num_subpatterns += 1;
                        }
                    } else {
                        self.fail(YarrErrorCode::InvalidGroupName);
                    }
                }
                _ => {
                    // Inline modifiers `(?flags:)` are out of this unit.
                    self.fail(YarrErrorCode::ParenthesesTypeInvalid);
                }
            }
        } else {
            self.constructor
                .atom_parentheses_subpattern_begin(true, None);
            self.num_subpatterns += 1;
        }

        if self.aborted() {
            return;
        }
        self.paren_stack.push(paren_type);
        // (`is_non_capturing` mirrors the C++ flag; capture counting is handled at
        // the call sites above.)
        let _ = is_non_capturing;
    }

    fn parse_parentheses_end(&mut self) -> TokenType {
        // YarrParser.h:1668 parseParenthesesEnd.
        self.consume(); // ')'
        if self.paren_stack.is_empty() {
            self.fail(YarrErrorCode::ParenthesesUnmatched);
            return TokenType::NotAtom;
        }
        self.constructor.atom_parentheses_end();
        let paren_type = self.paren_stack.pop().unwrap();
        match paren_type {
            ParenthesesType::LookbehindAssertion => TokenType::Lookbehind,
            ParenthesesType::Subpattern => TokenType::Atom,
            // Web-compat: a (?=)/(?!) assertion is quantifiable in legacy mode.
            ParenthesesType::Assertion => {
                if self.is_legacy() {
                    TokenType::Atom
                } else {
                    TokenType::NotAtom
                }
            }
        }
    }

    fn parse_quantifier(&mut self, last_token_type: TokenType, min: u32, max: u32) {
        // YarrParser.h:1698 parseQuantifier.
        match last_token_type {
            TokenType::Atom => {
                let greedy = !self.try_consume('?');
                self.constructor.quantify_atom(min, max, greedy);
            }
            TokenType::Lookbehind => self.fail(YarrErrorCode::CantQuantifyAtom),
            TokenType::NotAtom => self.fail(YarrErrorCode::QuantifierWithoutAtom),
        }
    }

    fn parse_atom_escape(&mut self) -> TokenType {
        self.parse_escape(EscapeMode::Normal, None)
    }

    fn parse_character_class(&mut self) {
        // YarrParser.h:1309 parseCharacterClass. Class-sets `[[..]]` are out of scope.
        self.consume(); // '['
        let invert = self.try_consume('^');
        let mut builder = ClassBuilder::new();
        let mut state = ClassState::Empty;
        let mut cached: char = '\u{0}';
        // Local error so the ClassDelegate does not borrow `self` across parse_escape.
        let mut class_error = YarrErrorCode::NoError;
        let is_unicode = matches!(self.compile_mode, CompileMode::Unicode);

        while !self.at_end() {
            match self.peek() {
                ']' => {
                    self.consume();
                    Self::class_flush_end(&mut builder, state, cached);
                    if class_error != YarrErrorCode::NoError {
                        self.fail(class_error);
                        return;
                    }
                    self.constructor.atom_character_class_end(invert, builder);
                    return;
                }
                '\\' => {
                    self.parse_escape(
                        EscapeMode::CharacterClass,
                        Some(ClassDelegate {
                            builder: &mut builder,
                            state: &mut state,
                            cached: &mut cached,
                            error: &mut class_error,
                            is_unicode,
                        }),
                    );
                }
                _ => {
                    let ch = self.consume();
                    Self::class_atom_pattern_character(
                        &mut builder,
                        &mut state,
                        &mut cached,
                        &mut class_error,
                        is_unicode,
                        ch,
                        true,
                    );
                }
            }
            if class_error != YarrErrorCode::NoError {
                self.fail(class_error);
                return;
            }
            if self.aborted() {
                return;
            }
        }
        self.fail(YarrErrorCode::CharacterClassUnmatched);
    }

    // YarrParser.h:251 CharacterClassParserDelegate::atomPatternCharacter (range
    // state machine). Standalone so both the inline path and parse_escape share it.
    #[allow(clippy::too_many_arguments)]
    fn class_atom_pattern_character(
        builder: &mut ClassBuilder,
        state: &mut ClassState,
        cached: &mut char,
        error: &mut YarrErrorCode,
        is_unicode: bool,
        ch: char,
        hyphen_is_range: bool,
    ) {
        match *state {
            ClassState::AfterCharacterClass => {
                if hyphen_is_range && ch == '-' {
                    builder.matches.push('-');
                    *state = ClassState::AfterCharacterClassHyphen;
                    return;
                }
                *cached = ch;
                *state = ClassState::CachedCharacter;
            }
            ClassState::Empty => {
                *cached = ch;
                *state = ClassState::CachedCharacter;
            }
            ClassState::CachedCharacter => {
                if hyphen_is_range && ch == '-' {
                    *state = ClassState::CachedCharacterHyphen;
                } else {
                    builder.matches.push(*cached);
                    *cached = ch;
                }
            }
            ClassState::CachedCharacterHyphen => {
                if (ch as u32) < (*cached as u32) {
                    if *error == YarrErrorCode::NoError {
                        *error = YarrErrorCode::CharacterClassRangeInvalid;
                    }
                    return;
                }
                builder.ranges.push(character_range(*cached, ch));
                *state = ClassState::Empty;
            }
            ClassState::AfterCharacterClassHyphen => {
                if is_unicode {
                    if *error == YarrErrorCode::NoError {
                        *error = YarrErrorCode::CharacterClassRangeInvalid;
                    }
                    return;
                }
                builder.matches.push(ch);
                *state = ClassState::Empty;
            }
        }
    }

    // YarrParser.h:312 CharacterClassParserDelegate::atomBuiltInCharacterClass.
    fn class_built_in(
        builder: &mut ClassBuilder,
        state: &mut ClassState,
        cached: &mut char,
        is_unicode: bool,
        error: &mut YarrErrorCode,
        id: BuiltInCharacterClassId,
        invert: bool,
    ) {
        match *state {
            ClassState::CachedCharacter => {
                builder.matches.push(*cached);
                builder.builtins.push((id, invert));
                *state = ClassState::AfterCharacterClass;
            }
            ClassState::Empty | ClassState::AfterCharacterClass => {
                builder.builtins.push((id, invert));
                *state = ClassState::AfterCharacterClass;
            }
            ClassState::CachedCharacterHyphen => {
                builder.matches.push(*cached);
                builder.matches.push('-');
                if is_unicode {
                    if *error == YarrErrorCode::NoError {
                        *error = YarrErrorCode::CharacterClassRangeInvalid;
                    }
                    return;
                }
                builder.builtins.push((id, invert));
                *state = ClassState::Empty;
            }
            ClassState::AfterCharacterClassHyphen => {
                if is_unicode {
                    if *error == YarrErrorCode::NoError {
                        *error = YarrErrorCode::CharacterClassRangeInvalid;
                    }
                    return;
                }
                builder.builtins.push((id, invert));
                *state = ClassState::Empty;
            }
        }
    }

    // YarrParser.h:353 CharacterClassParserDelegate::end (flush cached state).
    fn class_flush_end(builder: &mut ClassBuilder, state: ClassState, cached: char) {
        match state {
            ClassState::CachedCharacter => builder.matches.push(cached),
            ClassState::CachedCharacterHyphen => {
                builder.matches.push(cached);
                builder.matches.push('-');
            }
            _ => {}
        }
    }

    // YarrParser.h:904 parseEscape (legacy subset). When `class_delegate` is set
    // the escape feeds the character-class collector; otherwise the constructor.
    fn parse_escape(
        &mut self,
        mode: EscapeMode,
        class_delegate: Option<ClassDelegate<'_>>,
    ) -> TokenType {
        self.consume(); // '\\'
        if self.at_end() {
            self.fail(YarrErrorCode::EscapeUnterminated);
            return TokenType::NotAtom;
        }

        // Route a produced character to the right collector.
        let mut class = class_delegate;
        macro_rules! emit_char {
            ($ch:expr, $hyphen:expr) => {
                match &mut class {
                    Some(cd) => Self::class_atom_pattern_character(
                        cd.builder,
                        cd.state,
                        cd.cached,
                        cd.error,
                        cd.is_unicode,
                        $ch,
                        $hyphen,
                    ),
                    None => self.constructor.atom_pattern_character($ch),
                }
            };
        }
        macro_rules! emit_built_in {
            ($id:expr, $invert:expr) => {
                match &mut class {
                    Some(cd) => Self::class_built_in(
                        cd.builder,
                        cd.state,
                        cd.cached,
                        cd.is_unicode,
                        cd.error,
                        $id,
                        $invert,
                    ),
                    None => self.constructor.atom_built_in_character_class($id, $invert),
                }
            };
        }

        match self.peek() {
            'b' => {
                self.consume();
                if mode != EscapeMode::Normal {
                    emit_char!('\u{8}', false);
                } else {
                    self.constructor.assertion_word_boundary(false);
                    return TokenType::NotAtom;
                }
            }
            'B' => {
                self.consume();
                if mode != EscapeMode::Normal {
                    emit_char!('B', false);
                } else {
                    self.constructor.assertion_word_boundary(true);
                    return TokenType::NotAtom;
                }
            }
            'd' => {
                self.consume();
                emit_built_in!(BuiltInCharacterClassId::Digit, false);
            }
            'D' => {
                self.consume();
                emit_built_in!(BuiltInCharacterClassId::Digit, true);
            }
            's' => {
                self.consume();
                emit_built_in!(BuiltInCharacterClassId::Space, false);
            }
            'S' => {
                self.consume();
                emit_built_in!(BuiltInCharacterClassId::Space, true);
            }
            'w' => {
                self.consume();
                emit_built_in!(BuiltInCharacterClassId::Word, false);
            }
            'W' => {
                self.consume();
                emit_built_in!(BuiltInCharacterClassId::Word, true);
            }
            '0' => {
                self.consume();
                if !self.peek_is_digit() {
                    emit_char!('\u{0}', false);
                } else if self.is_either_unicode() {
                    self.fail(YarrErrorCode::InvalidOctalEscape);
                } else {
                    let ch = self.consume_octal(2);
                    emit_char!(ch, false);
                }
            }
            '1'..='9' => {
                if mode == EscapeMode::Normal {
                    let state = self.index;
                    let back_reference = self.consume_number();
                    if back_reference <= self.back_reference_limit {
                        self.max_seen_back_reference =
                            self.max_seen_back_reference.max(back_reference);
                        self.constructor.atom_back_reference(back_reference);
                        return TokenType::Atom;
                    }
                    self.index = state;
                    if self.is_either_unicode() {
                        self.fail(YarrErrorCode::InvalidBackReference);
                        return TokenType::NotAtom;
                    }
                }
                if self.is_either_unicode() {
                    self.fail(YarrErrorCode::InvalidOctalEscape);
                } else {
                    let ch = if self.peek() < '8' {
                        self.consume_octal(3)
                    } else {
                        self.consume()
                    };
                    emit_char!(ch, false);
                }
            }
            'f' => {
                self.consume();
                emit_char!('\u{C}', false);
            }
            'n' => {
                self.consume();
                emit_char!('\n', false);
            }
            'r' => {
                self.consume();
                emit_char!('\r', false);
            }
            't' => {
                self.consume();
                emit_char!('\t', false);
            }
            'v' => {
                self.consume();
                emit_char!('\u{B}', false);
            }
            'c' => {
                let state = self.index;
                self.consume();
                if !self.at_end() {
                    let control = self.consume();
                    if control.is_ascii_alphabetic() {
                        emit_char!(char::from_u32((control as u32) & 0x1f).unwrap(), false);
                        return self.escape_result(mode);
                    }
                    if mode != EscapeMode::Normal && (control.is_ascii_digit() || control == '_') {
                        emit_char!(char::from_u32((control as u32) & 0x1f).unwrap(), false);
                        return self.escape_result(mode);
                    }
                }
                if self.is_either_unicode() {
                    self.fail(YarrErrorCode::InvalidIdentityEscape);
                } else {
                    self.index = state;
                    emit_char!('\\', false);
                }
            }
            'x' => {
                self.consume();
                match self.try_consume_hex(2) {
                    Some(ch) => emit_char!(ch, false),
                    None => emit_char!('x', false),
                }
            }
            'k' => {
                self.consume();
                let state = self.index;
                if mode == EscapeMode::Normal && self.try_consume('<') {
                    if let Some(name) = self.try_consume_group_name() {
                        if self.named_capture_names.contains(&name) {
                            self.constructor.atom_named_back_reference(&name);
                            return TokenType::Atom;
                        }
                        if self.is_named_forward_reference_allowed {
                            self.forward_reference_names.push(name);
                            self.constructor.atom_named_forward_reference();
                            return TokenType::Atom;
                        }
                    }
                    self.index = state;
                }
                emit_char!('k', false);
            }
            'u' => {
                // try_consume_unicode_escape consumes the 'u' and the digits.
                match self.try_consume_unicode_escape() {
                    Some(ch) => emit_char!(ch, false),
                    None => {
                        if !self.is_either_unicode() {
                            // Identity escape of 'u' (legacy): consume it literally.
                            self.try_consume('u');
                            emit_char!('u', false);
                        }
                    }
                }
            }
            _ => {
                // IdentityEscape: the escaped character is taken literally.
                let ch = self.consume();
                emit_char!(ch, false);
            }
        }

        self.escape_result(mode)
    }

    fn escape_result(&self, mode: EscapeMode) -> TokenType {
        match mode {
            EscapeMode::Normal => TokenType::Atom,
            EscapeMode::CharacterClass => TokenType::NotAtom,
        }
    }
}

/// Borrowed view of the in-progress character class for `parse_escape`.
struct ClassDelegate<'b> {
    builder: &'b mut ClassBuilder,
    state: &'b mut ClassState,
    cached: &'b mut char,
    error: &'b mut YarrErrorCode,
    is_unicode: bool,
}

/// Public entry: faithfully parse `source_text` into a nested PatternDisjunction
/// tree and run setupOffsets (YarrPattern.cpp:2665 YarrPattern::compile order),
/// returning a `YarrPattern` whose `disjunctions` arena the ByteCompiler lowers.
/// `id`/`source` identify the pattern for the bytecode/runtime layers.
pub fn construct_yarr_pattern(
    source_text: &str,
    flags: RegexFlags,
    id: YarrPatternId,
    source: StringId,
) -> Result<YarrPattern, YarrParseError> {
    let compile_mode = compile_mode_for_flags(flags);
    let mut constructor = YarrPatternConstructor::new(flags, compile_mode);
    let chars: Vec<char> = source_text.chars().collect();

    let (parse_err, parse_offset) = {
        let mut parser = YarrParser::new(&mut constructor, &chars, compile_mode);
        let err = parser.parse();
        (err, parser.error_offset)
    };

    if constructor.error != YarrErrorCode::NoError {
        return Err(YarrParseError {
            code: constructor.error,
            offset: parse_offset as u32,
        });
    }
    if parse_err != YarrErrorCode::NoError {
        return Err(YarrParseError {
            code: parse_err,
            offset: parse_offset as u32,
        });
    }

    // YarrPattern.cpp:2678 post-parse order (dot-star / BOL unrolling omitted).
    constructor.check_for_terminal_parentheses();
    if constructor.error != YarrErrorCode::NoError {
        return Err(YarrParseError {
            code: constructor.error,
            offset: 0,
        });
    }
    if let Err(code) = constructor.setup_offsets() {
        return Err(YarrParseError { code, offset: 0 });
    }

    Ok(constructor.into_pattern(id, source))
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
    fn parse_plan_quantifies_parenthesized_subpattern() {
        let plan = plan_yarr_parse(r"(?:ab)*c", RegexFlags::default()).unwrap();

        assert_eq!(plan.capture_count, 0);
        assert!(matches!(
            plan.atoms.first().map(|atom| atom.kind),
            Some(YarrParsePlanAtomKind::NonCaptureGroup)
        ));
        assert!(plan.atoms[0].quantified);
        assert!(!plan.atoms[2].quantified);
    }

    #[test]
    fn parse_plan_allows_quantified_backreference() {
        let plan = plan_yarr_parse(r"(a)\1*", RegexFlags::default()).unwrap();

        assert_eq!(plan.capture_count, 1);
        assert!(plan.contains_backreferences);
        assert!(plan
            .atoms
            .iter()
            .any(|atom| atom.kind == YarrParsePlanAtomKind::BackReference && atom.quantified));
    }

    #[test]
    fn parse_plan_accepts_typescript_amd_dependency_pattern() {
        let flags = parse_regex_flags("gim").unwrap();
        let pattern =
            r#"^(\/\/\/\s*<amd-dependency\s+path=)('|")(.+?)\2\s*(static=('|")(.+?)\2\s*)*\/>"#;
        let plan = plan_yarr_parse(pattern, flags).unwrap();

        assert_eq!(plan.capture_count, 6);
        assert_eq!(plan.disjunction_count, 3);
        assert!(plan.contains_backreferences);
        assert!(plan.contains_bol);
        assert!(plan
            .atoms
            .iter()
            .any(|atom| atom.kind == YarrParsePlanAtomKind::CaptureGroup && atom.quantified));
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

    // ---- YarrPatternConstructor + setupOffsets (Yarr B1) ----

    fn child_of(p: &YarrPattern, term: &PatternTerm) -> usize {
        term.parentheses
            .as_ref()
            .unwrap()
            .disjunction
            .map(|d| d as usize)
            .filter(|d| *d < p.disjunctions.len())
            .unwrap()
    }

    #[test]
    fn constructs_lookahead_tree_and_offsets() {
        // Pins YarrParser.h:1534 (?=) -> atomParentheticalAssertionBegin and the
        // ParentheticalAssertion offset rule YarrPattern.cpp:1905 (forward look:
        // disjunctionInitialInputPosition = currentInputPosition). The /i flag keeps
        // ASCII PatternCharacters (YarrPattern.cpp:1248), it does not fold them.
        let flags = RegexFlags {
            ignore_case: true,
            ..RegexFlags::default()
        };
        let p = construct_yarr_pattern(
            "HF(?=;)",
            flags,
            YarrPatternId(0),
            crate::strings::StringId(0),
        )
        .unwrap();
        assert_eq!(p.error, YarrErrorCode::NoError);
        assert_eq!(p.capture_count, 0);

        let body = p.body();
        assert_eq!(body.alternatives.len(), 1);
        let terms = &body.alternatives[0].terms;
        assert_eq!(terms.len(), 3);

        assert_eq!(terms[0].kind, PatternTermKind::PatternCharacter);
        assert_eq!(terms[0].character, Some('H'));
        assert_eq!(terms[0].input_position, 0);
        assert_eq!(terms[1].character, Some('F'));
        assert_eq!(terms[1].input_position, 1);

        assert_eq!(
            terms[2].kind,
            PatternTermKind::Assertion(PatternAssertion::LookAhead)
        );
        assert!(!terms[2].invert);
        assert_eq!(terms[2].input_position, 2);

        // YarrPattern.cpp:1933 alternative m_minimumSize / :1976 m_callFrameSize.
        assert_eq!(body.alternatives[0].minimum_size, Some(2));
        assert_eq!(body.minimum_size, Some(2));
        assert_eq!(body.call_frame_size, 1); // YarrStackSpaceForBackTrackInfoParentheticalAssertion

        let child = &p.disjunctions[child_of(&p, &terms[2])];
        assert_eq!(child.alternatives[0].terms[0].character, Some(';'));
        assert_eq!(child.alternatives[0].terms[0].input_position, 2);
        assert_eq!(child.minimum_size, Some(1));
        assert_eq!(child.call_frame_size, 1);
    }

    #[test]
    fn constructs_word_boundary_offsets() {
        // Pins YarrParser.h:918 \b -> assertionWordBoundary and the assertion offset
        // rule YarrPattern.cpp:1805 (inputPosition = currentInputPosition, no advance).
        let p = construct_yarr_pattern(
            "\\bfoo",
            RegexFlags::default(),
            YarrPatternId(0),
            crate::strings::StringId(0),
        )
        .unwrap();
        let body = p.body();
        let terms = &body.alternatives[0].terms;
        assert_eq!(terms.len(), 4);
        assert_eq!(
            terms[0].kind,
            PatternTermKind::Assertion(PatternAssertion::WordBoundary)
        );
        assert!(!terms[0].invert);
        assert_eq!(terms[0].input_position, 0);
        assert_eq!(terms[1].character, Some('f'));
        assert_eq!(terms[1].input_position, 0);
        assert_eq!(terms[2].input_position, 1);
        assert_eq!(terms[3].input_position, 2);
        assert_eq!(body.minimum_size, Some(3));
        assert_eq!(body.call_frame_size, 0);

        // \B sets the inverted word-boundary assertion (YarrParser.h:927).
        let neg = construct_yarr_pattern(
            "\\Bx",
            RegexFlags::default(),
            YarrPatternId(0),
            crate::strings::StringId(0),
        )
        .unwrap();
        assert_eq!(
            neg.body().alternatives[0].terms[0].kind,
            PatternTermKind::Assertion(PatternAssertion::NotWordBoundary)
        );
        assert!(neg.body().alternatives[0].terms[0].invert);
    }

    #[test]
    fn constructs_capturing_group_offsets() {
        // Pins YarrPattern.cpp:1457 atomParenthesesSubpatternBegin and the fixed-once
        // parentheses offset rule YarrPattern.cpp:1870 (ParenthesesOnce frame; fixed
        // count pre-advances currentInputPosition by the nested minimumSize).
        let p = construct_yarr_pattern(
            "(ab)c",
            RegexFlags::default(),
            YarrPatternId(0),
            crate::strings::StringId(0),
        )
        .unwrap();
        assert_eq!(p.capture_count, 1);
        let body = p.body();
        let terms = &body.alternatives[0].terms;
        assert_eq!(terms.len(), 2);
        assert_eq!(terms[0].kind, PatternTermKind::ParenthesesSubpattern);
        assert!(terms[0].capture);
        let parens = terms[0].parentheses.as_ref().unwrap();
        assert_eq!(parens.subpattern_id, 1);
        assert_eq!(parens.last_subpattern_id, 1);
        // Fixed-once parens inputPosition is set after its content (YarrPattern.cpp:1883).
        assert_eq!(terms[0].input_position, 2);
        assert_eq!(terms[1].character, Some('c'));
        assert_eq!(terms[1].input_position, 2);
        assert_eq!(body.minimum_size, Some(3));
        assert_eq!(body.call_frame_size, 2); // YarrStackSpaceForBackTrackInfoParenthesesOnce

        let child = &p.disjunctions[child_of(&p, &terms[0])];
        assert_eq!(child.alternatives[0].terms[0].character, Some('a'));
        assert_eq!(child.alternatives[0].terms[0].input_position, 0);
        assert_eq!(child.alternatives[0].terms[1].input_position, 1);
        assert_eq!(child.minimum_size, Some(2));
    }

    #[test]
    fn constructs_greedy_noncapturing_group_offsets() {
        // Pins YarrParser.h:1528 (?:) and the greedy quantifier (YarrPattern.cpp:1744)
        // plus the unbounded-parentheses offset rule (YarrPattern.cpp:1892): a non-fixed
        // parens does NOT advance currentInputPosition, so 'c' stays at position 0.
        let p = construct_yarr_pattern(
            "(?:ab)*c",
            RegexFlags::default(),
            YarrPatternId(0),
            crate::strings::StringId(0),
        )
        .unwrap();
        assert_eq!(p.capture_count, 0);
        let body = p.body();
        let terms = &body.alternatives[0].terms;
        assert_eq!(terms.len(), 2);
        assert_eq!(terms[0].kind, PatternTermKind::ParenthesesSubpattern);
        assert!(!terms[0].capture);
        assert_eq!(terms[0].input_position, 0);
        assert_eq!(terms[1].character, Some('c'));
        assert_eq!(terms[1].input_position, 0);
        // Only 'c' is mandatory.
        assert_eq!(body.minimum_size, Some(1));
        assert_eq!(body.call_frame_size, 4); // YarrStackSpaceForBackTrackInfoParentheses

        let child = &p.disjunctions[child_of(&p, &terms[0])];
        assert_eq!(child.alternatives[0].terms[0].character, Some('a'));
        assert_eq!(child.alternatives[0].terms[1].character, Some('b'));
        assert_eq!(child.minimum_size, Some(2));
    }

    #[test]
    fn constructs_braced_quantifier_split() {
        // Pins quantifyAtom min!=max forward split (YarrPattern.cpp:1746): a{2,4}
        // becomes a{2} (fixed) followed by a copy a{0,2} (greedy). Fixed prefix
        // advances currentInputPosition by 2, the greedy copy by 0.
        let p = construct_yarr_pattern(
            "a{2,4}",
            RegexFlags::default(),
            YarrPatternId(0),
            crate::strings::StringId(0),
        )
        .unwrap();
        let body = p.body();
        let terms = &body.alternatives[0].terms;
        assert_eq!(terms.len(), 2);
        assert!(terms.iter().all(|t| t.character == Some('a')));
        assert_eq!(terms[0].input_position, 0);
        assert_eq!(terms[1].input_position, 2);
        assert_eq!(body.minimum_size, Some(2));
    }

    #[test]
    fn constructs_character_class_range() {
        // Pins YarrParser.h:251 range state machine -> YarrPattern.cpp:1337
        // atomCharacterClassRange. Negation is carried on the term (atomCharacterClassEnd
        // YarrPattern.cpp:1419 PatternTerm m_invert), not on the descriptor.
        let p = construct_yarr_pattern(
            "[a-c]x",
            RegexFlags::default(),
            YarrPatternId(0),
            crate::strings::StringId(0),
        )
        .unwrap();
        let terms = &p.body().alternatives[0].terms;
        assert_eq!(terms[0].kind, PatternTermKind::CharacterClass);
        let cc = terms[0].character_class.as_ref().unwrap();
        assert_eq!(
            cc.ranges,
            vec![CharacterRange {
                begin: 'a',
                end: 'c'
            }]
        );
        assert!(cc.matches.is_empty());
        assert!(cc.built_in.is_none());
        assert!(!terms[0].invert);
        assert_eq!(terms[0].input_position, 0);
        assert_eq!(terms[1].input_position, 1);

        // [^\d] keeps the built-in identity with the negation folded onto the term.
        let neg = construct_yarr_pattern(
            "[^\\d]",
            RegexFlags::default(),
            YarrPatternId(0),
            crate::strings::StringId(0),
        )
        .unwrap();
        let neg_cc = neg.body().alternatives[0].terms[0]
            .character_class
            .as_ref()
            .unwrap();
        assert_eq!(neg_cc.built_in, Some(BuiltInCharacterClassId::Digit));
        assert!(neg.body().alternatives[0].terms[0].invert);
    }

    #[test]
    fn constructs_dot_and_builtin_classes() {
        // Pins YarrParser.h:1754 '.' and YarrParser.h:941 \d -> atomBuiltInCharacterClass.
        let p = construct_yarr_pattern(
            "\\d.",
            RegexFlags::default(),
            YarrPatternId(0),
            crate::strings::StringId(0),
        )
        .unwrap();
        let terms = &p.body().alternatives[0].terms;
        assert_eq!(
            terms[0].character_class.as_ref().unwrap().built_in,
            Some(BuiltInCharacterClassId::Digit)
        );
        assert_eq!(
            terms[1].character_class.as_ref().unwrap().built_in,
            Some(BuiltInCharacterClassId::Dot)
        );
        assert_eq!(terms[0].input_position, 0);
        assert_eq!(terms[1].input_position, 1);
        assert_eq!(p.body().minimum_size, Some(2));
    }

    #[test]
    fn constructs_alternation() {
        // Pins YarrParser.h:1726 '|' -> disjunction (ForNextAlternative).
        let p = construct_yarr_pattern(
            "ab|cde",
            RegexFlags::default(),
            YarrPatternId(0),
            crate::strings::StringId(0),
        )
        .unwrap();
        let body = p.body();
        assert_eq!(body.alternatives.len(), 2);
        assert_eq!(body.alternatives[0].terms.len(), 2);
        assert_eq!(body.alternatives[1].terms.len(), 3);
        // m_minimumSize is the minimum across alternatives (YarrPattern.cpp:1963).
        assert_eq!(body.minimum_size, Some(2));
    }

    #[test]
    fn constructs_backreference() {
        // Pins YarrParser.h:1008 decimal escape -> atomBackReference (YarrPattern.cpp:1550).
        let p = construct_yarr_pattern(
            "(a)\\1",
            RegexFlags::default(),
            YarrPatternId(0),
            crate::strings::StringId(0),
        )
        .unwrap();
        assert!(p.contains_backreferences);
        let terms = &p.body().alternatives[0].terms;
        assert_eq!(terms.len(), 2);
        assert_eq!(terms[1].kind, PatternTermKind::NumberedBackReference);
        assert_eq!(terms[1].subpattern_id, Some(1));
        // YarrStackSpaceForBackTrackInfoBackReference adds 3 to the frame.
        assert_eq!(p.body().call_frame_size, 2 + 3);
    }

    #[test]
    fn constructs_named_capture_and_lookbehind() {
        // Pins YarrParser.h:1546 (?<name>) and (?<=) lookbehind (YarrParser.h:1565),
        // plus the backward ParentheticalAssertion offset rule (YarrPattern.cpp:1906:
        // disjunctionInitialInputPosition = 0 for Backward).
        let named = construct_yarr_pattern(
            "(?<word>\\w+)",
            RegexFlags::default(),
            YarrPatternId(0),
            crate::strings::StringId(0),
        )
        .unwrap();
        assert_eq!(named.capture_count, 1);
        assert_eq!(named.named_capture_count, 1);
        let child = &named.disjunctions[child_of(&named, &named.body().alternatives[0].terms[0])];
        // \w+ split into \w{1} fixed + \w{0,} greedy (YarrPattern.cpp:1746).
        assert_eq!(child.alternatives[0].terms.len(), 2);
        assert!(child.alternatives[0]
            .terms
            .iter()
            .all(|t| t.kind == PatternTermKind::CharacterClass));

        let lb = construct_yarr_pattern(
            "(?<=ab)c",
            RegexFlags::default(),
            YarrPatternId(0),
            crate::strings::StringId(0),
        )
        .unwrap();
        assert!(lb.contains_lookbehinds);
        let terms = &lb.body().alternatives[0].terms;
        assert_eq!(
            terms[0].kind,
            PatternTermKind::Assertion(PatternAssertion::LookBehind)
        );
        assert_eq!(terms[0].input_position, 0);
        let lb_child = &lb.disjunctions[child_of(&lb, &terms[0])];
        // Lookbehind body positions start at 0 even though it matches backward.
        assert_eq!(lb_child.alternatives[0].terms[0].input_position, 0);
        assert_eq!(lb_child.alternatives[0].terms[1].input_position, 1);
        assert_eq!(terms[1].character, Some('c'));
        assert_eq!(terms[1].input_position, 0);
    }

    #[test]
    fn rejects_unmatched_and_dangling_quantifier() {
        // Pins YarrParser.h:1850 MissingParentheses and YarrParser.h:1708
        // QuantifierWithoutAtom.
        assert_eq!(
            construct_yarr_pattern(
                "(ab",
                RegexFlags::default(),
                YarrPatternId(0),
                crate::strings::StringId(0)
            )
            .unwrap_err()
            .code,
            YarrErrorCode::MissingParentheses
        );
        assert_eq!(
            construct_yarr_pattern(
                "*a",
                RegexFlags::default(),
                YarrPatternId(0),
                crate::strings::StringId(0)
            )
            .unwrap_err()
            .code,
            YarrErrorCode::QuantifierWithoutAtom
        );
    }
}
