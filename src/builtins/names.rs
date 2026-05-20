use crate::strings::{Identifier, PrivateName, SymbolUid};

/// VM-local slot in the builtin name table.
///
/// A slot records where a name lives in the VM-owned table initialized beside
/// common identifiers. It is not a string index and must not be persisted across
/// VM instances.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BuiltinNameSlot(u16);

impl BuiltinNameSlot {
    pub const fn from_table_index(index: u16) -> Self {
        Self(index)
    }

    pub const fn table_index(self) -> u16 {
        self.0
    }
}

/// Stable private key reserved for builtin implementation details.
///
/// The private-name identity itself is owned by `strings::PrivateName`. Builtin
/// tables only pair that canonical identity with VM-local slots and optional
/// public aliases.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BuiltinPrivateName {
    private_name: PrivateName,
    slot: Option<BuiltinNameSlot>,
    public_alias: Option<Identifier>,
}

impl BuiltinPrivateName {
    pub const fn from_private_name(private_name: PrivateName) -> Self {
        Self {
            private_name,
            slot: None,
            public_alias: None,
        }
    }

    pub const fn from_table_slot(private_name: PrivateName, slot: BuiltinNameSlot) -> Self {
        Self {
            private_name,
            slot: Some(slot),
            public_alias: None,
        }
    }

    pub const fn with_public_alias(
        private_name: PrivateName,
        slot: BuiltinNameSlot,
        public_alias: Identifier,
    ) -> Self {
        Self {
            private_name,
            slot: Some(slot),
            public_alias: Some(public_alias),
        }
    }

    pub const fn private_name(self) -> PrivateName {
        self.private_name
    }

    pub const fn slot(self) -> Option<BuiltinNameSlot> {
        self.slot
    }

    pub const fn public_alias(self) -> Option<Identifier> {
        self.public_alias
    }
}

/// Well-known builtin name categories.
///
/// Each variant borrows canonical string/symbol/private-name identity from the
/// `strings` layer. Builtins do not allocate parallel name IDs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WellKnownBuiltinName {
    PublicIdentifier(Identifier),
    Private(BuiltinPrivateName),
    WellKnownSymbol(SymbolUid),
}

impl WellKnownBuiltinName {
    pub const fn is_private(self) -> bool {
        matches!(self, Self::Private(_))
    }

    pub const fn is_symbol(self) -> bool {
        matches!(self, Self::WellKnownSymbol(_))
    }
}

/// Direction of the public/private lookup tables maintained by `BuiltinNames`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinNameLookupKind {
    /// Textual public name to private builtin symbol, used by builtin parser paths.
    PublicToPrivate,
    /// Textual well-known symbol name to VM-owned symbol identity.
    PublicToWellKnownSymbol,
    /// Direct private name lookup when symbol identity is already preserved.
    PrivateIdentity,
}

/// Entry in the builtin public/private name map.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinNameMapEntry {
    public_name: Identifier,
    private_name: BuiltinPrivateName,
    lookup: BuiltinNameLookupKind,
}

impl BuiltinNameMapEntry {
    pub const fn new(
        public_name: Identifier,
        private_name: BuiltinPrivateName,
        lookup: BuiltinNameLookupKind,
    ) -> Self {
        Self {
            public_name,
            private_name,
            lookup,
        }
    }

    pub const fn public_name(self) -> Identifier {
        self.public_name
    }

    pub const fn private_name(self) -> BuiltinPrivateName {
        self.private_name
    }

    pub const fn lookup(self) -> BuiltinNameLookupKind {
        self.lookup
    }
}

/// Initialization state for builtin private and symbol names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinNameTableState {
    StaticSymbolsReserved,
    CommonIdentifiersReady,
    PrivateNamesInterned,
    PublicPrivateMapReady,
    WellKnownSymbolsReady,
    ExternalNamesAppended,
}

/// Private-name lookup table contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinPrivateNameTable {
    state: BuiltinNameTableState,
    private_name_count: u32,
    external_name_count: u32,
}

impl BuiltinPrivateNameTable {
    pub const fn new(
        state: BuiltinNameTableState,
        private_name_count: u32,
        external_name_count: u32,
    ) -> Self {
        Self {
            state,
            private_name_count,
            external_name_count,
        }
    }

    pub const fn state(self) -> BuiltinNameTableState {
        self.state
    }
}

/// Host-provided builtin name admitted into the builtin private-name set.
///
/// WebCore and test-only hosts can append external names after VM name-table
/// construction. Rust models this as a separate descriptor because the string
/// must already be interned in the VM and the private symbol must not be exposed
/// through public enumeration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExternalBuiltinName {
    public_name: Identifier,
    private_name: BuiltinPrivateName,
}

impl ExternalBuiltinName {
    pub const fn new(public_name: Identifier, private_name: BuiltinPrivateName) -> Self {
        Self {
            public_name,
            private_name,
        }
    }

    pub const fn public_name(self) -> Identifier {
        self.public_name
    }

    pub const fn private_name(self) -> BuiltinPrivateName {
        self.private_name
    }
}

/// VM-owned builtin name table.
///
/// Name tables are initialized with the VM and then treated as immutable except
/// for explicit external-name registration. Private builtin names must not be
/// exposed through public property enumeration.
#[derive(Debug, Default)]
pub struct BuiltinNames {
    _sealed: (),
}

impl BuiltinNames {
    pub const fn new_uninitialized() -> Self {
        Self { _sealed: () }
    }
}

/// Component that owns immutable builtin name metadata.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BuiltinNameRegistryOwner {
    #[default]
    BuiltinGenerator,
    VmCommonIdentifiers,
    HostEmbedder,
}

/// Authority allowed to append or replace builtin-name registry entries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BuiltinNameMutationAuthority {
    #[default]
    GeneratedDataRefresh,
    VmStartup,
    ExternalNameRegistration,
}

/// Immutable generated name table descriptor.
///
/// Canonical string, symbol, and private-name identity stays in `strings`.
/// Builtins only publish generated lookup tables that borrow those identities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticBuiltinNameRegistry {
    pub state: BuiltinNameTableState,
    pub owner: BuiltinNameRegistryOwner,
    pub mutation_authority: BuiltinNameMutationAuthority,
    pub names: &'static [WellKnownBuiltinName],
    pub public_private_map: &'static [BuiltinNameMapEntry],
    pub external_names: &'static [ExternalBuiltinName],
    pub private_table: BuiltinPrivateNameTable,
}

impl StaticBuiltinNameRegistry {
    pub const fn names(self) -> &'static [WellKnownBuiltinName] {
        self.names
    }

    pub const fn public_private_map(self) -> &'static [BuiltinNameMapEntry] {
        self.public_private_map
    }

    pub const fn external_names(self) -> &'static [ExternalBuiltinName] {
        self.external_names
    }

    pub fn private_entry_for_public_name(
        self,
        public_name: Identifier,
    ) -> Option<&'static BuiltinNameMapEntry> {
        self.public_private_map
            .iter()
            .find(|entry| entry.public_name() == public_name)
    }

    pub fn validate(self) -> BuiltinNameValidationReport {
        let mut findings = Vec::new();
        for (index, name) in self.names.iter().enumerate() {
            if self.names[..index]
                .iter()
                .any(|candidate| candidate == name)
            {
                findings.push(BuiltinNameValidationFinding::DuplicateWellKnownName { name: *name });
            }
        }

        for (index, entry) in self.public_private_map.iter().enumerate() {
            if self.public_private_map[..index]
                .iter()
                .any(|candidate| candidate.public_name() == entry.public_name())
            {
                findings.push(BuiltinNameValidationFinding::DuplicatePublicPrivateEntry {
                    public_name: entry.public_name(),
                });
            }
            if entry.lookup() != BuiltinNameLookupKind::PrivateIdentity
                && entry.private_name().slot().is_none()
            {
                findings.push(BuiltinNameValidationFinding::MappedPrivateNameWithoutSlot {
                    public_name: entry.public_name(),
                });
            }
        }

        if self.private_table.private_name_count
            < self.names.iter().filter(|name| name.is_private()).count() as u32
        {
            findings.push(BuiltinNameValidationFinding::PrivateNameCountTooSmall {
                declared: self.private_table.private_name_count,
            });
        }
        if self.private_table.external_name_count != self.external_names.len() as u32 {
            findings.push(BuiltinNameValidationFinding::ExternalNameCountMismatch {
                declared: self.private_table.external_name_count,
                actual: self.external_names.len() as u32,
            });
        }
        if !self.external_names.is_empty()
            && builtin_name_state_rank(self.state)
                < builtin_name_state_rank(BuiltinNameTableState::ExternalNamesAppended)
        {
            findings.push(
                BuiltinNameValidationFinding::ExternalNamesBeforeRegistryState {
                    state: self.state,
                },
            );
        }

        BuiltinNameValidationReport { findings }
    }
}

fn builtin_name_state_rank(state: BuiltinNameTableState) -> u8 {
    match state {
        BuiltinNameTableState::StaticSymbolsReserved => 0,
        BuiltinNameTableState::CommonIdentifiersReady => 1,
        BuiltinNameTableState::PrivateNamesInterned => 2,
        BuiltinNameTableState::PublicPrivateMapReady => 3,
        BuiltinNameTableState::WellKnownSymbolsReady => 4,
        BuiltinNameTableState::ExternalNamesAppended => 5,
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BuiltinNameValidationReport {
    pub findings: Vec<BuiltinNameValidationFinding>,
}

impl BuiltinNameValidationReport {
    pub fn is_valid(&self) -> bool {
        self.findings.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinNameValidationFinding {
    DuplicateWellKnownName { name: WellKnownBuiltinName },
    DuplicatePublicPrivateEntry { public_name: Identifier },
    MappedPrivateNameWithoutSlot { public_name: Identifier },
    PrivateNameCountTooSmall { declared: u32 },
    ExternalNameCountMismatch { declared: u32, actual: u32 },
    ExternalNamesBeforeRegistryState { state: BuiltinNameTableState },
}
