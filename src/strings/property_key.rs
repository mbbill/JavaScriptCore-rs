use crate::strings::atom::{Identifier, UniquedIdentifier};
use crate::strings::symbol::{PrivateName, SymbolUid};

/// Canonical array/property index once numeric-index parsing is defined.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PropertyIndex(u32);

impl PropertyIndex {
    pub const fn from_canonical_index(index: u32) -> Self {
        Self(index)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Numeric property-name classification.
///
/// JSC distinguishes array indices from canonical numeric index strings used by
/// integer-indexed exotic objects. Parsing stays in VM/runtime code because it
/// depends on ECMAScript number conversion and allocation policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumericPropertyKeyKind {
    NotNumeric,
    ArrayIndex(PropertyIndex),
    CanonicalNumericIndex,
}

/// High-level property-key category for dispatch and diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyKeyKind {
    String,
    Symbol,
    PrivateName,
    Index,
}

/// Visibility filter used while collecting property names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyNameMode {
    Strings,
    Symbols,
    StringsAndSymbols,
}

impl PropertyNameMode {
    /// Returns whether public string keys should be included.
    pub const fn includes_strings(self) -> bool {
        matches!(self, Self::Strings | Self::StringsAndSymbols)
    }

    /// Returns whether symbol keys should be included.
    pub const fn includes_symbols(self) -> bool {
        matches!(self, Self::Symbols | Self::StringsAndSymbols)
    }
}

/// Whether private symbols can cross an enumeration boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateSymbolMode {
    Exclude,
    Include,
}

/// Conversion policy for `ToPropertyKey`-like entry points.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyKeyConversion {
    /// Ordinary ECMAScript conversion: strings/indices/symbols are allowed, but
    /// private names are not produced for user code.
    Public,
    /// Internal conversion for builtin/private-name lookup paths.
    InternalWithPrivateNames,
    /// Cache lookup conversion that must preserve symbol uid identity.
    Cacheable,
}

/// Canonical property-name identity.
///
/// This type owns the Rust-side distinction between string names, symbols,
/// private names, and numeric indices. Runtime/object layers may borrow or
/// re-export it, but they must not define alternate property-key identity
/// enums for storage, enumeration, or cache keys.
///
/// This avoids the C++ hazard where string-only identifier paths can
/// accidentally discard symbol-ness.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PropertyKey {
    String(Identifier),
    Symbol(SymbolUid),
    PrivateName(PrivateName),
    Index(PropertyIndex),
}

impl PropertyKey {
    /// Creates a public string property key.
    pub const fn from_identifier(identifier: Identifier) -> Self {
        Self::String(identifier)
    }

    /// Creates a public symbol property key.
    pub const fn from_symbol_uid(uid: SymbolUid) -> Self {
        Self::Symbol(uid)
    }

    /// Creates an internal private-name property key.
    pub const fn from_private_name(name: PrivateName) -> Self {
        Self::PrivateName(name)
    }

    /// Creates a numeric index key whose canonical string form is not stored.
    pub const fn from_index(index: PropertyIndex) -> Self {
        Self::Index(index)
    }

    pub const fn kind(self) -> PropertyKeyKind {
        match self {
            Self::String(_) => PropertyKeyKind::String,
            Self::Symbol(_) => PropertyKeyKind::Symbol,
            Self::PrivateName(_) => PropertyKeyKind::PrivateName,
            Self::Index(_) => PropertyKeyKind::Index,
        }
    }

    /// Converts a symbol-aware uniqued id into a property key, preserving
    /// symbol/private-name identity.
    pub const fn from_uniqued_identifier(uid: UniquedIdentifier) -> Self {
        match uid {
            UniquedIdentifier::String(identifier) => Self::String(identifier),
            UniquedIdentifier::Symbol(symbol) => Self::Symbol(symbol),
            UniquedIdentifier::PrivateName(name) => Self::PrivateName(name),
        }
    }

    /// Returns the public string name when this key is string-only.
    pub const fn as_identifier(self) -> Option<Identifier> {
        match self {
            Self::String(identifier) => Some(identifier),
            _ => None,
        }
    }

    /// Returns the public symbol uid when this key is a symbol.
    pub const fn as_symbol_uid(self) -> Option<SymbolUid> {
        match self {
            Self::Symbol(uid) => Some(uid),
            _ => None,
        }
    }

    /// Returns the private name when this key is internal/private.
    pub const fn as_private_name(self) -> Option<PrivateName> {
        match self {
            Self::PrivateName(name) => Some(name),
            _ => None,
        }
    }

    /// Returns the numeric index when this key was already canonicalized.
    pub const fn as_index(self) -> Option<PropertyIndex> {
        match self {
            Self::Index(index) => Some(index),
            _ => None,
        }
    }

    /// Returns true when this key must be hidden from public property lists.
    pub const fn is_private(self) -> bool {
        matches!(self, Self::PrivateName(_))
    }
}

/// Result of classifying a property key for lookup/enumeration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PropertyKeyClassification {
    kind: PropertyKeyKind,
    numeric: NumericPropertyKeyKind,
    private: bool,
}

impl PropertyKeyClassification {
    /// Creates a classification value after runtime parsing has completed.
    pub const fn new(
        kind: PropertyKeyKind,
        numeric: NumericPropertyKeyKind,
        private: bool,
    ) -> Self {
        Self {
            kind,
            numeric,
            private,
        }
    }

    /// Returns the top-level key kind.
    pub const fn kind(self) -> PropertyKeyKind {
        self.kind
    }

    /// Returns numeric parsing outcome.
    pub const fn numeric(self) -> NumericPropertyKeyKind {
        self.numeric
    }

    /// Returns whether the key is private/internal.
    pub const fn is_private(self) -> bool {
        self.private
    }
}

/// Classifies a key with optional public string text for numeric parsing.
pub fn classify_property_key(
    key: PropertyKey,
    public_string_text: Option<&str>,
) -> PropertyKeyClassification {
    let numeric = match key {
        PropertyKey::Index(index) => NumericPropertyKeyKind::ArrayIndex(index),
        PropertyKey::String(_) => public_string_text
            .map(classify_property_name_text)
            .unwrap_or(NumericPropertyKeyKind::NotNumeric),
        PropertyKey::Symbol(_) | PropertyKey::PrivateName(_) => NumericPropertyKeyKind::NotNumeric,
    };

    PropertyKeyClassification::new(key.kind(), numeric, key.is_private())
}

/// Parses the descriptor-only numeric portion of property-name classification.
pub fn classify_property_name_text(text: &str) -> NumericPropertyKeyKind {
    if let Some(index) = parse_array_index(text) {
        return NumericPropertyKeyKind::ArrayIndex(index);
    }

    if is_canonical_numeric_index_string(text) {
        NumericPropertyKeyKind::CanonicalNumericIndex
    } else {
        NumericPropertyKeyKind::NotNumeric
    }
}

fn parse_array_index(text: &str) -> Option<PropertyIndex> {
    if text.is_empty() || (text.len() > 1 && text.starts_with('0')) {
        return None;
    }

    let Ok(index) = text.parse::<u32>() else {
        return None;
    };
    if index == u32::MAX || index.to_string() != text {
        return None;
    }

    Some(PropertyIndex::from_canonical_index(index))
}

fn is_canonical_numeric_index_string(text: &str) -> bool {
    if matches!(text, "-0" | "NaN" | "Infinity" | "-Infinity") {
        return true;
    }
    if text.is_empty() || text.starts_with('+') {
        return false;
    }

    let Ok(number) = text.parse::<f64>() else {
        return false;
    };
    number.is_finite() && number.to_string() == text
}

/// Runtime services required for property-key conversion.
///
/// This is not implemented here because conversion may allocate strings,
/// consult symbol registries, and raise JS exceptions.
pub trait PropertyKeyConverter<Input> {
    /// Converts a runtime value or object-specific token into a property key.
    fn to_property_key(
        &mut self,
        input: Input,
        policy: PropertyKeyConversion,
    ) -> Option<PropertyKey>;

    /// Classifies a key for object lookup, enumeration, or indexed access.
    fn classify_property_key(&self, key: PropertyKey) -> PropertyKeyClassification;
}

/// Property enumeration filtering contract.
pub trait PropertyNameFilter {
    /// Returns whether a key should be reported to the caller.
    fn include_key(
        &self,
        key: PropertyKey,
        property_name_mode: PropertyNameMode,
        private_symbol_mode: PrivateSymbolMode,
    ) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::atom::{AtomId, Identifier};

    #[test]
    fn property_name_text_classifies_array_indices_and_numeric_indices() {
        assert_eq!(
            classify_property_name_text("0"),
            NumericPropertyKeyKind::ArrayIndex(PropertyIndex::from_canonical_index(0))
        );
        assert_eq!(
            classify_property_name_text("4294967294"),
            NumericPropertyKeyKind::ArrayIndex(PropertyIndex::from_canonical_index(u32::MAX - 1))
        );
        assert_eq!(
            classify_property_name_text("4294967295"),
            NumericPropertyKeyKind::CanonicalNumericIndex
        );
        assert_eq!(
            classify_property_name_text("01"),
            NumericPropertyKeyKind::NotNumeric
        );
    }

    #[test]
    fn property_key_classification_preserves_private_and_symbol_boundaries() {
        let string_key =
            PropertyKey::from_identifier(Identifier::from_atom(AtomId::from_table_slot(1)));
        let private_key =
            PropertyKey::from_private_name(crate::strings::PrivateName::from_symbol_uid(
                crate::strings::SymbolUid::from_table_slot(2),
            ));

        assert_eq!(
            classify_property_key(string_key, Some("7")).numeric(),
            NumericPropertyKeyKind::ArrayIndex(PropertyIndex::from_canonical_index(7))
        );
        assert!(classify_property_key(private_key, None).is_private());
    }
}
