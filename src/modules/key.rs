use crate::strings::{Identifier, PropertyKey};

/// Realm-local module map slot.
///
/// The loader maps resolved identity plus module type to a registry entry. The
/// slot is diagnostic identity only; map ownership remains with the loader.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ModuleMapSlot(u32);

impl ModuleMapSlot {
    pub const fn from_index(index: u32) -> Self {
        Self(index)
    }

    pub const fn index(self) -> u32 {
        self.0
    }
}

/// Module source type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ModuleType {
    JavaScript,
    Json,
    Wasm,
    Synthetic,
    HostDefined,
}

/// Resolved module specifier identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ResolvedSpecifier {
    identifier: Identifier,
}

impl ResolvedSpecifier {
    pub const fn from_identifier(identifier: Identifier) -> Self {
        Self { identifier }
    }

    pub const fn identifier(self) -> Identifier {
        self.identifier
    }
}

/// Origin of a resolved specifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolvedSpecifierKind {
    HostResolvedString,
    HostResolvedSymbol,
    ImportMapResolved,
    ImportMapScopeResolved,
    RegistrySyntheticKey,
}

/// Resolved specifier plus host resolution provenance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleSpecifierResolution {
    specifier: ResolvedSpecifier,
    kind: ResolvedSpecifierKind,
}

impl ModuleSpecifierResolution {
    pub const fn new(specifier: ResolvedSpecifier, kind: ResolvedSpecifierKind) -> Self {
        Self { specifier, kind }
    }

    pub const fn specifier(self) -> ResolvedSpecifier {
        self.specifier
    }

    pub const fn kind(self) -> ResolvedSpecifierKind {
        self.kind
    }
}

/// Import attributes associated with a module request.
///
/// Attribute storage and validation belong to parser/module-analysis and host
/// integration. This placeholder preserves the key-space boundary.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ImportAttributes {
    list_id: Option<ImportAttributeListId>,
    validation: ImportAttributeValidation,
}

impl ImportAttributes {
    pub const fn empty() -> Self {
        Self {
            list_id: None,
            validation: ImportAttributeValidation::NotRequired,
        }
    }

    pub const fn from_list_id(list_id: ImportAttributeListId) -> Self {
        Self {
            list_id: Some(list_id),
            validation: ImportAttributeValidation::Parsed,
        }
    }

    pub const fn with_validation(
        list_id: Option<ImportAttributeListId>,
        validation: ImportAttributeValidation,
    ) -> Self {
        Self {
            list_id,
            validation,
        }
    }

    pub const fn list_id(&self) -> Option<ImportAttributeListId> {
        self.list_id
    }

    pub const fn validation(&self) -> ImportAttributeValidation {
        self.validation
    }
}

/// Validation state for import attributes or assertions.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ImportAttributeValidation {
    NotRequired,
    Parsed,
    HostValidated,
    UnsupportedKey,
    UnsupportedValue,
    DuplicateKey,
}

/// Parser-owned import-attribute list identity after validation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ImportAttributeListId(u32);

impl ImportAttributeListId {
    pub const fn from_parser_slot(slot: u32) -> Self {
        Self(slot)
    }

    pub const fn parser_slot(self) -> u32 {
        self.0
    }
}

/// Registry key for resolved module identity and type.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ModuleKey {
    specifier: ResolvedSpecifier,
    module_type: ModuleType,
    attributes: ImportAttributes,
    map_slot: Option<ModuleMapSlot>,
}

impl ModuleKey {
    pub const fn new(
        specifier: ResolvedSpecifier,
        module_type: ModuleType,
        attributes: ImportAttributes,
    ) -> Self {
        Self {
            specifier,
            module_type,
            attributes,
            map_slot: None,
        }
    }

    pub const fn with_map_slot(
        specifier: ResolvedSpecifier,
        module_type: ModuleType,
        attributes: ImportAttributes,
        map_slot: ModuleMapSlot,
    ) -> Self {
        Self {
            specifier,
            module_type,
            attributes,
            map_slot: Some(map_slot),
        }
    }

    pub const fn module_type(&self) -> ModuleType {
        self.module_type
    }

    pub const fn specifier(&self) -> ResolvedSpecifier {
        self.specifier
    }

    pub const fn attributes(&self) -> &ImportAttributes {
        &self.attributes
    }

    pub const fn map_slot(&self) -> Option<ModuleMapSlot> {
        self.map_slot
    }
}

/// Parsed import attribute pair.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImportAttributePair {
    pub key: PropertyKey,
    pub value: Identifier,
}

/// Realm-owned import map identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ImportMapId(u32);

impl ImportMapId {
    pub const fn from_realm_slot(slot: u32) -> Self {
        Self(slot)
    }

    pub const fn realm_slot(self) -> u32 {
        self.0
    }
}

/// Import-map resolution record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImportMapResolution {
    pub import_map: ImportMapId,
    pub base_url: Identifier,
    pub requested_specifier: Identifier,
    pub resolved_specifier: ResolvedSpecifier,
    pub integrity_metadata: Option<Identifier>,
    pub kind: ResolvedSpecifierKind,
}

/// Merge policy for HTML import maps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportMapMergePolicy {
    InitialMap,
    MergeNewMap,
    RejectAfterResolution,
    WarnAndDropConflicts,
}
