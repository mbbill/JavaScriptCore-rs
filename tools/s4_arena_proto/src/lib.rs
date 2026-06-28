//! S4 arena soundness prototype -- the de-risking gate for the irreversible
//! "JSCell identity IS its raw machine address" cutover.
//!
//! WHAT THIS PROVES (the crux): a Heap-owned arena where
//!   (a) cells live in raw, never-moved, never-realloc'd backing memory the
//!       arena owns, with interior mutability at CELL granularity (UnsafeCell);
//!   (b) the cell's machine address is EXPOSED ONCE at allocation
//!       (`ptr::expose_provenance`) and carried in the value as a bare integer;
//!   (c) the typed deref RECOVERS it with valid provenance
//!       (`ptr::with_exposed_provenance`) and reads/writes through the arena's
//!       interior-mutable backing under a WRITTEN aliasing contract;
//! and this is miri-CLEAN under BOTH Stacked Borrows and Tree Borrows.
//!
//! FAITHFUL TO C++ JSC (source of truth, cited at each site):
//!   - MarkedBlock: 16KB page, 16-byte atoms, block-aligned, header at offset 0,
//!     per-block mark BitSet in the header reached by `blockFor` = `p & blockMask`
//!     (heap/MarkedBlock.h:77,80,84,86,313,489-492,586-592,633-637).
//!   - MarkedSpace size classes: sizeStep==atomSize==16, preciseCutoff=80,
//!     optimalSizeFor rounds up to atomSize below the cutoff
//!     (heap/MarkedSpace.h:53,56,265-269).
//!   - BlockDirectory owns one size class' blocks as a vector of POINTERS
//!     (Vector<MarkedBlock::Handle*>, heap/BlockDirectory.h:171); the 16KB blocks
//!     come from an AlignedMemoryAllocator and NEVER move, so growing the
//!     directory relocates only 8-byte owning pointers.
//!   - PreciseAllocation for large cells: one cell behind a prepended header,
//!     distinguished by the halfAlignment (=atomSize/2=8) bit SET on the cell
//!     address: `isPreciseAllocation(cell) = cell & 8`
//!     (heap/PreciseAllocation.h:68-71,158-159).
//!   - JSCell header is a fixed 8 bytes: m_structureID@0 (u32), then a blob union
//!     m_indexingTypeAndMisc@4 / m_type@5 / m_flags@6 / m_cellState@7
//!     (runtime/JSCell.h:293-302). These are the offsets the JIT/ICs load by
//!     absolute displacement. The cell's address IS its identity and the value
//!     carries a raw JSCell* (runtime/JSCJSValue.h: asCell()/isCell mask test).
//!
//! ============================ ALIASING CONTRACT ============================
//! (documented verbatim again at every unsafe deref site)
//!
//! C1. SOLE OWNER / NO co-owning reference. Backing memory is raw aligned pages
//!     the arena owns as `*mut u8` (faithful to AlignedMemoryAllocator; the JSC
//!     header is PLACED INTO raw memory, MarkedBlock is never a managed object).
//!     The arena NEVER forms `&MarkedBlock`/`&mut MarkedBlock` over the payload,
//!     and NEVER wraps a cell in `Box`/`Pin<Box>`. A `Box`/`Pin<Box>` would be a
//!     Unique (noalias) co-owner: minting a `&mut` through it retags the whole
//!     allocation and pops/Disables the carried raw pointer's tag -- the EXACT
//!     S2 UB (see bin/s2_ub.rs). Eliminating that second owning path is the
//!     load-bearing fix.
//!
//! C2. EXPOSE ONCE at allocation. When a page is created, its whole-page
//!     allocation provenance is exposed exactly once (`expose_provenance`).
//!     Because a page is ONE allocation, exposing the base makes EVERY interior
//!     address recoverable with valid provenance: cells (payload) AND the
//!     per-block mark words (header reached by `blockFor` masking).
//!
//! C3. RECOVER at deref. The deref re-derives provenance from the carried
//!     integer (`with_exposed_provenance`). At runtime (non-miri) expose/recover
//!     compile to plain int<->ptr casts (no-ops) -> zero cost, faithful machine
//!     code. miri tracks provenance so soundness is checkable.
//!
//! C4. INTERIOR MUTABILITY at cell granularity. A cell slot is reached as
//!     `*const UnsafeCell<Cell>` via the page's exposed provenance, then `.get()`
//!     yields the read/write pointer. Forming `&UnsafeCell<Cell>` does NOT freeze
//!     the interior, so writing through `.get()` is sound and re-derivation never
//!     invalidates a sibling cell.
//!
//! C5. TEMPORAL DISCIPLINE (single-thread STW horizon). The MUTATOR forms
//!     `&mut Cell` ONLY between safepoints, in minimal lexical scope, dropped
//!     before the next safepoint/collector entry; one mutator thread; never two
//!     overlapping `&mut` to one cell. The COLLECTOR forms `&Cell` (and flips the
//!     atomic mark word) ONLY at stop-the-world, when the mutator holds no `&mut`.
//!     For JIT-style field access prefer `addr_of!`/`addr_of_mut!` + read()/write()
//!     (forms NO reference -> narrowest miri footprint).
//!
//! C6. !Send + !Sync (PhantomData<*const ()>) on the arena/blocks until a
//!     concurrent collector exists, so the mutator-&mut and collector-& windows
//!     can never overlap across threads. STW-only contract horizon: a future
//!     concurrent/generational collector must migrate concurrently-touched fields
//!     to atomics (cellState/marks already are) before relaxing C5.
//!
//! STANDING CONSTRAINT: this design is a permissive-provenance model (the value
//! carries a bare address, exactly the C++ semantics). It can never be validated
//! under -Zmiri-strict-provenance; -Zmiri-permissive-provenance is mandatory.
//! ==========================================================================

#![allow(clippy::missing_safety_doc)]

use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ptr;
use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::collections::HashMap;

// ===================== JSC MarkedSpace constants =====================
// heap/MarkedBlock.h:77,80,84,86
const ATOM_SIZE: usize = 16; // MarkedBlock::atomSize
const BLOCK_SIZE: usize = 16 * 1024; // MarkedBlock::blockSize (16KB, power of two)
const BLOCK_MASK: usize = !(BLOCK_SIZE - 1); // MarkedBlock::blockMask
const ATOMS_PER_BLOCK: usize = BLOCK_SIZE / ATOM_SIZE; // 1024
const MARK_WORDS: usize = ATOMS_PER_BLOCK / 64; // 16 (one bit per atom)
// heap/MarkedSpace.h:53,56 -- sizeStep == atomSize; classes per step up to 80.
const SIZE_STEP: usize = ATOM_SIZE;
const PRECISE_CUTOFF: usize = 80;
// heap/PreciseAllocation.h:158-159 -- halfAlignment bit distinguishes precise cells.
const HALF_ALIGNMENT: usize = ATOM_SIZE / 2; // 8

// Header occupies the first HEADER_ATOMS atoms at offset 0 (MarkedBlock.h:333-334
// offsetOfHeader==0; atoms()==reinterpret_cast<Atom*>(this), :469-471).
const HEADER_ATOMS: usize = 16; // 256 bytes; comfortably holds the Header below
const PAYLOAD_BYTES: usize = BLOCK_SIZE - HEADER_ATOMS * ATOM_SIZE;
const FIRST_PAYLOAD_ATOM: usize = HEADER_ATOMS; // ~ m_startAtom

/// `optimalSizeFor` for the size-class router (heap/MarkedSpace.h:265-269):
/// below preciseCutoff, round up to atomSize; otherwise -> PreciseAllocation.
fn optimal_size_for(bytes: usize) -> SizeRoute {
    assert!(bytes != 0);
    if bytes <= PRECISE_CUTOFF {
        SizeRoute::Marked(round_up(bytes, SIZE_STEP))
    } else {
        // (geometric size classes between preciseCutoff and largeCutoff are
        // elided in the prototype; the BRANCH to PreciseAllocation is what S4
        // must prove provenance-clean -- see PreciseSpace below.)
        SizeRoute::Precise(round_up(bytes, SIZE_STEP))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SizeRoute {
    Marked(usize),
    Precise(usize),
}

const fn round_up(x: usize, to: usize) -> usize {
    (x + to - 1) / to * to
}

// ===================== JSCell header (the JIT/IC load offsets) =====================

/// The fixed 8-byte JSCell header (runtime/JSCell.h:293-302). repr(C) pins the
/// offsets the JIT/ICs load by absolute displacement.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct JsCellHeader {
    pub structure_id: u32,          // @0  m_structureID (StructureID.h)
    pub indexing_type_and_misc: u8, // @4  m_indexingTypeAndMisc
    pub js_type: u8,                // @5  m_type (JSType; the find()/IC type gate)
    pub flags: u8,                  // @6  m_flags
    pub cell_state: u8,             // @7  m_cellState (CellState.h)
}
const _: () = assert!(core::mem::size_of::<JsCellHeader>() == 8);
const _: () = assert!(core::mem::offset_of!(JsCellHeader, structure_id) == 0);
const _: () = assert!(core::mem::offset_of!(JsCellHeader, js_type) == 5);

/// A demo cell sized to a real JSC size class (80 == preciseCutoff, 5 atoms):
/// 8-byte JSCell header + inline field words. Only the header offsets are
/// load-bearing for the JIT; `field0`/`field1` stand in for inline slots.
#[repr(C)]
pub struct Cell {
    pub header: JsCellHeader,
    pub field0: u64,
    pub field1: u64,
    _inline: [u64; 7], // pad to 80 bytes (size class 80)
}
const _: () = assert!(core::mem::size_of::<Cell>() == 80);
const CELL_BYTES: usize = core::mem::size_of::<Cell>(); // 80
#[cfg_attr(not(test), allow(dead_code))] // used by the #[cfg(test)] size-class math
const ATOMS_PER_CELL: usize = round_up(CELL_BYTES, ATOM_SIZE) / ATOM_SIZE; // 5

impl Cell {
    fn new(js_type: u8, payload: u64) -> Self {
        Cell {
            header: JsCellHeader {
                structure_id: 0,
                indexing_type_and_misc: 0,
                js_type,
                flags: 0,
                cell_state: 1, // DefinitelyWhite-ish (CellState.h)
            },
            field0: payload,
            field1: 0,
            _inline: [0; 7],
        }
    }
}

// ===================== MarkedBlock (raw aligned page) =====================

/// MarkedBlock::Header (heap/MarkedBlock.h:259-316). Mark bits are atomic words,
/// faithful to concurrent `testAndSetMarked` (:633-637): the collector flips them
/// through a shared/raw access path while the cell payload stays untouched.
#[repr(C)]
struct Header {
    marks: [AtomicU64; MARK_WORDS],
    atoms_per_cell: u16, // cellSize/atomSize for this block's size class
    start_atom: u16,     // m_startAtom
    _pad: [u8; HEADER_ATOMS * ATOM_SIZE - MARK_WORDS * 8 - 4],
}

/// MarkedBlock (heap/MarkedBlock.h:64). Used ONLY as a layout/offset DESCRIPTOR
/// for the raw aligned page the arena owns; the arena never forms a
/// `&MarkedBlock`/`&mut MarkedBlock` to it (contract C1). Block-aligned so
/// `addr & BLOCK_MASK` recovers the block base from any interior cell address
/// (blockFor, MarkedBlock.h:489-492).
#[repr(C, align(16384))]
struct MarkedBlock {
    header: Header,
    payload: [u8; PAYLOAD_BYTES],
}
const _: () = assert!(core::mem::size_of::<MarkedBlock>() == BLOCK_SIZE);
const _: () = assert!(core::mem::align_of::<MarkedBlock>() == BLOCK_SIZE);

fn block_layout() -> Layout {
    Layout::from_size_align(BLOCK_SIZE, BLOCK_SIZE).unwrap()
}

/// Recover the interior-mutable pointer to a cell slot from its machine address
/// (contract C3 + C4). This is the SOLE access path to cell bytes; mutator and
/// collector both go through it. The slot is modeled as `UnsafeCell<Cell>`
/// (interior mutability at cell granularity): forming `&UnsafeCell<Cell>` from
/// the page's exposed provenance does NOT freeze the interior, and `.get()`
/// yields a pointer that may read AND write. No `&MarkedBlock` is ever formed.
#[inline]
fn cell_ptr(cell_addr: usize) -> *mut Cell {
    let uc: *const UnsafeCell<Cell> = ptr::with_exposed_provenance::<u8>(cell_addr).cast();
    // SAFETY (C3,C4): cell_addr lies inside an exposed page allocation and holds
    // an initialized-on-alloc UnsafeCell<Cell>. `&*uc` is a shared ref to an
    // UnsafeCell (interior stays mutable); `.get()` is the interior-mutable ptr.
    unsafe { (*uc).get() }
}

// ===================== BlockDirectory (one size class) =====================

/// BlockDirectory (heap/BlockDirectory.h): owns the block list for ONE size class
/// and the bump cursor (FreeList fast path, heap/FreeList.h:82-123). Blocks never
/// move; the Vec holds only the raw owning page pointers (mirrors
/// Vector<MarkedBlock::Handle*>, BlockDirectory.h:171). Growing the Vec moves only
/// 8-byte owning pointers -> cell addresses stay stable and provenance-valid.
struct BlockDirectory {
    cell_size_atoms: usize,
    /// owning raw page pointers (for dealloc on Drop). Never turned into a
    /// `&MarkedBlock`; all block access goes through exposed provenance.
    pages: Vec<*mut u8>,
    /// per-block once-exposed base address (== the page pointer's address).
    block_base_addr: Vec<usize>,
    /// bump cursor into the current (last) block: next free atom index.
    next_atom: usize,
}

impl BlockDirectory {
    fn new(cell_size_atoms: usize) -> Self {
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
        // SAFETY: nonzero, power-of-two-aligned layout; alloc_zeroed gives a fresh
        // page whose allocation-root provenance grants read+write over all
        // BLOCK_SIZE bytes. Zeroing initializes mark words and payload to 0.
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
    /// on overflow), initialize the header through a raw pointer, return the cell's
    /// machine address (its identity, carried in the JsValue).
    fn allocate(&mut self, init: Cell) -> usize {
        if self.pages.is_empty() || self.next_atom + self.cell_size_atoms > ATOMS_PER_BLOCK {
            self.add_block();
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
        debug_assert_eq!(cell_addr & HALF_ALIGNMENT, 0, "MarkedBlock cells are 16-aligned");
        cell_addr
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

// ===================== PreciseAllocation (large cells) =====================

/// PreciseAllocation (heap/PreciseAllocation.h): one cell behind a prepended
/// header. The cell address has the halfAlignment bit SET (cell & 8 != 0) so deref
/// dispatch distinguishes it from a 16-aligned MarkedBlock cell. The prototype
/// uses an 8-byte header (JSC rounds headerSize up to 16, :165 -- noted divergence;
/// only the +8 dispatch bit and mask-then-recover are load-bearing here).
#[repr(C)]
struct PreciseHeader {
    marked: AtomicU8, // single-cell mark bit (BitSet not needed for one cell)
    _pad: [u8; 7],
}
const _: () = assert!(core::mem::size_of::<PreciseHeader>() == HALF_ALIGNMENT);

struct PreciseSpace {
    /// (owning base ptr, layout) for dealloc on Drop.
    allocations: Vec<(*mut u8, Layout)>,
}

impl PreciseSpace {
    fn new() -> Self {
        PreciseSpace { allocations: Vec::new() }
    }

    /// Allocate one large cell. base (16-aligned) holds the PreciseHeader at
    /// offset 0; the cell starts at base+8 -> cell address is 8-mod-16 (the
    /// halfAlignment bit), faithful to isPreciseAllocation (:68-71). Expose ONCE.
    fn allocate(&mut self, cell_bytes: usize, init: Cell) -> usize {
        let total = HALF_ALIGNMENT + cell_bytes;
        let layout = Layout::from_size_align(total, ATOM_SIZE).unwrap();
        // SAFETY: nonzero, atom-aligned layout; fresh zeroed allocation.
        let raw = unsafe { alloc_zeroed(layout) };
        assert!(!raw.is_null());
        let base = raw.expose_provenance(); // expose whole allocation ONCE (C2)
        let cell_addr = base + HALF_ALIGNMENT; // 8-mod-16
        debug_assert_ne!(cell_addr & HALF_ALIGNMENT, 0, "precise cells carry the +8 bit");
        let cp = cell_ptr(cell_addr);
        // SAFETY (C3,C4): cell_addr is inside the exposed precise allocation.
        unsafe { ptr::write(cp, init) };
        self.allocations.push((raw, layout));
        cell_addr
    }

    /// Recover the precise header (fromCell = cell - headerSize, :165) by masking
    /// off the +8 bit to reach the 16-aligned base, then recover with provenance.
    fn header_ptr(cell_addr: usize) -> *const PreciseHeader {
        let base = cell_addr & !(ATOM_SIZE - 1); // mask to 16-aligned base
        ptr::with_exposed_provenance::<u8>(base).cast()
    }
}

impl Drop for PreciseSpace {
    fn drop(&mut self) {
        for &(raw, layout) in &self.allocations {
            // SAFETY: each (raw,layout) came from one alloc_zeroed; freed once.
            unsafe { dealloc(raw, layout) };
        }
    }
}

// ===================== Arena (Heap-owned, the cutover target) =====================

/// The Heap-owned arena: per-size-class MarkedBlock directories + a PreciseSpace,
/// plus the membership/validity gate that REPLACES the old index map. This is the
/// proven cutover target for interpreter/mod.rs CoreObjectStore (the four
/// Vec<Pin<Box<Core*Cell>>> stores) and gc/heap.rs payload<->CellId maps.
pub struct Arena {
    /// One directory per size class actually used (keyed by cell_size_atoms).
    directories: HashMap<usize, BlockDirectory>,
    precise: PreciseSpace,
    /// VALIDITY GATE (contract: replaces object_indices_by_payload as the
    /// memory-safety membership check). Maps a live cell address -> a DEBUG
    /// borrow flag sidecar. The flag lives OUTSIDE the cell's UnsafeCell (a
    /// separate allocation) so it can never retag the cell (contract risk #4).
    live: HashMap<usize, Box<AtomicU8>>,
    _not_send_sync: PhantomData<*const ()>, // contract C6
}

const BORROW_FREE: u8 = 0;
const BORROW_MUT: u8 = 1;

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

impl Arena {
    pub fn new() -> Self {
        Arena {
            directories: HashMap::new(),
            precise: PreciseSpace::new(),
            live: HashMap::new(),
            _not_send_sync: PhantomData,
        }
    }

    /// Route a size to a directory/precise space (heap/MarkedSpace.h:265-269) and
    /// allocate one cell; expose its address ONCE; record membership. Returns the
    /// machine address the JsValue carries.
    pub fn allocate(&mut self, js_type: u8, payload: u64) -> usize {
        let addr = match optimal_size_for(CELL_BYTES) {
            SizeRoute::Marked(sz) => {
                let atoms = sz / ATOM_SIZE;
                let dir = self
                    .directories
                    .entry(atoms)
                    .or_insert_with(|| BlockDirectory::new(atoms));
                dir.allocate(Cell::new(js_type, payload))
            }
            SizeRoute::Precise(sz) => self.precise.allocate(sz, Cell::new(js_type, payload)),
        };
        self.live.insert(addr, Box::new(AtomicU8::new(BORROW_FREE)));
        addr
    }

    /// Force-route through PreciseAllocation regardless of size (to exercise the
    /// +8 dispatch path in tests).
    pub fn allocate_precise(&mut self, js_type: u8, payload: u64) -> usize {
        let addr = self.precise.allocate(CELL_BYTES, Cell::new(js_type, payload));
        self.live.insert(addr, Box::new(AtomicU8::new(BORROW_FREE)));
        addr
    }

    /// VALIDITY GATE (find()/llint_get_by_id_fast at interpreter/mod.rs:11910-11967
    /// resolve membership BEFORE deref). Called with an ARBITRARY carried address:
    /// returns it only if it is a live arena cell, so deref never touches a
    /// non-arena / dangling pointer. This is the memory-safety gate that the index
    /// map provided and the address model preserves (in production this is the
    /// blockFor-owner / mark-bit liveness check, not a HashMap).
    pub fn find(&self, addr: usize) -> Option<usize> {
        if self.live.contains_key(&addr) {
            Some(addr)
        } else {
            None
        }
    }

    /// Dispatch helper: which space owns this carried address (PreciseAllocation.h
    /// :68-71). MarkedBlock cells are 16-aligned (bit 3 clear); precise cells carry
    /// the +8 bit. Deref MUST dispatch on this BEFORE any field load.
    pub fn is_precise(addr: usize) -> bool {
        addr & HALF_ALIGNMENT != 0
    }

    // ---- MUTATOR (contract C5: &mut only between safepoints, minimal scope) ----

    /// MUTATOR write of field0. Forms `&mut Cell` in MINIMAL scope, between
    /// safepoints, dropped before any other access path runs.
    pub fn mutator_write_field0(&self, addr: usize, v: u64) {
        let _g = self.borrow_mut_guard(addr);
        let cp = cell_ptr(addr);
        // SAFETY (C3,C4,C5): addr validated live; arena is the sole owner/access
        // path (UnsafeCell, no Box/Unique alias); &mut scope is minimal and
        // exclusive; collector is stopped; no two overlapping &mut to one cell.
        unsafe {
            let cell = &mut *cp;
            cell.field0 = v;
        } // &mut dropped here
    }

    pub fn mutator_write_structure_id(&self, addr: usize, sid: u32) {
        let _g = self.borrow_mut_guard(addr);
        let cp = cell_ptr(addr);
        // SAFETY: as mutator_write_field0.
        unsafe {
            let cell = &mut *cp;
            cell.header.structure_id = sid;
        }
    }

    /// JIT-style field read via `addr_of!` -> read() : forms NO reference, the
    /// narrowest possible miri footprint (contract C5). This is the shape the
    /// optimizing JIT emits: `load [base + disp]`.
    pub fn raw_read_field0(&self, addr: usize) -> u64 {
        let cp = cell_ptr(addr);
        // SAFETY (C3,C4): live cell; raw place read forms no reference.
        unsafe { ptr::addr_of!((*cp).field0).read() }
    }

    /// Read the JSType byte (offset 5) the JIT/ICs gate on, via addr_of (no ref).
    pub fn raw_read_js_type(&self, addr: usize) -> u8 {
        let cp = cell_ptr(addr);
        // SAFETY (C3,C4): live cell; raw place read forms no reference.
        unsafe { ptr::addr_of!((*cp).header.js_type).read() }
    }

    pub fn raw_read_structure_id(&self, addr: usize) -> u32 {
        let cp = cell_ptr(addr);
        // SAFETY: as above.
        unsafe { ptr::addr_of!((*cp).header.structure_id).read() }
    }

    // ---- COLLECTOR (contract C5: & only at stop-the-world) ----

    /// COLLECTOR read at stop-the-world: forms `&Cell` (no concurrent mutator &mut).
    pub fn collector_read(&self, addr: usize) -> (u32, u8, u64) {
        let cp = cell_ptr(addr);
        // SAFETY (C3,C4,C5): stop-the-world; no &mut alive; addr is a live cell.
        unsafe {
            let cell = &*cp;
            (cell.header.structure_id, cell.header.js_type, cell.field0)
        }
    }

    /// COLLECTOR mark via `blockFor` masking + the per-block atomic mark word
    /// (MarkedBlock.h:489-492,586-592,633-637). Dispatches precise vs marked on the
    /// +8 bit first. Returns true if this call set the bit (testAndSetMarked).
    pub fn collector_mark(&self, addr: usize) -> bool {
        if Self::is_precise(addr) {
            let hp = PreciseSpace::header_ptr(addr);
            // SAFETY: hp points at the precise allocation's header (atomic field).
            let prev = unsafe { (*ptr::addr_of!((*hp).marked)).swap(1, Ordering::Relaxed) };
            return prev == 0;
        }
        let block_base = addr & BLOCK_MASK; // blockFor (:489-492)
        let atom_number = (addr - block_base) / ATOM_SIZE; // atomNumber (:586-592)
        // Recover the block HEADER from the SAME exposed page allocation (whole
        // page is one allocation, so masking down to base stays in-bounds).
        let bp: *const MarkedBlock = ptr::with_exposed_provenance::<u8>(block_base).cast();
        let word = atom_number / 64;
        let bit = atom_number % 64;
        let mask = 1u64 << bit;
        // SAFETY: bp points at the block header; marks are atomic (interior
        // mutable), so a shared/raw access path is sound. addr_of! forms no ref.
        let prev = unsafe { (*ptr::addr_of!((*bp).header.marks[word])).fetch_or(mask, Ordering::Relaxed) };
        (prev & mask) == 0
    }

    pub fn collector_is_marked(&self, addr: usize) -> bool {
        if Self::is_precise(addr) {
            let hp = PreciseSpace::header_ptr(addr);
            // SAFETY: atomic read through the precise header.
            return unsafe { (*ptr::addr_of!((*hp).marked)).load(Ordering::Relaxed) } != 0;
        }
        let block_base = addr & BLOCK_MASK;
        let atom_number = (addr - block_base) / ATOM_SIZE;
        let bp: *const MarkedBlock = ptr::with_exposed_provenance::<u8>(block_base).cast();
        let word = atom_number / 64;
        let bit = atom_number % 64;
        // SAFETY: atomic read through the recovered block header.
        let cur = unsafe { (*ptr::addr_of!((*bp).header.marks[word])).load(Ordering::Relaxed) };
        (cur & (1u64 << bit)) != 0
    }

    // ---- DEBUG borrow flag (contract C: #[cfg(debug_assertions)] overlap check) ----

    /// On `&mut` entry, set the per-cell DEBUG borrow flag (sidecar AtomicU8 in a
    /// SEPARATE allocation, NOT inside the cell's UnsafeCell). Panics (does NOT form
    /// a conflicting &mut) if the cell is already mutably borrowed -> catches a
    /// careless overlapping/nested &mut WITHOUT itself committing the UB.
    fn borrow_mut_guard(&self, addr: usize) -> MutGuard<'_> {
        let flag: &AtomicU8 = self
            .live
            .get(&addr)
            .expect("mutator deref of non-live address (find() gate violated)");
        if cfg!(debug_assertions) {
            let prev = flag.swap(BORROW_MUT, Ordering::Relaxed);
            assert_eq!(prev, BORROW_FREE, "overlapping &mut to one cell (contract C5)");
        }
        MutGuard { flag }
    }
}

struct MutGuard<'a> {
    flag: &'a AtomicU8,
}
impl Drop for MutGuard<'_> {
    fn drop(&mut self) {
        if cfg!(debug_assertions) {
            self.flag.store(BORROW_FREE, Ordering::Relaxed);
        }
    }
}

/// Models the (now-removed) transitional value carry `(addr << 8) | 0x20`. The
/// shift carries NO provenance (provenance is recovered from the integer via the
/// global exposed set), so dropping the shift in the S4 cutover changes nothing
/// about soundness -- proven by exercising both forms in tests.
pub fn encode_legacy(addr: usize) -> u64 {
    ((addr as u64) << 8) | 0x20
}
pub fn decode_legacy(enc: u64) -> usize {
    (enc >> 8) as usize
}

// ================================== TESTS ==================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Cells-per-block for the demo size class (after the header atoms).
    fn cells_per_block() -> usize {
        (ATOMS_PER_BLOCK - FIRST_PAYLOAD_ATOM) / ATOMS_PER_CELL
    }

    /// THE CRUX TEST. The exact interleaving that made S2 UB:
    ///   1. allocate cell A and MUTATE a field through its carried address;
    ///   2. allocate MANY more cells, GROWING the block list (push new pages ->
    ///      realloc the Vec<*mut u8> of owning pointers) AND the Arena's `live`
    ///      map -- i.e. the operations S2 routed through the Box's Unique &mut;
    ///   3. re-deref A and read/write it again.
    /// In S2 step 2's owner-&mut popped A's carried tag -> deref in step 3 = UB.
    /// Here there is NO co-owning reference, so A's exposed provenance survives.
    #[test]
    fn interleaved_mutate_grow_then_rederef_is_sound() {
        let mut arena = Arena::new();

        // (1) Allocate A in the first block and mutate it.
        let a = arena.allocate(0x10, 0xA000);
        arena.mutator_write_field0(a, 0x1111);
        arena.mutator_write_structure_id(a, 0xABCD);
        assert_eq!(arena.raw_read_field0(a), 0x1111);

        // (2) Allocate enough to force at least TWO new blocks (two Vec growths),
        //     re-deref A after EACH growth to prove provenance stability.
        let per_block = cells_per_block();
        let mut others = Vec::new();
        let mut blocks_seen = std::collections::HashSet::new();
        blocks_seen.insert(a & BLOCK_MASK);
        for i in 0..(per_block * 2 + 3) {
            let b = arena.allocate(0x11, 0xB000 + i as u64);
            // mutate the freshly allocated cell through ITS carried address
            arena.mutator_write_field0(b, 0xB000 + i as u64);
            others.push(b);
            let nb = b & BLOCK_MASK;
            if blocks_seen.insert(nb) {
                // A NEW block was just created (the Vec<*mut u8> grew). Re-deref A
                // immediately: this is the precise S2 interleaving point.
                assert_eq!(arena.raw_read_field0(a), 0x1111, "A field survives growth");
                assert_eq!(arena.raw_read_structure_id(a), 0xABCD);
                arena.mutator_write_field0(a, 0x1111); // write again through A
            }
        }
        assert!(blocks_seen.len() >= 3, "must have grown to >=3 blocks (got {})", blocks_seen.len());

        // (3) Final re-deref of A after all growth: still valid + correct.
        assert_eq!(arena.raw_read_field0(a), 0x1111);
        arena.mutator_write_field0(a, 0x2222);
        assert_eq!(arena.raw_read_field0(a), 0x2222);

        // Every other cell still independently readable (no cross-corruption).
        for (i, &b) in others.iter().enumerate() {
            assert_eq!(arena.raw_read_field0(b), 0xB000 + i as u64);
        }
    }

    /// blockFor masking recovers the right block; collector mark/sweep round-trips
    /// across multiple blocks; mutator re-enters cleanly after a full GC cycle.
    #[test]
    fn marked_space_roundtrip_and_collector() {
        let mut arena = Arena::new();
        let n = cells_per_block() + 7; // force a second block
        let mut addrs = Vec::new();
        for i in 0..n {
            addrs.push(arena.allocate(0x10 + (i as u8 & 7), i as u64));
        }

        // blockFor(cell) must hit a known block base.
        for &a in &addrs {
            assert!(!Arena::is_precise(a));
            let base = a & BLOCK_MASK;
            let dir = arena.directories.get(&ATOMS_PER_CELL).unwrap();
            assert!(dir.block_base_addr.contains(&base), "blockFor must hit a known block");
        }

        // Mutator phase: write+read each cell via its carried address.
        for (i, &a) in addrs.iter().enumerate() {
            arena.mutator_write_field0(a, 0xDEAD_0000 + i as u64);
        }
        for (i, &a) in addrs.iter().enumerate() {
            assert_eq!(arena.raw_read_field0(a), 0xDEAD_0000 + i as u64);
            assert_eq!(arena.raw_read_js_type(a), 0x10 + (i as u8 & 7));
        }

        // Collector phase (STW): mark via masking; idempotent; readable.
        for &a in &addrs {
            assert!(arena.collector_mark(a), "first mark sets the bit");
            assert!(!arena.collector_mark(a), "second mark finds it set");
            assert!(arena.collector_is_marked(a));
            let (_sid, _ty, _f) = arena.collector_read(a);
        }

        // Re-enter mutator after collection: fields still writable (provenance
        // survived a full cycle through the atomic mark words in the header).
        for (i, &a) in addrs.iter().enumerate() {
            arena.mutator_write_field0(a, 0xBEEF_0000 + i as u64);
            assert_eq!(arena.raw_read_field0(a), 0xBEEF_0000 + i as u64);
        }
    }

    /// PreciseAllocation +8 dispatch: a precise cell carries the halfAlignment bit;
    /// deref masks-then-recovers; mark lives in the precise header.
    #[test]
    fn precise_allocation_plus8_dispatch() {
        let mut arena = Arena::new();
        let p = arena.allocate_precise(0x20, 0xC0DE);
        assert!(Arena::is_precise(p), "precise cell must carry the +8 bit");
        assert_eq!(p & HALF_ALIGNMENT, HALF_ALIGNMENT);

        arena.mutator_write_field0(p, 0x9999);
        assert_eq!(arena.raw_read_field0(p), 0x9999);
        assert_eq!(arena.raw_read_js_type(p), 0x20);

        assert!(arena.collector_mark(p));
        assert!(!arena.collector_mark(p));
        assert!(arena.collector_is_marked(p));
        let (_sid, ty, f) = arena.collector_read(p);
        assert_eq!(ty, 0x20);
        assert_eq!(f, 0x9999);

        // Mix marked + precise in one arena; both dispatch correctly.
        let m = arena.allocate(0x10, 1);
        assert!(!Arena::is_precise(m));
        assert!(arena.collector_mark(m));
        assert!(arena.collector_is_marked(m));
        assert!(arena.collector_is_marked(p)); // precise mark independent
    }

    /// The validity gate refuses non-arena / dangling addresses without deref.
    #[test]
    fn find_gate_rejects_non_arena_addresses() {
        let mut arena = Arena::new();
        let a = arena.allocate(0x10, 7);
        assert_eq!(arena.find(a), Some(a));
        assert!(arena.find(0xdead_beef).is_none());
        assert!(arena.find(0).is_none());
        assert!(arena.find(a + 1).is_none()); // interior/misaligned not a live key
    }

    /// The legacy (addr<<8)|0x20 carry is provenance-irrelevant: encode/decode then
    /// deref is identical to carrying the raw address. (Proves S4 dropping the shift
    /// changes nothing about soundness.)
    #[test]
    fn legacy_encoding_is_provenance_irrelevant() {
        let mut arena = Arena::new();
        let a = arena.allocate(0x10, 42);
        let enc = encode_legacy(a);
        let dec = decode_legacy(enc);
        assert_eq!(dec, a);
        let v = arena.find(dec).expect("decoded address validates");
        arena.mutator_write_field0(v, 0x5151);
        assert_eq!(arena.raw_read_field0(v), 0x5151);
    }

    /// DEBUG borrow flag catches an overlapping &mut by PANICKING at the flag check
    /// BEFORE forming the second &mut -> no real UB, just a clean panic. (Only
    /// meaningful with debug_assertions; miri builds keep them on.)
    #[test]
    #[should_panic(expected = "overlapping &mut")]
    fn debug_borrow_flag_detects_overlap() {
        let mut arena = Arena::new();
        let a = arena.allocate(0x10, 1);
        let _g1 = arena.borrow_mut_guard(a); // holds the flag (no live &mut Cell)
        // Second borrow of the SAME cell: the flag check panics before any second
        // &mut is formed. Catches the logical overlap without committing UB.
        let _g2 = arena.borrow_mut_guard(a);
    }
}
