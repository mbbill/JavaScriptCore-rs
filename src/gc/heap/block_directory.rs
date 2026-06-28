//! BlockDirectory: owns the block list for ONE MarkedSpace size class plus the
//! bump cursor (heap/BlockDirectory.h:171; FreeList fast path heap/FreeList.h:82-123).
//! Faithful port of the proven prototype `BlockDirectory`
//! (tools/s4_arena_proto/src/lib.rs:237-315): the unsafe alloc/expose/init core is
//! byte-for-byte the proven core; R1 adds only the newlyAllocated (alloc) bitmap
//! write and the `CellPtr` return type.

#![allow(dead_code)]

use core::ptr;
use std::alloc::{alloc_zeroed, dealloc};

use super::marked_block::{
    block_layout, cell_ptr, set_newly_allocated, Cell, CellPtr, MarkedBlock, ATOMS_PER_BLOCK,
    ATOM_SIZE, FIRST_PAYLOAD_ATOM, HALF_ALIGNMENT,
};

/// BlockDirectory (heap/BlockDirectory.h): owns the block list for ONE size class
/// and the bump cursor (FreeList fast path, heap/FreeList.h:82-123). Blocks never
/// move; the Vec holds only the raw owning page pointers (mirrors
/// Vector<MarkedBlock::Handle*>, BlockDirectory.h:171). Growing the Vec moves only
/// 8-byte owning pointers -> cell addresses stay stable and provenance-valid.
pub(crate) struct BlockDirectory {
    pub(crate) cell_size_atoms: usize,
    /// owning raw page pointers (for dealloc on Drop). Never turned into a
    /// `&MarkedBlock`; all block access goes through exposed provenance.
    pages: Vec<*mut u8>,
    /// per-block once-exposed base address (== the page pointer's address).
    pub(crate) block_base_addr: Vec<usize>,
    /// bump cursor into the current (last) block: next free atom index.
    next_atom: usize,
}

impl BlockDirectory {
    pub(crate) fn new(cell_size_atoms: usize) -> Self {
        BlockDirectory {
            cell_size_atoms,
            pages: Vec::new(),
            block_base_addr: Vec::new(),
            next_atom: ATOMS_PER_BLOCK, // force a fresh block on first alloc
        }
    }

    /// MarkedBlock::tryCreate via AlignedMemoryAllocator: allocate a raw,
    /// block-aligned, zeroed page; write the header through raw pointers; expose
    /// the whole-page provenance ONCE (contract C2). Returns the new block base.
    fn add_block(&mut self) -> usize {
        // SAFETY (C1): nonzero, power-of-two-aligned layout; alloc_zeroed gives a
        // fresh page whose allocation-root provenance grants read+write over all
        // BLOCK_SIZE bytes. Zeroing initializes mark/newlyAllocated words and
        // payload to 0. The page is owned as a bare `*mut u8` (never Box<MarkedBlock>,
        // which would be a Unique co-owner that retags and pops the carried tag).
        let raw = unsafe { alloc_zeroed(block_layout()) };
        assert!(!raw.is_null(), "page allocation failed");
        let bp = raw.cast::<MarkedBlock>();
        // Initialize header fields via raw field pointers (no &MarkedBlock formed).
        // SAFETY: bp is a fresh page of MarkedBlock layout; these offsets are in-bounds.
        unsafe {
            ptr::addr_of_mut!((*bp).header.atoms_per_cell).write(self.cell_size_atoms as u16);
            ptr::addr_of_mut!((*bp).header.start_atom).write(FIRST_PAYLOAD_ATOM as u16);
        }
        // Expose the WHOLE-PAGE provenance exactly once (contract C2).
        let base_addr = raw.expose_provenance();
        self.pages.push(raw); // may realloc the Vec buffer; pages never move
        self.block_base_addr.push(base_addr);
        self.next_atom = FIRST_PAYLOAD_ATOM;
        base_addr
    }

    /// LocalAllocator::allocate fast path: bump within the current block (new page
    /// on overflow), initialize the header through a raw pointer, set the
    /// newlyAllocated (alloc) bit, return the cell's machine address (`CellPtr`,
    /// the identity carried in the JsValue) plus `Some(base)` when a new block was
    /// created (so MarkedSpace can register it via didAddBlock -> m_blocks.add,
    /// heap/MarkedSpace.cpp didAddBlock / MarkedBlockSet.h:51-55).
    pub(crate) fn allocate(&mut self, init: Cell) -> (CellPtr, Option<usize>) {
        let mut new_base = None;
        if self.pages.is_empty() || self.next_atom + self.cell_size_atoms > ATOMS_PER_BLOCK {
            new_base = Some(self.add_block());
        }
        let base_addr = *self.block_base_addr.last().unwrap();
        let atom = self.next_atom;
        self.next_atom += self.cell_size_atoms;
        let cell_addr = base_addr + atom * ATOM_SIZE;

        // Recover an interior-mutable pointer (valid provenance from the once-
        // exposed page) and initialize the cell. NO &MarkedBlock is formed; the
        // cell SLOT is an UnsafeCell<Cell> so writing through `.get()` is sound.
        let cp = cell_ptr(cell_addr);
        // SAFETY (C3,C4): fresh, never-before-handed-out, atom-aligned slot inside
        // this page; provenance is the page's exposed allocation root spanning the
        // whole block; we are the sole accessor.
        unsafe { ptr::write(cp, init) };
        debug_assert_eq!(
            cell_addr & HALF_ALIGNMENT,
            0,
            "MarkedBlock cells are 16-aligned"
        );
        // setNewlyAllocated: this cell is mutator-live until the next collection
        // (MarkedBlock::setNewlyAllocated, heap/MarkedBlock.h:366). This is the
        // per-block alloc bitmap the find() liveness gate reads — no side table.
        set_newly_allocated(cell_addr);
        (CellPtr::from_addr(cell_addr), new_base)
    }
}

impl Drop for BlockDirectory {
    fn drop(&mut self) {
        for &raw in &self.pages {
            // SAFETY: each `raw` came from alloc_zeroed(block_layout()) and is freed
            // exactly once here; no live pointer into the page outlives the arena.
            unsafe { dealloc(raw, block_layout()) };
        }
    }
}
