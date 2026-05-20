use crate::strings::symbol::{PrivateName, SymbolUid};
use std::collections::HashSet;

/// Stable identity for an interned string.
///
/// `AtomId` is a handle, not ownership of string storage. The VM-owned atom
/// table owns the mapping from source text to this identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AtomId(u32);

impl AtomId {
    /// Creates an identity from a table slot reserved by the VM atom table.
    pub const fn from_table_slot(slot: u32) -> Self {
        Self(slot)
    }

    /// Returns the table slot for diagnostics, serialization, or cache keys.
    pub const fn table_slot(self) -> u32 {
        self.0
    }
}

/// Identity of the atom table that minted an `AtomId`.
///
/// JavaScriptCore normally enters a VM by installing that VM's atom string
/// table as the current thread table. Handles therefore need a cheap way to
/// record the table domain without borrowing the table itself.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AtomTableId(u32);

impl AtomTableId {
    /// Table id reserved for handles whose domain is not yet wired to a VM.
    pub const UNASSIGNED: Self = Self(0);

    /// Creates a table id from a VM-assigned slot.
    pub const fn from_vm_slot(slot: u32) -> Self {
        Self(slot)
    }

    /// Returns the VM-assigned slot for diagnostics and cache keys.
    pub const fn vm_slot(self) -> u32 {
        self.0
    }
}

/// Coarse lifetime of atom storage behind an `AtomId`.
///
/// This does not express Rust borrowing. It records the JSC ownership promise
/// that makes a copyable identifier handle valid.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AtomLifetime {
    /// Normal VM-owned atom string table entry.
    VmAtomTable,
    /// Static literal or small string with process lifetime.
    Static,
    /// VM common identifier initialized during VM construction.
    VmCommonIdentifier,
    /// Parser arena entry that must be atomized before escaping parser-owned data.
    ParserArena,
    /// Host-provided external name whose owner promises VM lifetime.
    ExternalVmName,
}

/// Domain information carried beside an atom handle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AtomDomain {
    table: AtomTableId,
    lifetime: AtomLifetime,
}

impl AtomDomain {
    /// Default domain for a VM-owned atom table entry.
    pub const fn vm(table: AtomTableId) -> Self {
        Self {
            table,
            lifetime: AtomLifetime::VmAtomTable,
        }
    }

    /// Domain used by `CommonIdentifiers` and builtin name tables.
    pub const fn common_identifier(table: AtomTableId) -> Self {
        Self {
            table,
            lifetime: AtomLifetime::VmCommonIdentifier,
        }
    }

    /// Returns the table that minted the atom.
    pub const fn table(self) -> AtomTableId {
        self.table
    }

    /// Returns the storage lifetime contract behind the atom.
    pub const fn lifetime(self) -> AtomLifetime {
        self.lifetime
    }
}

/// Entry kind for VM/global common identifiers.
///
/// JSC keeps public names, keywords, private builtin names, and well-known
/// symbols in adjacent VM-owned tables. The Rust model keeps the category
/// explicit so object/runtime code does not infer visibility from text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommonIdentifierKind {
    PublicName,
    Keyword,
    PrivateBuiltinName,
    WellKnownSymbol,
    PrivateFieldName,
    ExternalName,
}

/// Stable slot in the VM's common identifier table.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CommonIdentifierSlot(u16);

impl CommonIdentifierSlot {
    /// Creates a common identifier slot allocated by VM initialization.
    pub const fn from_index(index: u16) -> Self {
        Self(index)
    }

    /// Returns the VM-local common identifier index.
    pub const fn index(self) -> u16 {
        self.0
    }
}

/// Metadata for a VM common identifier entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommonIdentifier {
    slot: CommonIdentifierSlot,
    identifier: Identifier,
    kind: CommonIdentifierKind,
}

impl CommonIdentifier {
    /// Records a common identifier slot after VM initialization has interned it.
    pub const fn new(
        slot: CommonIdentifierSlot,
        identifier: Identifier,
        kind: CommonIdentifierKind,
    ) -> Self {
        Self {
            slot,
            identifier,
            kind,
        }
    }

    /// Returns the VM-local slot.
    pub const fn slot(self) -> CommonIdentifierSlot {
        self.slot
    }

    /// Returns the identifier handle stored in the slot.
    pub const fn identifier(self) -> Identifier {
        self.identifier
    }

    /// Returns the common-name category.
    pub const fn kind(self) -> CommonIdentifierKind {
        self.kind
    }
}

/// Owner of immutable atom/common-identifier registry metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtomRegistryOwner {
    VmAtomTable,
    CommonIdentifiers,
    ParserStaticNames,
    GeneratedBuiltinNames,
    HostStaticNames,
}

/// Provenance for atom registry descriptors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtomRegistryProvenance {
    HandAuthoredRust,
    GeneratedFromEngineMetadata,
    Ecma262Names,
    HostEmbedding,
}

/// Static descriptor for a common identifier before or after interning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommonIdentifierDescriptor {
    pub slot: CommonIdentifierSlot,
    pub kind: CommonIdentifierKind,
    pub text: &'static str,
    pub atom: Option<AtomId>,
}

/// Immutable descriptor for an atom/common-identifier registry.
///
/// The VM string table owns mutation and interning. This descriptor only names
/// generated or hand-authored static data that can be installed by that owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AtomRegistryDescriptor {
    pub name: &'static str,
    pub table: AtomTableId,
    pub scope: AtomTableScope,
    pub owner: AtomRegistryOwner,
    pub provenance: AtomRegistryProvenance,
    common_identifiers: &'static [CommonIdentifierDescriptor],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtomRegistryValidationError {
    EmptyRegistryName,
    EmptyIdentifierText(CommonIdentifierSlot),
    DuplicateSlot(CommonIdentifierSlot),
    DuplicateText(&'static str),
    DuplicateAtom(AtomId),
    CommonIdentifierWithoutTable(CommonIdentifierSlot),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AtomRegistryDescriptorBuilder {
    name: &'static str,
    table: AtomTableId,
    scope: AtomTableScope,
    owner: AtomRegistryOwner,
    provenance: AtomRegistryProvenance,
    common_identifiers: &'static [CommonIdentifierDescriptor],
}

impl AtomRegistryDescriptorBuilder {
    pub const fn new(
        name: &'static str,
        table: AtomTableId,
        scope: AtomTableScope,
        owner: AtomRegistryOwner,
        provenance: AtomRegistryProvenance,
    ) -> Self {
        Self {
            name,
            table,
            scope,
            owner,
            provenance,
            common_identifiers: &[],
        }
    }

    pub const fn common_identifiers(
        mut self,
        common_identifiers: &'static [CommonIdentifierDescriptor],
    ) -> Self {
        self.common_identifiers = common_identifiers;
        self
    }

    pub fn build(self) -> Result<AtomRegistryDescriptor, AtomRegistryValidationError> {
        let descriptor = AtomRegistryDescriptor::new(
            self.name,
            self.table,
            self.scope,
            self.owner,
            self.provenance,
            self.common_identifiers,
        );
        descriptor.validate()?;
        Ok(descriptor)
    }
}

impl AtomRegistryDescriptor {
    pub const fn new(
        name: &'static str,
        table: AtomTableId,
        scope: AtomTableScope,
        owner: AtomRegistryOwner,
        provenance: AtomRegistryProvenance,
        common_identifiers: &'static [CommonIdentifierDescriptor],
    ) -> Self {
        Self {
            name,
            table,
            scope,
            owner,
            provenance,
            common_identifiers,
        }
    }

    /// Returns immutable common-identifier descriptors for this registry.
    pub const fn common_identifiers(&self) -> &'static [CommonIdentifierDescriptor] {
        self.common_identifiers
    }

    /// Returns one existing common-identifier descriptor by table index.
    pub const fn common_identifier_at(
        &self,
        index: usize,
    ) -> Option<&'static CommonIdentifierDescriptor> {
        if index < self.common_identifiers.len() {
            Some(&self.common_identifiers[index])
        } else {
            None
        }
    }

    pub fn validate(&self) -> Result<(), AtomRegistryValidationError> {
        validate_atom_registry_descriptor(self)
    }
}

pub fn validate_atom_registry_descriptor(
    descriptor: &AtomRegistryDescriptor,
) -> Result<(), AtomRegistryValidationError> {
    if descriptor.name.is_empty() {
        return Err(AtomRegistryValidationError::EmptyRegistryName);
    }

    let mut seen_slots = HashSet::new();
    let mut seen_text = HashSet::new();
    let mut seen_atoms = HashSet::new();
    for entry in descriptor.common_identifiers {
        if entry.text.is_empty() {
            return Err(AtomRegistryValidationError::EmptyIdentifierText(entry.slot));
        }
        if !seen_slots.insert(entry.slot) {
            return Err(AtomRegistryValidationError::DuplicateSlot(entry.slot));
        }
        if !seen_text.insert(entry.text) {
            return Err(AtomRegistryValidationError::DuplicateText(entry.text));
        }
        if let Some(atom) = entry.atom {
            if descriptor.table == AtomTableId::UNASSIGNED {
                return Err(AtomRegistryValidationError::CommonIdentifierWithoutTable(
                    entry.slot,
                ));
            }
            if !seen_atoms.insert(atom) {
                return Err(AtomRegistryValidationError::DuplicateAtom(atom));
            }
        }
    }
    Ok(())
}

/// Non-mutating interning action for a common identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtomInterningAction {
    AlreadyInterned(AtomId),
    ReuseExisting(AtomId),
    InternStatic,
}

/// One planned atom-table operation for VM initialization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AtomInterningPlanEntry {
    pub slot: CommonIdentifierSlot,
    pub kind: CommonIdentifierKind,
    pub text: &'static str,
    pub action: AtomInterningAction,
    pub domain: AtomDomain,
}

/// Builds a deterministic interning plan without mutating an atom table.
pub fn plan_atom_registry_interning(
    descriptor: &AtomRegistryDescriptor,
    existing_atoms: &[(&str, AtomId)],
) -> Result<Vec<AtomInterningPlanEntry>, AtomRegistryValidationError> {
    descriptor.validate()?;

    let domain = AtomDomain::common_identifier(descriptor.table);
    let mut plan = Vec::with_capacity(descriptor.common_identifiers.len());
    for entry in descriptor.common_identifiers {
        let action = if let Some(atom) = entry.atom {
            AtomInterningAction::AlreadyInterned(atom)
        } else if let Some((_, atom)) = existing_atoms
            .iter()
            .find(|(text, _)| *text == entry.text)
            .copied()
        {
            AtomInterningAction::ReuseExisting(atom)
        } else {
            AtomInterningAction::InternStatic
        };

        plan.push(AtomInterningPlanEntry {
            slot: entry.slot,
            kind: entry.kind,
            text: entry.text,
            action,
            domain,
        });
    }

    Ok(plan)
}

/// Parser/runtime identifier for a string name.
///
/// Rust keeps ordinary identifiers string-only. This is the safer default for
/// parser names, module specifiers, and public property names. C++ JSC also has
/// `Identifier::fromUid`, which can preserve symbol-ness. Rust models that
/// through `UniquedIdentifier` and `PropertyKey` so callers must choose whether
/// symbol/private-name identity is allowed to cross the API boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Identifier {
    atom: AtomId,
    domain: AtomDomain,
}

impl Identifier {
    pub const fn from_atom(atom: AtomId) -> Self {
        Self {
            atom,
            domain: AtomDomain::vm(AtomTableId::UNASSIGNED),
        }
    }

    /// Creates an identifier with explicit atom-table lifetime metadata.
    pub const fn from_atom_in_domain(atom: AtomId, domain: AtomDomain) -> Self {
        Self { atom, domain }
    }

    pub const fn atom(self) -> AtomId {
        self.atom
    }

    /// Returns the table/lifetime contract for this identifier.
    pub const fn domain(self) -> AtomDomain {
        self.domain
    }
}

/// Symbol-aware uniqued identifier used at the C++ `Identifier::fromUid` edge.
///
/// This type deliberately does not replace `Identifier`. Most parser/runtime
/// APIs should continue to require `Identifier` when they only accept public
/// string names. APIs that mirror `PropertyName`, private names, or inline
/// cache keys can accept this wider type and then classify it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UniquedIdentifier {
    String(Identifier),
    Symbol(SymbolUid),
    PrivateName(PrivateName),
}

impl UniquedIdentifier {
    /// Mirrors `Identifier::fromString`: symbol-ness has already been discarded.
    pub const fn from_string(identifier: Identifier) -> Self {
        Self::String(identifier)
    }

    /// Mirrors the symbol-preserving `Identifier::fromUid` path.
    pub const fn from_symbol_uid(uid: SymbolUid) -> Self {
        Self::Symbol(uid)
    }

    /// Mirrors `Identifier::fromUid(PrivateName)` while keeping privacy visible.
    pub const fn from_private_name(name: PrivateName) -> Self {
        Self::PrivateName(name)
    }

    /// Returns true when the uniqued id is backed by a symbol uid.
    pub const fn is_symbol(self) -> bool {
        matches!(self, Self::Symbol(_) | Self::PrivateName(_))
    }

    /// Returns true for private names and private registered symbols.
    pub const fn is_private_name(self) -> bool {
        matches!(self, Self::PrivateName(_))
    }
}

/// Key form suitable for property caches and future inline caches.
///
/// Numeric index parsing is named here but not implemented; the canonical
/// parser for numeric indices is an unresolved cross-module decision.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CacheableIdentifier {
    /// A string-only identifier whose atom storage is known to outlive the cache.
    Identifier(Identifier),
    /// Symbol uid whose corresponding `SymbolCell` can be materialized by the VM.
    Symbol(SymbolUid),
    /// Private name uid. This must not be exposed through public enumeration.
    PrivateName(PrivateName),
    /// Canonical array index stored without re-stringifying.
    NumericIndex(u32),
}

/// Ownership mode for a cacheable identifier entry.
///
/// C++ uses tagged raw bits that are either a GC cell pointer or a uniqued uid.
/// The Rust skeleton names that distinction before choosing an encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheableIdentifierStorage {
    /// The cache holds a uid owned by a code block, VM common table, or stub.
    UniquedId,
    /// The cache holds a GC cell and must visit it if the cache itself is traced.
    GcCell,
}

/// Lifetime proof for a uid stored in a cache.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheableIdentifierOwner {
    CodeBlock,
    ImmortalVmName,
    SharedStub,
    RuntimeCell,
}

/// Classification result for cache users that need barrier behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CacheableIdentifierClassification {
    storage: CacheableIdentifierStorage,
    owner: CacheableIdentifierOwner,
}

impl CacheableIdentifierClassification {
    /// Records how the cache entry is stored and which owner keeps it live.
    pub const fn new(storage: CacheableIdentifierStorage, owner: CacheableIdentifierOwner) -> Self {
        Self { storage, owner }
    }

    /// Returns the low-level storage class.
    pub const fn storage(self) -> CacheableIdentifierStorage {
        self.storage
    }

    /// Returns the lifetime owner.
    pub const fn owner(self) -> CacheableIdentifierOwner {
        self.owner
    }
}

/// VM-owned intern table.
///
/// Mutating this table requires VM/string-table authority. The table is the
/// owner of interned string identity, while `Identifier` and `AtomId` remain
/// small copyable handles.
#[derive(Debug)]
pub struct AtomTable {
    id: AtomTableId,
    scope: AtomTableScope,
}

impl AtomTable {
    pub const fn new_uninitialized() -> Self {
        Self {
            id: AtomTableId::UNASSIGNED,
            scope: AtomTableScope::VmEntryThread,
        }
    }

    /// Creates metadata for a VM-owned table.
    pub const fn for_vm(id: AtomTableId, scope: AtomTableScope) -> Self {
        Self { id, scope }
    }

    /// Returns the table identity.
    pub const fn id(&self) -> AtomTableId {
        self.id
    }

    /// Returns how the table is installed for lookup.
    pub const fn scope(&self) -> AtomTableScope {
        self.scope
    }
}

impl Default for AtomTable {
    fn default() -> Self {
        Self::new_uninitialized()
    }
}

/// Where atom-table lookups are resolved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtomTableScope {
    /// The VM uses the thread's current atom table while entered through JSLock.
    VmEntryThread,
    /// A non-default VM owns a private atom table.
    VmPrivate,
    /// Static process-wide atoms that do not require a VM lookup.
    StaticProcess,
}

/// Mutation surface for VM-controlled interning.
///
/// Implementations must make interning atomic with respect to the owning VM and
/// must not expose table storage lifetimes through returned handles.
pub trait AtomTableMutation {
    /// Interns text as a public string identifier, discarding symbol-ness.
    fn intern_identifier(&mut self, text: &str) -> Identifier;

    /// Interns static text; implementations may point at static/small strings.
    fn intern_static_identifier(&mut self, text: &'static str) -> Identifier;

    /// Looks up an existing public string identifier without allocating.
    fn lookup_identifier(&self, text: &str) -> Option<Identifier>;

    /// Returns a common identifier by VM slot when it has been initialized.
    fn common_identifier(&self, slot: CommonIdentifierSlot) -> Option<CommonIdentifier>;
}

#[cfg(test)]
mod tests {
    use super::*;

    static COMMON: &[CommonIdentifierDescriptor] = &[
        CommonIdentifierDescriptor {
            slot: CommonIdentifierSlot::from_index(0),
            kind: CommonIdentifierKind::PublicName,
            text: "length",
            atom: Some(AtomId::from_table_slot(1)),
        },
        CommonIdentifierDescriptor {
            slot: CommonIdentifierSlot::from_index(1),
            kind: CommonIdentifierKind::Keyword,
            text: "await",
            atom: None,
        },
    ];

    #[test]
    fn atom_interning_plan_reuses_existing_atoms_without_mutating_table() {
        let registry = AtomRegistryDescriptorBuilder::new(
            "common",
            AtomTableId::from_vm_slot(3),
            AtomTableScope::VmEntryThread,
            AtomRegistryOwner::CommonIdentifiers,
            AtomRegistryProvenance::HandAuthoredRust,
        )
        .common_identifiers(COMMON)
        .build()
        .unwrap();

        let plan =
            plan_atom_registry_interning(&registry, &[("await", AtomId::from_table_slot(9))])
                .unwrap();

        assert_eq!(
            plan[0].action,
            AtomInterningAction::AlreadyInterned(AtomId::from_table_slot(1))
        );
        assert_eq!(
            plan[1].action,
            AtomInterningAction::ReuseExisting(AtomId::from_table_slot(9))
        );
        assert_eq!(
            plan[1].domain,
            AtomDomain::common_identifier(AtomTableId::from_vm_slot(3))
        );
    }
}
