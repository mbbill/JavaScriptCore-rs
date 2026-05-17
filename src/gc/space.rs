//! Subspace, block, size-class, and allocator descriptors.
//!
//! These types describe the shape of JSC's paged and precise allocation
//! domains without allocating memory or sweeping cells.

use core::marker::PhantomData;

use crate::gc::{CellMetadata, DestructionMode, HeapCellKind, HeapEpoch, HeapId};

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

/// Allocation strategy for a request before the allocator exists.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AllocationMode {
    #[default]
    Normal,
    NoCollection,
    MustAlreadyHaveAllocator,
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
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct BlockDirectoryId(pub u64);

/// Opaque marked block identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct MarkedBlockId(pub u64);

/// Opaque precise allocation identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct PreciseAllocationId(pub u64);

/// Opaque allocator identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct AllocatorId(pub u64);

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

/// Descriptor for a block directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockDirectoryDescriptor {
    pub id: BlockDirectoryId,
    pub subspace: &'static str,
    pub size_class: SizeClass,
    pub allocator: Option<AllocatorId>,
}

/// Descriptor for a page-aligned marked block.
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
        }
    }
}

/// Descriptor for a malloc-backed precise allocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreciseAllocationDescriptor {
    pub id: PreciseAllocationId,
    pub subspace: &'static str,
    pub cell_size: usize,
    pub is_newly_allocated: bool,
    pub is_marked: bool,
    pub lower_tier_precise_index: Option<u8>,
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalAllocatorDescriptor {
    pub id: AllocatorId,
    pub directory: BlockDirectoryId,
    pub size_class: SizeClass,
    pub current_block: Option<MarkedBlockId>,
    pub free_list: FreeListDescriptor,
}

/// Handle-like allocator descriptor exposed to allocation clients.
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

/// Subspace descriptor independent from a concrete Rust cell type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubspaceDescriptor {
    pub name: &'static str,
    pub kind: SubspaceKind,
    pub metadata: CellMetadata,
    pub aligned_allocator: Option<&'static str>,
    pub lower_tier_precise_cells: u8,
}

impl SubspaceDescriptor {
    pub fn complete(name: &'static str, metadata: CellMetadata) -> Self {
        Self {
            name,
            kind: SubspaceKind::Complete,
            metadata,
            aligned_allocator: None,
            lower_tier_precise_cells: 0,
        }
    }

    pub fn iso(name: &'static str, metadata: CellMetadata, lower_tier_precise_cells: u8) -> Self {
        Self {
            name,
            kind: SubspaceKind::Iso,
            metadata,
            aligned_allocator: None,
            lower_tier_precise_cells,
        }
    }

    pub fn precise(name: &'static str, metadata: CellMetadata) -> Self {
        Self {
            name,
            kind: SubspaceKind::Precise,
            metadata,
            aligned_allocator: None,
            lower_tier_precise_cells: 0,
        }
    }
}

/// Typed subspace handle used by Rust allocation sites.
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
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarkedSpaceDescriptor {
    pub heap: HeapId,
    pub marking_epoch: HeapEpoch,
    pub newly_allocated_epoch: HeapEpoch,
    pub eden_epoch: HeapEpoch,
    pub capacity: usize,
    pub size_classes: Vec<SizeClass>,
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
