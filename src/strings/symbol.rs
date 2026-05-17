use crate::strings::atom::AtomId;

/// Stable identity for a symbol or private name.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SymbolUid(u32);

impl SymbolUid {
    pub const fn from_table_slot(slot: u32) -> Self {
        Self(slot)
    }

    pub const fn table_slot(self) -> u32 {
        self.0
    }
}

/// VM-local registry identity for `Symbol.for` and private symbol registries.
///
/// JSC stores registered symbols in a VM-owned registry, not in the atom table.
/// Registered symbol descriptions can compare by string inside that registry,
/// but property identity remains the resulting `SymbolUid`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SymbolRegistryId(u32);

impl SymbolRegistryId {
    /// Registry id reserved before VM wiring assigns the public registry.
    pub const PUBLIC: Self = Self(1);

    /// Registry id reserved for private registered symbols used by cached code.
    pub const PRIVATE: Self = Self(2);

    /// Creates a registry id from a VM-owned slot.
    pub const fn from_vm_slot(slot: u32) -> Self {
        Self(slot)
    }

    /// Returns the VM-owned registry slot.
    pub const fn vm_slot(self) -> u32 {
        self.0
    }
}

/// Kind of symbol registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolRegistryKind {
    Public,
    Private,
}

/// Boundary where a symbol uid is guaranteed unique.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolUniquenessDomain {
    /// `Symbol()` creates a fresh uid every time even with the same description.
    Fresh,
    /// `Symbol.for()` reuses the uid for matching keys in one VM registry.
    Registered(SymbolRegistryId),
    /// Builtin static well-known symbol.
    WellKnown,
    /// Builtin or parser private name.
    Private,
}

/// Symbol visibility and identity class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolKind {
    /// Fresh public symbol created by `Symbol()`.
    Public,
    /// Public symbol retrieved from the VM's `Symbol.for` registry.
    RegisteredPublic,
    /// Spec well-known symbol such as `Symbol.iterator`.
    WellKnown,
    /// Fresh or parser-generated private name.
    PrivateName,
    /// Private symbol stored in a VM-owned private symbol registry.
    RegisteredPrivateName,
    /// Builtin private name compiled into the engine.
    BuiltinPrivateName,
}

impl SymbolKind {
    /// Returns true when the uid must not be exposed as a public property key.
    pub const fn is_private(self) -> bool {
        matches!(
            self,
            Self::PrivateName | Self::RegisteredPrivateName | Self::BuiltinPrivateName
        )
    }

    /// Returns true when string equality in a registry can recover the uid.
    pub const fn is_registered(self) -> bool {
        matches!(self, Self::RegisteredPublic | Self::RegisteredPrivateName)
    }
}

/// Optional human-readable symbol description.
///
/// This is metadata; property identity is the `SymbolUid`, not the description.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SymbolDescription {
    atom: Option<AtomId>,
}

impl SymbolDescription {
    pub const fn none() -> Self {
        Self { atom: None }
    }

    pub const fn from_atom(atom: AtomId) -> Self {
        Self { atom: Some(atom) }
    }

    pub const fn atom(self) -> Option<AtomId> {
        self.atom
    }
}

/// Symbol creation metadata before the GC cell exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SymbolIdentity {
    uid: SymbolUid,
    kind: SymbolKind,
    uniqueness: SymbolUniquenessDomain,
    description: SymbolDescription,
}

impl SymbolIdentity {
    /// Records a fresh or builtin symbol identity allocated by the VM.
    pub const fn new(
        uid: SymbolUid,
        kind: SymbolKind,
        uniqueness: SymbolUniquenessDomain,
        description: SymbolDescription,
    ) -> Self {
        Self {
            uid,
            kind,
            uniqueness,
            description,
        }
    }

    /// Returns the immutable uid.
    pub const fn uid(self) -> SymbolUid {
        self.uid
    }

    /// Returns the symbol kind.
    pub const fn kind(self) -> SymbolKind {
        self.kind
    }

    /// Returns the registry/freshness domain.
    pub const fn uniqueness(self) -> SymbolUniquenessDomain {
        self.uniqueness
    }

    /// Returns optional descriptive metadata.
    pub const fn description(self) -> SymbolDescription {
        self.description
    }
}

/// GC-managed symbol cell.
///
/// The heap owns the cell. Symbol identity is immutable after creation; any
/// fields that reference GC things must be initialized before escape or written
/// through owner-aware barriers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SymbolCell {
    uid: SymbolUid,
    kind: SymbolKind,
    description: SymbolDescription,
}

impl SymbolCell {
    pub const fn new_unpublished(
        uid: SymbolUid,
        kind: SymbolKind,
        description: SymbolDescription,
    ) -> Self {
        Self {
            uid,
            kind,
            description,
        }
    }

    pub const fn uid(self) -> SymbolUid {
        self.uid
    }

    pub const fn kind(self) -> SymbolKind {
        self.kind
    }

    /// Returns the optional descriptive atom.
    pub const fn description(self) -> SymbolDescription {
        self.description
    }

    /// Returns the immutable identity metadata with a conservative uniqueness
    /// domain. VM construction should provide a more precise `SymbolIdentity`
    /// when registry information is known.
    pub const fn identity(self) -> SymbolIdentity {
        let uniqueness = match self.kind {
            SymbolKind::Public => SymbolUniquenessDomain::Fresh,
            SymbolKind::RegisteredPublic => {
                SymbolUniquenessDomain::Registered(SymbolRegistryId::PUBLIC)
            }
            SymbolKind::WellKnown => SymbolUniquenessDomain::WellKnown,
            SymbolKind::PrivateName
            | SymbolKind::RegisteredPrivateName
            | SymbolKind::BuiltinPrivateName => SymbolUniquenessDomain::Private,
        };
        SymbolIdentity::new(self.uid, self.kind, uniqueness, self.description)
    }
}

/// Private field/name identity.
///
/// Private names must not be converted through string-only identifier paths.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PrivateName {
    uid: SymbolUid,
}

impl PrivateName {
    pub const fn from_symbol_uid(uid: SymbolUid) -> Self {
        Self { uid }
    }

    pub const fn uid(self) -> SymbolUid {
        self.uid
    }
}

/// Allocation authority for symbol uids.
///
/// The implementation will live with the VM because JSC also maintains a
/// `SymbolImpl` to `Symbol` weak map. This trait only names the creation
/// contract and the registry boundary.
pub trait SymbolUidAllocator {
    /// Allocates a fresh public symbol uid.
    fn fresh_symbol(&mut self, description: SymbolDescription) -> SymbolIdentity;

    /// Allocates a fresh private name uid.
    fn fresh_private_name(&mut self, description: SymbolDescription) -> SymbolIdentity;

    /// Looks up or creates a registered uid by atomized registry key.
    fn registered_symbol(
        &mut self,
        registry: SymbolRegistryId,
        kind: SymbolRegistryKind,
        key: AtomId,
    ) -> SymbolIdentity;

    /// Returns a builtin or well-known symbol that was installed during VM startup.
    fn builtin_symbol(&self, uid: SymbolUid) -> Option<SymbolIdentity>;
}
