//! Unicode and character-class lookup contracts for Yarr.
//!
//! Tables and canonicalization logic are generated elsewhere. These descriptors
//! allow regexp parsing, bytecode, and JIT planning to name those tables without
//! embedding Unicode data in the skeleton.

use crate::runtime::StringId;

/// Built-in or generated character class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltInCharacterClassId {
    Digit,
    Space,
    Word,
    Dot,
    UnicodeProperty(u32),
}

/// Inclusive Unicode scalar range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CharacterRange {
    pub begin: char,
    pub end: char,
}

/// Property alias/name pair from Unicode property escapes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnicodePropertyName {
    pub property: Option<StringId>,
    pub value: StringId,
}

/// Canonicalization mode selected by compile mode and flags.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnicodeCanonicalizationMode {
    None,
    IgnoreCaseLegacy,
    IgnoreCaseUnicode,
    UnicodeSets,
}

/// Result of looking up a Unicode property.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnicodePropertyLookup {
    pub name: UnicodePropertyName,
    pub class: Option<BuiltInCharacterClassId>,
    pub may_contain_strings: bool,
}

/// Descriptor for generated Unicode class data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnicodeClassDescriptor {
    pub class: BuiltInCharacterClassId,
    pub bmp_ranges: Vec<CharacterRange>,
    pub non_bmp_ranges: Vec<CharacterRange>,
    pub string_set: Vec<StringId>,
    pub canonicalization: UnicodeCanonicalizationMode,
}
