//! Common cell identity and header contracts.
//!
//! Every heap-managed JavaScript allocation starts with a `JsCellHeader`.
//! Subtype payloads are owned by `Heap`; Rust code reaches them through typed
//! GC references, handles, roots, or barriered fields.

use core::fmt;

use crate::gc::{Trace, Tracer};

/// Encoded reference to an object's structure.
///
/// The compression strategy is deliberately unresolved. Layout-compatible
/// encoding for JIT or mixed C++/Rust operation must be introduced at this
/// boundary rather than hidden in object code.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct StructureId(pub u32);

impl StructureId {
    pub const INVALID: Self = Self(0);

    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Coarse runtime kind carried by a cell header.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(u16)]
pub enum CellType {
    #[default]
    Cell = 0,
    Object = 1,
    Structure = 2,
    String = 3,
    Symbol = 4,
    CodeBlock = 5,
    GlobalObject = 6,
    Butterfly = 7,
    Host = 0xffff,
}

/// Collector-visible cell state.
///
/// The exact meaning of each state depends on the eventual incremental or
/// generational collector. State transitions belong to the GC, not arbitrary
/// subtype code.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum CellState {
    /// Newly allocated or eden-like. During a collection this is not yet proven live.
    #[default]
    DefinitelyWhite = 0,
    /// Barriered or queued for scanning. This may still be white in a full collection.
    PossiblyGrey = 1,
    /// Scanned or otherwise treated as live by the current marking epoch.
    PossiblyBlack = 2,
    /// The collector has committed to finalization/destruction for this cell.
    Finalizing = 3,
}

/// Whether a cell needs a subtype destructor/finalizer after liveness is lost.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DestructionMode {
    #[default]
    DoesNotNeedDestruction,
    NeedsDestruction,
}

/// Allocation family used by the heap and subspace layer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeapCellKind {
    #[default]
    JsCell,
    JsCellWithIndexingHeader,
    Auxiliary,
    HostOwned,
}

/// Header flags whose exact bit layout is intentionally deferred.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct CellHeaderFlags(pub u32);

impl CellHeaderFlags {
    pub const MAY_HAVE_WEAK_EDGES: Self = Self(1 << 0);
    pub const HAS_FINALIZER: Self = Self(1 << 1);
    pub const HAS_INDEXING_HEADER: Self = Self(1 << 2);
    pub const PINNED_BY_HOST: Self = Self(1 << 3);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0
    }
}

/// Collector-visible metadata that belongs to a cell type, not to one object.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CellMetadata {
    pub type_info: TypeInfo,
    pub heap_cell_kind: HeapCellKind,
    pub destruction: DestructionMode,
    pub vtable: CellVTable,
}

/// Object/callability/indexing metadata common to JSC runtime dispatch.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TypeInfo {
    pub cell_type: CellType,
    pub is_callable: bool,
    pub is_constructor: bool,
    pub has_indexed_storage: bool,
    pub overrides_get_own_property_slot: bool,
    pub intercepts_indexed_access: bool,
}

/// Common header for every GC cell.
///
/// `repr(C)` marks the intended compatibility boundary. Exact offsets are not
/// promised by this skeleton; any offset-stable mode must be documented here.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct JsCellHeader {
    pub structure_id: StructureId,
    pub cell_type: CellType,
    pub state: CellState,
    pub flags: CellHeaderFlags,
}

impl JsCellHeader {
    pub const fn new(structure_id: StructureId, cell_type: CellType) -> Self {
        Self {
            structure_id,
            cell_type,
            state: CellState::DefinitelyWhite,
            flags: CellHeaderFlags::empty(),
        }
    }

    pub const fn metadata_key(&self) -> CellMetadataKey {
        CellMetadataKey {
            structure_id: self.structure_id,
            cell_type: self.cell_type,
        }
    }
}

/// Stable key for looking up cell metadata without exposing payload layout.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CellMetadataKey {
    pub structure_id: StructureId,
    pub cell_type: CellType,
}

/// Erased GC cell identity.
///
/// This type is a header-level view, not a boxed owner. Heap allocation and
/// subtype casting are GC responsibilities.
#[repr(C)]
pub struct JsCell {
    header: JsCellHeader,
}

impl JsCell {
    pub fn header(&self) -> &JsCellHeader {
        &self.header
    }
}

impl fmt::Debug for JsCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JsCell")
            .field("header", &self.header)
            .finish_non_exhaustive()
    }
}

impl Trace for JsCell {
    fn trace(&self, _tracer: &mut dyn Tracer) {
        // Erased cells do not know their payload. Future method-table dispatch
        // or generated tracing code must enter through `TraceCell`.
    }
}

/// Tracing contract for concrete cell payloads.
pub trait TraceCell: Trace {
    fn cell_header(&self) -> &JsCellHeader;
}

/// Placeholder for a per-cell lock if the Rust design keeps JSC's lock order.
#[derive(Debug, Default)]
pub struct CellLock {
    _private: (),
}

/// Method-table identity for dynamic cell behavior.
///
/// Function pointers and layout-sensitive dispatch are intentionally absent.
/// The eventual implementation must decide between Rust traits, explicit
/// tables, or generated tables at this boundary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CellVTable {
    pub name: &'static str,
    pub type_info: TypeInfo,
}
