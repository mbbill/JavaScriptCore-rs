//! Common cell identity and header contracts.
//!
//! Every heap-managed JavaScript allocation starts with a `JsCellHeader`.
//! Subtype payloads are owned by `Heap`; Rust code reaches them through typed
//! GC references, handles, roots, or barriered fields.

use core::fmt;

use crate::gc::{Trace, Tracer};

/// Encoded reference to an object's structure.
///
/// This is metadata identity, not heap-cell identity. The structure table or
/// VM owns the mapping from this value to layout facts; `CellId` remains the
/// only raw identity for a GC cell.
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

/// Stable heap-cell identity owned by the GC layer.
///
/// Runtime-facing IDs such as `ObjectId`, `ExecutableId`, and `CodeBlockId`
/// are typed wrappers around this handle. `JsValue` may encode a cell-carrying
/// value, but it does not own or interpret this identity. The heap may later
/// implement this as a table index, compressed pointer, allocation epoch, or
/// tracing handle, but the authority to interpret the raw identity stays with
/// `gc`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct CellId(pub u32);

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
    BigInt = 8,
    Executable = 9,
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
    /// Scanned or otherwise treated as live by the current marking epoch.
    #[default]
    PossiblyBlack = 0,
    /// Newly allocated or eden-like. During a collection this is not yet proven live.
    DefinitelyWhite = 1,
    /// Barriered or queued for scanning. This may still be white in a full collection.
    PossiblyGrey = 2,
}

/// Whether a cell needs a subtype destructor/finalizer after liveness is lost.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DestructionMode {
    #[default]
    DoesNotNeedDestruction,
    NeedsDestruction,
    /// The subspace or heap-cell type must decide at sweep time.
    MayNeedDestruction,
}

/// Allocation family used by the heap and subspace layer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeapCellKind {
    #[default]
    JsCell,
    JsCellWithIndexingHeader,
    Auxiliary,
}

/// Reason a cell header was invalidated for diagnostics.
///
/// JSC's C++ `HeapCell::zap` only clears selected header words. Rust code that
/// adopts zapping must keep that mutation under heap/sweeper authority.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CellZapReason {
    #[default]
    Unspecified,
    Destruction,
    StopAllocating,
}

/// Destruction lifecycle visible before storage is recycled.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CellDestructionState {
    /// The cell is live or has not been classified for sweep.
    #[default]
    NotPending,
    /// The cell is dead, but the owning block or precise allocation has not swept it.
    PendingDestruction,
    /// The cell's destructor/finalizer authority has already run.
    Destroyed,
}

/// Collector-owned lifecycle facts for a cell.
///
/// These are not part of `JsCellHeader` layout. JSC derives them from block,
/// precise-allocation, and sweep state; Rust code should keep that authority
/// at the heap/container layer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CellLifecycleRecord {
    pub destruction_state: CellDestructionState,
    pub zap_reason: Option<CellZapReason>,
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

/// Static owner for generated or hand-authored cell metadata.
///
/// Runtime object code may read these entries, but only the named owner may
/// change the table source or regenerate its contents.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CellSchemaOwner {
    /// Rust-side canonical metadata authored in this module.
    #[default]
    GcCellSchema,
    /// Metadata generated from C++ JSC cell declarations.
    GeneratedFromCppCellDeclarations,
    /// Metadata generated from JavaScriptCore IDL or builtin tables.
    GeneratedFromRuntimeTables,
}

/// Provenance of a static metadata row.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CellSchemaProvenance {
    /// Hand-authored Rust bootstrap row.
    #[default]
    RustStaticSeed,
    /// Future generated row copied from the C++ JavaScriptCore source of truth.
    CppGenerated,
    /// Future row generated from object model or builtin declarations.
    RuntimeGenerated,
}

/// Registry mutation authority for static cell metadata.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CellMetadataRegistryAuthority {
    /// Tables are immutable after crate initialization.
    #[default]
    StaticReadOnly,
    /// A generated source refresh may replace the compiled table.
    GeneratedSourceRefresh,
}

/// Named descriptor for one `TypeInfo` payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TypeInfoDescriptor {
    pub name: &'static str,
    pub type_info: TypeInfo,
    pub owner: CellSchemaOwner,
    pub provenance: CellSchemaProvenance,
}

/// Static metadata row for one cell family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellMetadataDescriptor {
    pub name: &'static str,
    pub metadata: CellMetadata,
    pub attributes: CellAttributes,
    pub owner: CellSchemaOwner,
    pub provenance: CellSchemaProvenance,
}

/// Immutable cell metadata registry.
///
/// This registry owns no heap cells and grants no mutation authority. It is a
/// compiled schema table consumed by allocation and dispatch planning layers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellMetadataRegistry {
    pub name: &'static str,
    pub authority: CellMetadataRegistryAuthority,
    pub type_info: &'static [TypeInfoDescriptor],
    pub metadata: &'static [CellMetadataDescriptor],
}

impl CellMetadataRegistry {
    pub const fn type_info_descriptors(&self) -> &'static [TypeInfoDescriptor] {
        self.type_info
    }

    pub const fn metadata_descriptors(&self) -> &'static [CellMetadataDescriptor] {
        self.metadata
    }

    pub fn metadata_for_type(
        &self,
        cell_type: CellType,
    ) -> Option<&'static CellMetadataDescriptor> {
        self.metadata
            .iter()
            .find(|descriptor| descriptor.metadata.type_info.cell_type == cell_type)
    }

    pub fn validate(&self) -> Result<(), CellMetadataValidationError> {
        if self.name.is_empty() {
            return Err(CellMetadataValidationError::EmptyRegistryName);
        }

        for (index, descriptor) in self.type_info.iter().enumerate() {
            descriptor.validate()?;
            if self.type_info[..index]
                .iter()
                .any(|previous| previous.name == descriptor.name)
            {
                return Err(CellMetadataValidationError::DuplicateTypeInfoName(
                    descriptor.name,
                ));
            }
            if self.type_info[..index]
                .iter()
                .any(|previous| previous.type_info.cell_type == descriptor.type_info.cell_type)
            {
                return Err(CellMetadataValidationError::DuplicateCellType(
                    descriptor.type_info.cell_type,
                ));
            }
        }

        for (index, descriptor) in self.metadata.iter().enumerate() {
            descriptor.validate()?;
            if self.metadata[..index]
                .iter()
                .any(|previous| previous.name == descriptor.name)
            {
                return Err(CellMetadataValidationError::DuplicateMetadataName(
                    descriptor.name,
                ));
            }
            if self.metadata[..index].iter().any(|previous| {
                previous.metadata.type_info.cell_type == descriptor.metadata.type_info.cell_type
            }) {
                return Err(CellMetadataValidationError::DuplicateMetadataCellType(
                    descriptor.metadata.type_info.cell_type,
                ));
            }
            if !self.type_info.iter().any(|type_info| {
                type_info.type_info.cell_type == descriptor.metadata.type_info.cell_type
                    && type_info.type_info == descriptor.metadata.type_info
            }) {
                return Err(CellMetadataValidationError::MissingTypeInfoForMetadata(
                    descriptor.metadata.type_info.cell_type,
                ));
            }
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CellMetadataValidationError {
    EmptyRegistryName,
    EmptyDescriptorName,
    EmptyVTableName,
    DuplicateTypeInfoName(&'static str),
    DuplicateCellType(CellType),
    DuplicateMetadataName(&'static str),
    DuplicateMetadataCellType(CellType),
    MissingTypeInfoForMetadata(CellType),
    VTableTypeInfoMismatch(CellType),
    AttributeMismatch(CellType),
    ConstructorWithoutCallable(CellType),
}

impl TypeInfoDescriptor {
    pub const fn new(
        name: &'static str,
        type_info: TypeInfo,
        owner: CellSchemaOwner,
        provenance: CellSchemaProvenance,
    ) -> Self {
        Self {
            name,
            type_info,
            owner,
            provenance,
        }
    }

    pub fn validate(&self) -> Result<(), CellMetadataValidationError> {
        if self.name.is_empty() {
            return Err(CellMetadataValidationError::EmptyDescriptorName);
        }
        self.type_info.validate()
    }
}

impl CellMetadataDescriptor {
    pub const fn new(
        name: &'static str,
        metadata: CellMetadata,
        attributes: CellAttributes,
        owner: CellSchemaOwner,
        provenance: CellSchemaProvenance,
    ) -> Self {
        Self {
            name,
            metadata,
            attributes,
            owner,
            provenance,
        }
    }

    pub fn validate(&self) -> Result<(), CellMetadataValidationError> {
        if self.name.is_empty() {
            return Err(CellMetadataValidationError::EmptyDescriptorName);
        }
        self.metadata.validate()?;
        if self.metadata.vtable.name.is_empty() {
            return Err(CellMetadataValidationError::EmptyVTableName);
        }
        if self.metadata.vtable.type_info != self.metadata.type_info {
            return Err(CellMetadataValidationError::VTableTypeInfoMismatch(
                self.metadata.type_info.cell_type,
            ));
        }
        if self.attributes.destruction != self.metadata.destruction
            || self.attributes.heap_cell_kind != self.metadata.heap_cell_kind
        {
            return Err(CellMetadataValidationError::AttributeMismatch(
                self.metadata.type_info.cell_type,
            ));
        }
        Ok(())
    }
}

impl CellMetadata {
    pub const fn new(
        type_info: TypeInfo,
        heap_cell_kind: HeapCellKind,
        destruction: DestructionMode,
        vtable_name: &'static str,
    ) -> Self {
        Self {
            type_info,
            heap_cell_kind,
            destruction,
            vtable: CellVTable {
                name: vtable_name,
                type_info,
            },
        }
    }

    pub fn validate(&self) -> Result<(), CellMetadataValidationError> {
        self.type_info.validate()?;
        if self.vtable.name.is_empty() {
            return Err(CellMetadataValidationError::EmptyVTableName);
        }
        if self.vtable.type_info != self.type_info {
            return Err(CellMetadataValidationError::VTableTypeInfoMismatch(
                self.type_info.cell_type,
            ));
        }
        Ok(())
    }
}

impl TypeInfo {
    pub const fn new(cell_type: CellType) -> Self {
        Self {
            cell_type,
            is_callable: false,
            is_constructor: false,
            has_indexed_storage: false,
            overrides_get_own_property_slot: false,
            intercepts_indexed_access: false,
        }
    }

    pub const fn callable(mut self) -> Self {
        self.is_callable = true;
        self
    }

    pub const fn constructor(mut self) -> Self {
        self.is_constructor = true;
        self.is_callable = true;
        self
    }

    pub const fn indexed_storage(mut self) -> Self {
        self.has_indexed_storage = true;
        self
    }

    pub const fn overrides_get_own_property_slot(mut self) -> Self {
        self.overrides_get_own_property_slot = true;
        self
    }

    pub const fn intercepts_indexed_access(mut self) -> Self {
        self.intercepts_indexed_access = true;
        self
    }

    pub fn validate(&self) -> Result<(), CellMetadataValidationError> {
        if self.is_constructor && !self.is_callable {
            return Err(CellMetadataValidationError::ConstructorWithoutCallable(
                self.cell_type,
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellMetadataDescriptorBuilder {
    name: &'static str,
    type_info: TypeInfo,
    heap_cell_kind: HeapCellKind,
    destruction: DestructionMode,
    owner: CellSchemaOwner,
    provenance: CellSchemaProvenance,
}

impl CellMetadataDescriptorBuilder {
    pub const fn new(name: &'static str, type_info: TypeInfo) -> Self {
        Self {
            name,
            type_info,
            heap_cell_kind: HeapCellKind::JsCell,
            destruction: DestructionMode::MayNeedDestruction,
            owner: CellSchemaOwner::GcCellSchema,
            provenance: CellSchemaProvenance::RustStaticSeed,
        }
    }

    pub const fn heap_cell_kind(mut self, heap_cell_kind: HeapCellKind) -> Self {
        self.heap_cell_kind = heap_cell_kind;
        self
    }

    pub const fn destruction(mut self, destruction: DestructionMode) -> Self {
        self.destruction = destruction;
        self
    }

    pub const fn owner(mut self, owner: CellSchemaOwner) -> Self {
        self.owner = owner;
        self
    }

    pub const fn provenance(mut self, provenance: CellSchemaProvenance) -> Self {
        self.provenance = provenance;
        self
    }

    pub fn build(self) -> Result<CellMetadataDescriptor, CellMetadataValidationError> {
        let metadata = CellMetadata::new(
            self.type_info,
            self.heap_cell_kind,
            self.destruction,
            self.name,
        );
        let descriptor = CellMetadataDescriptor::new(
            self.name,
            metadata,
            CellAttributes {
                destruction: self.destruction,
                heap_cell_kind: self.heap_cell_kind,
            },
            self.owner,
            self.provenance,
        );
        descriptor.validate()?;
        Ok(descriptor)
    }
}

/// C++ `CellAttributes` equivalent kept separate from dynamic type metadata.
///
/// The subspace and block directory own these attributes. Individual objects
/// must not mutate them after allocation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CellAttributes {
    pub destruction: DestructionMode,
    pub heap_cell_kind: HeapCellKind,
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
/// Header mutation belongs to heap allocation, collector marking, or sweeping
/// authority. Borrowers may inspect it through `GcRef`-backed views only while
/// liveness and pinning are proven elsewhere.
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
            state: CellState::PossiblyBlack,
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
///
/// This key is derived from header metadata. It is not stable object identity
/// and must not be used in place of `CellId`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CellMetadataKey {
    pub structure_id: StructureId,
    pub cell_type: CellType,
}

/// Erased GC cell view.
///
/// This type is a header-level view, not a boxed owner. Heap allocation and
/// subtype casting are GC responsibilities. A `GcRef<JsCell>` borrows this
/// erased view for a lifetime proven by roots, handles, barriers, or active
/// collector traversal.
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

const TYPE_INFO_CELL: TypeInfo = TypeInfo {
    cell_type: CellType::Cell,
    is_callable: false,
    is_constructor: false,
    has_indexed_storage: false,
    overrides_get_own_property_slot: false,
    intercepts_indexed_access: false,
};

const TYPE_INFO_OBJECT: TypeInfo = TypeInfo {
    cell_type: CellType::Object,
    is_callable: false,
    is_constructor: false,
    has_indexed_storage: false,
    overrides_get_own_property_slot: true,
    intercepts_indexed_access: false,
};

const TYPE_INFO_STRUCTURE: TypeInfo = TypeInfo {
    cell_type: CellType::Structure,
    is_callable: false,
    is_constructor: false,
    has_indexed_storage: false,
    overrides_get_own_property_slot: false,
    intercepts_indexed_access: false,
};

const TYPE_INFO_STRING: TypeInfo = TypeInfo {
    cell_type: CellType::String,
    is_callable: false,
    is_constructor: false,
    has_indexed_storage: false,
    overrides_get_own_property_slot: false,
    intercepts_indexed_access: true,
};

const TYPE_INFO_SYMBOL: TypeInfo = TypeInfo {
    cell_type: CellType::Symbol,
    is_callable: false,
    is_constructor: false,
    has_indexed_storage: false,
    overrides_get_own_property_slot: false,
    intercepts_indexed_access: false,
};

const TYPE_INFO_BIGINT: TypeInfo = TypeInfo {
    cell_type: CellType::BigInt,
    is_callable: false,
    is_constructor: false,
    has_indexed_storage: false,
    overrides_get_own_property_slot: false,
    intercepts_indexed_access: false,
};

const TYPE_INFO_EXECUTABLE: TypeInfo = TypeInfo {
    cell_type: CellType::Executable,
    is_callable: false,
    is_constructor: false,
    has_indexed_storage: false,
    overrides_get_own_property_slot: false,
    intercepts_indexed_access: false,
};

const TYPE_INFO_CODE_BLOCK: TypeInfo = TypeInfo {
    cell_type: CellType::CodeBlock,
    is_callable: false,
    is_constructor: false,
    has_indexed_storage: false,
    overrides_get_own_property_slot: false,
    intercepts_indexed_access: false,
};

const TYPE_INFO_GLOBAL_OBJECT: TypeInfo = TypeInfo {
    cell_type: CellType::GlobalObject,
    is_callable: false,
    is_constructor: false,
    has_indexed_storage: false,
    overrides_get_own_property_slot: true,
    intercepts_indexed_access: false,
};

const TYPE_INFO_BUTTERFLY: TypeInfo = TypeInfo {
    cell_type: CellType::Butterfly,
    is_callable: false,
    is_constructor: false,
    has_indexed_storage: true,
    overrides_get_own_property_slot: false,
    intercepts_indexed_access: false,
};

/// Canonical static type-info descriptors owned by `gc::cell`.
pub const STATIC_TYPE_INFO_DESCRIPTORS: &[TypeInfoDescriptor] = &[
    TypeInfoDescriptor {
        name: "Cell",
        type_info: TYPE_INFO_CELL,
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    TypeInfoDescriptor {
        name: "Object",
        type_info: TYPE_INFO_OBJECT,
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    TypeInfoDescriptor {
        name: "Structure",
        type_info: TYPE_INFO_STRUCTURE,
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    TypeInfoDescriptor {
        name: "String",
        type_info: TYPE_INFO_STRING,
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    TypeInfoDescriptor {
        name: "Symbol",
        type_info: TYPE_INFO_SYMBOL,
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    TypeInfoDescriptor {
        name: "BigInt",
        type_info: TYPE_INFO_BIGINT,
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    TypeInfoDescriptor {
        name: "Executable",
        type_info: TYPE_INFO_EXECUTABLE,
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    TypeInfoDescriptor {
        name: "CodeBlock",
        type_info: TYPE_INFO_CODE_BLOCK,
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    TypeInfoDescriptor {
        name: "GlobalObject",
        type_info: TYPE_INFO_GLOBAL_OBJECT,
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    TypeInfoDescriptor {
        name: "Butterfly",
        type_info: TYPE_INFO_BUTTERFLY,
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
];

/// Canonical static cell metadata descriptors owned by `gc::cell`.
pub const STATIC_CELL_METADATA_DESCRIPTORS: &[CellMetadataDescriptor] = &[
    CellMetadataDescriptor {
        name: "Cell",
        metadata: CellMetadata {
            type_info: TYPE_INFO_CELL,
            heap_cell_kind: HeapCellKind::JsCell,
            destruction: DestructionMode::MayNeedDestruction,
            vtable: CellVTable {
                name: "Cell",
                type_info: TYPE_INFO_CELL,
            },
        },
        attributes: CellAttributes {
            destruction: DestructionMode::MayNeedDestruction,
            heap_cell_kind: HeapCellKind::JsCell,
        },
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    CellMetadataDescriptor {
        name: "Object",
        metadata: CellMetadata {
            type_info: TYPE_INFO_OBJECT,
            heap_cell_kind: HeapCellKind::JsCellWithIndexingHeader,
            destruction: DestructionMode::MayNeedDestruction,
            vtable: CellVTable {
                name: "Object",
                type_info: TYPE_INFO_OBJECT,
            },
        },
        attributes: CellAttributes {
            destruction: DestructionMode::MayNeedDestruction,
            heap_cell_kind: HeapCellKind::JsCellWithIndexingHeader,
        },
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    CellMetadataDescriptor {
        name: "Structure",
        metadata: CellMetadata {
            type_info: TYPE_INFO_STRUCTURE,
            heap_cell_kind: HeapCellKind::JsCell,
            destruction: DestructionMode::NeedsDestruction,
            vtable: CellVTable {
                name: "Structure",
                type_info: TYPE_INFO_STRUCTURE,
            },
        },
        attributes: CellAttributes {
            destruction: DestructionMode::NeedsDestruction,
            heap_cell_kind: HeapCellKind::JsCell,
        },
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    CellMetadataDescriptor {
        name: "String",
        metadata: CellMetadata {
            type_info: TYPE_INFO_STRING,
            heap_cell_kind: HeapCellKind::Auxiliary,
            destruction: DestructionMode::MayNeedDestruction,
            vtable: CellVTable {
                name: "String",
                type_info: TYPE_INFO_STRING,
            },
        },
        attributes: CellAttributes {
            destruction: DestructionMode::MayNeedDestruction,
            heap_cell_kind: HeapCellKind::Auxiliary,
        },
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    CellMetadataDescriptor {
        name: "Symbol",
        metadata: CellMetadata {
            type_info: TYPE_INFO_SYMBOL,
            heap_cell_kind: HeapCellKind::Auxiliary,
            destruction: DestructionMode::MayNeedDestruction,
            vtable: CellVTable {
                name: "Symbol",
                type_info: TYPE_INFO_SYMBOL,
            },
        },
        attributes: CellAttributes {
            destruction: DestructionMode::MayNeedDestruction,
            heap_cell_kind: HeapCellKind::Auxiliary,
        },
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    CellMetadataDescriptor {
        name: "BigInt",
        metadata: CellMetadata {
            type_info: TYPE_INFO_BIGINT,
            heap_cell_kind: HeapCellKind::Auxiliary,
            destruction: DestructionMode::MayNeedDestruction,
            vtable: CellVTable {
                name: "BigInt",
                type_info: TYPE_INFO_BIGINT,
            },
        },
        attributes: CellAttributes {
            destruction: DestructionMode::MayNeedDestruction,
            heap_cell_kind: HeapCellKind::Auxiliary,
        },
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    CellMetadataDescriptor {
        name: "Executable",
        metadata: CellMetadata {
            type_info: TYPE_INFO_EXECUTABLE,
            heap_cell_kind: HeapCellKind::Auxiliary,
            destruction: DestructionMode::NeedsDestruction,
            vtable: CellVTable {
                name: "Executable",
                type_info: TYPE_INFO_EXECUTABLE,
            },
        },
        attributes: CellAttributes {
            destruction: DestructionMode::NeedsDestruction,
            heap_cell_kind: HeapCellKind::Auxiliary,
        },
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    CellMetadataDescriptor {
        name: "CodeBlock",
        metadata: CellMetadata {
            type_info: TYPE_INFO_CODE_BLOCK,
            heap_cell_kind: HeapCellKind::Auxiliary,
            destruction: DestructionMode::NeedsDestruction,
            vtable: CellVTable {
                name: "CodeBlock",
                type_info: TYPE_INFO_CODE_BLOCK,
            },
        },
        attributes: CellAttributes {
            destruction: DestructionMode::NeedsDestruction,
            heap_cell_kind: HeapCellKind::Auxiliary,
        },
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
    CellMetadataDescriptor {
        name: "GlobalObject",
        metadata: CellMetadata {
            type_info: TYPE_INFO_GLOBAL_OBJECT,
            heap_cell_kind: HeapCellKind::JsCellWithIndexingHeader,
            destruction: DestructionMode::MayNeedDestruction,
            vtable: CellVTable {
                name: "GlobalObject",
                type_info: TYPE_INFO_GLOBAL_OBJECT,
            },
        },
        attributes: CellAttributes {
            destruction: DestructionMode::MayNeedDestruction,
            heap_cell_kind: HeapCellKind::JsCellWithIndexingHeader,
        },
        owner: CellSchemaOwner::GcCellSchema,
        provenance: CellSchemaProvenance::RustStaticSeed,
    },
];

pub const STATIC_CELL_METADATA_REGISTRY: CellMetadataRegistry = CellMetadataRegistry {
    name: "gc.cell.static-metadata",
    authority: CellMetadataRegistryAuthority::StaticReadOnly,
    type_info: STATIC_TYPE_INFO_DESCRIPTORS,
    metadata: STATIC_CELL_METADATA_DESCRIPTORS,
};

pub const fn static_type_info_descriptors() -> &'static [TypeInfoDescriptor] {
    STATIC_TYPE_INFO_DESCRIPTORS
}

pub const fn static_cell_metadata_descriptors() -> &'static [CellMetadataDescriptor] {
    STATIC_CELL_METADATA_DESCRIPTORS
}

pub const fn static_cell_metadata_registry() -> &'static CellMetadataRegistry {
    &STATIC_CELL_METADATA_REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_metadata_registry_is_read_only() {
        assert_eq!(
            static_cell_metadata_registry().authority,
            CellMetadataRegistryAuthority::StaticReadOnly
        );
        assert_eq!(static_cell_metadata_registry().validate(), Ok(()));
    }

    #[test]
    fn object_metadata_keeps_object_type_info() {
        let descriptor = static_cell_metadata_registry()
            .metadata_for_type(CellType::Object)
            .expect("static object metadata row");
        assert_eq!(descriptor.metadata.type_info.cell_type, CellType::Object);
        assert_eq!(
            descriptor.metadata.vtable.type_info,
            descriptor.metadata.type_info
        );
    }

    #[test]
    fn primitive_cell_metadata_includes_symbol_and_bigint() {
        for (cell_type, expected_name) in
            [(CellType::Symbol, "Symbol"), (CellType::BigInt, "BigInt")]
        {
            let descriptor = static_cell_metadata_registry()
                .metadata_for_type(cell_type)
                .expect("static primitive cell metadata row");

            assert_eq!(descriptor.name, expected_name);
            assert_eq!(descriptor.metadata.type_info.cell_type, cell_type);
            assert_eq!(descriptor.metadata.heap_cell_kind, HeapCellKind::Auxiliary);
            assert_eq!(
                descriptor.metadata.destruction,
                DestructionMode::MayNeedDestruction
            );
        }
    }

    #[test]
    fn executable_cell_metadata_is_auxiliary_and_destructible() {
        let descriptor = static_cell_metadata_registry()
            .metadata_for_type(CellType::Executable)
            .expect("static executable cell metadata row");

        assert_eq!(descriptor.name, "Executable");
        assert_eq!(
            descriptor.metadata.type_info.cell_type,
            CellType::Executable
        );
        assert_eq!(descriptor.metadata.heap_cell_kind, HeapCellKind::Auxiliary);
        assert_eq!(
            descriptor.metadata.destruction,
            DestructionMode::NeedsDestruction
        );
    }

    #[test]
    fn existing_cell_type_discriminants_stay_stable() {
        assert_eq!(CellType::CodeBlock as u16, 5);
        assert_eq!(CellType::BigInt as u16, 8);
        assert_eq!(CellType::Executable as u16, 9);
    }

    #[test]
    fn metadata_builder_keeps_attributes_in_sync() {
        let descriptor = CellMetadataDescriptorBuilder::new(
            "CallableHost",
            TypeInfo::new(CellType::Host).constructor(),
        )
        .heap_cell_kind(HeapCellKind::JsCell)
        .destruction(DestructionMode::NeedsDestruction)
        .build();

        assert_eq!(
            descriptor.map(|descriptor| descriptor.attributes.destruction),
            Ok(DestructionMode::NeedsDestruction)
        );
    }

    #[test]
    fn metadata_validator_rejects_vtable_mismatch() {
        let descriptor = CellMetadataDescriptor::new(
            "bad",
            CellMetadata {
                type_info: TYPE_INFO_CELL,
                heap_cell_kind: HeapCellKind::JsCell,
                destruction: DestructionMode::DoesNotNeedDestruction,
                vtable: CellVTable {
                    name: "bad",
                    type_info: TYPE_INFO_OBJECT,
                },
            },
            CellAttributes {
                destruction: DestructionMode::DoesNotNeedDestruction,
                heap_cell_kind: HeapCellKind::JsCell,
            },
            CellSchemaOwner::GcCellSchema,
            CellSchemaProvenance::RustStaticSeed,
        );

        assert_eq!(
            descriptor.validate(),
            Err(CellMetadataValidationError::VTableTypeInfoMismatch(
                CellType::Cell
            ))
        );
    }
}
