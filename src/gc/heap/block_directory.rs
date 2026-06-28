//! BlockDirectory: owns the block list for ONE MarkedSpace size class plus the
//! LocalAllocator-style FreeList bump path (heap/BlockDirectory.h:171;
//! heap/LocalAllocator.h:71-72; FreeList fast path heap/FreeList.h:82-123).
//! Faithful port of the proven prototype `BlockDirectory`
//! (tools/s4_arena_proto/src/lib.rs:237-315): the unsafe alloc/expose/init core is
//! byte-for-byte the proven core; R1 adds the newlyAllocated (alloc) bitmap write,
//! the `CellPtr` return type, and the FreeList interval allocator.
//!
//! DIVERGENCE (R1 fuses two C++ types): JSC splits block STORAGE (BlockDirectory,
//! the Vector<MarkedBlock::Handle*>) from the per-mutator-thread allocation cursor
//! (LocalAllocator, heap/LocalAllocator.h, which holds the FreeList). R1 is
//! single-threaded with no thread-local allocators yet, so it holds the FreeList
//! inline here (the proven prototype likewise fused the bump cursor into the
//! directory). R2/R3 split the LocalAllocator out when a concurrent collector or
//! thread-local allocation lands.

#![allow(dead_code)]

use core::ptr;
use std::alloc::{alloc_zeroed, dealloc};

use super::free_list::FreeList;
use super::marked_block::{
    block_layout, cell_ptr, set_newly_allocated, Cell, CellPtr, MarkedBlock, ATOM_SIZE, END_ATOM,
    FIRST_PAYLOAD_ATOM, HALF_ALIGNMENT,
};

/// FreeCell link obfuscation secret. JSC draws a fresh `vm.heapRandom().getUint64()`
/// per sweep (heap/MarkedBlockInlines.h:263) as exploit-hardening entropy against
/// FreeList-corruption attacks; R1 has no VM RNG yet and allocation correctness is
/// independent of the secret value (scramble/descramble round-trip through the
/// FreeList's own stored secret), so a fixed nonzero obfuscation constant is used
/// until R2 wires `vm.heapRandom()`.
const FREELIST_SECRET: u64 = 0x9E37_79B9_7F4A_7C15;

/// MarkedBlock::Handle constructor start atom (heap/MarkedBlock.cpp:414-422): push
/// the unallocatable front slop forward so that `startAtom + k*atomsPerCell` reaches
/// `endAtom` EXACTLY when the payload is full. This makes the FreeList interval an
/// integer number of cells and the bump terminate precisely at the payload end
/// (no partial trailing cell, no overrun).
///
/// DIVERGENCE (corrected): the prior R1 stub used a fixed `start_atom ==
/// FIRST_PAYLOAD_ATOM` and a tail-slop `next_atom > ATOMS_PER_BLOCK` termination
/// guard. A faithful single-interval FreeList over `[startAtom, endAtom)` requires
/// JSC's front-slop geometry, so this restores it; cells-per-block is unchanged.
fn start_atom_for(atoms_per_cell: usize) -> usize {
    // numberOfPayloadAtoms == endAtom - firstPayloadRegionAtom (MarkedBlock.h:339).
    let number_of_payload_atoms = END_ATOM - FIRST_PAYLOAD_ATOM;
    let number_of_unallocatable_atoms = number_of_payload_atoms % atoms_per_cell;
    FIRST_PAYLOAD_ATOM + number_of_unallocatable_atoms
}

/// BlockDirectory (heap/BlockDirectory.h): owns the block list for ONE size class
/// and the LocalAllocator FreeList fast path (heap/FreeList.h:82-123). Blocks never
/// move; the Vec holds only the raw owning page pointers (mirrors
/// Vector<MarkedBlock::Handle*>, BlockDirectory.h:171). Growing the Vec moves only
/// 8-byte owning pointers -> cell addresses stay stable and provenance-valid.
pub(crate) struct BlockDirectory {
    pub(crate) cell_size_atoms: usize,
    /// Cell size in bytes (== cell_size_atoms * ATOM_SIZE) — the LocalAllocator
    /// cellSize the FreeList bumps by (heap/LocalAllocator.h:48).
    cell_size: usize,
    /// owning raw page pointers (for dealloc on Drop). Never turned into a
    /// `&MarkedBlock`; all block access goes through exposed provenance.
    pages: Vec<*mut u8>,
    /// per-block once-exposed base address (== the page pointer's address).
    pub(crate) block_base_addr: Vec<usize>,
    /// LocalAllocator-style FreeList over this directory's exposed pages
    /// (heap/LocalAllocator.h:72 `m_freeList`). Replaces the prior raw `next_atom`
    /// bump cursor with the faithful FreeCell-interval fast path.
    free_list: FreeList,
}

impl BlockDirectory {
    pub(crate) fn new(cell_size_atoms: usize) -> Self {
        let cell_size = cell_size_atoms * ATOM_SIZE;
        BlockDirectory {
            cell_size_atoms,
            cell_size,
            pages: Vec::new(),
            block_base_addr: Vec::new(),
            // A fresh FreeList is in the always-fail state, so the first allocate
            // takes the slow path and adds a block (heap/FreeList.h:117-122).
            free_list: FreeList::new(cell_size as u32),
        }
    }

    /// MarkedBlock::tryCreate via AlignedMemoryAllocator: allocate a raw,
    /// block-aligned, zeroed page; write the header through raw pointers; expose
    /// the whole-page provenance ONCE (contract C2); then sweep the fresh empty
    /// block to the FreeList (one interval over the whole payload). Returns the new
    /// block base.
    fn add_block(&mut self) -> usize {
        // SAFETY (C1): nonzero, power-of-two-aligned layout; alloc_zeroed gives a
        // fresh page whose allocation-root provenance grants read+write over all
        // BLOCK_SIZE bytes. Zeroing initializes mark/newlyAllocated words and
        // payload to 0. The page is owned as a bare `*mut u8` (never Box<MarkedBlock>,
        // which would be a Unique co-owner that retags and pops the carried tag).
        let raw = unsafe { alloc_zeroed(block_layout()) };
        assert!(!raw.is_null(), "page allocation failed");
        let bp = raw.cast::<MarkedBlock>();
        // MarkedBlock::Handle ctor (heap/MarkedBlock.cpp:414-422): front-slop start
        // atom so the bump terminates exactly at endAtom.
        let start_atom = start_atom_for(self.cell_size_atoms);
        // Initialize header fields via raw field pointers (no &MarkedBlock formed).
        // SAFETY: bp is a fresh page of MarkedBlock layout; these offsets are in-bounds.
        unsafe {
            ptr::addr_of_mut!((*bp).header.atoms_per_cell).write(self.cell_size_atoms as u16);
            ptr::addr_of_mut!((*bp).header.start_atom).write(start_atom as u16);
        }
        // Expose the WHOLE-PAGE provenance exactly once (contract C2).
        let base_addr = raw.expose_provenance();
        self.pages.push(raw); // may realloc the Vec buffer; pages never move
        self.block_base_addr.push(base_addr);

        // Sweep the fresh empty block to the FreeList: ONE interval spanning
        // [startAtom, endAtom) (heap/MarkedBlockInlines.h:313-318, IsEmpty quick
        // path). payloadEnd == base + blockSize (endAtom * atomSize).
        let payload_begin = base_addr + start_atom * ATOM_SIZE;
        let payload_end = base_addr + END_ATOM * ATOM_SIZE;
        // SAFETY (C2/C3): payload_begin..payload_end lies inside the page whose
        // whole-allocation provenance was just exposed; writing the head FreeCell's
        // scrambled link bits is interior page memory this directory solely owns,
        // and no cell has been handed out yet.
        unsafe {
            self.free_list
                .initialize_empty_block(payload_begin, payload_end, FREELIST_SECRET);
        }
        base_addr
    }

    /// LocalAllocator::allocate (heap/LocalAllocatorInlines.h:33-43): the FreeList
    /// interval bump fast path; on exhaustion, allocateSlowCase
    /// (heap/LocalAllocator.cpp) gets a fresh block, sweeps it to a FreeList, and
    /// retries. Initializes the cell header through a raw pointer, sets the
    /// newlyAllocated (alloc) bit, and returns the cell's machine address
    /// (`CellPtr`, the identity carried in the JsValue) plus `Some(base)` when a new
    /// block was created (so MarkedSpace can register it via didAddBlock ->
    /// m_blocks.add, heap/MarkedSpace.cpp didAddBlock / MarkedBlockSet.h:51-55).
    pub(crate) fn allocate(&mut self, init: Cell) -> (CellPtr, Option<usize>) {
        let mut new_base = None;
        // Fast path: bump within the current FreeList interval.
        // SAFETY: the FreeList's intervals reference live, exposed pages this
        // directory owns; single mutator thread (contract C5/C6).
        let cell_addr = match unsafe { self.free_list.allocate() } {
            Some(addr) => addr,
            None => {
                // allocateSlowCase: the FreeList is exhausted -> add a fresh block
                // (which sweeps it to a new FreeList), then retry.
                let base = self.add_block();
                new_base = Some(base);
                // The fresh block has a non-empty interval, so this always succeeds
                // (FreeList "we don't create empty intervals" invariant,
                // heap/FreeListInlines.h:50-51).
                // SAFETY: as the fast path above; the FreeList was just initialized
                // over the freshly exposed block.
                unsafe { self.free_list.allocate() }
                    .expect("fresh-block FreeList must yield a cell")
            }
        };

        // Recover an interior-mutable pointer (valid provenance from the once-
        // exposed page) and initialize the cell. NO &MarkedBlock is formed; the
        // cell SLOT is an UnsafeCell<Cell> so writing through `.get()` is sound.
        let cp = cell_ptr(cell_addr);
        // SAFETY (C3,C4): fresh, never-before-handed-out, atom-aligned slot inside
        // this page; provenance is the page's exposed allocation root spanning the
        // whole block; we are the sole accessor. The FreeCell link bytes the
        // FreeList wrote at this address were already decoded (advance) before the
        // cell was handed out, so overwriting them is sound.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::heap::marked_block::{ATOMS_PER_BLOCK, ATOMS_PER_CELL, BLOCK_MASK};

    /// The FreeList interval fast path fills one block, then the LocalAllocator
    /// slow path adds a SECOND block and continues — allocation spans blocks. The
    /// per-block cell count equals JSC's front-slop geometry exactly, and a new
    /// block is registered (Some(base)) precisely at the first alloc and when the
    /// first block fills.
    #[test]
    fn freelist_fills_a_block_then_spans_to_a_new_block() {
        let mut dir = BlockDirectory::new(ATOMS_PER_CELL); // 5 atoms == 80B class
        let start_atom = start_atom_for(ATOMS_PER_CELL);
        let per_block = (ATOMS_PER_BLOCK - start_atom) / ATOMS_PER_CELL;
        assert!(per_block >= 2, "size class must fit >=2 cells per block");

        let mut blocks = std::collections::HashSet::new();
        let mut new_block_events = 0usize;
        // Allocate past the first block boundary but not into a third block.
        for i in 0..(per_block + 5) {
            let (cp, new_base) = dir.allocate(Cell::new(0x10, i as u64));
            if new_base.is_some() {
                new_block_events += 1;
            }
            // Every cell is atom/cell aligned within its block.
            assert_eq!(cp.addr() & (ATOM_SIZE - 1), 0, "atom-aligned");
            blocks.insert(cp.addr() & BLOCK_MASK);
        }

        assert_eq!(
            new_block_events, 2,
            "a fresh block is created on the first alloc and when block 1 fills"
        );
        assert_eq!(blocks.len(), 2, "allocations span exactly two blocks");
    }

    /// Within a single block the FreeList yields strictly increasing, contiguous
    /// cell addresses spaced by the cell size (faithful interval bump).
    #[test]
    fn freelist_addresses_are_contiguous_within_a_block() {
        let mut dir = BlockDirectory::new(ATOMS_PER_CELL);
        let cell_size = ATOMS_PER_CELL * ATOM_SIZE;
        let mut prev: Option<usize> = None;
        let mut first_base = None;
        for i in 0..10 {
            let (cp, _new) = dir.allocate(Cell::new(0x10, i as u64));
            let base = cp.addr() & BLOCK_MASK;
            first_base.get_or_insert(base);
            if let Some(p) = prev {
                // All 10 fit in the first block (per_block >> 10), so contiguous.
                assert_eq!(cp.addr(), p + cell_size, "contiguous bump within interval");
            }
            assert_eq!(base, first_base.unwrap(), "all in the first block");
            prev = Some(cp.addr());
        }
    }
}
