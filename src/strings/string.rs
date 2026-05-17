use crate::strings::atom::AtomId;

/// Runtime string representation category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeStringKind {
    /// Flat `StringImpl`-like text owned by the runtime string cell.
    Flat,
    /// Deferred concatenation of multiple `JsString` fibers.
    Rope,
    /// Flat text that is already atomized and can become an `Identifier`.
    AtomBacked,
    /// Text whose bytes are owned by an embedder or another engine object.
    External,
    /// Substring view over a resolved base string.
    Substring,
}

/// Who owns or pins the bytes behind a string cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StringOwnership {
    GcCell,
    AtomTable,
    StaticProcess,
    External(ExternalStringOwner),
    RopeFibers,
    SubstringBase(SubstringBase),
}

/// Publication state for a GC-owned string cell.
///
/// Construction may use unbarriered initialization before escape. After
/// publication, cached references or flattened rope storage must use the owning
/// cell's GC barrier discipline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StringCellState {
    Initializing,
    Published,
}

/// Width of the underlying character storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StringEncoding {
    Latin1,
    Utf16,
}

/// Length in UTF-16 code units.
///
/// JSC relies on string length fitting in signed 32-bit arithmetic. This handle
/// names that invariant without implementing overflow checks in this module.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct StringLength(u32);

impl StringLength {
    /// Maximum length mirrored from `JSString::MaxLength`.
    pub const MAX_CODE_UNITS: u32 = i32::MAX as u32;

    /// Creates a length that has already been checked by the VM.
    pub const fn from_checked_code_units(code_units: u32) -> Self {
        Self(code_units)
    }

    /// Returns the UTF-16 code unit length.
    pub const fn code_units(self) -> u32 {
        self.0
    }
}

/// Atomization state for a runtime string.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StringAtomizationState {
    /// No atom lookup has been attempted or cached.
    Unknown,
    /// An atom lookup failed and did not allocate.
    KnownNonAtom,
    /// The string has an atom identity.
    Atom(AtomId),
}

/// Atom lookup or creation policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtomizationMode {
    LookupOnly,
    InternIfMissing,
    CommonIdentifierOnly,
}

/// Flat storage responsibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlatStringStorage {
    /// Cell owns the ref-counted string implementation.
    Owned,
    /// Cell points at a static or small string owned by the VM/process.
    StaticOrSmallString,
    /// Cell points at atomized storage in the VM atom table.
    AtomTable,
    /// Cell borrows bytes from a host object that must outlive the JS string.
    ExternalOwner,
}

/// Flat string metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FlatString {
    length: StringLength,
    encoding: StringEncoding,
    storage: FlatStringStorage,
    atomization: StringAtomizationState,
}

impl FlatString {
    /// Records flat text after the VM has decided the owner and encoding.
    pub const fn new(
        length: StringLength,
        encoding: StringEncoding,
        storage: FlatStringStorage,
        atomization: StringAtomizationState,
    ) -> Self {
        Self {
            length,
            encoding,
            storage,
            atomization,
        }
    }

    /// Returns the code unit length.
    pub const fn length(self) -> StringLength {
        self.length
    }

    /// Returns the character width.
    pub const fn encoding(self) -> StringEncoding {
        self.encoding
    }

    /// Returns the storage owner category.
    pub const fn storage(self) -> FlatStringStorage {
        self.storage
    }

    /// Returns atomization cache state.
    pub const fn atomization(self) -> StringAtomizationState {
        self.atomization
    }
}

/// Boundary for external strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalStringOwner {
    /// Host data promises the JS string cannot outlive the bytes.
    HostObject,
    /// Parser/source provider storage.
    SourceProvider,
    /// Static embedder data.
    Static,
}

/// Pinning requirement for external text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalStringPinning {
    PinnedForVmLifetime,
    PinnedByHostObject,
    CopyBeforeGc,
    FinalizeWithCell,
}

/// External string metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExternalString {
    length: StringLength,
    encoding: StringEncoding,
    owner: ExternalStringOwner,
    pinning: ExternalStringPinning,
}

impl ExternalString {
    /// Records an external string boundary.
    pub const fn new(
        length: StringLength,
        encoding: StringEncoding,
        owner: ExternalStringOwner,
    ) -> Self {
        Self {
            length,
            encoding,
            owner,
            pinning: ExternalStringPinning::CopyBeforeGc,
        }
    }

    /// Records an external string boundary with an explicit pinning contract.
    pub const fn new_with_pinning(
        length: StringLength,
        encoding: StringEncoding,
        owner: ExternalStringOwner,
        pinning: ExternalStringPinning,
    ) -> Self {
        Self {
            length,
            encoding,
            owner,
            pinning,
        }
    }

    /// Returns the code unit length.
    pub const fn length(self) -> StringLength {
        self.length
    }

    /// Returns the character width.
    pub const fn encoding(self) -> StringEncoding {
        self.encoding
    }

    /// Returns the external owner category.
    pub const fn owner(self) -> ExternalStringOwner {
        self.owner
    }

    /// Returns the lifetime/pinning rule for the external bytes.
    pub const fn pinning(self) -> ExternalStringPinning {
        self.pinning
    }
}

/// GC-managed JavaScript string cell.
///
/// The heap owns this cell. Rust code must access it through handles, roots, or
/// explicitly scoped borrowed access supplied by the GC module. This skeleton
/// does not choose the final text storage or C++ layout compatibility mode.
#[derive(Debug)]
pub struct JsString {
    kind: RuntimeStringKind,
    state: StringCellState,
    atom: Option<AtomId>,
}

impl JsString {
    pub const fn layout_placeholder(kind: RuntimeStringKind, atom: Option<AtomId>) -> Self {
        Self {
            kind,
            state: StringCellState::Initializing,
            atom,
        }
    }

    pub const fn kind(&self) -> RuntimeStringKind {
        self.kind
    }

    pub const fn atom(&self) -> Option<AtomId> {
        self.atom
    }

    pub const fn state(&self) -> StringCellState {
        self.state
    }

    /// Returns true when the cell can expose an atom identity without resolving
    /// a rope or consulting the atom table.
    pub const fn is_atom_backed(&self) -> bool {
        self.atom.is_some()
    }
}

/// One side of a deferred rope concatenation.
///
/// The exact representation must be GC-aware and may need unsafe layout work if
/// JSC's low-bit rope tagging is preserved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RopePiece {
    /// Already atomized flat text.
    Atom(AtomId),
    /// External text that must be copied or pinned before flattening.
    ExternalText,
    /// Heap-owned `JsString` fiber. The actual pointer is GC-owned and omitted
    /// from this skeleton until the object model selects handle types.
    RuntimeString,
    /// Substring fiber over a resolved base.
    Substring,
}

/// Ownership of a rope fiber.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RopeFiberOwnership {
    GcString,
    Atom,
    ExternalPinned,
    ExternalNeedsCopy,
}

/// Metadata for one rope fiber.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RopeFiber {
    pub piece: RopePieceKind,
    pub ownership: RopeFiberOwnership,
    pub length: StringLength,
    pub encoding: StringEncoding,
}

/// Copyable rope-piece category for metadata tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RopePieceKind {
    Atom(AtomId),
    ExternalText,
    RuntimeString,
    Substring,
}

/// Number of fibers encoded in a non-substring rope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RopeArity {
    Two,
    Three,
}

/// Rope topology.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RopeKind {
    /// Concatenation of two or three fibers.
    Concatenation(RopeArity),
    /// View into a resolved base string.
    Substring,
}

/// Contract for resolving a rope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RopeResolutionMode {
    /// Flatten to ordinary owned string storage.
    FlatString,
    /// Flatten and atomize, allocating an atom table entry if needed.
    Atomize,
    /// Look up an existing atom only; leave non-atoms unresolved if absent.
    ExistingAtomOnly,
}

/// Result shape of resolving or flattening a rope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RopeResolutionOutcome {
    AlreadyFlat(FlatString),
    Flattened(FlatString),
    Atomized(AtomId),
    Deferred,
}

/// Deferred string concatenation state.
///
/// Flattening is an explicit VM/GC operation. Any cached flat result must be
/// barriered because it points from one GC-owned object to another.
#[derive(Debug)]
pub struct RopeString {
    left: RopePiece,
    right: RopePiece,
    kind: RopeKind,
    length: StringLength,
    encoding: StringEncoding,
}

impl RopeString {
    pub const fn new_unflattened(left: RopePiece, right: RopePiece) -> Self {
        Self {
            left,
            right,
            kind: RopeKind::Concatenation(RopeArity::Two),
            length: StringLength::from_checked_code_units(0),
            encoding: StringEncoding::Latin1,
        }
    }

    /// Creates metadata for an unresolved rope after the VM has checked length.
    pub const fn new_with_metadata(
        left: RopePiece,
        right: RopePiece,
        kind: RopeKind,
        length: StringLength,
        encoding: StringEncoding,
    ) -> Self {
        Self {
            left,
            right,
            kind,
            length,
            encoding,
        }
    }

    pub const fn pieces(&self) -> (&RopePiece, &RopePiece) {
        (&self.left, &self.right)
    }

    /// Returns whether the rope is a concatenation or substring view.
    pub const fn kind(&self) -> RopeKind {
        self.kind
    }

    /// Returns the checked code unit length.
    pub const fn length(&self) -> StringLength {
        self.length
    }

    /// Returns the aggregate encoding.
    pub const fn encoding(&self) -> StringEncoding {
        self.encoding
    }
}

/// Substring view metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SubstringString {
    offset: u32,
    length: StringLength,
    encoding: StringEncoding,
    base: SubstringBase,
}

impl SubstringString {
    /// Records a substring over a resolved base string.
    pub const fn new(offset: u32, length: StringLength, encoding: StringEncoding) -> Self {
        Self {
            offset,
            length,
            encoding,
            base: SubstringBase::ResolvedFlat,
        }
    }

    /// Records a substring with explicit base ownership.
    pub const fn new_with_base(
        offset: u32,
        length: StringLength,
        encoding: StringEncoding,
        base: SubstringBase,
    ) -> Self {
        Self {
            offset,
            length,
            encoding,
            base,
        }
    }

    /// Returns the base offset in UTF-16 code units.
    pub const fn offset(self) -> u32 {
        self.offset
    }

    /// Returns the substring length.
    pub const fn length(self) -> StringLength {
        self.length
    }

    /// Returns the substring encoding.
    pub const fn encoding(self) -> StringEncoding {
        self.encoding
    }

    /// Returns the ownership class of the base string.
    pub const fn base(self) -> SubstringBase {
        self.base
    }
}

/// Base ownership for substring strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubstringBase {
    ResolvedFlat,
    AtomBacked,
    ExternalPinned,
    RopeMustResolveFirst,
}

/// Public operation boundary for string conversion.
///
/// These methods are intentionally trait requirements rather than default
/// implementations. VM/string-table code must decide when allocation, GC, and
/// rope resolution are permitted.
pub trait StringInterner {
    /// Converts a string cell to an atom, allocating if allowed by the caller.
    fn to_atom_string(&mut self, string: &JsString) -> Option<AtomId>;

    /// Looks up an existing atom without allocating a new atom table entry.
    fn to_existing_atom_string(&self, string: &JsString) -> Option<AtomId>;

    /// Resolves a rope according to the requested mode.
    fn resolve_rope(&mut self, rope: &RopeString, mode: RopeResolutionMode) -> Option<FlatString>;

    /// Performs an atom lookup according to an explicit allocation policy.
    fn atomize_with_mode(&mut self, string: &JsString, mode: AtomizationMode) -> Option<AtomId>;
}
