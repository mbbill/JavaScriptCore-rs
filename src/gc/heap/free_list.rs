//! FreeList: the MarkedSpace allocation fast path — a singly-linked list of
//! bump-allocation INTERVALS over a block's free cells (heap/FreeList.h:82-123;
//! heap/FreeListInlines.h:35-55; heap/FreeList.cpp). Faithful port of `FreeList` +
//! `FreeCell`.
//!
//! WHAT JSC DOES (the model this ports):
//!   - A swept MarkedBlock's free cells are coalesced into maximal contiguous
//!     runs ("intervals"). The HEAD cell of each interval stores, IN ITS OWN
//!     BYTES, a `FreeCell` record: `(offsetToNext, lengthInBytes)`, XOR-scrambled
//!     with a per-sweep secret (heap/FreeList.h:39-80). Intermediate cells carry
//!     nothing — the interval is walked by plain pointer bump.
//!   - `allocateWithCellSize` (heap/FreeListInlines.h:35-55): if `intervalStart <
//!     intervalEnd`, return `intervalStart` and bump it by `cellSize`; otherwise
//!     load the next interval via `FreeCell::advance` (decoding the head cell),
//!     or, when the next pointer is the sentinel (LSB set), fall to the slow path.
//!   - A FRESH empty block sweeps to ONE interval spanning the whole payload
//!     (`makeLast` + `initialize`, heap/MarkedBlockInlines.h:313-318).
//!
//! WHERE THE BYTES LIVE (the S4 unsafe contract — see marked_block.rs C1-C6): the
//! `FreeCell` link records live in the SAME once-exposed, exposed-provenance page
//! memory the arena owns (contract C2). `FreeList` reads/writes only the head
//! `FreeCell`'s scrambled bits, through `with_exposed_provenance{,_mut}` raw
//! places (no reference formed, contract C3/C4/C5), and only on cells it has not
//! yet handed out. Each head's bits are decoded exactly ONCE (at `advance`) before
//! that cell is returned and overwritten by `ptr::write(Cell)`, so the link memory
//! and the live `Cell` never alias in time. Single mutator thread (contract C6).

#![allow(dead_code)]
#![allow(clippy::missing_safety_doc)]

use core::ptr;

use super::marked_block::ATOM_SIZE;

// ===================== FreeCell (heap/FreeList.h:39-80) =====================

/// `FreeCell` (heap/FreeList.h:39-80): the per-interval link record JSC writes
/// into a free cell's own first atom. `repr(C)` pins `scrambled_bits` at offset 8,
/// matching `OBJECT_OFFSETOF(FreeCell, scrambledBits)`. Only `scrambled_bits` is
/// load-bearing for the interval walk; `preserved_bits_for_crash_analysis` is
/// untouched here (JSC keeps it purely for crash forensics).
#[repr(C)]
struct FreeCell {
    preserved_bits_for_crash_analysis: u64,
    scrambled_bits: u64,
}
const _: () = assert!(core::mem::size_of::<FreeCell>() == ATOM_SIZE); // one atom (16)
const _: () = assert!(core::mem::offset_of!(FreeCell, scrambled_bits) == 8);

impl FreeCell {
    /// `FreeCell::scramble` (heap/FreeList.h:40-44). Packs `lengthInBytes` (high 32)
    /// and `offsetToNext` (low 32), XOR the secret.
    ///
    /// DIVERGENCE (unreachable corner): JSC's `... | offsetToNext` converts the
    /// `int32_t` to `uint64_t`, sign-extending a NEGATIVE offset into the length
    /// bits. The allocator only ever encodes POSITIVE offsets (`makeLast` uses 1;
    /// `setNext` uses `next - this > 0` since intervals are linked low->high), so
    /// `offset_to_next as u32 as u64` (no sign extension) reproduces C++ bit-for-bit
    /// for every reachable value while keeping the length bits clean.
    #[inline]
    fn scramble(offset_to_next: i32, length_in_bytes: u32, secret: u64) -> u64 {
        // ASSERT(static_cast<uint64_t>(lengthInBytes) << 32 | offsetToNext): nonzero.
        debug_assert!(((length_in_bytes as u64) << 32 | (offset_to_next as u32 as u64)) != 0);
        ((length_in_bytes as u64) << 32 | (offset_to_next as u32 as u64)) ^ secret
    }

    /// `FreeCell::descramble` (heap/FreeList.h:46-51): inverse of `scramble`.
    #[inline]
    fn descramble(scrambled_bits: u64, secret: u64) -> (i32, u32) {
        let descrambled = scrambled_bits ^ secret;
        ((descrambled as u32) as i32, (descrambled >> 32) as u32)
    }

    /// Read the head cell's scrambled bits from page memory via exposed provenance.
    ///
    /// SAFETY (C2/C3): `addr` is the head of a free interval inside a once-exposed
    /// page; the raw place read of `scrambled_bits` forms no reference.
    #[inline]
    unsafe fn read_scrambled_bits(addr: usize) -> u64 {
        let fc: *const FreeCell = ptr::with_exposed_provenance::<u8>(addr).cast();
        // SAFETY: per fn contract — `addr` is an interval head in a once-exposed page.
        unsafe { ptr::addr_of!((*fc).scrambled_bits).read() }
    }

    /// Write the head cell's scrambled bits into page memory via exposed provenance.
    ///
    /// SAFETY (C2/C3/C4): `addr` is a free (not-yet-handed-out) cell inside a
    /// once-exposed page the directory solely owns; the raw place write of
    /// `scrambled_bits` forms no reference and aliases no live `Cell`.
    #[inline]
    unsafe fn write_scrambled_bits(addr: usize, bits: u64) {
        let fc: *mut FreeCell = ptr::with_exposed_provenance_mut::<u8>(addr).cast();
        // SAFETY: per fn contract — `addr` is a free cell in a once-exposed page.
        unsafe { ptr::addr_of_mut!((*fc).scrambled_bits).write(bits) };
    }

    /// `FreeCell::decode` (heap/FreeList.h:63-66).
    ///
    /// SAFETY: as `read_scrambled_bits`.
    #[inline]
    unsafe fn decode(addr: usize, secret: u64) -> (i32, u32) {
        // SAFETY: per fn contract — see `read_scrambled_bits`.
        Self::descramble(unsafe { Self::read_scrambled_bits(addr) }, secret)
    }

    /// `FreeCell::makeLast` (heap/FreeList.h:53-56): encode this cell as the LAST
    /// interval — a set-LSB (`offsetToNext == 1`) sentinel next pointer.
    ///
    /// SAFETY: as `write_scrambled_bits`.
    #[inline]
    unsafe fn make_last(cell_addr: usize, length_in_bytes: u32, secret: u64) {
        // SAFETY: per fn contract — see `write_scrambled_bits`.
        unsafe {
            Self::write_scrambled_bits(cell_addr, Self::scramble(1, length_in_bytes, secret))
        };
    }

    /// `FreeCell::setNext` (heap/FreeList.h:58-61): link `this` -> `next`.
    ///
    /// JSC encodes `(next - this) * sizeof(FreeCell)`; the `FreeCell*` subtraction
    /// already divides the byte delta by `sizeof(FreeCell)`, so in this address
    /// (byte) model the encoded offset is exactly `next - this`.
    ///
    /// SAFETY: as `write_scrambled_bits`.
    #[inline]
    unsafe fn set_next(this: usize, next: usize, length_in_bytes: u32, secret: u64) {
        let offset_to_next = (next as isize - this as isize) as i32;
        // SAFETY: per fn contract — see `write_scrambled_bits`.
        unsafe {
            Self::write_scrambled_bits(
                this,
                Self::scramble(offset_to_next, length_in_bytes, secret),
            )
        };
    }

    /// `FreeCell::advance` (heap/FreeList.h:68-74): decode the current interval head
    /// into `[interval_start, interval_end)` and step `interval` to the next head.
    ///
    /// SAFETY: `*interval` is a real (non-sentinel) interval head in a once-exposed
    /// page; see `decode`.
    #[inline]
    unsafe fn advance(
        secret: u64,
        interval: &mut usize,
        interval_start: &mut usize,
        interval_end: &mut usize,
    ) {
        // SAFETY: per fn contract — `*interval` is a non-sentinel head; see `decode`.
        let (offset_to_next, length_in_bytes) = unsafe { Self::decode(*interval, secret) };
        *interval_start = *interval;
        *interval_end = *interval_start + length_in_bytes as usize;
        *interval = (*interval_start).wrapping_add_signed(offset_to_next as isize);
    }
}

// ===================== FreeList (heap/FreeList.h:82-123) =====================

/// The sentinel `m_nextInterval` value (a set LSB; heap/FreeList.h:119,
/// `bit_cast<FreeCell*>(static_cast<uintptr_t>(1))`).
const SENTINEL: usize = 1;

/// `FreeList` (heap/FreeList.h:82-123): the interval bump cursor a LocalAllocator
/// drives. `m_intervalStart`/`m_intervalEnd` are the current run; `m_nextInterval`
/// points at the next interval head (sentinel = exhausted); `m_secret` keys the
/// FreeCell descramble.
pub(crate) struct FreeList {
    interval_start: usize, // m_intervalStart (char*)
    interval_end: usize,   // m_intervalEnd (char*)
    next_interval: usize,  // m_nextInterval (FreeCell*; sentinel == LSB set)
    secret: u64,           // m_secret
    original_size: u32,    // m_originalSize
    cell_size: u32,        // m_cellSize
}

impl FreeList {
    /// `FreeList::FreeList(unsigned cellSize)` (heap/FreeList.cpp:31-34) plus the
    /// member initializers (heap/FreeList.h:117-122).
    pub(crate) fn new(cell_size: u32) -> Self {
        FreeList {
            interval_start: 0,
            interval_end: 0,
            next_interval: SENTINEL,
            secret: 0,
            original_size: 0,
            cell_size,
        }
    }

    /// `FreeList::clear` (heap/FreeList.cpp:38-45).
    pub(crate) fn clear(&mut self) {
        self.interval_start = 0;
        self.interval_end = 0;
        self.next_interval = SENTINEL;
        self.secret = 0;
        self.original_size = 0;
    }

    /// `FreeList::isSentinel` (heap/FreeList.h:102).
    #[inline]
    fn is_sentinel(cell: usize) -> bool {
        cell & 1 != 0
    }

    /// `FreeList::allocationWillFail` (heap/FreeList.h:91): no bytes left in the
    /// current interval AND the next pointer is the sentinel.
    pub(crate) fn allocation_will_fail(&self) -> bool {
        self.interval_start >= self.interval_end && Self::is_sentinel(self.next_interval)
    }

    /// `FreeList::allocationWillSucceed` (heap/FreeList.h:92).
    pub(crate) fn allocation_will_succeed(&self) -> bool {
        !self.allocation_will_fail()
    }

    /// `FreeList::initialize` (heap/FreeList.cpp:47-57): adopt `head` (a FreeCell
    /// address, or 0/nullptr -> clear) and load its first interval.
    ///
    /// SAFETY: `head` is 0 or a live interval head in a once-exposed page; `advance`
    /// reads its scrambled bits.
    pub(crate) unsafe fn initialize(&mut self, head: usize, secret: u64, bytes: u32) {
        if head == 0 {
            self.clear();
            return;
        }
        self.secret = secret;
        self.next_interval = head;
        // SAFETY: per fn contract — `head` is a live interval head; see `advance`.
        unsafe {
            FreeCell::advance(
                self.secret,
                &mut self.next_interval,
                &mut self.interval_start,
                &mut self.interval_end,
            );
        }
        self.original_size = bytes;
    }

    /// Sweep a FRESH, fully-empty block to a FreeList: ONE interval spanning
    /// `[payload_begin, payload_end)` (heap/MarkedBlockInlines.h:313-318, the
    /// `IsEmpty` quick path — a single `makeLast` head cell, then `initialize`).
    ///
    /// SAFETY: the range is the just-exposed page's payload; writing the head
    /// FreeCell's bits is interior page memory the directory solely owns.
    pub(crate) unsafe fn initialize_empty_block(
        &mut self,
        payload_begin: usize,
        payload_end: usize,
        secret: u64,
    ) {
        let length = (payload_end - payload_begin) as u32;
        // SAFETY: per fn contract — payload_begin is the head of the fresh page's
        // single free interval; see `make_last` / `initialize`.
        unsafe {
            FreeCell::make_last(payload_begin, length, secret);
            self.initialize(payload_begin, secret, length);
        }
    }

    /// `FreeList::allocateWithCellSize` (heap/FreeListInlines.h:35-55) with the slow
    /// path hoisted to the caller: bump within the current interval; on interval
    /// exhaustion advance to the next interval, or return `None` (== JSC's
    /// `slowPath()`) when the next pointer is the sentinel.
    ///
    /// SAFETY: this FreeList's intervals reference live, exposed pages owned by the
    /// calling directory; single mutator thread (contract C5/C6). `advance` only
    /// reads not-yet-handed-out interval heads.
    #[inline]
    pub(crate) unsafe fn allocate(&mut self) -> Option<usize> {
        let cell_size = self.cell_size as usize;
        if self.interval_start < self.interval_end {
            let result = self.interval_start;
            self.interval_start += cell_size;
            return Some(result);
        }

        if Self::is_sentinel(self.next_interval) {
            return None; // slowPath()
        }

        // SAFETY: per fn contract — next_interval is a non-sentinel live head; see
        // `advance`.
        unsafe {
            FreeCell::advance(
                self.secret,
                &mut self.next_interval,
                &mut self.interval_start,
                &mut self.interval_end,
            );
        }

        // Invariant (heap/FreeListInlines.h:50-51): we never create empty intervals,
        // so the freshly-advanced interval always has room for one cell.
        let result = self.interval_start;
        self.interval_start += cell_size;
        Some(result)
    }

    /// `FreeList::cellSize` (heap/FreeList.h:112).
    pub(crate) fn cell_size(&self) -> u32 {
        self.cell_size
    }

    /// `FreeList::originalSize` (heap/FreeList.h:100).
    pub(crate) fn original_size(&self) -> u32 {
        self.original_size
    }
}

// DEFERRED: `FreeList::forEach` (heap/FreeListInlines.h:57-76) iterates every free
// cell (used by Scribble / verification sweeps), not the allocation fast path this
// unit ports. R2 adds it when sweep/scribble lands.

// ================================== TESTS ==================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::heap::marked_block::{block_layout, ATOMS_PER_BLOCK, ATOM_SIZE, BLOCK_MASK};
    use std::alloc::{alloc_zeroed, dealloc};

    /// One exposed, block-aligned, zeroed page — exactly how `BlockDirectory::
    /// add_block` creates backing memory (contract C1/C2), so the FreeList walks
    /// real exposed-provenance page bytes.
    struct TestPage {
        raw: *mut u8,
        base: usize,
    }

    impl TestPage {
        fn new() -> Self {
            // SAFETY: nonzero, power-of-two-aligned layout (block_layout()).
            let raw = unsafe { alloc_zeroed(block_layout()) };
            assert!(!raw.is_null());
            let base = raw.expose_provenance(); // expose ONCE (contract C2)
            TestPage { raw, base }
        }
    }

    impl Drop for TestPage {
        fn drop(&mut self) {
            // SAFETY: `raw` came from alloc_zeroed(block_layout()), freed once.
            unsafe { dealloc(self.raw, block_layout()) };
        }
    }

    /// scramble/descramble are exact inverses for every reachable (positive-offset)
    /// encoding, and the secret actually obfuscates.
    #[test]
    fn scramble_descramble_round_trips() {
        let secret = 0xDEAD_BEEF_1234_5678;
        for &(off, len) in &[(1i32, 16u32), (80, 80), (160, 320), (16, 16080)] {
            let bits = FreeCell::scramble(off, len, secret);
            assert_eq!(FreeCell::descramble(bits, secret), (off, len));
        }
        assert_ne!(
            FreeCell::scramble(1, 16, secret),
            FreeCell::scramble(1, 16, secret ^ 1),
            "secret obfuscates the bits"
        );
    }

    /// A fresh empty block sweeps to ONE interval; `allocate` fills it cell by cell
    /// (bumping `m_intervalStart`), yields contiguous cell-aligned addresses all in
    /// the one block, then signals the slow path (`None`) exactly when full.
    #[test]
    fn single_interval_fills_a_block_then_signals_slow_path() {
        let page = TestPage::new();
        let cell_size = 80u32;
        let cs = cell_size as usize;
        // Front-slop start atom so [start, endAtom) is an exact multiple of the cell
        // (matches BlockDirectory::start_atom_for(5)).
        let start_atom = 19usize;
        let payload_begin = page.base + start_atom * ATOM_SIZE;
        let payload_end = page.base + ATOMS_PER_BLOCK * ATOM_SIZE; // endAtom * atomSize
        let length = (payload_end - payload_begin) as u32;
        assert_eq!(length % cell_size, 0, "exact-termination geometry");

        let secret = 0x1122_3344_5566_7788;
        let mut fl = FreeList::new(cell_size);
        // SAFETY: payload_begin..payload_end is the exposed page payload.
        unsafe { fl.initialize_empty_block(payload_begin, payload_end, secret) };
        assert!(fl.allocation_will_succeed());

        let expected = (length / cell_size) as usize;
        let mut addrs = Vec::new();
        // SAFETY: intervals reference the exposed page above.
        while let Some(a) = unsafe { fl.allocate() } {
            addrs.push(a);
        }

        assert_eq!(
            addrs.len(),
            expected,
            "FreeList yields every cell in the block"
        );
        for (i, &a) in addrs.iter().enumerate() {
            assert_eq!(a, payload_begin + i * cs, "contiguous bump");
            assert_eq!(a & (ATOM_SIZE - 1), 0, "atom-aligned");
            assert_eq!(a & BLOCK_MASK, page.base, "all in the one block");
        }
        assert!(fl.allocation_will_fail(), "exhausted after the last cell");
    }

    /// `allocate` ADVANCES across multiple linked intervals: a live gap between two
    /// free runs is skipped by following the head FreeCell's `setNext` link.
    #[test]
    fn allocate_advances_across_multiple_intervals() {
        let page = TestPage::new();
        let cell_size = 80u32;
        let cs = cell_size as usize;
        let secret = 0xABCD_0123_4567_89AB;

        // Interval A (3 cells), a 2-cell live gap, then last interval B (4 cells).
        let a_start = page.base + 19 * ATOM_SIZE;
        let a_len = 3 * cs;
        let b_start = a_start + a_len + 2 * cs;
        let b_len = 4 * cs;

        // SAFETY: both heads lie in the exposed page; writes target free cells only.
        unsafe {
            FreeCell::make_last(b_start, b_len as u32, secret); // B is last
            FreeCell::set_next(a_start, b_start, a_len as u32, secret); // A -> B
        }

        let mut fl = FreeList::new(cell_size);
        // SAFETY: A is a live interval head in the exposed page.
        unsafe { fl.initialize(a_start, secret, (a_len + b_len) as u32) };

        let mut addrs = Vec::new();
        // SAFETY: as above.
        while let Some(a) = unsafe { fl.allocate() } {
            addrs.push(a);
        }

        let mut expected = Vec::new();
        for i in 0..3 {
            expected.push(a_start + i * cs); // interval A
        }
        for i in 0..4 {
            expected.push(b_start + i * cs); // interval B (gap skipped)
        }
        assert_eq!(addrs, expected);
        assert_eq!(fl.original_size(), (a_len + b_len) as u32);
        assert_eq!(fl.cell_size(), cell_size);
    }

    /// `initialize(0, ..)` (nullptr head) clears to the always-fail state, matching
    /// the slow-path-only FreeList JSC hands a fully-live block.
    #[test]
    fn initialize_null_head_clears() {
        let mut fl = FreeList::new(80);
        // SAFETY: head == 0 takes the clear() branch (no memory touched).
        unsafe { fl.initialize(0, 0xFFFF, 123) };
        assert!(fl.allocation_will_fail());
        // SAFETY: exhausted FreeList just returns None.
        assert_eq!(unsafe { fl.allocate() }, None);
    }
}
