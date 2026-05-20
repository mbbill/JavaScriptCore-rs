//! Unicode and character-class lookup contracts for Yarr.
//!
//! Generated Unicode tables live elsewhere. These descriptors and lookup
//! helpers allow regexp parsing, bytecode, and JIT planning to name those tables
//! without embedding Unicode data in the skeleton.

use crate::strings::StringId;
use crate::yarr::{
    CharacterClassDescriptor, CharacterClassSetOperation, CharacterClassWidth, CompileMode,
    RegexFlags,
};

/// Built-in or generated character class.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BuiltInCharacterClassId {
    Digit,
    Space,
    Word,
    Dot,
    UnicodeProperty(u32),
}

/// Static owner of a character-class or Unicode-property class row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum YarrUnicodeSchemaOwner {
    Parser,
    Bytecode,
    GeneratedUnicodeTables,
}

/// Registry mutation authority for generated Yarr Unicode metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum YarrUnicodeRegistryAuthority {
    StaticGeneratedData,
    RegExpParserCache,
}

/// Static descriptor for built-in character-class identities.
///
/// Generated Unicode tables own range contents. Yarr parser and bytecode code
/// borrow these rows only to preserve canonical class identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltInCharacterClassDescriptor {
    pub id: BuiltInCharacterClassId,
    pub canonical_name: &'static str,
    pub owner: YarrUnicodeSchemaOwner,
    pub authority: YarrUnicodeRegistryAuthority,
    pub can_be_inverted: bool,
    pub may_contain_strings: bool,
}

const BUILT_IN_CHARACTER_CLASSES: &[BuiltInCharacterClassDescriptor] = &[
    BuiltInCharacterClassDescriptor {
        id: BuiltInCharacterClassId::Digit,
        canonical_name: "Digit",
        owner: YarrUnicodeSchemaOwner::Parser,
        authority: YarrUnicodeRegistryAuthority::StaticGeneratedData,
        can_be_inverted: true,
        may_contain_strings: false,
    },
    BuiltInCharacterClassDescriptor {
        id: BuiltInCharacterClassId::Space,
        canonical_name: "Space",
        owner: YarrUnicodeSchemaOwner::Parser,
        authority: YarrUnicodeRegistryAuthority::StaticGeneratedData,
        can_be_inverted: true,
        may_contain_strings: false,
    },
    BuiltInCharacterClassDescriptor {
        id: BuiltInCharacterClassId::Word,
        canonical_name: "Word",
        owner: YarrUnicodeSchemaOwner::Parser,
        authority: YarrUnicodeRegistryAuthority::RegExpParserCache,
        can_be_inverted: true,
        may_contain_strings: false,
    },
    BuiltInCharacterClassDescriptor {
        id: BuiltInCharacterClassId::Dot,
        canonical_name: "Dot",
        owner: YarrUnicodeSchemaOwner::Parser,
        authority: YarrUnicodeRegistryAuthority::RegExpParserCache,
        can_be_inverted: true,
        may_contain_strings: false,
    },
];

/// ECMAScript Unicode property escape family recognized by Yarr.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnicodePropertyClassKind {
    GeneralCategory,
    Script,
    ScriptExtensions,
    BinaryProperty,
    EmojiProperty,
    StringProperty,
}

/// Static descriptor for property class tables used by Unicode property escapes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnicodePropertyClassDescriptor {
    pub kind: UnicodePropertyClassKind,
    pub canonical_name: &'static str,
    pub source_table: crate::ucd::UnicodeTableKind,
    pub owner: YarrUnicodeSchemaOwner,
    pub can_be_lone_property: bool,
    pub may_contain_strings: bool,
}

const UNICODE_PROPERTY_CLASSES: &[UnicodePropertyClassDescriptor] = &[
    UnicodePropertyClassDescriptor {
        kind: UnicodePropertyClassKind::GeneralCategory,
        canonical_name: "General_Category",
        source_table: crate::ucd::UnicodeTableKind::GeneralCategory,
        owner: YarrUnicodeSchemaOwner::GeneratedUnicodeTables,
        can_be_lone_property: true,
        may_contain_strings: false,
    },
    UnicodePropertyClassDescriptor {
        kind: UnicodePropertyClassKind::Script,
        canonical_name: "Script",
        source_table: crate::ucd::UnicodeTableKind::RegExpProperty,
        owner: YarrUnicodeSchemaOwner::GeneratedUnicodeTables,
        can_be_lone_property: false,
        may_contain_strings: false,
    },
    UnicodePropertyClassDescriptor {
        kind: UnicodePropertyClassKind::ScriptExtensions,
        canonical_name: "Script_Extensions",
        source_table: crate::ucd::UnicodeTableKind::ScriptExtensions,
        owner: YarrUnicodeSchemaOwner::GeneratedUnicodeTables,
        can_be_lone_property: false,
        may_contain_strings: false,
    },
    UnicodePropertyClassDescriptor {
        kind: UnicodePropertyClassKind::BinaryProperty,
        canonical_name: "Binary_Property",
        source_table: crate::ucd::UnicodeTableKind::BinaryProperty,
        owner: YarrUnicodeSchemaOwner::GeneratedUnicodeTables,
        can_be_lone_property: true,
        may_contain_strings: false,
    },
    UnicodePropertyClassDescriptor {
        kind: UnicodePropertyClassKind::EmojiProperty,
        canonical_name: "Emoji_Property",
        source_table: crate::ucd::UnicodeTableKind::EmojiProperty,
        owner: YarrUnicodeSchemaOwner::GeneratedUnicodeTables,
        can_be_lone_property: true,
        may_contain_strings: true,
    },
    UnicodePropertyClassDescriptor {
        kind: UnicodePropertyClassKind::StringProperty,
        canonical_name: "String_Property",
        source_table: crate::ucd::UnicodeTableKind::RegExpStringProperty,
        owner: YarrUnicodeSchemaOwner::GeneratedUnicodeTables,
        can_be_lone_property: true,
        may_contain_strings: true,
    },
];

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

/// Generated canonicalization table family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CanonicalizationTableMode {
    Ucs2,
    Unicode,
}

/// Canonicalization range kind generated for Yarr case-insensitive matching.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CanonicalizationRangeKind {
    Unique,
    Set,
    RangeLo,
    RangeHi,
    AlternatingAligned,
    AlternatingUnaligned,
}

/// Descriptor for one generated canonicalization range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CanonicalizationRangeDescriptor {
    pub begin: char,
    pub end: char,
    pub value: u32,
    pub kind: CanonicalizationRangeKind,
    pub table_mode: CanonicalizationTableMode,
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

/// Immutable registry facade for Yarr Unicode class metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct YarrUnicodeRegistry {
    pub built_in_classes: &'static [BuiltInCharacterClassDescriptor],
    pub property_classes: &'static [UnicodePropertyClassDescriptor],
    pub authority: YarrUnicodeRegistryAuthority,
}

/// Owned Yarr Unicode registry artifact produced by generated-data builders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedYarrUnicodeRegistry {
    pub built_in_classes: Vec<BuiltInCharacterClassDescriptor>,
    pub property_classes: Vec<UnicodePropertyClassDescriptor>,
    pub authority: YarrUnicodeRegistryAuthority,
}

/// Structural error reported by Yarr Unicode builders and validators.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum YarrUnicodeValidationError {
    EmptyClassName,
    DuplicateBuiltInClass(BuiltInCharacterClassId),
    DuplicatePropertyClass(UnicodePropertyClassKind),
    UnknownPropertyClass(UnicodePropertyClassKind),
    UnknownUcdTable(crate::ucd::UnicodeTableKind),
    InvalidRange { begin: char, end: char },
    OverlappingRange { previous_end: char, begin: char },
    BmpRangeContainsNonBmp(CharacterRange),
    NonBmpRangeContainsBmp(CharacterRange),
    StringSetNotAllowed(BuiltInCharacterClassId),
    UnknownBuiltInClass(BuiltInCharacterClassId),
    UnsupportedSetOperation(CharacterClassSetOperation),
    StringPropertyRequiresUnicodeSets(UnicodePropertyClassKind),
    CharacterClassStringsRequireUnicodeSets,
    ClassSetOperationRequiresUnicodeSets(CharacterClassSetOperation),
}

/// Semantic descriptor for RegExp Unicode property escapes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnicodePropertySemanticDescriptor {
    pub kind: UnicodePropertyClassKind,
    pub canonical_name: &'static str,
    pub source_table: crate::ucd::UnicodeTableKind,
    pub can_be_lone_property: bool,
    pub may_contain_strings: bool,
    pub requires_unicode_sets: bool,
    pub valid_in_compile_mode: bool,
}

/// Semantic descriptor for a parsed character class boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharacterClassSemanticDescriptor {
    pub width: CharacterClassWidth,
    pub built_in: Option<BuiltInCharacterClassId>,
    pub inverted: bool,
    pub table_inverted: bool,
    pub any_character: bool,
    pub uses_set_operation: bool,
    pub may_contain_strings: bool,
    pub requires_unicode_sets: bool,
    pub in_canonical_form: bool,
}

/// Non-executing case-folding semantics selected for Yarr tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct YarrCanonicalizationSemanticDescriptor {
    pub mode: UnicodeCanonicalizationMode,
    pub table_mode: Option<CanonicalizationTableMode>,
    pub compile_mode: CompileMode,
    pub ignore_case: bool,
    pub unicode_aware: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YarrUnicodeRegistryBuilder {
    built_in_classes: Vec<BuiltInCharacterClassDescriptor>,
    property_classes: Vec<UnicodePropertyClassDescriptor>,
    authority: YarrUnicodeRegistryAuthority,
}

impl YarrUnicodeRegistryBuilder {
    pub fn new(authority: YarrUnicodeRegistryAuthority) -> Self {
        Self {
            built_in_classes: Vec::new(),
            property_classes: Vec::new(),
            authority,
        }
    }

    pub fn built_in_class(mut self, descriptor: BuiltInCharacterClassDescriptor) -> Self {
        self.built_in_classes.push(descriptor);
        self
    }

    pub fn property_class(mut self, descriptor: UnicodePropertyClassDescriptor) -> Self {
        self.property_classes.push(descriptor);
        self
    }

    pub fn build(self) -> Result<OwnedYarrUnicodeRegistry, YarrUnicodeValidationError> {
        validate_yarr_unicode_parts(&self.built_in_classes, &self.property_classes)?;

        Ok(OwnedYarrUnicodeRegistry {
            built_in_classes: self.built_in_classes,
            property_classes: self.property_classes,
            authority: self.authority,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnicodeClassDescriptorBuilder {
    descriptor: UnicodeClassDescriptor,
}

impl UnicodeClassDescriptorBuilder {
    pub fn new(class: BuiltInCharacterClassId) -> Self {
        Self {
            descriptor: UnicodeClassDescriptor {
                class,
                bmp_ranges: Vec::new(),
                non_bmp_ranges: Vec::new(),
                string_set: Vec::new(),
                canonicalization: UnicodeCanonicalizationMode::None,
            },
        }
    }

    pub fn bmp_range(mut self, begin: char, end: char) -> Self {
        self.descriptor
            .bmp_ranges
            .push(CharacterRange { begin, end });
        self
    }

    pub fn non_bmp_range(mut self, begin: char, end: char) -> Self {
        self.descriptor
            .non_bmp_ranges
            .push(CharacterRange { begin, end });
        self
    }

    pub fn string(mut self, string: StringId) -> Self {
        self.descriptor.string_set.push(string);
        self
    }

    pub fn canonicalization(mut self, canonicalization: UnicodeCanonicalizationMode) -> Self {
        self.descriptor.canonicalization = canonicalization;
        self
    }

    pub fn build(self) -> Result<UnicodeClassDescriptor, YarrUnicodeValidationError> {
        validate_unicode_class_descriptor(&self.descriptor)?;
        Ok(self.descriptor)
    }
}

pub const YARR_UNICODE_REGISTRY: YarrUnicodeRegistry = YarrUnicodeRegistry {
    built_in_classes: BUILT_IN_CHARACTER_CLASSES,
    property_classes: UNICODE_PROPERTY_CLASSES,
    authority: YarrUnicodeRegistryAuthority::StaticGeneratedData,
};

/// Returns the immutable Yarr Unicode registry descriptor.
pub const fn yarr_unicode_registry() -> &'static YarrUnicodeRegistry {
    &YARR_UNICODE_REGISTRY
}

/// Returns static built-in class metadata.
pub fn built_in_character_class_descriptor(
    id: BuiltInCharacterClassId,
) -> Option<&'static BuiltInCharacterClassDescriptor> {
    BUILT_IN_CHARACTER_CLASSES
        .iter()
        .find(|descriptor| descriptor.id == id)
}

pub fn canonicalize_character_class_descriptor(
    descriptor: &CharacterClassDescriptor,
) -> Result<CharacterClassDescriptor, YarrUnicodeValidationError> {
    if let Some(class) = descriptor.built_in {
        built_in_character_class_descriptor(class)
            .ok_or(YarrUnicodeValidationError::UnknownBuiltInClass(class))?;
    }
    let mut canonical = descriptor.clone();
    canonical.matches.sort_unstable();
    canonical.matches.dedup();
    canonical.unicode_matches.sort_unstable();
    canonical.unicode_matches.dedup();
    canonical.ranges = canonicalize_ranges(&canonical.ranges)?;
    canonical.unicode_ranges = canonicalize_ranges(&canonical.unicode_ranges)?;
    canonical.width = character_class_width(&canonical);
    canonical.in_canonical_form = true;
    Ok(canonical)
}

pub fn character_class_contains(
    descriptor: &CharacterClassDescriptor,
    character: char,
) -> Result<bool, YarrUnicodeValidationError> {
    if let Some(operation) = descriptor.operation {
        if operation != CharacterClassSetOperation::Default
            && operation != CharacterClassSetOperation::Union
        {
            return Err(YarrUnicodeValidationError::UnsupportedSetOperation(
                operation,
            ));
        }
    }

    let descriptor = canonicalize_character_class_descriptor(descriptor)?;
    let mut contains = descriptor.any_character
        || descriptor.matches.binary_search(&character).is_ok()
        || descriptor.unicode_matches.binary_search(&character).is_ok()
        || range_list_contains(&descriptor.ranges, character)
        || range_list_contains(&descriptor.unicode_ranges, character);

    if let Some(class) = descriptor.built_in {
        contains |= built_in_class_contains(class, character);
    }

    Ok(if descriptor.inverted {
        !contains
    } else {
        contains
    })
}

pub fn unicode_property_class_descriptor(
    kind: UnicodePropertyClassKind,
) -> Option<&'static UnicodePropertyClassDescriptor> {
    UNICODE_PROPERTY_CLASSES
        .iter()
        .find(|descriptor| descriptor.kind == kind)
}

pub fn canonicalization_mode_for_flags(flags: RegexFlags) -> UnicodeCanonicalizationMode {
    if !flags.ignore_case {
        UnicodeCanonicalizationMode::None
    } else if flags.unicode_sets {
        UnicodeCanonicalizationMode::UnicodeSets
    } else if flags.unicode {
        UnicodeCanonicalizationMode::IgnoreCaseUnicode
    } else {
        UnicodeCanonicalizationMode::IgnoreCaseLegacy
    }
}

pub fn describe_yarr_canonicalization_semantics(
    flags: RegexFlags,
) -> YarrCanonicalizationSemanticDescriptor {
    let compile_mode = crate::yarr::compile_mode_for_flags(flags);
    let mode = canonicalization_mode_for_flags(flags);
    let table_mode = match mode {
        UnicodeCanonicalizationMode::None => None,
        UnicodeCanonicalizationMode::IgnoreCaseLegacy => Some(CanonicalizationTableMode::Ucs2),
        UnicodeCanonicalizationMode::IgnoreCaseUnicode
        | UnicodeCanonicalizationMode::UnicodeSets => Some(CanonicalizationTableMode::Unicode),
    };

    YarrCanonicalizationSemanticDescriptor {
        mode,
        table_mode,
        compile_mode,
        ignore_case: flags.ignore_case,
        unicode_aware: flags.unicode || flags.unicode_sets,
    }
}

pub fn describe_unicode_property_semantics(
    kind: UnicodePropertyClassKind,
    compile_mode: CompileMode,
) -> Result<UnicodePropertySemanticDescriptor, YarrUnicodeValidationError> {
    let descriptor = unicode_property_class_descriptor(kind)
        .ok_or(YarrUnicodeValidationError::UnknownPropertyClass(kind))?;
    let requires_unicode_sets = descriptor.may_contain_strings;
    let valid_in_compile_mode = !requires_unicode_sets || compile_mode == CompileMode::UnicodeSets;
    if !valid_in_compile_mode {
        return Err(YarrUnicodeValidationError::StringPropertyRequiresUnicodeSets(kind));
    }

    Ok(UnicodePropertySemanticDescriptor {
        kind,
        canonical_name: descriptor.canonical_name,
        source_table: descriptor.source_table,
        can_be_lone_property: descriptor.can_be_lone_property,
        may_contain_strings: descriptor.may_contain_strings,
        requires_unicode_sets,
        valid_in_compile_mode,
    })
}

pub fn describe_character_class_semantics(
    descriptor: &CharacterClassDescriptor,
    flags: RegexFlags,
) -> Result<CharacterClassSemanticDescriptor, YarrUnicodeValidationError> {
    let canonical = canonicalize_character_class_descriptor(descriptor)?;
    let may_contain_strings = !canonical.strings.is_empty()
        || canonical
            .built_in
            .and_then(built_in_character_class_descriptor)
            .map(|class| class.may_contain_strings)
            .unwrap_or(false);
    if may_contain_strings && !flags.unicode_sets {
        return Err(YarrUnicodeValidationError::CharacterClassStringsRequireUnicodeSets);
    }
    if let Some(operation) = canonical.operation {
        if operation != CharacterClassSetOperation::Default
            && operation != CharacterClassSetOperation::Union
            && !flags.unicode_sets
        {
            return Err(
                YarrUnicodeValidationError::ClassSetOperationRequiresUnicodeSets(operation),
            );
        }
    }
    let requires_unicode_sets = may_contain_strings
        || canonical
            .operation
            .map(|operation| {
                operation != CharacterClassSetOperation::Default
                    && operation != CharacterClassSetOperation::Union
            })
            .unwrap_or(false);

    Ok(CharacterClassSemanticDescriptor {
        width: canonical.width,
        built_in: canonical.built_in,
        inverted: canonical.inverted,
        table_inverted: canonical.table_inverted,
        any_character: canonical.any_character,
        uses_set_operation: canonical.operation.is_some(),
        may_contain_strings,
        requires_unicode_sets,
        in_canonical_form: canonical.in_canonical_form,
    })
}

fn canonicalize_ranges(
    ranges: &[CharacterRange],
) -> Result<Vec<CharacterRange>, YarrUnicodeValidationError> {
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|range| range.begin);
    let mut canonical: Vec<CharacterRange> = Vec::new();
    for range in sorted {
        if range.begin > range.end {
            return Err(YarrUnicodeValidationError::InvalidRange {
                begin: range.begin,
                end: range.end,
            });
        }
        match canonical.last_mut() {
            Some(last) if (last.end as u32).saturating_add(1) >= range.begin as u32 => {
                if range.end > last.end {
                    last.end = range.end;
                }
            }
            _ => canonical.push(range),
        }
    }
    Ok(canonical)
}

fn character_class_width(descriptor: &CharacterClassDescriptor) -> CharacterClassWidth {
    let has_bmp = descriptor
        .matches
        .iter()
        .any(|character| *character <= '\u{ffff}')
        || descriptor
            .ranges
            .iter()
            .any(|range| range.begin <= '\u{ffff}')
        || descriptor
            .unicode_ranges
            .iter()
            .any(|range| range.begin <= '\u{ffff}')
        || descriptor
            .unicode_matches
            .iter()
            .any(|character| *character <= '\u{ffff}')
        || descriptor.built_in.is_some()
        || descriptor.any_character;
    let has_non_bmp = descriptor
        .unicode_matches
        .iter()
        .any(|character| *character > '\u{ffff}')
        || descriptor
            .unicode_ranges
            .iter()
            .any(|range| range.end > '\u{ffff}')
        || descriptor
            .matches
            .iter()
            .any(|character| *character > '\u{ffff}')
        || descriptor.ranges.iter().any(|range| range.end > '\u{ffff}')
        || descriptor.any_character;

    match (has_bmp, has_non_bmp) {
        (true, true) => CharacterClassWidth::BmpAndNonBmp,
        (true, false) => CharacterClassWidth::BmpOnly,
        (false, true) => CharacterClassWidth::NonBmpOnly,
        (false, false) => CharacterClassWidth::Unknown,
    }
}

fn range_list_contains(ranges: &[CharacterRange], character: char) -> bool {
    ranges
        .binary_search_by(|range| {
            if character < range.begin {
                core::cmp::Ordering::Greater
            } else if character > range.end {
                core::cmp::Ordering::Less
            } else {
                core::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

fn built_in_class_contains(class: BuiltInCharacterClassId, character: char) -> bool {
    match class {
        BuiltInCharacterClassId::Digit => character.is_ascii_digit(),
        BuiltInCharacterClassId::Space => matches!(
            character,
            '\t' | '\n' | '\u{000b}' | '\u{000c}' | '\r' | ' ' | '\u{00a0}'
        ),
        BuiltInCharacterClassId::Word => character.is_ascii_alphanumeric() || character == '_',
        BuiltInCharacterClassId::Dot => !matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}'),
        BuiltInCharacterClassId::UnicodeProperty(_) => false,
    }
}

pub fn validate_yarr_unicode_registry(
    registry: &YarrUnicodeRegistry,
) -> Result<(), YarrUnicodeValidationError> {
    validate_yarr_unicode_parts(registry.built_in_classes, registry.property_classes)
}

pub fn validate_owned_yarr_unicode_registry(
    registry: &OwnedYarrUnicodeRegistry,
) -> Result<(), YarrUnicodeValidationError> {
    validate_yarr_unicode_parts(&registry.built_in_classes, &registry.property_classes)
}

pub fn validate_unicode_class_descriptor(
    descriptor: &UnicodeClassDescriptor,
) -> Result<(), YarrUnicodeValidationError> {
    validate_ranges(&descriptor.bmp_ranges)?;
    validate_ranges(&descriptor.non_bmp_ranges)?;

    for range in &descriptor.bmp_ranges {
        if range.end > '\u{ffff}' {
            return Err(YarrUnicodeValidationError::BmpRangeContainsNonBmp(*range));
        }
    }
    for range in &descriptor.non_bmp_ranges {
        if range.begin <= '\u{ffff}' {
            return Err(YarrUnicodeValidationError::NonBmpRangeContainsBmp(*range));
        }
    }

    if !descriptor.string_set.is_empty() {
        let allows_strings = match descriptor.class {
            BuiltInCharacterClassId::UnicodeProperty(_) => true,
            _ => built_in_character_class_descriptor(descriptor.class)
                .map(|class| class.may_contain_strings)
                .unwrap_or(false),
        };
        if !allows_strings {
            return Err(YarrUnicodeValidationError::StringSetNotAllowed(
                descriptor.class,
            ));
        }
    }

    Ok(())
}

fn validate_yarr_unicode_parts(
    built_in_classes: &[BuiltInCharacterClassDescriptor],
    property_classes: &[UnicodePropertyClassDescriptor],
) -> Result<(), YarrUnicodeValidationError> {
    for (index, descriptor) in built_in_classes.iter().enumerate() {
        if descriptor.canonical_name.is_empty() {
            return Err(YarrUnicodeValidationError::EmptyClassName);
        }
        for other in built_in_classes.iter().skip(index + 1) {
            if descriptor.id == other.id {
                return Err(YarrUnicodeValidationError::DuplicateBuiltInClass(
                    descriptor.id,
                ));
            }
        }
    }

    for (index, descriptor) in property_classes.iter().enumerate() {
        if descriptor.canonical_name.is_empty() {
            return Err(YarrUnicodeValidationError::EmptyClassName);
        }
        if crate::ucd::unicode_table_descriptor(descriptor.source_table).is_none() {
            return Err(YarrUnicodeValidationError::UnknownUcdTable(
                descriptor.source_table,
            ));
        }
        for other in property_classes.iter().skip(index + 1) {
            if descriptor.kind == other.kind {
                return Err(YarrUnicodeValidationError::DuplicatePropertyClass(
                    descriptor.kind,
                ));
            }
        }
    }

    Ok(())
}

fn validate_ranges(ranges: &[CharacterRange]) -> Result<(), YarrUnicodeValidationError> {
    let mut previous_end = None;
    for range in ranges {
        if range.begin > range.end {
            return Err(YarrUnicodeValidationError::InvalidRange {
                begin: range.begin,
                end: range.end,
            });
        }
        if let Some(previous_end) = previous_end {
            if previous_end >= range.begin {
                return Err(YarrUnicodeValidationError::OverlappingRange {
                    previous_end,
                    begin: range.begin,
                });
            }
        }
        previous_end = Some(range.end);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yarr_unicode_registry_exposes_static_tables() {
        let registry = yarr_unicode_registry();

        assert!(!registry.built_in_classes.is_empty());
        assert!(!registry.property_classes.is_empty());
        assert!(registry
            .property_classes
            .iter()
            .any(|descriptor| descriptor.may_contain_strings));
        assert!(built_in_character_class_descriptor(BuiltInCharacterClassId::Digit).is_some());
    }

    #[test]
    fn yarr_unicode_static_registry_is_structurally_valid() {
        assert!(validate_yarr_unicode_registry(yarr_unicode_registry()).is_ok());
    }

    #[test]
    fn yarr_unicode_class_builder_rejects_overlapping_ranges() {
        let error = UnicodeClassDescriptorBuilder::new(BuiltInCharacterClassId::Digit)
            .bmp_range('0', '9')
            .bmp_range('5', '8')
            .build()
            .unwrap_err();

        assert_eq!(
            error,
            YarrUnicodeValidationError::OverlappingRange {
                previous_end: '9',
                begin: '5',
            }
        );
    }

    #[test]
    fn character_class_canonicalization_sorts_merges_and_sets_width() {
        let descriptor = CharacterClassDescriptor {
            built_in: None,
            matches: vec!['b', 'a', 'a'],
            ranges: vec![
                CharacterRange {
                    begin: 'd',
                    end: 'f',
                },
                CharacterRange {
                    begin: 'c',
                    end: 'c',
                },
            ],
            unicode_matches: vec!['\u{1f600}'],
            unicode_ranges: Vec::new(),
            strings: Vec::new(),
            inverted: false,
            table_inverted: false,
            any_character: false,
            width: CharacterClassWidth::Unknown,
            operation: None,
            in_canonical_form: false,
        };

        let canonical = canonicalize_character_class_descriptor(&descriptor).unwrap();

        assert_eq!(canonical.matches, vec!['a', 'b']);
        assert_eq!(
            canonical.ranges,
            vec![CharacterRange {
                begin: 'c',
                end: 'f',
            }]
        );
        assert_eq!(canonical.width, CharacterClassWidth::BmpAndNonBmp);
        assert!(canonical.in_canonical_form);
    }

    #[test]
    fn character_class_lookup_uses_built_ins_and_inversion() {
        let descriptor = CharacterClassDescriptor {
            built_in: Some(BuiltInCharacterClassId::Digit),
            matches: Vec::new(),
            ranges: Vec::new(),
            unicode_matches: Vec::new(),
            unicode_ranges: Vec::new(),
            strings: Vec::new(),
            inverted: true,
            table_inverted: false,
            any_character: false,
            width: CharacterClassWidth::Unknown,
            operation: None,
            in_canonical_form: false,
        };

        assert!(!character_class_contains(&descriptor, '5').unwrap());
        assert!(character_class_contains(&descriptor, 'x').unwrap());
    }

    #[test]
    fn unicode_property_semantics_require_unicode_sets_for_strings() {
        let error = describe_unicode_property_semantics(
            UnicodePropertyClassKind::StringProperty,
            CompileMode::Unicode,
        )
        .unwrap_err();

        assert_eq!(
            error,
            YarrUnicodeValidationError::StringPropertyRequiresUnicodeSets(
                UnicodePropertyClassKind::StringProperty
            )
        );

        let descriptor = describe_unicode_property_semantics(
            UnicodePropertyClassKind::StringProperty,
            CompileMode::UnicodeSets,
        )
        .unwrap();
        assert!(descriptor.may_contain_strings);
        assert!(descriptor.requires_unicode_sets);
    }

    #[test]
    fn canonicalization_semantics_follow_flags_without_case_mapping() {
        let legacy = describe_yarr_canonicalization_semantics(RegexFlags {
            ignore_case: true,
            ..RegexFlags::default()
        });
        assert_eq!(legacy.mode, UnicodeCanonicalizationMode::IgnoreCaseLegacy);
        assert_eq!(legacy.table_mode, Some(CanonicalizationTableMode::Ucs2));

        let unicode_sets = describe_yarr_canonicalization_semantics(RegexFlags {
            ignore_case: true,
            unicode_sets: true,
            ..RegexFlags::default()
        });
        assert_eq!(unicode_sets.mode, UnicodeCanonicalizationMode::UnicodeSets);
        assert_eq!(
            unicode_sets.table_mode,
            Some(CanonicalizationTableMode::Unicode)
        );
    }

    #[test]
    fn character_class_semantics_allow_set_operation_only_in_unicode_sets() {
        let descriptor = CharacterClassDescriptor {
            built_in: None,
            matches: vec!['a'],
            ranges: Vec::new(),
            unicode_matches: Vec::new(),
            unicode_ranges: Vec::new(),
            strings: Vec::new(),
            inverted: false,
            table_inverted: false,
            any_character: false,
            width: CharacterClassWidth::Unknown,
            operation: Some(CharacterClassSetOperation::Intersection),
            in_canonical_form: false,
        };

        assert_eq!(
            describe_character_class_semantics(&descriptor, RegexFlags::default()).unwrap_err(),
            YarrUnicodeValidationError::ClassSetOperationRequiresUnicodeSets(
                CharacterClassSetOperation::Intersection
            )
        );

        let semantic = describe_character_class_semantics(
            &descriptor,
            RegexFlags {
                unicode_sets: true,
                ..RegexFlags::default()
            },
        )
        .unwrap();
        assert!(semantic.requires_unicode_sets);
        assert!(semantic.uses_set_operation);
    }
}
