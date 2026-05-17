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
