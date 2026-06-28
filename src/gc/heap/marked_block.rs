//! MarkedBlock: the raw, block-aligned 16KB page that backs a MarkedSpace size
//! class (heap/MarkedBlock.h). This module owns the S4 UNSAFE CORE — the
//! provenance / aliasing contract proven miri-clean (Stacked + Tree Borrows,
//! symbolic alignment) in the standalone prototype
//! `tools/s4_arena_proto/src/lib.rs`. The alloc/deref/provenance/contract here is
//! a FAITHFUL match to that proven core; only bookkeeping (the production
//! validity gate, size classes, module boundaries, type names) is added by R1.
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

#![allow(dead_code)]
#![allow(clippy::missing_safety_doc)]

use core::cell::UnsafeCell;
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};
use std::alloc::Layout;

// ===================== JSC MarkedSpace constants =====================
// heap/MarkedBlock.h:77,80,84,86
pub(crate) const ATOM_SIZE: usize = 16; // MarkedBlock::atomSize
pub(crate) const BLOCK_SIZE: usize = 16 * 1024; // MarkedBlock::blockSize (16KB, power of two)
pub(crate) const BLOCK_MASK: usize = !(BLOCK_SIZE - 1); // MarkedBlock::blockMask
pub(crate) const ATOMS_PER_BLOCK: usize = BLOCK_SIZE / ATOM_SIZE; // 1024 (MarkedBlock::atomsPerBlock)
pub(crate) const MARK_WORDS: usize = ATOMS_PER_BLOCK / 64; // 16 (one bit per atom)
pub(crate) const ATOM_ALIGNMENT_MASK: usize = ATOM_SIZE - 1; // MarkedBlock::atomAlignmentMask (15)
                                                             // heap/MarkedSpace.h:53,56 -- sizeStep == atomSize; classes per step up to 80.
pub(crate) const SIZE_STEP: usize = ATOM_SIZE;
pub(crate) const PRECISE_CUTOFF: usize = 80;
// heap/PreciseAllocation.h:158-159 -- halfAlignment bit distinguishes precise cells.
pub(crate) const HALF_ALIGNMENT: usize = ATOM_SIZE / 2; // 8

// MarkedBlock::Header occupies the first HEADER_ATOMS atoms at offset 0
// (MarkedBlock.h:333-334 offsetOfHeader==0; atoms()==reinterpret_cast<Atom*>(this),
// :469-471). Sized to hold the two per-block BitSets (m_marks + m_newlyAllocated,
// MarkedBlock.h:313-314) plus the size-class fields, then padded up to a whole
// number of atoms (faithful to `static_assert(sizeof(Header) <= headerSize)`,
// MarkedBlock.h:329). DIVERGENCE (R2): the remaining real JSC Header fields
// (m_lock, m_biasedMarkCount, m_markCountBias, m_markingVersion,
// m_newlyAllocatedVersion, m_vm, m_verifierMemo — MarkedBlock.h:259-316) are not
// yet ported, so the exact numeric headerSize/payloadSize/largeCutoff differ from
// C++ until R2 lands the full Header; the size-class ALGORITHM (geometric snap,
// per-step classes, inject 256) is faithful and derives from these constants.
pub(crate) const HEADER_ATOMS: usize = 17; // 272 bytes; holds the Header below
pub(crate) const PAYLOAD_BYTES: usize = BLOCK_SIZE - HEADER_ATOMS * ATOM_SIZE; // 16112
pub(crate) const FIRST_PAYLOAD_ATOM: usize = HEADER_ATOMS; // m_startAtom / firstPayloadRegionAtom
pub(crate) const END_ATOM: usize = ATOMS_PER_BLOCK; // MarkedBlock::endAtom (:332)

pub(crate) const fn round_up(x: usize, to: usize) -> usize {
    (x + to - 1) / to * to
}

// ===================== JSCell header (the JIT/IC load offsets) =====================

/// The fixed 8-byte JSCell header (runtime/JSCell.h:293-302). `repr(C)` pins the
/// offsets the JIT/ICs load by absolute displacement.
///
/// DIVERGENCE — distinct from `gc::cell::JsCellHeader` (gc/cell.rs:567-574). That
/// type is the descriptor skeleton (StructureId / CellType(repr u16) / CellState /
/// CellHeaderFlags) whose doc explicitly says "Exact offsets are not promised".
/// THIS type is the absolute-displacement memory layout the optimizing JIT and
/// ICs load from. The two are deliberately not unified in R1 and not re-exported
/// together; R3 reconciles them. Kept module-scoped so the two `JsCellHeader`
/// names never collide at a shared path.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct JsCellHeader {
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
pub(crate) struct Cell {
    pub header: JsCellHeader,
    pub field0: u64,
    pub field1: u64,
    _inline: [u64; 7], // pad to 80 bytes (size class 80)
}
const _: () = assert!(core::mem::size_of::<Cell>() == 80);
pub(crate) const CELL_BYTES: usize = core::mem::size_of::<Cell>(); // 80
pub(crate) const ATOMS_PER_CELL: usize = round_up(CELL_BYTES, ATOM_SIZE) / ATOM_SIZE; // 5

impl Cell {
    pub(crate) fn new(js_type: u8, payload: u64) -> Self {
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

// ===================== CellPtr (the carried machine address) =====================

/// The bare cell machine address a `JsValue` will carry post-cutover (the C++
/// `JSCell*`: the cell's address IS its identity, runtime/JSCJSValue.h asCell()).
/// A thin-pointer newtype with no provenance of its own; provenance is recovered
/// at deref from the per-page once-exposed allocation (contract C2/C3).
///
/// NOT YET PLUMBED into `value::repr` — R4 does the JsValue cutover. R1 defines it
/// so allocate/find/deref already speak in cell addresses, not `CellId`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CellPtr(usize);

impl CellPtr {
    #[inline]
    pub(crate) fn from_addr(addr: usize) -> Self {
        CellPtr(addr)
    }

    #[inline]
    pub(crate) fn addr(self) -> usize {
        self.0
    }
}

// ===================== MarkedBlock (raw aligned page) =====================

/// MarkedBlock::Header (heap/MarkedBlock.h:259-316). Mark bits are atomic words,
/// faithful to concurrent `testAndSetMarked` (:633-637): the collector flips them
/// through a shared/raw access path while the cell payload stays untouched.
/// `newly_allocated` mirrors `m_newlyAllocated` (:314), the mutator-phase liveness
/// bitmap set at bump allocation (setNewlyAllocated, :366).
#[repr(C)]
pub(crate) struct Header {
    pub marks: [AtomicU64; MARK_WORDS],           // m_marks (:313)
    pub newly_allocated: [AtomicU64; MARK_WORDS], // m_newlyAllocated (:314) — the alloc bitmap
    pub atoms_per_cell: u16,                      // cellSize/atomSize for this block's size class
    pub start_atom: u16,                          // m_startAtom (firstPayloadRegionAtom)
    _pad: [u8; HEADER_ATOMS * ATOM_SIZE - 2 * MARK_WORDS * 8 - 4],
}
const _: () = assert!(core::mem::size_of::<Header>() == HEADER_ATOMS * ATOM_SIZE);

/// MarkedBlock (heap/MarkedBlock.h:64). Used ONLY as a layout/offset DESCRIPTOR
/// for the raw aligned page the arena owns; the arena never forms a
/// `&MarkedBlock`/`&mut MarkedBlock` to it (contract C1). Block-aligned so
/// `addr & BLOCK_MASK` recovers the block base from any interior cell address
/// (blockFor, MarkedBlock.h:489-492).
#[repr(C, align(16384))]
pub(crate) struct MarkedBlock {
    pub header: Header,
    payload: [u8; PAYLOAD_BYTES],
}
const _: () = assert!(core::mem::size_of::<MarkedBlock>() == BLOCK_SIZE);
const _: () = assert!(core::mem::align_of::<MarkedBlock>() == BLOCK_SIZE);

pub(crate) fn block_layout() -> Layout {
    Layout::from_size_align(BLOCK_SIZE, BLOCK_SIZE).unwrap()
}

/// Recover the interior-mutable pointer to a cell slot from its machine address
/// (contract C3 + C4). This is the SOLE access path to cell bytes; mutator and
/// collector both go through it. The slot is modeled as `UnsafeCell<Cell>`
/// (interior mutability at cell granularity): forming `&UnsafeCell<Cell>` from
/// the page's exposed provenance does NOT freeze the interior, and `.get()`
/// yields a pointer that may read AND write. No `&MarkedBlock` is ever formed.
#[inline]
pub(crate) fn cell_ptr(cell_addr: usize) -> *mut Cell {
    let uc: *const UnsafeCell<Cell> = ptr::with_exposed_provenance::<u8>(cell_addr).cast();
    // SAFETY (C3,C4): cell_addr lies inside an exposed page allocation and holds
    // an initialized-on-alloc UnsafeCell<Cell>. `&*uc` is a shared ref to an
    // UnsafeCell (interior stays mutable); `.get()` is the interior-mutable ptr.
    unsafe { (*uc).get() }
}

// ===================== blockFor / atom math (no deref) =====================

/// MarkedBlock::blockFor (heap/MarkedBlock.h:489-492): mask any interior cell
/// address down to its block base. Pure arithmetic; recovers no provenance.
#[inline]
pub(crate) fn block_for(addr: usize) -> usize {
    addr & BLOCK_MASK
}

/// MarkedBlock::isAtomAligned (heap/MarkedBlock.h:474-476): `!(p & atomAlignmentMask)`.
#[inline]
pub(crate) fn is_atom_aligned(addr: usize) -> bool {
    addr & ATOM_ALIGNMENT_MASK == 0
}

/// MarkedBlock::candidateAtomNumber (heap/MarkedBlock.h:579-584): `(p - this)/atomSize`.
#[inline]
pub(crate) fn candidate_atom_number(base: usize, addr: usize) -> usize {
    (addr - base) / ATOM_SIZE
}

// ===================== isAtom / liveness (header deref via exposed provenance) =====================
//
// Each function below recovers the block header from `addr & BLOCK_MASK` via the
// page's once-exposed allocation provenance (contract C2/C3). They are sound ONLY
// when the caller has already confirmed the base is a registered block
// (MarkedBlockSet::contains in the find() gate) — exactly the HeapUtil ordering
// (heap/HeapUtil.h:79 set.contains BEFORE :85 isLiveCell). All reads use
// `addr_of!` + read()/load(), forming NO reference to the MarkedBlock (C5).

/// MarkedBlock::isAtom (heap/MarkedBlock.h:664-676): the address sits at a valid
/// cell start — in [startAtom, endAtom) and aligned to a whole number of cells
/// (rejects pointers into the middle of a cell).
pub(crate) fn is_atom(cell_addr: usize) -> bool {
    let base = block_for(cell_addr);
    let atom_number = candidate_atom_number(base, cell_addr);
    let bp: *const MarkedBlock = ptr::with_exposed_provenance::<u8>(base).cast();
    // SAFETY (C3): `base` is a registered, once-exposed page (the caller proved
    // set.contains); addr_of! reads of the header form no reference.
    let (start_atom, atoms_per_cell) = unsafe {
        (
            ptr::addr_of!((*bp).header.start_atom).read() as usize,
            ptr::addr_of!((*bp).header.atoms_per_cell).read() as usize,
        )
    };
    if atom_number < start_atom || atom_number >= END_ATOM {
        return false;
    }
    (atom_number - start_atom) % atoms_per_cell == 0 // filters cell-middle pointers
}

/// MarkedBlock::Handle::isLiveCell (heap/MarkedBlockInlines.h:192-197): isAtom then
/// isLive. R1 single-STW liveness (contract C5/C6): a cell is live if it is
/// newlyAllocated OR marked (the version-stable case of
/// heap/MarkedBlockInlines.h:178-189). DIVERGENCE (R2): HeapVersion staleness
/// (markingVersion / newlyAllocatedVersion fencing, :155-189) is deferred; the
/// single-mutator STW horizon treats versions as never stale.
pub(crate) fn is_live_cell(cell_addr: usize) -> bool {
    if !is_atom(cell_addr) {
        return false;
    }
    let base = block_for(cell_addr);
    let atom = candidate_atom_number(base, cell_addr);
    let bp: *const MarkedBlock = ptr::with_exposed_provenance::<u8>(base).cast();
    let word = atom / 64;
    let bit = 1u64 << (atom % 64);
    // SAFETY (C3): registered, once-exposed page; atomic reads via addr_of! (no ref).
    unsafe {
        let na = (*ptr::addr_of!((*bp).header.newly_allocated[word])).load(Ordering::Relaxed);
        if na & bit != 0 {
            return true;
        }
        let mk = (*ptr::addr_of!((*bp).header.marks[word])).load(Ordering::Relaxed);
        mk & bit != 0
    }
}

/// MarkedBlock::setNewlyAllocated (heap/MarkedBlock.h:366): mark the freshly
/// bump-allocated cell live for the mutator phase (m_newlyAllocated, :314).
pub(crate) fn set_newly_allocated(cell_addr: usize) {
    let base = block_for(cell_addr);
    let atom = candidate_atom_number(base, cell_addr);
    let bp: *const MarkedBlock = ptr::with_exposed_provenance::<u8>(base).cast();
    let word = atom / 64;
    let bit = 1u64 << (atom % 64);
    // SAFETY (C3): cell_addr lies in a registered, once-exposed page; atomic
    // fetch_or via addr_of! forms no reference.
    unsafe {
        (*ptr::addr_of!((*bp).header.newly_allocated[word])).fetch_or(bit, Ordering::Relaxed);
    }
}

/// MarkedBlock::testAndSetMarked (heap/MarkedBlock.h:633-637): `blockFor` masking
/// + the per-block atomic mark word (:489-492,586-592). Returns true if this call
/// set the bit (it was previously clear).
pub(crate) fn test_and_set_marked(cell_addr: usize) -> bool {
    let block_base = block_for(cell_addr); // blockFor (:489-492)
    let atom_number = candidate_atom_number(block_base, cell_addr); // atomNumber (:586-592)
    let bp: *const MarkedBlock = ptr::with_exposed_provenance::<u8>(block_base).cast();
    let word = atom_number / 64;
    let mask = 1u64 << (atom_number % 64);
    // SAFETY (C3): bp points at the registered block header; marks are atomic
    // (interior mutable), so a shared/raw access path is sound. addr_of! forms no ref.
    let prev =
        unsafe { (*ptr::addr_of!((*bp).header.marks[word])).fetch_or(mask, Ordering::Relaxed) };
    (prev & mask) == 0
}

/// MarkedBlock::isMarked (heap/MarkedBlock.h:613-616) — version-stable read.
pub(crate) fn is_marked(cell_addr: usize) -> bool {
    let block_base = block_for(cell_addr);
    let atom_number = candidate_atom_number(block_base, cell_addr);
    let bp: *const MarkedBlock = ptr::with_exposed_provenance::<u8>(block_base).cast();
    let word = atom_number / 64;
    let mask = 1u64 << (atom_number % 64);
    // SAFETY (C3): atomic read through the recovered registered block header.
    let cur = unsafe { (*ptr::addr_of!((*bp).header.marks[word])).load(Ordering::Relaxed) };
    (cur & mask) != 0
}
