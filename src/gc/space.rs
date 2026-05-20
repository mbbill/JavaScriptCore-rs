//! Subspace, block, size-class, and allocator descriptors.
//!
//! These types describe the shape of JSC's paged and precise allocation
//! domains without allocating memory or sweeping cells.

use core::marker::PhantomData;

use crate::gc::{CellAttributes, CellMetadata, DestructionMode, HeapCellKind, HeapEpoch, HeapId};

pub const MARKED_BLOCK_ATOM_SIZE: usize = 16;
pub const MARKED_BLOCK_SIZE: usize = 16 * 1024;
pub const WEAK_BLOCK_SIZE: usize = 1024;
pub const MARKED_SPACE_PRECISE_CUTOFF: usize = 80;

/// Family of subspace behavior selected for a cell type.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SubspaceKind {
    #[default]
    Complete,
    Iso,
    Precise,
}

/// Authority allowed to mutate subspace/block-directory topology.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SubspaceMutationAuthority {
    /// VM setup or heap initialization can attach subspaces and allocators.
    #[default]
    HeapInitialization,
    /// Collector-owned resizing of mark/newly-allocated bit storage.
    CollectorBitResize,
    /// Sweeper-owned block removal and free-list publication.
    Sweeper,
    /// Mutator allocation slow path may create directories or local allocators.
    MutatorAllocationSlowPath,
}

/// Static owner for heap topology schema rows.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AllocationSchemaOwner {
    /// `gc::space` owns the Rust bootstrap topology schema.
    #[default]
    GcSpaceSchema,
    /// A future generated table owns rows derived from C++ JSC heap metadata.
    GeneratedHeapTopology,
}

/// Provenance of static allocation schema data.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AllocationSchemaProvenance {
    /// Hand-authored Rust seed data.
    #[default]
    RustStaticSeed,
    /// Future generated data copied from C++ JavaScriptCore declarations.
    CppGenerated,
}

/// Registry mutation authority for immutable allocation schema.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AllocationRegistryAuthority {
    /// The registry is compiled static data.
    #[default]
    StaticReadOnly,
    /// A generated source refresh may replace the compiled registry.
    GeneratedSourceRefresh,
}

/// Allocation strategy for a request before the allocator exists.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AllocationMode {
    #[default]
    Normal,
    NoCollection,
    MustAlreadyHaveAllocator,
    EnsureAllocator,
    AllocatorIfExists,
}

/// Failure policy for a future allocation slow path.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AllocationFailureMode {
    #[default]
    ReturnNull,
    Crash,
    ThrowOutOfMemory,
}

/// Size-class index used by marked blocks.
///
/// This indexes allocator configuration only. It is not object identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct SizeClassIndex(pub usize);

/// Marked-space size class. JSC rounds cell sizes to the block atom size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SizeClass {
    pub index: SizeClassIndex,
    pub cell_size: usize,
}

impl SizeClass {
    pub const fn for_size(size: usize) -> Self {
        let index = size.div_ceil(MARKED_BLOCK_ATOM_SIZE);
        Self {
            index: SizeClassIndex(index),
            cell_size: index * MARKED_BLOCK_ATOM_SIZE,
        }
    }
}

/// Opaque directory identity for blocks of the same size class.
///
/// Directory identity belongs to the subspace topology registry and must not be
/// treated as heap-cell identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct BlockDirectoryId(pub u64);

/// Opaque marked block identity.
///
/// Marked blocks own ranges of cell storage under heap authority. The block ID
/// names the container, not any individual cell inside it.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct MarkedBlockId(pub u64);

/// Opaque precise allocation identity.
///
/// Precise allocation IDs name allocation containers. They do not replace
/// `CellId` for runtime-facing cell identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct PreciseAllocationId(pub u64);

/// Opaque allocator identity.
///
/// Allocator IDs name allocation machinery and carry no authority to interpret
/// stored cell payloads.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct AllocatorId(pub u64);

/// Opaque aligned-memory allocator identity.
///
/// This identifies virtual-memory ownership for allocation domains, not cell
/// identity or liveness.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct AlignedMemoryAllocatorId(pub u64);

/// Block-directory bitvector lanes guarded by the directory bitvector lock.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BlockDirectoryBit {
    #[default]
    Empty,
    CanAllocate,
    Unswept,
    IsLive,
    IsNewlyAllocated,
}

/// State tracked by block directories and sweeping.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BlockState {
    #[default]
    Empty,
    Allocating,
    Unswept,
    Sweeping,
    FreeListed,
    Retired,
}

/// Sweeping action selected by the owning block handle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SweepMode {
    #[default]
    SweepOnly,
    SweepToFreeList,
}

/// Marked-space iteration guard state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SpaceIterationState {
    #[default]
    NotIterating,
    IteratingLiveCells,
    IteratingDeadCells,
    IteratingBlocks,
}

/// Descriptor for a block directory.
///
/// Directory mutation is allowed only through the recorded
/// `mutation_authority`; individual object code may borrow this descriptor but
/// must not mutate topology.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockDirectoryDescriptor {
    pub id: BlockDirectoryId,
    pub subspace: &'static str,
    pub size_class: SizeClass,
    pub allocator: Option<AllocatorId>,
    pub attributes: CellAttributes,
    pub mutation_authority: SubspaceMutationAuthority,
    pub guarded_bits: &'static [BlockDirectoryBit],
}

/// Descriptor for a page-aligned marked block.
///
/// The block owns storage slots as part of the heap. Marking, sweeping, and
/// weak-set attachment are collector/sweeper mutations, not borrower actions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkedBlockDescriptor {
    pub id: MarkedBlockId,
    pub directory: BlockDirectoryId,
    pub subspace: &'static str,
    pub cell_size: usize,
    pub cells_per_block: usize,
    pub state: BlockState,
    pub mark_epoch: HeapEpoch,
    pub newly_allocated_epoch: HeapEpoch,
    pub destruction: DestructionMode,
    pub sweep_mode: SweepMode,
    pub weak_set_attached: bool,
}

impl MarkedBlockDescriptor {
    pub fn for_size(
        id: MarkedBlockId,
        directory: BlockDirectoryId,
        subspace: &'static str,
        cell_size: usize,
        epoch: HeapEpoch,
        destruction: DestructionMode,
    ) -> Self {
        let cells_per_block = MARKED_BLOCK_SIZE.checked_div(cell_size).unwrap_or(0);
        Self {
            id,
            directory,
            subspace,
            cell_size,
            cells_per_block,
            state: BlockState::Empty,
            mark_epoch: epoch,
            newly_allocated_epoch: epoch,
            destruction,
            sweep_mode: SweepMode::SweepOnly,
            weak_set_attached: false,
        }
    }
}

/// Precise allocation tier inside a subspace.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PreciseAllocationTier {
    #[default]
    Large,
    /// IsoSubspace lower tier, capped by JSC's small fixed count.
    LowerTierIso,
    PreciseOnlySubspace,
}

/// Descriptor for a malloc-backed precise allocation.
///
/// A precise allocation owns one allocation container. Its `id` is allocation
/// metadata; any contained runtime object is still named by `CellId`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreciseAllocationDescriptor {
    pub id: PreciseAllocationId,
    pub subspace: &'static str,
    pub cell_size: usize,
    pub is_newly_allocated: bool,
    pub is_marked: bool,
    pub lower_tier_precise_index: Option<u8>,
    pub tier: PreciseAllocationTier,
    pub destruction: DestructionMode,
}

/// Free-list shape used by a local allocator. It does not expose cell storage.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FreeListDescriptor {
    pub cell_size: usize,
    pub original_size: usize,
    pub interval_count: usize,
}

/// Local allocator state for one block directory.
///
/// The allocator may advance cursors under allocation-slow-path authority. It
/// borrows directory/block identities and does not own cell payload lifetimes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalAllocatorDescriptor {
    pub id: AllocatorId,
    pub directory: BlockDirectoryId,
    pub size_class: SizeClass,
    pub current_block: Option<MarkedBlockId>,
    pub last_active_block: Option<MarkedBlockId>,
    pub allocation_cursor: usize,
    pub free_list: FreeListDescriptor,
}

/// Aligned allocator linkage shared by subspaces and block directories.
///
/// Virtual-memory ownership is tracked here. Cell construction and destruction
/// remain heap/container responsibilities.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AlignedMemoryAllocatorDescriptor {
    pub id: AlignedMemoryAllocatorId,
    pub name: &'static str,
    pub subspaces: Vec<&'static str>,
    pub directories: Vec<BlockDirectoryId>,
    pub owns_virtual_memory: bool,
}

/// Handle-like allocator descriptor exposed to allocation clients.
///
/// Allocation clients may use this as availability metadata. It is not a
/// lifetime token and does not grant mutation authority over existing cells.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Allocator {
    pub id: Option<AllocatorId>,
    pub cell_size: usize,
    pub mode: AllocationMode,
}

impl Allocator {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn for_size(id: AllocatorId, cell_size: usize, mode: AllocationMode) -> Self {
        Self {
            id: Some(id),
            cell_size,
            mode,
        }
    }

    pub fn is_available(&self) -> bool {
        self.id.is_some()
    }
}

/// Immutable allocator schema used by subspace registry rows.
///
/// This describes allocator availability and size-class ownership only. It
/// does not own allocator cursors, blocks, free lists, or virtual memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticAllocatorDescriptor {
    pub name: &'static str,
    pub mode: AllocationMode,
    pub failure_mode: AllocationFailureMode,
    pub aligned_memory: bool,
    pub local_allocator: bool,
    pub size_classes: &'static [SizeClass],
    pub owner: AllocationSchemaOwner,
    pub provenance: AllocationSchemaProvenance,
}

/// Immutable subspace schema independent from runtime topology links.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticSubspaceDescriptor {
    pub name: &'static str,
    pub kind: SubspaceKind,
    pub attributes: CellAttributes,
    pub allocator: &'static StaticAllocatorDescriptor,
    pub precise_cutoff: usize,
    pub lower_tier_precise_cells: u8,
    pub mutation_authority: SubspaceMutationAuthority,
    pub owner: AllocationSchemaOwner,
    pub provenance: AllocationSchemaProvenance,
}

/// Immutable marked-space schema for one heap family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticMarkedSpaceDescriptor {
    pub name: &'static str,
    pub block_size: usize,
    pub atom_size: usize,
    pub precise_cutoff: usize,
    pub weak_block_size: usize,
    pub size_classes: &'static [SizeClass],
    pub owner: AllocationSchemaOwner,
    pub provenance: AllocationSchemaProvenance,
}

/// Immutable registry for subspaces, allocators, and marked-space shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AllocationSchemaRegistry {
    pub name: &'static str,
    pub authority: AllocationRegistryAuthority,
    pub marked_space: &'static StaticMarkedSpaceDescriptor,
    pub allocators: &'static [StaticAllocatorDescriptor],
    pub subspaces: &'static [StaticSubspaceDescriptor],
}

impl AllocationSchemaRegistry {
    pub const fn marked_space(&self) -> &'static StaticMarkedSpaceDescriptor {
        self.marked_space
    }

    pub const fn allocators(&self) -> &'static [StaticAllocatorDescriptor] {
        self.allocators
    }

    pub const fn subspaces(&self) -> &'static [StaticSubspaceDescriptor] {
        self.subspaces
    }

    pub fn subspace(&self, name: &str) -> Option<&'static StaticSubspaceDescriptor> {
        self.subspaces
            .iter()
            .find(|descriptor| descriptor.name == name)
    }

    pub fn select_allocation(
        &self,
        subspace_name: &str,
        requested_size: usize,
    ) -> Result<AllocationSizeClassSelection, AllocationSelectionError> {
        self.validate()
            .map_err(AllocationSelectionError::InvalidSchema)?;
        if requested_size == 0 {
            return Err(AllocationSelectionError::InvalidRequestedSize);
        }

        let subspace = self
            .subspace(subspace_name)
            .ok_or(AllocationSelectionError::UnknownSubspace)?;
        subspace.select_allocation(self, requested_size)
    }

    pub fn validate(&self) -> Result<(), AllocationSchemaValidationError> {
        if self.name.is_empty() {
            return Err(AllocationSchemaValidationError::EmptyRegistryName);
        }
        self.marked_space.validate()?;

        for (index, allocator) in self.allocators.iter().enumerate() {
            allocator.validate()?;
            if self.allocators[..index]
                .iter()
                .any(|previous| previous.name == allocator.name)
            {
                return Err(AllocationSchemaValidationError::DuplicateAllocatorName(
                    allocator.name,
                ));
            }
        }

        for (index, subspace) in self.subspaces.iter().enumerate() {
            subspace.validate()?;
            if self.subspaces[..index]
                .iter()
                .any(|previous| previous.name == subspace.name)
            {
                return Err(AllocationSchemaValidationError::DuplicateSubspaceName(
                    subspace.name,
                ));
            }
            if !self
                .allocators
                .iter()
                .any(|allocator| allocator.name == subspace.allocator.name)
            {
                return Err(AllocationSchemaValidationError::UnknownAllocator {
                    subspace: subspace.name,
                    allocator: subspace.allocator.name,
                });
            }
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllocationSchemaValidationError {
    EmptyRegistryName,
    EmptyDescriptorName,
    DuplicateAllocatorName(&'static str),
    DuplicateSubspaceName(&'static str),
    DuplicateSizeClass(SizeClassIndex),
    UnknownAllocator {
        subspace: &'static str,
        allocator: &'static str,
    },
    InvalidBlockGeometry,
    InvalidSizeClass(SizeClass),
    LocalAllocatorWithoutSizeClasses(&'static str),
    SizeClassListOnNonLocalAllocator(&'static str),
    InvalidSubspacePreciseCutoff(&'static str),
    LowerTierPreciseCellsOnNonIsoSubspace(&'static str),
}

impl StaticMarkedSpaceDescriptor {
    pub fn validate(&self) -> Result<(), AllocationSchemaValidationError> {
        if self.name.is_empty() {
            return Err(AllocationSchemaValidationError::EmptyDescriptorName);
        }
        if self.block_size == 0
            || self.atom_size == 0
            || self.weak_block_size == 0
            || !self.block_size.is_multiple_of(self.atom_size)
        {
            return Err(AllocationSchemaValidationError::InvalidBlockGeometry);
        }
        validate_size_classes(self.size_classes, self.atom_size)
    }
}

impl StaticAllocatorDescriptor {
    pub fn validate(&self) -> Result<(), AllocationSchemaValidationError> {
        if self.name.is_empty() {
            return Err(AllocationSchemaValidationError::EmptyDescriptorName);
        }
        if self.local_allocator {
            if self.size_classes.is_empty() {
                return Err(
                    AllocationSchemaValidationError::LocalAllocatorWithoutSizeClasses(self.name),
                );
            }
            validate_size_classes(self.size_classes, MARKED_BLOCK_ATOM_SIZE)?;
        } else if !self.size_classes.is_empty() {
            return Err(
                AllocationSchemaValidationError::SizeClassListOnNonLocalAllocator(self.name),
            );
        }
        Ok(())
    }
}

impl StaticSubspaceDescriptor {
    pub fn validate(&self) -> Result<(), AllocationSchemaValidationError> {
        if self.name.is_empty() || self.allocator.name.is_empty() {
            return Err(AllocationSchemaValidationError::EmptyDescriptorName);
        }
        match self.kind {
            SubspaceKind::Complete | SubspaceKind::Iso if self.precise_cutoff == 0 => {
                return Err(
                    AllocationSchemaValidationError::InvalidSubspacePreciseCutoff(self.name),
                );
            }
            SubspaceKind::Precise if self.precise_cutoff != 0 => {
                return Err(
                    AllocationSchemaValidationError::InvalidSubspacePreciseCutoff(self.name),
                );
            }
            _ => {}
        }
        if self.kind != SubspaceKind::Iso && self.lower_tier_precise_cells != 0 {
            return Err(
                AllocationSchemaValidationError::LowerTierPreciseCellsOnNonIsoSubspace(self.name),
            );
        }
        Ok(())
    }

    pub fn select_allocation(
        &self,
        registry: &AllocationSchemaRegistry,
        requested_size: usize,
    ) -> Result<AllocationSizeClassSelection, AllocationSelectionError> {
        self.validate()
            .map_err(AllocationSelectionError::InvalidSchema)?;
        if requested_size == 0 {
            return Err(AllocationSelectionError::InvalidRequestedSize);
        }

        if self.kind != SubspaceKind::Precise && requested_size <= self.precise_cutoff {
            let rounded_size = round_to_atom_size(requested_size);
            let size_class = self
                .allocator
                .size_classes
                .iter()
                .copied()
                .find(|size_class| size_class.cell_size >= rounded_size)
                .ok_or(AllocationSelectionError::NoSizeClass {
                    subspace: self.name,
                    requested_size,
                })?;
            return Ok(AllocationSizeClassSelection {
                subspace: self.name,
                requested_size,
                rounded_size,
                allocator: self.allocator.name,
                kind: AllocationSelectionKind::MarkedBlock,
                size_class: Some(size_class),
                precise_tier: None,
            });
        }

        let precise_allocator = registry
            .allocators
            .iter()
            .find(|allocator| !allocator.local_allocator)
            .ok_or(AllocationSelectionError::MissingPreciseAllocator)?;
        let precise_tier = match self.kind {
            SubspaceKind::Iso if self.lower_tier_precise_cells != 0 => {
                PreciseAllocationTier::LowerTierIso
            }
            SubspaceKind::Precise => PreciseAllocationTier::PreciseOnlySubspace,
            SubspaceKind::Complete | SubspaceKind::Iso => PreciseAllocationTier::Large,
        };

        Ok(AllocationSizeClassSelection {
            subspace: self.name,
            requested_size,
            rounded_size: round_to_atom_size(requested_size),
            allocator: precise_allocator.name,
            kind: AllocationSelectionKind::Precise,
            size_class: None,
            precise_tier: Some(precise_tier),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllocationSelectionKind {
    MarkedBlock,
    Precise,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AllocationSizeClassSelection {
    pub subspace: &'static str,
    pub requested_size: usize,
    pub rounded_size: usize,
    pub allocator: &'static str,
    pub kind: AllocationSelectionKind,
    pub size_class: Option<SizeClass>,
    pub precise_tier: Option<PreciseAllocationTier>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllocationSelectionError {
    InvalidSchema(AllocationSchemaValidationError),
    InvalidRequestedSize,
    UnknownSubspace,
    NoSizeClass {
        subspace: &'static str,
        requested_size: usize,
    },
    MissingPreciseAllocator,
}

fn round_to_atom_size(size: usize) -> usize {
    size.div_ceil(MARKED_BLOCK_ATOM_SIZE) * MARKED_BLOCK_ATOM_SIZE
}

fn validate_size_classes(
    size_classes: &[SizeClass],
    atom_size: usize,
) -> Result<(), AllocationSchemaValidationError> {
    for (index, size_class) in size_classes.iter().enumerate() {
        if size_class.cell_size == 0
            || size_class.cell_size % atom_size != 0
            || size_class.index.0 == 0
            || size_class.index.0 * atom_size != size_class.cell_size
        {
            return Err(AllocationSchemaValidationError::InvalidSizeClass(
                *size_class,
            ));
        }
        if size_classes[..index]
            .iter()
            .any(|previous| previous.index == size_class.index)
        {
            return Err(AllocationSchemaValidationError::DuplicateSizeClass(
                size_class.index,
            ));
        }
        if index > 0 && size_classes[index - 1].cell_size >= size_class.cell_size {
            return Err(AllocationSchemaValidationError::InvalidSizeClass(
                *size_class,
            ));
        }
    }
    Ok(())
}

const SIZE_CLASS_16: SizeClass = SizeClass {
    index: SizeClassIndex(1),
    cell_size: 16,
};
const SIZE_CLASS_32: SizeClass = SizeClass {
    index: SizeClassIndex(2),
    cell_size: 32,
};
const SIZE_CLASS_64: SizeClass = SizeClass {
    index: SizeClassIndex(4),
    cell_size: 64,
};
const SIZE_CLASS_80: SizeClass = SizeClass {
    index: SizeClassIndex(5),
    cell_size: MARKED_SPACE_PRECISE_CUTOFF,
};

/// Bootstrap marked-block size classes owned by `gc::space`.
pub const STATIC_MARKED_SIZE_CLASSES: &[SizeClass] =
    &[SIZE_CLASS_16, SIZE_CLASS_32, SIZE_CLASS_64, SIZE_CLASS_80];

pub const STATIC_MARKED_ALLOCATOR_DESCRIPTOR: StaticAllocatorDescriptor =
    StaticAllocatorDescriptor {
        name: "marked-block-allocator",
        mode: AllocationMode::Normal,
        failure_mode: AllocationFailureMode::ReturnNull,
        aligned_memory: true,
        local_allocator: true,
        size_classes: STATIC_MARKED_SIZE_CLASSES,
        owner: AllocationSchemaOwner::GcSpaceSchema,
        provenance: AllocationSchemaProvenance::RustStaticSeed,
    };

pub const STATIC_PRECISE_ALLOCATOR_DESCRIPTOR: StaticAllocatorDescriptor =
    StaticAllocatorDescriptor {
        name: "precise-allocator",
        mode: AllocationMode::EnsureAllocator,
        failure_mode: AllocationFailureMode::ReturnNull,
        aligned_memory: false,
        local_allocator: false,
        size_classes: &[],
        owner: AllocationSchemaOwner::GcSpaceSchema,
        provenance: AllocationSchemaProvenance::RustStaticSeed,
    };

pub const STATIC_ALLOCATOR_DESCRIPTORS: &[StaticAllocatorDescriptor] = &[
    STATIC_MARKED_ALLOCATOR_DESCRIPTOR,
    STATIC_PRECISE_ALLOCATOR_DESCRIPTOR,
];

pub const STATIC_MARKED_SPACE_DESCRIPTOR: StaticMarkedSpaceDescriptor =
    StaticMarkedSpaceDescriptor {
        name: "marked-space",
        block_size: MARKED_BLOCK_SIZE,
        atom_size: MARKED_BLOCK_ATOM_SIZE,
        precise_cutoff: MARKED_SPACE_PRECISE_CUTOFF,
        weak_block_size: WEAK_BLOCK_SIZE,
        size_classes: STATIC_MARKED_SIZE_CLASSES,
        owner: AllocationSchemaOwner::GcSpaceSchema,
        provenance: AllocationSchemaProvenance::RustStaticSeed,
    };

pub const STATIC_SUBSPACE_DESCRIPTORS: &[StaticSubspaceDescriptor] = &[
    StaticSubspaceDescriptor {
        name: "cell",
        kind: SubspaceKind::Complete,
        attributes: CellAttributes {
            destruction: DestructionMode::MayNeedDestruction,
            heap_cell_kind: HeapCellKind::JsCell,
        },
        allocator: &STATIC_MARKED_ALLOCATOR_DESCRIPTOR,
        precise_cutoff: MARKED_SPACE_PRECISE_CUTOFF,
        lower_tier_precise_cells: 0,
        mutation_authority: SubspaceMutationAuthority::HeapInitialization,
        owner: AllocationSchemaOwner::GcSpaceSchema,
        provenance: AllocationSchemaProvenance::RustStaticSeed,
    },
    StaticSubspaceDescriptor {
        name: "object",
        kind: SubspaceKind::Iso,
        attributes: CellAttributes {
            destruction: DestructionMode::MayNeedDestruction,
            heap_cell_kind: HeapCellKind::JsCellWithIndexingHeader,
        },
        allocator: &STATIC_MARKED_ALLOCATOR_DESCRIPTOR,
        precise_cutoff: MARKED_SPACE_PRECISE_CUTOFF,
        lower_tier_precise_cells: 4,
        mutation_authority: SubspaceMutationAuthority::HeapInitialization,
        owner: AllocationSchemaOwner::GcSpaceSchema,
        provenance: AllocationSchemaProvenance::RustStaticSeed,
    },
    StaticSubspaceDescriptor {
        name: "auxiliary",
        kind: SubspaceKind::Precise,
        attributes: CellAttributes {
            destruction: DestructionMode::NeedsDestruction,
            heap_cell_kind: HeapCellKind::Auxiliary,
        },
        allocator: &STATIC_PRECISE_ALLOCATOR_DESCRIPTOR,
        precise_cutoff: 0,
        lower_tier_precise_cells: 0,
        mutation_authority: SubspaceMutationAuthority::HeapInitialization,
        owner: AllocationSchemaOwner::GcSpaceSchema,
        provenance: AllocationSchemaProvenance::RustStaticSeed,
    },
];

pub const STATIC_ALLOCATION_SCHEMA_REGISTRY: AllocationSchemaRegistry = AllocationSchemaRegistry {
    name: "gc.space.static-allocation-schema",
    authority: AllocationRegistryAuthority::StaticReadOnly,
    marked_space: &STATIC_MARKED_SPACE_DESCRIPTOR,
    allocators: STATIC_ALLOCATOR_DESCRIPTORS,
    subspaces: STATIC_SUBSPACE_DESCRIPTORS,
};

pub const fn static_allocation_schema_registry() -> &'static AllocationSchemaRegistry {
    &STATIC_ALLOCATION_SCHEMA_REGISTRY
}

pub const fn static_subspace_descriptors() -> &'static [StaticSubspaceDescriptor] {
    STATIC_SUBSPACE_DESCRIPTORS
}

pub const fn static_allocator_descriptors() -> &'static [StaticAllocatorDescriptor] {
    STATIC_ALLOCATOR_DESCRIPTORS
}

/// Subspace descriptor independent from a concrete Rust cell type.
///
/// This records topology and metadata ownership. Mutating directory links,
/// allocator links, or precise tiers requires the named subspace authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubspaceDescriptor {
    pub name: &'static str,
    pub kind: SubspaceKind,
    pub metadata: CellMetadata,
    pub aligned_allocator: Option<AlignedMemoryAllocatorId>,
    pub lower_tier_precise_cells: u8,
    pub first_directory: Option<BlockDirectoryId>,
    pub directory_for_empty_allocation: Option<BlockDirectoryId>,
    pub next_subspace_in_aligned_allocator: Option<&'static str>,
    pub mutation_authority: SubspaceMutationAuthority,
}

impl SubspaceDescriptor {
    pub fn complete(name: &'static str, metadata: CellMetadata) -> Self {
        Self {
            name,
            kind: SubspaceKind::Complete,
            metadata,
            aligned_allocator: None,
            lower_tier_precise_cells: 0,
            first_directory: None,
            directory_for_empty_allocation: None,
            next_subspace_in_aligned_allocator: None,
            mutation_authority: SubspaceMutationAuthority::HeapInitialization,
        }
    }

    pub fn iso(name: &'static str, metadata: CellMetadata, lower_tier_precise_cells: u8) -> Self {
        Self {
            name,
            kind: SubspaceKind::Iso,
            metadata,
            aligned_allocator: None,
            lower_tier_precise_cells,
            first_directory: None,
            directory_for_empty_allocation: None,
            next_subspace_in_aligned_allocator: None,
            mutation_authority: SubspaceMutationAuthority::HeapInitialization,
        }
    }

    pub fn precise(name: &'static str, metadata: CellMetadata) -> Self {
        Self {
            name,
            kind: SubspaceKind::Precise,
            metadata,
            aligned_allocator: None,
            lower_tier_precise_cells: 0,
            first_directory: None,
            directory_for_empty_allocation: None,
            next_subspace_in_aligned_allocator: None,
            mutation_authority: SubspaceMutationAuthority::HeapInitialization,
        }
    }

    pub fn validate(&self) -> Result<(), AllocationDescriptorValidationError> {
        if self.name.is_empty() {
            return Err(AllocationDescriptorValidationError::EmptyName);
        }
        self.metadata
            .validate()
            .map_err(AllocationDescriptorValidationError::CellMetadata)?;
        if self.kind != SubspaceKind::Iso && self.lower_tier_precise_cells != 0 {
            return Err(AllocationDescriptorValidationError::LowerTierPreciseCellsOnNonIsoSubspace);
        }
        if self.kind == SubspaceKind::Precise
            && (self.first_directory.is_some() || self.directory_for_empty_allocation.is_some())
        {
            return Err(AllocationDescriptorValidationError::DirectoryOnPreciseSubspace);
        }
        if self.directory_for_empty_allocation.is_some() && self.first_directory.is_none() {
            return Err(AllocationDescriptorValidationError::EmptyDirectoryWithoutFirstDirectory);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllocationDescriptorValidationError {
    EmptyName,
    CellMetadata(crate::gc::CellMetadataValidationError),
    LowerTierPreciseCellsOnNonIsoSubspace,
    DirectoryOnPreciseSubspace,
    EmptyDirectoryWithoutFirstDirectory,
    InvalidMarkedBlockGeometry,
    MarkedBlockDirectoryMismatch,
    InvalidFreeList,
    AllocatorSizeClassMismatch,
    DuplicateSubspaceName(&'static str),
    DuplicateDirectory(BlockDirectoryId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubspaceDescriptorBuilder {
    descriptor: SubspaceDescriptor,
}

impl SubspaceDescriptorBuilder {
    pub fn new(name: &'static str, metadata: CellMetadata, kind: SubspaceKind) -> Self {
        Self {
            descriptor: SubspaceDescriptor {
                name,
                kind,
                metadata,
                aligned_allocator: None,
                lower_tier_precise_cells: 0,
                first_directory: None,
                directory_for_empty_allocation: None,
                next_subspace_in_aligned_allocator: None,
                mutation_authority: SubspaceMutationAuthority::HeapInitialization,
            },
        }
    }

    pub fn aligned_allocator(mut self, allocator: AlignedMemoryAllocatorId) -> Self {
        self.descriptor.aligned_allocator = Some(allocator);
        self
    }

    pub fn lower_tier_precise_cells(mut self, count: u8) -> Self {
        self.descriptor.lower_tier_precise_cells = count;
        self
    }

    pub fn first_directory(mut self, directory: BlockDirectoryId) -> Self {
        self.descriptor.first_directory = Some(directory);
        self
    }

    pub fn directory_for_empty_allocation(mut self, directory: BlockDirectoryId) -> Self {
        self.descriptor.directory_for_empty_allocation = Some(directory);
        self
    }

    pub fn next_subspace_in_aligned_allocator(mut self, name: &'static str) -> Self {
        self.descriptor.next_subspace_in_aligned_allocator = Some(name);
        self
    }

    pub fn mutation_authority(mut self, authority: SubspaceMutationAuthority) -> Self {
        self.descriptor.mutation_authority = authority;
        self
    }

    pub fn build(self) -> Result<SubspaceDescriptor, AllocationDescriptorValidationError> {
        self.descriptor.validate()?;
        Ok(self.descriptor)
    }
}

impl MarkedBlockDescriptor {
    pub fn validate(&self) -> Result<(), AllocationDescriptorValidationError> {
        if self.cell_size == 0
            || !self.cell_size.is_multiple_of(MARKED_BLOCK_ATOM_SIZE)
            || self.cells_per_block != MARKED_BLOCK_SIZE / self.cell_size
        {
            return Err(AllocationDescriptorValidationError::InvalidMarkedBlockGeometry);
        }
        if self.subspace.is_empty() {
            return Err(AllocationDescriptorValidationError::EmptyName);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkedBlockDescriptorBuilder {
    descriptor: MarkedBlockDescriptor,
}

impl MarkedBlockDescriptorBuilder {
    pub fn new(
        id: MarkedBlockId,
        directory: BlockDirectoryId,
        subspace: &'static str,
        cell_size: usize,
        epoch: HeapEpoch,
        destruction: DestructionMode,
    ) -> Self {
        Self {
            descriptor: MarkedBlockDescriptor::for_size(
                id,
                directory,
                subspace,
                cell_size,
                epoch,
                destruction,
            ),
        }
    }

    pub fn state(mut self, state: BlockState) -> Self {
        self.descriptor.state = state;
        self
    }

    pub fn sweep_mode(mut self, sweep_mode: SweepMode) -> Self {
        self.descriptor.sweep_mode = sweep_mode;
        self
    }

    pub fn weak_set_attached(mut self, attached: bool) -> Self {
        self.descriptor.weak_set_attached = attached;
        self
    }

    pub fn build(self) -> Result<MarkedBlockDescriptor, AllocationDescriptorValidationError> {
        self.descriptor.validate()?;
        Ok(self.descriptor)
    }
}

impl LocalAllocatorDescriptor {
    pub fn validate(&self) -> Result<(), AllocationDescriptorValidationError> {
        if self.size_class.cell_size == 0
            || self.size_class.index.0 * MARKED_BLOCK_ATOM_SIZE != self.size_class.cell_size
            || self.free_list.cell_size != self.size_class.cell_size
        {
            return Err(AllocationDescriptorValidationError::AllocatorSizeClassMismatch);
        }
        if self.free_list.original_size < self.free_list.cell_size
            && self.free_list.interval_count != 0
        {
            return Err(AllocationDescriptorValidationError::InvalidFreeList);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalAllocatorDescriptorBuilder {
    descriptor: LocalAllocatorDescriptor,
}

impl LocalAllocatorDescriptorBuilder {
    pub fn new(id: AllocatorId, directory: BlockDirectoryId, size_class: SizeClass) -> Self {
        Self {
            descriptor: LocalAllocatorDescriptor {
                id,
                directory,
                size_class,
                current_block: None,
                last_active_block: None,
                allocation_cursor: 0,
                free_list: FreeListDescriptor {
                    cell_size: size_class.cell_size,
                    original_size: size_class.cell_size,
                    interval_count: 0,
                },
            },
        }
    }

    pub fn current_block(mut self, block: MarkedBlockId) -> Self {
        self.descriptor.current_block = Some(block);
        self
    }

    pub fn last_active_block(mut self, block: MarkedBlockId) -> Self {
        self.descriptor.last_active_block = Some(block);
        self
    }

    pub fn allocation_cursor(mut self, cursor: usize) -> Self {
        self.descriptor.allocation_cursor = cursor;
        self
    }

    pub fn free_list(mut self, free_list: FreeListDescriptor) -> Self {
        self.descriptor.free_list = free_list;
        self
    }

    pub fn build(self) -> Result<LocalAllocatorDescriptor, AllocationDescriptorValidationError> {
        self.descriptor.validate()?;
        Ok(self.descriptor)
    }
}

impl AlignedMemoryAllocatorDescriptor {
    pub fn validate(&self) -> Result<(), AllocationDescriptorValidationError> {
        if self.name.is_empty() {
            return Err(AllocationDescriptorValidationError::EmptyName);
        }
        for (index, subspace) in self.subspaces.iter().enumerate() {
            if subspace.is_empty() {
                return Err(AllocationDescriptorValidationError::EmptyName);
            }
            if self.subspaces[..index]
                .iter()
                .any(|previous| previous == subspace)
            {
                return Err(AllocationDescriptorValidationError::DuplicateSubspaceName(
                    subspace,
                ));
            }
        }
        for (index, directory) in self.directories.iter().enumerate() {
            if self.directories[..index]
                .iter()
                .any(|previous| previous == directory)
            {
                return Err(AllocationDescriptorValidationError::DuplicateDirectory(
                    *directory,
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlignedMemoryAllocatorDescriptorBuilder {
    descriptor: AlignedMemoryAllocatorDescriptor,
}

impl AlignedMemoryAllocatorDescriptorBuilder {
    pub fn new(id: AlignedMemoryAllocatorId, name: &'static str) -> Self {
        Self {
            descriptor: AlignedMemoryAllocatorDescriptor {
                id,
                name,
                subspaces: Vec::new(),
                directories: Vec::new(),
                owns_virtual_memory: false,
            },
        }
    }

    pub fn subspace(mut self, name: &'static str) -> Self {
        self.descriptor.subspaces.push(name);
        self
    }

    pub fn directory(mut self, directory: BlockDirectoryId) -> Self {
        self.descriptor.directories.push(directory);
        self
    }

    pub fn owns_virtual_memory(mut self, owns_virtual_memory: bool) -> Self {
        self.descriptor.owns_virtual_memory = owns_virtual_memory;
        self
    }

    pub fn build(
        self,
    ) -> Result<AlignedMemoryAllocatorDescriptor, AllocationDescriptorValidationError> {
        self.descriptor.validate()?;
        Ok(self.descriptor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{static_cell_metadata_registry, CellType};

    fn cell_metadata() -> CellMetadata {
        static_cell_metadata_registry()
            .metadata_for_type(CellType::Cell)
            .map(|descriptor| descriptor.metadata)
            .unwrap_or_default()
    }

    #[test]
    fn static_allocation_schema_is_structurally_valid() {
        assert_eq!(static_allocation_schema_registry().validate(), Ok(()));
    }

    #[test]
    fn subspace_builder_accepts_iso_lower_tier_precise_count() {
        let descriptor =
            SubspaceDescriptorBuilder::new("object", cell_metadata(), SubspaceKind::Iso)
                .lower_tier_precise_cells(4)
                .build();

        assert_eq!(
            descriptor.map(|descriptor| descriptor.lower_tier_precise_cells),
            Ok(4)
        );
    }

    #[test]
    fn subspace_validator_rejects_precise_directories() {
        let descriptor =
            SubspaceDescriptorBuilder::new("precise", cell_metadata(), SubspaceKind::Precise)
                .first_directory(BlockDirectoryId(1))
                .build();

        assert_eq!(
            descriptor,
            Err(AllocationDescriptorValidationError::DirectoryOnPreciseSubspace)
        );
    }

    #[test]
    fn marked_block_builder_computes_cells_per_block() {
        let block = MarkedBlockDescriptorBuilder::new(
            MarkedBlockId(1),
            BlockDirectoryId(1),
            "cell",
            32,
            HeapEpoch(7),
            DestructionMode::DoesNotNeedDestruction,
        )
        .build();

        assert_eq!(
            block.map(|block| block.cells_per_block),
            Ok(MARKED_BLOCK_SIZE / 32)
        );
    }

    #[test]
    fn allocator_validator_rejects_free_list_size_mismatch() {
        let allocator = LocalAllocatorDescriptorBuilder::new(
            AllocatorId(1),
            BlockDirectoryId(1),
            SizeClass::for_size(32),
        )
        .free_list(FreeListDescriptor {
            cell_size: 16,
            original_size: 32,
            interval_count: 1,
        })
        .build();

        assert_eq!(
            allocator,
            Err(AllocationDescriptorValidationError::AllocatorSizeClassMismatch)
        );
    }

    #[test]
    fn allocation_selection_chooses_small_marked_block_size_class() {
        let selection = static_allocation_schema_registry().select_allocation("cell", 17);

        assert_eq!(
            selection.map(|selection| (selection.kind, selection.size_class)),
            Ok((
                AllocationSelectionKind::MarkedBlock,
                Some(SizeClass {
                    index: SizeClassIndex(2),
                    cell_size: 32
                })
            ))
        );
    }

    #[test]
    fn allocation_selection_chooses_precise_for_large_complete_subspace_cell() {
        let selection = static_allocation_schema_registry().select_allocation("cell", 81);

        assert_eq!(
            selection.map(|selection| (selection.kind, selection.precise_tier)),
            Ok((
                AllocationSelectionKind::Precise,
                Some(PreciseAllocationTier::Large)
            ))
        );
    }

    #[test]
    fn allocation_selection_rejects_zero_sized_request() {
        assert_eq!(
            static_allocation_schema_registry().select_allocation("cell", 0),
            Err(AllocationSelectionError::InvalidRequestedSize)
        );
    }
}

/// Typed subspace handle used by Rust allocation sites.
///
/// The type parameter constrains borrowers at compile time only. The handle
/// does not own cells and must not be used to reinterpret `CellId`.
#[derive(Debug)]
pub struct TypedSubspace<T: ?Sized> {
    descriptor: SubspaceDescriptor,
    _cell: PhantomData<T>,
}

impl<T: ?Sized> TypedSubspace<T> {
    pub fn new(descriptor: SubspaceDescriptor) -> Self {
        Self {
            descriptor,
            _cell: PhantomData,
        }
    }

    pub fn descriptor(&self) -> &SubspaceDescriptor {
        &self.descriptor
    }
}

/// Marked-space-wide version and allocation metadata.
///
/// Marked space owns aggregate allocator state for one heap. Iteration flags,
/// directory locks, and epochs are heap/collector mutation points.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarkedSpaceDescriptor {
    pub heap: HeapId,
    pub marking_epoch: HeapEpoch,
    pub newly_allocated_epoch: HeapEpoch,
    pub eden_epoch: HeapEpoch,
    pub capacity: usize,
    pub size_classes: Vec<SizeClass>,
    pub iteration_state: SpaceIterationState,
    pub conservative_scan_is_prepared: bool,
    pub directory_lock_required: bool,
}

impl MarkedSpaceDescriptor {
    pub fn new(heap: HeapId) -> Self {
        Self {
            heap,
            marking_epoch: HeapEpoch::default(),
            newly_allocated_epoch: HeapEpoch::default(),
            eden_epoch: HeapEpoch::default(),
            capacity: 0,
            size_classes: Vec::new(),
            iteration_state: SpaceIterationState::NotIterating,
            conservative_scan_is_prepared: false,
            directory_lock_required: false,
        }
    }

    pub fn size_class_for(size: usize) -> SizeClass {
        SizeClass::for_size(size)
    }
}

/// High-level allocation profile entry grouped by subspace and size class.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AllocationProfileEntry {
    pub subspace: &'static str,
    pub cell_kind: HeapCellKind,
    pub size_class: Option<SizeClass>,
    pub allocation_count: usize,
    pub allocated_bytes: usize,
    pub precise_allocation_count: usize,
}

/// Snapshot of allocation counters used by heuristics and diagnostics.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AllocationProfile {
    pub entries: Vec<AllocationProfileEntry>,
    pub oversized_bytes: usize,
    pub non_oversized_bytes: usize,
}
