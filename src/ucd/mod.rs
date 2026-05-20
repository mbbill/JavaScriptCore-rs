//! Unicode Character Database contracts.
//!
//! UCD data feeds identifiers, strings, Intl, Temporal, and Yarr. This module
//! records table ownership and versioning without embedding generated data.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnicodeVersion {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnicodeTableKind {
    IdentifierStart,
    IdentifierContinue,
    CaseFolding,
    Canonicalization,
    GeneralCategory,
    ScriptExtensions,
    GraphemeBreak,
    RegExpProperty,
    RegExpStringProperty,
    BinaryProperty,
    EmojiProperty,
    PropertyAliases,
    PropertyValueAliases,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnicodeDataSourceKind {
    UnicodeData,
    CaseFolding,
    DerivedCoreProperties,
    DerivedBinaryProperties,
    DerivedNormalizationProperties,
    PropList,
    PropertyAliases,
    PropertyValueAliases,
    Scripts,
    ScriptExtensions,
    EmojiData,
    EmojiSequences,
    EmojiZwJSequences,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnicodeDataSourceDescriptor {
    pub version: UnicodeVersion,
    pub source: UnicodeDataSourceKind,
    pub file_name: &'static str,
    pub provenance: UnicodeDataProvenance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnicodeDataProvenance {
    UnicodeCharacterDatabase,
    UnicodeEmojiData,
    JavaScriptCoreGenerated,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnicodeTableDescriptor {
    pub version: UnicodeVersion,
    pub kind: UnicodeTableKind,
    pub generated_artifact: Option<crate::generator::GeneratedArtifactId>,
    pub sources: &'static [UnicodeDataSourceKind],
    pub owner: UnicodeTableOwner,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnicodePropertyAliasKind {
    Property,
    PropertyValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnicodePropertyAliasOwner {
    UcdGenerator,
    YarrParser,
    Intl,
}

/// Static alias descriptor from UCD PropertyAliases or PropertyValueAliases.
///
/// Generated data owns alias mutation. Consumers keep borrowed names so that
/// parser, Intl, and table-generation code agree on canonical identity without
/// embedding lookup algorithms here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnicodePropertyAliasDescriptor {
    pub kind: UnicodePropertyAliasKind,
    pub property: &'static str,
    pub alias: &'static str,
    pub canonical: &'static str,
    pub source: UnicodeDataSourceKind,
    pub owner: UnicodePropertyAliasOwner,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnicodeTableOwner {
    Yarr,
    Lexer,
    Strings,
    Intl,
    Temporal,
    Shared,
}

/// Immutable registry of generated UCD descriptor tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnicodeDataRegistry {
    pub version: UnicodeVersion,
    pub sources: &'static [UnicodeDataSourceDescriptor],
    pub tables: &'static [UnicodeTableDescriptor],
    pub property_aliases: &'static [UnicodePropertyAliasDescriptor],
}

/// Owned UCD registry artifact produced by builder-only tests or generators.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedUnicodeDataRegistry {
    pub version: UnicodeVersion,
    pub sources: Vec<UnicodeDataSourceDescriptor>,
    pub tables: Vec<UnicodeTableDescriptor>,
    pub property_aliases: Vec<UnicodePropertyAliasDescriptor>,
}

/// Structural error reported by UCD registry builders and validators.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnicodeDataValidationError {
    VersionMismatch {
        expected: UnicodeVersion,
        actual: UnicodeVersion,
    },
    EmptyFileName(UnicodeDataSourceKind),
    DuplicateSource(UnicodeDataSourceKind),
    DuplicateSourceFile(&'static str),
    DuplicateTable(UnicodeTableKind),
    MissingTableSource(UnicodeTableKind),
    UnknownTableSource {
        table: UnicodeTableKind,
        source: UnicodeDataSourceKind,
    },
    EmptyAliasName {
        property: &'static str,
        alias: &'static str,
    },
    DuplicateAlias {
        kind: UnicodePropertyAliasKind,
        property: &'static str,
        alias: &'static str,
    },
    AliasSourceMismatch {
        kind: UnicodePropertyAliasKind,
        source: UnicodeDataSourceKind,
    },
    UnknownAliasSource(UnicodeDataSourceKind),
    MissingSemanticTable(UnicodeTableKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnicodePropertyAliasLookupMode {
    Exact,
    Loose,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnicodePropertyAliasLookup {
    pub descriptor: UnicodePropertyAliasDescriptor,
    pub matched_canonical_name: bool,
}

/// Case-folding table semantics exposed to parsers and Yarr canonicalization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnicodeCaseFoldingSemanticDescriptor {
    pub version: UnicodeVersion,
    pub case_folding_table: UnicodeTableKind,
    pub canonicalization_table: UnicodeTableKind,
    pub supports_simple_mapping: bool,
    pub supports_full_mapping: bool,
    pub supports_turkic_mapping: bool,
    pub yarr_uses_canonicalization_table: bool,
}

/// Unicode property table semantics exposed to RegExp property escapes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnicodePropertyTableSemanticDescriptor {
    pub version: UnicodeVersion,
    pub table: UnicodeTableKind,
    pub owner: UnicodeTableOwner,
    pub supports_lone_property: bool,
    pub supports_property_value_aliases: bool,
    pub may_contain_strings: bool,
    pub yarr_visible: bool,
}

/// Runtime lookup category that can cross an execution boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnicodeRuntimeLookupKind {
    CodePointTable,
    CaseFolding,
    RegExpProperty,
    PropertyAlias,
    PropertyValueAlias,
}

/// Boundary lookup outcome without generated table data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnicodeRuntimeLookupStatus {
    TableDescriptorFound,
    AliasResolved,
    AliasMissing,
    TableMissing,
}

/// Unicode lookup record handed to runtime execution boundaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnicodeRuntimeLookupResultRecord {
    pub kind: UnicodeRuntimeLookupKind,
    pub table: UnicodeTableKind,
    pub status: UnicodeRuntimeLookupStatus,
    pub owner: Option<UnicodeTableOwner>,
    pub alias: Option<UnicodePropertyAliasLookup>,
    pub yarr_visible: bool,
    pub may_contain_strings: bool,
    pub requires_generated_table: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnicodeDataDependencySummary {
    pub table: UnicodeTableKind,
    pub owner: UnicodeTableOwner,
    pub source_count: u32,
    pub generated_artifact: Option<crate::generator::GeneratedArtifactId>,
    pub yarr_visible: bool,
    pub may_contain_strings: bool,
    pub requires_generated_table: bool,
    pub depends_on_property_aliases: bool,
    pub depends_on_property_value_aliases: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnicodeDataDependencyReport {
    pub version: UnicodeVersion,
    pub summaries: Vec<UnicodeDataDependencySummary>,
}

impl UnicodeDataDependencyReport {
    pub fn generated_table_count(&self) -> u32 {
        self.summaries
            .iter()
            .filter(|summary| !summary.requires_generated_table)
            .count() as u32
    }

    pub fn deferred_table_count(&self) -> u32 {
        self.summaries
            .iter()
            .filter(|summary| summary.requires_generated_table)
            .count() as u32
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnicodeDataRegistryBuilder {
    version: UnicodeVersion,
    sources: Vec<UnicodeDataSourceDescriptor>,
    tables: Vec<UnicodeTableDescriptor>,
    property_aliases: Vec<UnicodePropertyAliasDescriptor>,
}

impl UnicodeDataRegistryBuilder {
    pub fn new(version: UnicodeVersion) -> Self {
        Self {
            version,
            sources: Vec::new(),
            tables: Vec::new(),
            property_aliases: Vec::new(),
        }
    }

    pub fn source(mut self, source: UnicodeDataSourceDescriptor) -> Self {
        self.sources.push(source);
        self
    }

    pub fn table(mut self, table: UnicodeTableDescriptor) -> Self {
        self.tables.push(table);
        self
    }

    pub fn property_alias(mut self, alias: UnicodePropertyAliasDescriptor) -> Self {
        self.property_aliases.push(alias);
        self
    }

    pub fn build(self) -> Result<OwnedUnicodeDataRegistry, UnicodeDataValidationError> {
        validate_unicode_data_parts(
            self.version,
            &self.sources,
            &self.tables,
            &self.property_aliases,
        )?;

        Ok(OwnedUnicodeDataRegistry {
            version: self.version,
            sources: self.sources,
            tables: self.tables,
            property_aliases: self.property_aliases,
        })
    }
}

const UCD_SCHEMA_VERSION: UnicodeVersion = UnicodeVersion {
    major: 0,
    minor: 0,
    patch: 0,
};

const UCD_SOURCE_DESCRIPTORS: &[UnicodeDataSourceDescriptor] = &[
    UnicodeDataSourceDescriptor {
        version: UCD_SCHEMA_VERSION,
        source: UnicodeDataSourceKind::UnicodeData,
        file_name: "UnicodeData.txt",
        provenance: UnicodeDataProvenance::UnicodeCharacterDatabase,
    },
    UnicodeDataSourceDescriptor {
        version: UCD_SCHEMA_VERSION,
        source: UnicodeDataSourceKind::CaseFolding,
        file_name: "CaseFolding.txt",
        provenance: UnicodeDataProvenance::UnicodeCharacterDatabase,
    },
    UnicodeDataSourceDescriptor {
        version: UCD_SCHEMA_VERSION,
        source: UnicodeDataSourceKind::DerivedCoreProperties,
        file_name: "DerivedCoreProperties.txt",
        provenance: UnicodeDataProvenance::UnicodeCharacterDatabase,
    },
    UnicodeDataSourceDescriptor {
        version: UCD_SCHEMA_VERSION,
        source: UnicodeDataSourceKind::DerivedBinaryProperties,
        file_name: "DerivedBinaryProperties.txt",
        provenance: UnicodeDataProvenance::UnicodeCharacterDatabase,
    },
    UnicodeDataSourceDescriptor {
        version: UCD_SCHEMA_VERSION,
        source: UnicodeDataSourceKind::DerivedNormalizationProperties,
        file_name: "DerivedNormalizationProps.txt",
        provenance: UnicodeDataProvenance::UnicodeCharacterDatabase,
    },
    UnicodeDataSourceDescriptor {
        version: UCD_SCHEMA_VERSION,
        source: UnicodeDataSourceKind::PropList,
        file_name: "PropList.txt",
        provenance: UnicodeDataProvenance::UnicodeCharacterDatabase,
    },
    UnicodeDataSourceDescriptor {
        version: UCD_SCHEMA_VERSION,
        source: UnicodeDataSourceKind::PropertyAliases,
        file_name: "PropertyAliases.txt",
        provenance: UnicodeDataProvenance::UnicodeCharacterDatabase,
    },
    UnicodeDataSourceDescriptor {
        version: UCD_SCHEMA_VERSION,
        source: UnicodeDataSourceKind::PropertyValueAliases,
        file_name: "PropertyValueAliases.txt",
        provenance: UnicodeDataProvenance::UnicodeCharacterDatabase,
    },
    UnicodeDataSourceDescriptor {
        version: UCD_SCHEMA_VERSION,
        source: UnicodeDataSourceKind::Scripts,
        file_name: "Scripts.txt",
        provenance: UnicodeDataProvenance::UnicodeCharacterDatabase,
    },
    UnicodeDataSourceDescriptor {
        version: UCD_SCHEMA_VERSION,
        source: UnicodeDataSourceKind::ScriptExtensions,
        file_name: "ScriptExtensions.txt",
        provenance: UnicodeDataProvenance::UnicodeCharacterDatabase,
    },
    UnicodeDataSourceDescriptor {
        version: UCD_SCHEMA_VERSION,
        source: UnicodeDataSourceKind::EmojiData,
        file_name: "emoji-data.txt",
        provenance: UnicodeDataProvenance::UnicodeEmojiData,
    },
    UnicodeDataSourceDescriptor {
        version: UCD_SCHEMA_VERSION,
        source: UnicodeDataSourceKind::EmojiSequences,
        file_name: "emoji-sequences.txt",
        provenance: UnicodeDataProvenance::UnicodeEmojiData,
    },
    UnicodeDataSourceDescriptor {
        version: UCD_SCHEMA_VERSION,
        source: UnicodeDataSourceKind::EmojiZwJSequences,
        file_name: "emoji-zwj-sequences.txt",
        provenance: UnicodeDataProvenance::UnicodeEmojiData,
    },
];

const IDENTIFIER_TABLE_SOURCES: &[UnicodeDataSourceKind] = &[
    UnicodeDataSourceKind::UnicodeData,
    UnicodeDataSourceKind::DerivedCoreProperties,
];

const CASE_FOLDING_TABLE_SOURCES: &[UnicodeDataSourceKind] = &[
    UnicodeDataSourceKind::UnicodeData,
    UnicodeDataSourceKind::CaseFolding,
];

const CANONICALIZATION_TABLE_SOURCES: &[UnicodeDataSourceKind] =
    &[UnicodeDataSourceKind::CaseFolding];

const GENERAL_CATEGORY_TABLE_SOURCES: &[UnicodeDataSourceKind] =
    &[UnicodeDataSourceKind::UnicodeData];

const SCRIPT_EXTENSIONS_TABLE_SOURCES: &[UnicodeDataSourceKind] =
    &[UnicodeDataSourceKind::ScriptExtensions];

const BINARY_PROPERTY_TABLE_SOURCES: &[UnicodeDataSourceKind] = &[
    UnicodeDataSourceKind::DerivedBinaryProperties,
    UnicodeDataSourceKind::PropList,
];

const EMOJI_PROPERTY_TABLE_SOURCES: &[UnicodeDataSourceKind] = &[UnicodeDataSourceKind::EmojiData];

const REGEXP_PROPERTY_TABLE_SOURCES: &[UnicodeDataSourceKind] = &[
    UnicodeDataSourceKind::DerivedBinaryProperties,
    UnicodeDataSourceKind::PropList,
    UnicodeDataSourceKind::Scripts,
    UnicodeDataSourceKind::ScriptExtensions,
    UnicodeDataSourceKind::EmojiData,
];

const REGEXP_STRING_PROPERTY_TABLE_SOURCES: &[UnicodeDataSourceKind] = &[
    UnicodeDataSourceKind::EmojiSequences,
    UnicodeDataSourceKind::EmojiZwJSequences,
];

const PROPERTY_ALIAS_TABLE_SOURCES: &[UnicodeDataSourceKind] =
    &[UnicodeDataSourceKind::PropertyAliases];

const PROPERTY_VALUE_ALIAS_TABLE_SOURCES: &[UnicodeDataSourceKind] =
    &[UnicodeDataSourceKind::PropertyValueAliases];

const UCD_TABLE_DESCRIPTORS: &[UnicodeTableDescriptor] = &[
    UnicodeTableDescriptor {
        version: UCD_SCHEMA_VERSION,
        kind: UnicodeTableKind::IdentifierStart,
        generated_artifact: None,
        sources: IDENTIFIER_TABLE_SOURCES,
        owner: UnicodeTableOwner::Lexer,
    },
    UnicodeTableDescriptor {
        version: UCD_SCHEMA_VERSION,
        kind: UnicodeTableKind::IdentifierContinue,
        generated_artifact: None,
        sources: IDENTIFIER_TABLE_SOURCES,
        owner: UnicodeTableOwner::Lexer,
    },
    UnicodeTableDescriptor {
        version: UCD_SCHEMA_VERSION,
        kind: UnicodeTableKind::CaseFolding,
        generated_artifact: None,
        sources: CASE_FOLDING_TABLE_SOURCES,
        owner: UnicodeTableOwner::Shared,
    },
    UnicodeTableDescriptor {
        version: UCD_SCHEMA_VERSION,
        kind: UnicodeTableKind::Canonicalization,
        generated_artifact: None,
        sources: CANONICALIZATION_TABLE_SOURCES,
        owner: UnicodeTableOwner::Yarr,
    },
    UnicodeTableDescriptor {
        version: UCD_SCHEMA_VERSION,
        kind: UnicodeTableKind::GeneralCategory,
        generated_artifact: None,
        sources: GENERAL_CATEGORY_TABLE_SOURCES,
        owner: UnicodeTableOwner::Shared,
    },
    UnicodeTableDescriptor {
        version: UCD_SCHEMA_VERSION,
        kind: UnicodeTableKind::ScriptExtensions,
        generated_artifact: None,
        sources: SCRIPT_EXTENSIONS_TABLE_SOURCES,
        owner: UnicodeTableOwner::Yarr,
    },
    UnicodeTableDescriptor {
        version: UCD_SCHEMA_VERSION,
        kind: UnicodeTableKind::BinaryProperty,
        generated_artifact: None,
        sources: BINARY_PROPERTY_TABLE_SOURCES,
        owner: UnicodeTableOwner::Yarr,
    },
    UnicodeTableDescriptor {
        version: UCD_SCHEMA_VERSION,
        kind: UnicodeTableKind::EmojiProperty,
        generated_artifact: None,
        sources: EMOJI_PROPERTY_TABLE_SOURCES,
        owner: UnicodeTableOwner::Yarr,
    },
    UnicodeTableDescriptor {
        version: UCD_SCHEMA_VERSION,
        kind: UnicodeTableKind::RegExpProperty,
        generated_artifact: None,
        sources: REGEXP_PROPERTY_TABLE_SOURCES,
        owner: UnicodeTableOwner::Yarr,
    },
    UnicodeTableDescriptor {
        version: UCD_SCHEMA_VERSION,
        kind: UnicodeTableKind::RegExpStringProperty,
        generated_artifact: None,
        sources: REGEXP_STRING_PROPERTY_TABLE_SOURCES,
        owner: UnicodeTableOwner::Yarr,
    },
    UnicodeTableDescriptor {
        version: UCD_SCHEMA_VERSION,
        kind: UnicodeTableKind::PropertyAliases,
        generated_artifact: None,
        sources: PROPERTY_ALIAS_TABLE_SOURCES,
        owner: UnicodeTableOwner::Shared,
    },
    UnicodeTableDescriptor {
        version: UCD_SCHEMA_VERSION,
        kind: UnicodeTableKind::PropertyValueAliases,
        generated_artifact: None,
        sources: PROPERTY_VALUE_ALIAS_TABLE_SOURCES,
        owner: UnicodeTableOwner::Shared,
    },
];

const UCD_PROPERTY_ALIASES: &[UnicodePropertyAliasDescriptor] = &[
    UnicodePropertyAliasDescriptor {
        kind: UnicodePropertyAliasKind::Property,
        property: "General_Category",
        alias: "gc",
        canonical: "General_Category",
        source: UnicodeDataSourceKind::PropertyAliases,
        owner: UnicodePropertyAliasOwner::UcdGenerator,
    },
    UnicodePropertyAliasDescriptor {
        kind: UnicodePropertyAliasKind::Property,
        property: "Script_Extensions",
        alias: "scx",
        canonical: "Script_Extensions",
        source: UnicodeDataSourceKind::PropertyAliases,
        owner: UnicodePropertyAliasOwner::YarrParser,
    },
    UnicodePropertyAliasDescriptor {
        kind: UnicodePropertyAliasKind::Property,
        property: "Script",
        alias: "sc",
        canonical: "Script",
        source: UnicodeDataSourceKind::PropertyAliases,
        owner: UnicodePropertyAliasOwner::YarrParser,
    },
    UnicodePropertyAliasDescriptor {
        kind: UnicodePropertyAliasKind::PropertyValue,
        property: "General_Category",
        alias: "L",
        canonical: "Letter",
        source: UnicodeDataSourceKind::PropertyValueAliases,
        owner: UnicodePropertyAliasOwner::YarrParser,
    },
    UnicodePropertyAliasDescriptor {
        kind: UnicodePropertyAliasKind::PropertyValue,
        property: "Script",
        alias: "Latn",
        canonical: "Latin",
        source: UnicodeDataSourceKind::PropertyValueAliases,
        owner: UnicodePropertyAliasOwner::YarrParser,
    },
];

pub const UNICODE_DATA_REGISTRY: UnicodeDataRegistry = UnicodeDataRegistry {
    version: UCD_SCHEMA_VERSION,
    sources: UCD_SOURCE_DESCRIPTORS,
    tables: UCD_TABLE_DESCRIPTORS,
    property_aliases: UCD_PROPERTY_ALIASES,
};

pub const fn unicode_data_registry() -> &'static UnicodeDataRegistry {
    &UNICODE_DATA_REGISTRY
}

pub fn unicode_data_source_descriptor(
    source: UnicodeDataSourceKind,
) -> Option<&'static UnicodeDataSourceDescriptor> {
    UCD_SOURCE_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.source == source)
}

pub fn unicode_table_descriptor(kind: UnicodeTableKind) -> Option<&'static UnicodeTableDescriptor> {
    UCD_TABLE_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.kind == kind)
}

pub fn lookup_unicode_property_alias(
    registry: &UnicodeDataRegistry,
    kind: UnicodePropertyAliasKind,
    property: &str,
    alias: &str,
    mode: UnicodePropertyAliasLookupMode,
) -> Option<UnicodePropertyAliasLookup> {
    lookup_unicode_property_alias_in_slice(registry.property_aliases, kind, property, alias, mode)
}

pub fn lookup_owned_unicode_property_alias(
    registry: &OwnedUnicodeDataRegistry,
    kind: UnicodePropertyAliasKind,
    property: &str,
    alias: &str,
    mode: UnicodePropertyAliasLookupMode,
) -> Option<UnicodePropertyAliasLookup> {
    lookup_unicode_property_alias_in_slice(&registry.property_aliases, kind, property, alias, mode)
}

pub fn describe_unicode_case_folding_semantics(
    registry: &UnicodeDataRegistry,
) -> Result<UnicodeCaseFoldingSemanticDescriptor, UnicodeDataValidationError> {
    validate_unicode_data_registry(registry)?;
    if !registry
        .tables
        .iter()
        .any(|table| table.kind == UnicodeTableKind::CaseFolding)
    {
        return Err(UnicodeDataValidationError::MissingSemanticTable(
            UnicodeTableKind::CaseFolding,
        ));
    }
    if !registry
        .tables
        .iter()
        .any(|table| table.kind == UnicodeTableKind::Canonicalization)
    {
        return Err(UnicodeDataValidationError::MissingSemanticTable(
            UnicodeTableKind::Canonicalization,
        ));
    }

    Ok(UnicodeCaseFoldingSemanticDescriptor {
        version: registry.version,
        case_folding_table: UnicodeTableKind::CaseFolding,
        canonicalization_table: UnicodeTableKind::Canonicalization,
        supports_simple_mapping: true,
        supports_full_mapping: true,
        supports_turkic_mapping: true,
        yarr_uses_canonicalization_table: true,
    })
}

pub fn describe_unicode_property_table_semantics(
    registry: &UnicodeDataRegistry,
    table: UnicodeTableKind,
) -> Result<UnicodePropertyTableSemanticDescriptor, UnicodeDataValidationError> {
    validate_unicode_data_registry(registry)?;
    let descriptor = registry
        .tables
        .iter()
        .find(|descriptor| descriptor.kind == table)
        .ok_or(UnicodeDataValidationError::MissingSemanticTable(table))?;
    let may_contain_strings = table == UnicodeTableKind::RegExpStringProperty;
    let supports_lone_property = matches!(
        table,
        UnicodeTableKind::GeneralCategory
            | UnicodeTableKind::BinaryProperty
            | UnicodeTableKind::EmojiProperty
            | UnicodeTableKind::RegExpStringProperty
    );
    let supports_property_value_aliases = matches!(
        table,
        UnicodeTableKind::GeneralCategory
            | UnicodeTableKind::ScriptExtensions
            | UnicodeTableKind::RegExpProperty
            | UnicodeTableKind::BinaryProperty
            | UnicodeTableKind::EmojiProperty
    );

    Ok(UnicodePropertyTableSemanticDescriptor {
        version: registry.version,
        table,
        owner: descriptor.owner,
        supports_lone_property,
        supports_property_value_aliases,
        may_contain_strings,
        yarr_visible: descriptor.owner == UnicodeTableOwner::Yarr
            || table == UnicodeTableKind::GeneralCategory,
    })
}

pub fn describe_unicode_runtime_table_lookup(
    registry: &UnicodeDataRegistry,
    kind: UnicodeRuntimeLookupKind,
    table: UnicodeTableKind,
) -> Result<UnicodeRuntimeLookupResultRecord, UnicodeDataValidationError> {
    validate_unicode_data_registry(registry)?;
    Ok(
        match registry
            .tables
            .iter()
            .find(|descriptor| descriptor.kind == table)
        {
            Some(descriptor) => {
                let semantics = describe_unicode_property_table_semantics(registry, table).ok();
                UnicodeRuntimeLookupResultRecord {
                    kind,
                    table,
                    status: UnicodeRuntimeLookupStatus::TableDescriptorFound,
                    owner: Some(descriptor.owner),
                    alias: None,
                    yarr_visible: semantics
                        .as_ref()
                        .map(|descriptor| descriptor.yarr_visible)
                        .unwrap_or(descriptor.owner == UnicodeTableOwner::Yarr),
                    may_contain_strings: semantics
                        .as_ref()
                        .map(|descriptor| descriptor.may_contain_strings)
                        .unwrap_or(false),
                    requires_generated_table: descriptor.generated_artifact.is_none(),
                }
            }
            None => UnicodeRuntimeLookupResultRecord {
                kind,
                table,
                status: UnicodeRuntimeLookupStatus::TableMissing,
                owner: None,
                alias: None,
                yarr_visible: false,
                may_contain_strings: false,
                requires_generated_table: false,
            },
        },
    )
}

pub fn describe_unicode_runtime_property_alias_lookup(
    registry: &UnicodeDataRegistry,
    kind: UnicodePropertyAliasKind,
    property: &str,
    alias: &str,
    mode: UnicodePropertyAliasLookupMode,
) -> Result<UnicodeRuntimeLookupResultRecord, UnicodeDataValidationError> {
    validate_unicode_data_registry(registry)?;
    let table = match kind {
        UnicodePropertyAliasKind::Property => UnicodeTableKind::PropertyAliases,
        UnicodePropertyAliasKind::PropertyValue => UnicodeTableKind::PropertyValueAliases,
    };
    let lookup_kind = match kind {
        UnicodePropertyAliasKind::Property => UnicodeRuntimeLookupKind::PropertyAlias,
        UnicodePropertyAliasKind::PropertyValue => UnicodeRuntimeLookupKind::PropertyValueAlias,
    };
    let table_descriptor = registry
        .tables
        .iter()
        .find(|descriptor| descriptor.kind == table);
    let alias_lookup = lookup_unicode_property_alias(registry, kind, property, alias, mode);

    Ok(UnicodeRuntimeLookupResultRecord {
        kind: lookup_kind,
        table,
        status: if alias_lookup.is_some() {
            UnicodeRuntimeLookupStatus::AliasResolved
        } else {
            UnicodeRuntimeLookupStatus::AliasMissing
        },
        owner: table_descriptor.map(|descriptor| descriptor.owner),
        alias: alias_lookup,
        yarr_visible: false,
        may_contain_strings: false,
        requires_generated_table: table_descriptor
            .map(|descriptor| descriptor.generated_artifact.is_none())
            .unwrap_or(false),
    })
}

pub fn summarize_unicode_data_dependencies(
    registry: &UnicodeDataRegistry,
) -> Result<UnicodeDataDependencyReport, UnicodeDataValidationError> {
    validate_unicode_data_registry(registry)?;
    let summaries = registry
        .tables
        .iter()
        .map(|table| {
            let semantics = describe_unicode_property_table_semantics(registry, table.kind).ok();
            UnicodeDataDependencySummary {
                table: table.kind,
                owner: table.owner,
                source_count: table.sources.len() as u32,
                generated_artifact: table.generated_artifact,
                yarr_visible: semantics
                    .as_ref()
                    .map(|descriptor| descriptor.yarr_visible)
                    .unwrap_or(table.owner == UnicodeTableOwner::Yarr),
                may_contain_strings: semantics
                    .as_ref()
                    .map(|descriptor| descriptor.may_contain_strings)
                    .unwrap_or(false),
                requires_generated_table: table.generated_artifact.is_none(),
                depends_on_property_aliases: table
                    .sources
                    .contains(&UnicodeDataSourceKind::PropertyAliases),
                depends_on_property_value_aliases: table
                    .sources
                    .contains(&UnicodeDataSourceKind::PropertyValueAliases),
            }
        })
        .collect();

    Ok(UnicodeDataDependencyReport {
        version: registry.version,
        summaries,
    })
}

fn lookup_unicode_property_alias_in_slice(
    aliases: &[UnicodePropertyAliasDescriptor],
    kind: UnicodePropertyAliasKind,
    property: &str,
    alias: &str,
    mode: UnicodePropertyAliasLookupMode,
) -> Option<UnicodePropertyAliasLookup> {
    aliases
        .iter()
        .find(|descriptor| {
            descriptor.kind == kind
                && alias_name_matches(descriptor.property, property, mode)
                && (alias_name_matches(descriptor.alias, alias, mode)
                    || alias_name_matches(descriptor.canonical, alias, mode))
        })
        .map(|descriptor| UnicodePropertyAliasLookup {
            descriptor: *descriptor,
            matched_canonical_name: alias_name_matches(descriptor.canonical, alias, mode),
        })
}

fn alias_name_matches(left: &str, right: &str, mode: UnicodePropertyAliasLookupMode) -> bool {
    match mode {
        UnicodePropertyAliasLookupMode::Exact => left == right,
        UnicodePropertyAliasLookupMode::Loose => loose_alias_eq(left, right),
    }
}

fn loose_alias_eq(left: &str, right: &str) -> bool {
    left.chars()
        .filter(|character| !matches!(character, '_' | '-' | ' '))
        .flat_map(char::to_lowercase)
        .eq(right
            .chars()
            .filter(|character| !matches!(character, '_' | '-' | ' '))
            .flat_map(char::to_lowercase))
}

pub fn validate_unicode_data_registry(
    registry: &UnicodeDataRegistry,
) -> Result<(), UnicodeDataValidationError> {
    validate_unicode_data_parts(
        registry.version,
        registry.sources,
        registry.tables,
        registry.property_aliases,
    )
}

pub fn validate_owned_unicode_data_registry(
    registry: &OwnedUnicodeDataRegistry,
) -> Result<(), UnicodeDataValidationError> {
    validate_unicode_data_parts(
        registry.version,
        &registry.sources,
        &registry.tables,
        &registry.property_aliases,
    )
}

fn validate_unicode_data_parts(
    version: UnicodeVersion,
    sources: &[UnicodeDataSourceDescriptor],
    tables: &[UnicodeTableDescriptor],
    property_aliases: &[UnicodePropertyAliasDescriptor],
) -> Result<(), UnicodeDataValidationError> {
    for (index, source) in sources.iter().enumerate() {
        if source.version != version {
            return Err(UnicodeDataValidationError::VersionMismatch {
                expected: version,
                actual: source.version,
            });
        }
        if source.file_name.is_empty() {
            return Err(UnicodeDataValidationError::EmptyFileName(source.source));
        }
        for other in sources.iter().skip(index + 1) {
            if source.source == other.source {
                return Err(UnicodeDataValidationError::DuplicateSource(source.source));
            }
            if source.file_name == other.file_name {
                return Err(UnicodeDataValidationError::DuplicateSourceFile(
                    source.file_name,
                ));
            }
        }
    }

    for (index, table) in tables.iter().enumerate() {
        if table.version != version {
            return Err(UnicodeDataValidationError::VersionMismatch {
                expected: version,
                actual: table.version,
            });
        }
        if table.sources.is_empty() {
            return Err(UnicodeDataValidationError::MissingTableSource(table.kind));
        }
        for source in table.sources {
            if !sources
                .iter()
                .any(|descriptor| descriptor.source == *source)
            {
                return Err(UnicodeDataValidationError::UnknownTableSource {
                    table: table.kind,
                    source: *source,
                });
            }
        }
        for other in tables.iter().skip(index + 1) {
            if table.kind == other.kind {
                return Err(UnicodeDataValidationError::DuplicateTable(table.kind));
            }
        }
    }

    for (index, alias) in property_aliases.iter().enumerate() {
        if alias.property.is_empty() || alias.alias.is_empty() || alias.canonical.is_empty() {
            return Err(UnicodeDataValidationError::EmptyAliasName {
                property: alias.property,
                alias: alias.alias,
            });
        }

        let expected_source = match alias.kind {
            UnicodePropertyAliasKind::Property => UnicodeDataSourceKind::PropertyAliases,
            UnicodePropertyAliasKind::PropertyValue => UnicodeDataSourceKind::PropertyValueAliases,
        };
        if alias.source != expected_source {
            return Err(UnicodeDataValidationError::AliasSourceMismatch {
                kind: alias.kind,
                source: alias.source,
            });
        }
        if !sources
            .iter()
            .any(|descriptor| descriptor.source == alias.source)
        {
            return Err(UnicodeDataValidationError::UnknownAliasSource(alias.source));
        }
        for other in property_aliases.iter().skip(index + 1) {
            if alias.kind == other.kind
                && alias.property == other.property
                && alias.alias == other.alias
            {
                return Err(UnicodeDataValidationError::DuplicateAlias {
                    kind: alias.kind,
                    property: alias.property,
                    alias: alias.alias,
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ucd_registry_has_alias_sources() {
        let registry = unicode_data_registry();

        assert!(registry.sources.len() >= 2);
        assert!(registry.property_aliases.iter().all(|alias| alias.source
            == UnicodeDataSourceKind::PropertyAliases
            || alias.source == UnicodeDataSourceKind::PropertyValueAliases));
        assert!(
            unicode_table_descriptor(UnicodeTableKind::RegExpStringProperty)
                .map(|descriptor| descriptor.owner == UnicodeTableOwner::Yarr)
                .unwrap_or(false)
        );
    }

    #[test]
    fn ucd_static_registry_is_structurally_valid() {
        assert!(validate_unicode_data_registry(unicode_data_registry()).is_ok());
    }

    #[test]
    fn ucd_builder_rejects_missing_table_source() {
        let version = UnicodeVersion {
            major: 1,
            minor: 0,
            patch: 0,
        };
        let error = UnicodeDataRegistryBuilder::new(version)
            .source(UnicodeDataSourceDescriptor {
                version,
                source: UnicodeDataSourceKind::UnicodeData,
                file_name: "UnicodeData.txt",
                provenance: UnicodeDataProvenance::UnicodeCharacterDatabase,
            })
            .table(UnicodeTableDescriptor {
                version,
                kind: UnicodeTableKind::CaseFolding,
                generated_artifact: None,
                sources: &[UnicodeDataSourceKind::CaseFolding],
                owner: UnicodeTableOwner::Shared,
            })
            .build()
            .unwrap_err();

        assert_eq!(
            error,
            UnicodeDataValidationError::UnknownTableSource {
                table: UnicodeTableKind::CaseFolding,
                source: UnicodeDataSourceKind::CaseFolding,
            }
        );
    }

    #[test]
    fn ucd_alias_lookup_finds_exact_alias_and_canonical_name() {
        let registry = unicode_data_registry();

        let alias = lookup_unicode_property_alias(
            registry,
            UnicodePropertyAliasKind::Property,
            "Script",
            "sc",
            UnicodePropertyAliasLookupMode::Exact,
        )
        .unwrap();
        assert_eq!(alias.descriptor.canonical, "Script");
        assert!(!alias.matched_canonical_name);

        let canonical = lookup_unicode_property_alias(
            registry,
            UnicodePropertyAliasKind::Property,
            "Script",
            "Script",
            UnicodePropertyAliasLookupMode::Exact,
        )
        .unwrap();
        assert!(canonical.matched_canonical_name);
    }

    #[test]
    fn ucd_alias_lookup_supports_loose_descriptor_matching() {
        let registry = unicode_data_registry();

        let alias = lookup_unicode_property_alias(
            registry,
            UnicodePropertyAliasKind::Property,
            "generalcategory",
            "GENERAL-CATEGORY",
            UnicodePropertyAliasLookupMode::Loose,
        )
        .unwrap();

        assert_eq!(alias.descriptor.property, "General_Category");
    }

    #[test]
    fn ucd_semantics_describe_case_folding_boundary() {
        let descriptor = describe_unicode_case_folding_semantics(unicode_data_registry()).unwrap();

        assert_eq!(descriptor.case_folding_table, UnicodeTableKind::CaseFolding);
        assert_eq!(
            descriptor.canonicalization_table,
            UnicodeTableKind::Canonicalization
        );
        assert!(descriptor.supports_simple_mapping);
        assert!(descriptor.yarr_uses_canonicalization_table);
    }

    #[test]
    fn ucd_semantics_describe_regexp_string_properties() {
        let descriptor = describe_unicode_property_table_semantics(
            unicode_data_registry(),
            UnicodeTableKind::RegExpStringProperty,
        )
        .unwrap();

        assert!(descriptor.may_contain_strings);
        assert!(descriptor.supports_lone_property);
        assert!(descriptor.yarr_visible);
    }

    #[test]
    fn ucd_runtime_table_lookup_reports_deferred_generated_data() {
        let record = describe_unicode_runtime_table_lookup(
            unicode_data_registry(),
            UnicodeRuntimeLookupKind::RegExpProperty,
            UnicodeTableKind::RegExpStringProperty,
        )
        .unwrap();

        assert_eq!(
            record.status,
            UnicodeRuntimeLookupStatus::TableDescriptorFound
        );
        assert_eq!(record.owner, Some(UnicodeTableOwner::Yarr));
        assert!(record.yarr_visible);
        assert!(record.may_contain_strings);
        assert!(record.requires_generated_table);
    }

    #[test]
    fn ucd_runtime_alias_lookup_records_hit_and_miss_without_tables() {
        let hit = describe_unicode_runtime_property_alias_lookup(
            unicode_data_registry(),
            UnicodePropertyAliasKind::Property,
            "Script",
            "sc",
            UnicodePropertyAliasLookupMode::Exact,
        )
        .unwrap();
        assert_eq!(hit.status, UnicodeRuntimeLookupStatus::AliasResolved);
        assert_eq!(hit.alias.unwrap().descriptor.canonical, "Script");

        let miss = describe_unicode_runtime_property_alias_lookup(
            unicode_data_registry(),
            UnicodePropertyAliasKind::Property,
            "Script",
            "missing",
            UnicodePropertyAliasLookupMode::Exact,
        )
        .unwrap();
        assert_eq!(miss.status, UnicodeRuntimeLookupStatus::AliasMissing);
        assert!(miss.alias.is_none());
    }

    #[test]
    fn ucd_dependency_summary_reports_yarr_and_alias_inputs() {
        let report = summarize_unicode_data_dependencies(unicode_data_registry()).unwrap();

        let regexp_strings = report
            .summaries
            .iter()
            .find(|summary| summary.table == UnicodeTableKind::RegExpStringProperty)
            .expect("regexp string table");
        assert!(regexp_strings.yarr_visible);
        assert!(regexp_strings.may_contain_strings);
        assert!(regexp_strings.requires_generated_table);

        let aliases = report
            .summaries
            .iter()
            .find(|summary| summary.table == UnicodeTableKind::PropertyAliases)
            .expect("alias table");
        assert!(aliases.depends_on_property_aliases);
        assert_eq!(report.deferred_table_count(), report.summaries.len() as u32);
    }
}
