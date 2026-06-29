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

use super::free_list::{FreeList, NewlyAllocatedMode, SweepResult};
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

    /// gc-r4 R3 (reversible shadow oracle): identical FreeList/slow-path geometry to
    /// `allocate`, but writes an arbitrary POD BYTE BLOB into the cell slot instead of
    /// the fixed demo `Cell`. This is the path the R3 shadow oracle uses to ACCEPT +
    /// STORE a real `CoreObjectCell`-sized POD cell (the R4 precondition that the arena
    /// can hold the live cell byte-identically; gc-r4.md "R3 (reversible)"). Only the
    /// cell-slot write differs (`copy_nonoverlapping` of `len` bytes vs `ptr::write(Cell)`);
    /// the size class is selected by the caller (`MarkedSpace::allocate_blob`) so this
    /// directory's `cell_size` already accommodates `len`.
    ///
    /// SAFETY (contract C1-C6, marked_block.rs): `src..src+len` is `len` readable bytes
    /// of an initialized POD value (`needs_drop == false`, no destructor); `len <=
    /// self.cell_size`; single mutator thread; the fresh slot is atom-aligned, never
    /// before handed out, and its FreeCell link bytes were decoded (advance) before the
    /// cell was handed out, so the raw byte copy aliases no live cell and forms no reference.
    pub(crate) unsafe fn allocate_blob(
        &mut self,
        src: *const u8,
        len: usize,
    ) -> (CellPtr, Option<usize>) {
        debug_assert!(
            len <= self.cell_size,
            "blob ({len}) exceeds this directory's size class ({})",
            self.cell_size
        );
        let mut new_base = None;
        // SAFETY: as `allocate` — the FreeList's intervals reference live, exposed pages
        // this directory owns; single mutator (C5/C6).
        let cell_addr = match unsafe { self.free_list.allocate() } {
            Some(addr) => addr,
            None => {
                let base = self.add_block();
                new_base = Some(base);
                // SAFETY: the freshly-swept block's FreeList always yields one cell.
                unsafe { self.free_list.allocate() }
                    .expect("fresh-block FreeList must yield a cell")
            }
        };
        // Recover a raw writable byte pointer with the page's exposed provenance and
        // copy the blob in. NO `&MarkedBlock`/`&mut Cell` is formed (contract C4/C5);
        // the raw place copy is the narrowest footprint.
        let dst = ptr::with_exposed_provenance_mut::<u8>(cell_addr);
        // SAFETY (C2,C3,C4): `cell_addr` is a fresh, atom-aligned, never-before-handed-out
        // slot inside a once-exposed page this directory owns; `src` is `len` readable
        // POD bytes; the regions do not overlap (distinct allocations).
        unsafe { ptr::copy_nonoverlapping(src, dst, len) };
        debug_assert_eq!(
            cell_addr & HALF_ALIGNMENT,
            0,
            "MarkedBlock cells are 16-aligned"
        );
        set_newly_allocated(cell_addr);
        (CellPtr::from_addr(cell_addr), new_base)
    }

    /// gc-r4 R4b-sweep — sweep EVERY block in this directory into ONE combined FreeList
    /// (`DoesNotHave`: a full collection, so post-sweep liveness == marks alone and the
    /// newlyAllocated bitmap is reset). Blocks are swept in DESCENDING base order so every
    /// threaded interval links low->high (a positive offset), matching the single-block
    /// invariant. After this returns, the directory's `allocate`/`allocate_blob` fast path
    /// reuses every reclaimed cell across all blocks before adding a fresh one — so the
    /// arena stays bounded by the live working set (faithful to JSC reusing swept blocks,
    /// BlockDirectory::findBlockForAllocation, without the per-block lazy-sweep machinery).
    ///
    /// CALLER CONTRACT (gc-r4 R4b ORDERING): the store's pre-sweep reconciliation
    /// (`reconcile_dead_cells_before_sweep`) MUST have already read every dead cell's
    /// out-of-line slab handles, because this writes `FreeCell` link records over those
    /// dead cells (clobbering the butterfly slot at offset 8) — see `MarkedSpace::
    /// sweep_all_object_blocks`.
    pub(crate) fn sweep_all_blocks(&mut self) -> SweepResult {
        // Descending base order keeps every cross-block link offset positive (see
        // `FreeList::sweep_blocks`). Clone the bases so `&mut self.free_list` is free.
        let mut bases = self.block_base_addr.clone();
        bases.sort_unstable_by(|a, b| b.cmp(a));
        // SAFETY (contract C1-C6): each base is a registered, once-exposed, directory-owned
        // block; the collector is stopped (single mutator); FreeCell records land only in
        // dead cells. FREELIST_SECRET keys the rebuild — the SAME constant the directory
        // always uses (`add_block`), so the FreeList descrambles its own records.
        unsafe {
            self.free_list
                .sweep_blocks(&bases, FREELIST_SECRET, NewlyAllocatedMode::DoesNotHave)
        }
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
    use crate::gc::heap::free_list::NewlyAllocatedMode;
    use crate::gc::heap::marked_block::{
        cell_ptr, is_marked, is_newly_allocated, test_and_set_marked, ATOMS_PER_BLOCK,
        ATOMS_PER_CELL, BLOCK_MASK,
    };
    use std::collections::HashSet;

    /// The demo cell IS POD (gc-r4.md): `needs_drop == false` is exactly what makes
    /// the `DoesNotNeedDestruction` sweep legal — no destructor to run on reclaim.
    const _: () = assert!(!core::mem::needs_drop::<Cell>());

    /// Cells that fit in one block of the 80B demo size class (after the front slop).
    fn per_block_cells() -> usize {
        (ATOMS_PER_BLOCK - start_atom_for(ATOMS_PER_CELL)) / ATOMS_PER_CELL
    }

    /// Fill EXACTLY one block (block 0) of `dir` with demo cells; return their
    /// addresses by allocation index. Asserts only the first alloc grows the
    /// directory (the rest land in block 0).
    fn fill_one_block(dir: &mut BlockDirectory) -> Vec<usize> {
        let per_block = per_block_cells();
        let mut cells = Vec::with_capacity(per_block);
        for i in 0..per_block {
            let (cp, new_base) = dir.allocate(Cell::new(0x10, 0xC000 + i as u64));
            assert_eq!(
                new_base.is_some(),
                i == 0,
                "exactly one block holds per_block cells"
            );
            cells.push(cp.addr());
        }
        let base = dir.block_base_addr[0];
        for &a in &cells {
            assert_eq!(a & BLOCK_MASK, base, "all demo cells live in block 0");
        }
        cells
    }

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

    /// GAP B (gc-r4.md) — the MarkedBlock SWEEP, end to end. Fill one block, mark a
    /// subset, sweep the FreeList over it, and prove the faithful specializedSweep
    /// contract for `DoesNotNeedDestruction`: every UNMARKED cell is reclaimed to the
    /// FreeList and re-allocatable; every MARKED cell is retained untouched and never
    /// re-handed-out; the counts are exact; the state flip cleared newlyAllocated.
    /// The even/odd mark pattern isolates each dead cell between two live ones, so the
    /// sweep threads MANY single-cell intervals (exercises `setNext` chaining).
    #[test]
    fn sweep_reclaims_unmarked_rebuilds_free_list_and_re_allocates() {
        let mut dir = BlockDirectory::new(ATOMS_PER_CELL); // 80B demo class
        let cells = fill_one_block(&mut dir);
        let base = dir.block_base_addr[0];

        // Mark even-indexed cells (live); odd-indexed are dead.
        let mut marked = HashSet::new();
        let mut unmarked = HashSet::new();
        for (i, &a) in cells.iter().enumerate() {
            if i % 2 == 0 {
                assert!(test_and_set_marked(a), "fresh mark sets the bit");
                marked.insert(a);
            } else {
                unmarked.insert(a);
            }
        }

        // Full-collection sweep. A fresh per-sweep secret (distinct from the block-
        // creation secret) proves the sweep's interval encoding is self-consistent.
        let sweep_secret = 0x0F0F_0F0F_5A5A_5A5A;
        // SAFETY: block 0 is a registered, once-exposed page this directory owns; no
        // mutator &mut is live; dead cells alias no live Cell.
        let res = unsafe {
            dir.free_list
                .sweep_block(base, sweep_secret, NewlyAllocatedMode::DoesNotHave)
        };
        assert_eq!(res.retained_cells, marked.len());
        assert_eq!(res.freed_cells, unmarked.len());
        assert_eq!(
            res.freed_bytes as usize,
            unmarked.len() * (ATOMS_PER_CELL * ATOM_SIZE)
        );

        // STATE FLIP + retention: newlyAllocated cleared block-wide; marks kept; marked
        // cells' bytes preserved (the sweep never touched a live cell).
        for (i, &a) in cells.iter().enumerate() {
            assert!(!is_newly_allocated(a), "newlyAllocated cleared by the flip");
            if i % 2 == 0 {
                assert!(is_marked(a), "marked (retained) cell keeps its mark bit");
                // SAFETY: `a` is a live, retained cell in the once-exposed block.
                let f = unsafe { ptr::addr_of!((*cell_ptr(a)).field0).read() };
                assert_eq!(f, 0xC000 + i as u64, "marked cell bytes survive the sweep");
            }
        }

        // Re-allocate exactly freed_cells cells: each MUST be a reclaimed (unmarked)
        // cell, never a retained (marked) one, and NO new block is created.
        let mut reclaimed = HashSet::new();
        for _ in 0..res.freed_cells {
            let (cp, new_base) = dir.allocate(Cell::new(0x11, 0));
            assert!(
                new_base.is_none(),
                "reclaimed cells satisfy alloc — no new block"
            );
            let a = cp.addr();
            assert!(
                unmarked.contains(&a),
                "re-allocated an unmarked (reclaimed) cell"
            );
            assert!(
                !marked.contains(&a),
                "never re-allocated a marked (retained) cell"
            );
            assert!(reclaimed.insert(a), "each reclaimed cell handed out once");
        }
        assert_eq!(
            reclaimed, unmarked,
            "every unmarked cell reclaimed exactly once"
        );
        assert!(
            dir.free_list.allocation_will_fail(),
            "block fully re-consumed after reclaim"
        );
    }

    /// Sweep edge cases: a CONTIGUOUS dead run (one multi-cell interval), a FULLY-LIVE
    /// block (frees nothing -> always-fail FreeList), and a FULLY-DEAD block (one
    /// whole-payload interval -> every cell reclaimed).
    #[test]
    fn sweep_contiguous_run_fully_live_and_fully_dead() {
        // (a) Mark a PREFIX; the dead suffix is ONE contiguous interval.
        {
            let mut dir = BlockDirectory::new(ATOMS_PER_CELL);
            let cells = fill_one_block(&mut dir);
            let base = dir.block_base_addr[0];
            let live_prefix = cells.len() / 3;
            for &a in &cells[..live_prefix] {
                test_and_set_marked(a);
            }
            // SAFETY: registered once-exposed block; no live mutator &mut.
            let res = unsafe {
                dir.free_list
                    .sweep_block(base, 0x1234_5678, NewlyAllocatedMode::DoesNotHave)
            };
            assert_eq!(res.retained_cells, live_prefix);
            assert_eq!(res.freed_cells, cells.len() - live_prefix);
            let dead_suffix: HashSet<_> = cells[live_prefix..].iter().copied().collect();
            for _ in 0..res.freed_cells {
                let (cp, n) = dir.allocate(Cell::new(0x11, 0));
                assert!(n.is_none());
                assert!(
                    dead_suffix.contains(&cp.addr()),
                    "reclaimed from the dead suffix"
                );
            }
            assert!(dir.free_list.allocation_will_fail());
        }

        // (b) Fully-live block: sweep frees nothing, FreeList is always-fail.
        {
            let mut dir = BlockDirectory::new(ATOMS_PER_CELL);
            let cells = fill_one_block(&mut dir);
            let base = dir.block_base_addr[0];
            for &a in &cells {
                test_and_set_marked(a);
            }
            // SAFETY: as above.
            let res = unsafe {
                dir.free_list
                    .sweep_block(base, 0x5678_1234, NewlyAllocatedMode::DoesNotHave)
            };
            assert_eq!(res.freed_cells, 0);
            assert_eq!(res.retained_cells, cells.len());
            assert!(
                dir.free_list.allocation_will_fail(),
                "fully-live block yields the always-fail FreeList"
            );
        }

        // (c) Fully-dead block (nothing marked): every cell reclaimed, one interval.
        {
            let mut dir = BlockDirectory::new(ATOMS_PER_CELL);
            let cells: HashSet<_> = fill_one_block(&mut dir).into_iter().collect();
            let base = dir.block_base_addr[0];
            // SAFETY: as above.
            let res = unsafe {
                dir.free_list
                    .sweep_block(base, 0x9ABC_DEF0, NewlyAllocatedMode::DoesNotHave)
            };
            assert_eq!(res.freed_cells, cells.len());
            assert_eq!(res.retained_cells, 0);
            let mut reclaimed = HashSet::new();
            for _ in 0..res.freed_cells {
                let (cp, n) = dir.allocate(Cell::new(0x11, 0));
                assert!(n.is_none());
                reclaimed.insert(cp.addr());
            }
            assert_eq!(
                reclaimed, cells,
                "every cell reclaimed after a full-dead sweep"
            );
        }
    }

    /// Eden-mode sweep (`NewlyAllocatedMode::Has`): newlyAllocated cells are live even
    /// when unmarked, so an eden sweep retains them all and keeps the alloc bitmap.
    #[test]
    fn sweep_eden_mode_retains_newly_allocated() {
        let mut dir = BlockDirectory::new(ATOMS_PER_CELL);
        let cells = fill_one_block(&mut dir);
        let base = dir.block_base_addr[0];
        for &a in &cells {
            assert!(is_newly_allocated(a), "alloc set the newlyAllocated bit");
        }
        // SAFETY: registered once-exposed block; no live mutator &mut.
        let res = unsafe {
            dir.free_list
                .sweep_block(base, 0x0EDE_0EDE, NewlyAllocatedMode::Has)
        };
        assert_eq!(
            res.retained_cells,
            cells.len(),
            "eden retains every newlyAllocated cell"
        );
        assert_eq!(res.freed_cells, 0);
        assert!(dir.free_list.allocation_will_fail());
        for &a in &cells {
            assert!(
                is_newly_allocated(a),
                "eden sweep preserves the alloc bitmap"
            );
        }
    }

    /// gc-r4 R4b-sweep — `sweep_all_blocks` reclaims dead cells across MANY blocks into ONE
    /// combined FreeList, so re-allocation reuses them (spanning blocks) before adding a
    /// fresh block. Fill TWO blocks, mark a scattered subset in BOTH, sweep all blocks, then
    /// prove every dead cell (in either block) is re-allocatable with NO new block created —
    /// the multi-block generalization the single-block sweep cannot show.
    #[test]
    fn sweep_all_blocks_reclaims_across_blocks_into_one_free_list() {
        let mut dir = BlockDirectory::new(ATOMS_PER_CELL);
        let per_block = per_block_cells();
        // Fill EXACTLY two full blocks so every walked slot is an allocated cell (no
        // never-allocated tail), keeping the retained/freed counts exact against my marks.
        let total = 2 * per_block;
        let mut cells = Vec::with_capacity(total);
        for i in 0..total {
            let (cp, _n) = dir.allocate(Cell::new(0x10, 0xD000 + i as u64));
            cells.push(cp.addr());
        }
        assert_eq!(
            dir.block_base_addr.len(),
            2,
            "the population fills exactly two blocks"
        );

        // Mark every 3rd cell live (scattered across BOTH blocks); the rest are dead.
        let mut marked = HashSet::new();
        let mut unmarked = HashSet::new();
        for (i, &a) in cells.iter().enumerate() {
            if i % 3 == 0 {
                assert!(test_and_set_marked(a));
                marked.insert(a);
            } else {
                unmarked.insert(a);
            }
        }

        let res = dir.sweep_all_blocks();
        assert_eq!(
            res.retained_cells,
            marked.len(),
            "only marked cells retained"
        );
        assert_eq!(res.freed_cells, unmarked.len(), "every unmarked cell freed");

        // Re-allocate exactly freed_cells cells: each MUST be a reclaimed (unmarked) cell
        // from EITHER block, never a retained one, and NO new block is created (the combined
        // free list spans both blocks).
        let blocks_before = dir.block_base_addr.len();
        let mut reclaimed = HashSet::new();
        let mut blocks_touched = HashSet::new();
        for _ in 0..res.freed_cells {
            let (cp, new_base) = dir.allocate(Cell::new(0x11, 0));
            assert!(
                new_base.is_none(),
                "reclaimed cells satisfy alloc — no new block"
            );
            let a = cp.addr();
            assert!(
                unmarked.contains(&a),
                "re-allocated a reclaimed (unmarked) cell"
            );
            assert!(!marked.contains(&a), "never re-allocated a retained cell");
            assert!(reclaimed.insert(a), "each reclaimed cell handed out once");
            blocks_touched.insert(a & BLOCK_MASK);
        }
        assert_eq!(
            reclaimed, unmarked,
            "every unmarked cell reclaimed exactly once"
        );
        assert!(
            blocks_touched.len() >= 2,
            "reclaimed cells span both blocks (one combined free list)"
        );
        assert_eq!(
            dir.block_base_addr.len(),
            blocks_before,
            "no new block was added during reclaim reuse"
        );
        assert!(
            dir.free_list.allocation_will_fail(),
            "the combined free list is fully re-consumed"
        );

        // Retained cells kept their marks and had newlyAllocated cleared by the flip.
        for (i, &a) in cells.iter().enumerate() {
            if i % 3 == 0 {
                assert!(is_marked(a), "retained cell keeps its mark");
                assert!(
                    !is_newly_allocated(a),
                    "full-collection flip cleared newlyAllocated"
                );
            }
        }
    }
}
