//! PreciseAllocation / PreciseSpace: large cells allocated one-per-allocation
//! behind a prepended header (heap/PreciseAllocation.h). The cell address carries
//! the halfAlignment bit (cell & 8 != 0), which is the deref-dispatch key that
//! distinguishes a precise cell from a 16-aligned MarkedBlock cell
//! (isPreciseAllocation, PreciseAllocation.h:68-71). Faithful port of the proven
//! prototype `PreciseSpace` (tools/s4_arena_proto/src/lib.rs:319-375): the +8
//! dispatch + mask-then-recover unsafe core is byte-for-byte the proven core.

#![allow(dead_code)]

use core::ptr;
use core::sync::atomic::{AtomicU8, Ordering};
use std::alloc::{alloc_zeroed, dealloc, Layout};

use super::marked_block::{cell_ptr, Cell, CellPtr, ATOM_SIZE, HALF_ALIGNMENT};

/// PreciseAllocation header (heap/PreciseAllocation.h:172-180). The prototype/R1
/// header is exactly `halfAlignment` (8) bytes so the cell lands at base+8 and
/// carries the +8 dispatch bit; only that bit and the single mark byte are
/// load-bearing for soundness. DIVERGENCE (R2): the real JSC header
/// (m_indexInSpace, m_cellSize, m_hasValidCell, m_isNewlyAllocated, the
/// BasicRawSentinelNode links, and the roundUpToMultipleOf<16>(sizeof) headerSize
/// at PreciseAllocation.h:165) is deferred; the +8/mask-recover core is unchanged.
#[repr(C)]
struct PreciseHeader {
    marked: AtomicU8, // single-cell mark bit (BitSet not needed for one cell)
    _pad: [u8; 7],
}
const _: () = assert!(core::mem::size_of::<PreciseHeader>() == HALF_ALIGNMENT);

/// PreciseSpace owns the large-cell allocations for dealloc on Drop. Membership of
/// live precise cells is tracked by MarkedSpace's `precise_set`
/// (m_preciseAllocationSet, heap/MarkedSpace.h:163,207); this struct only owns the
/// backing memory.
pub(crate) struct PreciseSpace {
    /// (owning base ptr, layout) for dealloc on Drop.
    allocations: Vec<(*mut u8, Layout)>,
}

impl PreciseSpace {
    pub(crate) fn new() -> Self {
        PreciseSpace {
            allocations: Vec::new(),
        }
    }

    /// Allocate one large cell. base (16-aligned) holds the PreciseHeader at
    /// offset 0; the cell starts at base+8 -> cell address is 8-mod-16 (the
    /// halfAlignment bit), faithful to isPreciseAllocation (:68-71). Expose ONCE.
    pub(crate) fn allocate(&mut self, cell_bytes: usize, init: Cell) -> CellPtr {
        let total = HALF_ALIGNMENT + cell_bytes;
        let layout = Layout::from_size_align(total, ATOM_SIZE).unwrap();
        // SAFETY (C1): nonzero, atom-aligned layout; fresh zeroed allocation owned
        // as a bare `*mut u8` (never Box).
        let raw = unsafe { alloc_zeroed(layout) };
        assert!(!raw.is_null());
        let base = raw.expose_provenance(); // expose whole allocation ONCE (C2)
        let cell_addr = base + HALF_ALIGNMENT; // 8-mod-16
        debug_assert_ne!(
            cell_addr & HALF_ALIGNMENT,
            0,
            "precise cells carry the +8 bit"
        );
        let cp = cell_ptr(cell_addr);
        // SAFETY (C3,C4): cell_addr is inside the exposed precise allocation; the
        // slot is an UnsafeCell<Cell> so the initializing write through `.get()` is sound.
        unsafe { ptr::write(cp, init) };
        self.allocations.push((raw, layout));
        CellPtr::from_addr(cell_addr)
    }

    /// gc-r4 R3 (reversible shadow oracle): like `allocate`, but writes an arbitrary
    /// POD BYTE BLOB into the cell slot instead of the fixed demo `Cell`. The precise
    /// path for an over-largeCutoff cell; the +8 dispatch / mask-recover core is
    /// unchanged. SAFETY: `src..src+len` is `len` readable bytes of an initialized POD
    /// value (`needs_drop == false`); `len <= cell_bytes`; single mutator thread.
    pub(crate) unsafe fn allocate_blob(
        &mut self,
        cell_bytes: usize,
        src: *const u8,
        len: usize,
    ) -> CellPtr {
        debug_assert!(len <= cell_bytes);
        let total = HALF_ALIGNMENT + cell_bytes;
        let layout = Layout::from_size_align(total, ATOM_SIZE).unwrap();
        // SAFETY (C1): nonzero, atom-aligned layout; fresh zeroed allocation owned as a
        // bare `*mut u8` (never Box).
        let raw = unsafe { alloc_zeroed(layout) };
        assert!(!raw.is_null());
        let base = raw.expose_provenance(); // expose whole allocation ONCE (C2)
        let cell_addr = base + HALF_ALIGNMENT; // 8-mod-16
        let dst = ptr::with_exposed_provenance_mut::<u8>(cell_addr);
        // SAFETY (C2,C3,C4): `cell_addr` is inside the just-exposed precise allocation;
        // the raw byte copy forms no reference and aliases no live cell.
        unsafe { ptr::copy_nonoverlapping(src, dst, len) };
        self.allocations.push((raw, layout));
        CellPtr::from_addr(cell_addr)
    }

    /// Recover the precise header (fromCell = cell - headerSize, :58-61,165) by
    /// masking off the +8 bit to reach the 16-aligned base, then recover with
    /// provenance.
    fn header_ptr(cell_addr: usize) -> *const PreciseHeader {
        let base = cell_addr & !(ATOM_SIZE - 1); // mask to 16-aligned base
        ptr::with_exposed_provenance::<u8>(base).cast()
    }

    /// PreciseAllocation mark (heap/PreciseAllocation.h:90 isMarked / flip):
    /// `m_isMarked` is a single relaxed atomic. Returns true if this call set it.
    pub(crate) fn mark(cell_addr: usize) -> bool {
        let hp = Self::header_ptr(cell_addr);
        // SAFETY (C3): hp points at the precise allocation's header (atomic field)
        // recovered from the once-exposed base; addr_of! forms no reference.
        let prev = unsafe { (*ptr::addr_of!((*hp).marked)).swap(1, Ordering::Relaxed) };
        prev == 0
    }

    /// PreciseAllocation::isMarked (heap/PreciseAllocation.h:90).
    pub(crate) fn is_marked(cell_addr: usize) -> bool {
        let hp = Self::header_ptr(cell_addr);
        // SAFETY (C3): atomic read through the recovered precise header.
        unsafe { (*ptr::addr_of!((*hp).marked)).load(Ordering::Relaxed) != 0 }
    }

    /// Clear the precise cell's mark bit (gc-r4 R4b-mark `MarkedSpace::clear_all_marks`
    /// begin-marking step). JSC clears the precise-allocation marks via the
    /// markingVersion bump at full-collection start (heap/MarkedSpace::beginMarking);
    /// the single-STW model resets the single `m_isMarked` byte directly.
    pub(crate) fn clear_mark(cell_addr: usize) {
        let hp = Self::header_ptr(cell_addr);
        // SAFETY (C3): atomic store through the recovered precise header.
        unsafe { (*ptr::addr_of!((*hp).marked)).store(0, Ordering::Relaxed) };
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
