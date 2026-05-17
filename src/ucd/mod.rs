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
    GeneralCategory,
    ScriptExtensions,
    GraphemeBreak,
    RegExpProperty,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnicodeTableDescriptor {
    pub version: UnicodeVersion,
    pub kind: UnicodeTableKind,
    pub generated_artifact: Option<crate::generator::GeneratedArtifactId>,
}
