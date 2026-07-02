//! Physical-page commit/decommit for empty `MarkedBlock` pages — the OS-facing half
//! of leak-fix C3 (swept memory never returned to the OS): `BlockDirectory::shrink`
//! (block_directory.rs) calls [`decommit`] on a block's bytes once proven completely
//! empty; [`recommit`] runs before that block is handed back out.
//!
//! WHAT C++ JSC DOES: an empty `MarkedBlock::Handle` is fully deleted
//! (`MarkedSpace::freeBlock` -> `delete` -> `Handle::~Handle` ->
//! `AlignedMemoryAllocator::freeAlignedMemory`, heap/MarkedSpace.cpp:401-406 +
//! heap/MarkedBlock.cpp:77-89), which for the common `FastMallocAlignedMemoryAllocator`
//! (heap/FastMallocAlignedMemoryAllocator.cpp:53-58) is WTF `fastFree` -> bmalloc.
//! bmalloc does not return pages to the OS synchronously on free; its own background
//! Scavenger + span cache do that later via the SAME primitive this module ports:
//!   - `decommitAlignedPhysical`/`commitAlignedPhysical`
//!     (Source/bmalloc/bmalloc/bmalloc.cpp:166-176), backed by
//!   - `vmDeallocatePhysicalPages`/`vmAllocatePhysicalPages`
//!     (Source/bmalloc/bmalloc/VMAllocate.h:319-349):
//!       Darwin decommit: `madvise(p, size, MADV_FREE_REUSABLE)` (VMAllocate.h:322-323)
//!         -- NOT plain `MADV_FREE`; `MADV_FREE_REUSABLE` is the Darwin-paired
//!         "volatile/purgeable" flag bmalloc uses so the kernel MAY reclaim the
//!         physical pages while the virtual mapping stays live.
//!       Darwin commit: A NO-OP (VMAllocate.h:337-342) -- verbatim: "we don't need to
//!         call madvise(..., MADV_FREE_REUSE) to commit physical memory to back a
//!         range of allocated virtual memory. Instead the kernel will commit pages as
//!         they are touched." Recommit IS simply writing to the page again.
//!       non-Darwin (this crate also builds on Linux dev/CI hosts): decommit is
//!         `madvise(MADV_DONTNEED)`; commit is `madvise(MADV_NORMAL)`
//!         (VMAllocate.h:326-331, :343-344) -- the portable POSIX pairing, used here
//!         as the non-Darwin fallback (this project's measured platform is Darwin,
//!         see CLAUDE.md's Measuring Rule; the Linux path is kept buildable, not
//!         perf-tuned).
//!
//! DIVERGENCE (ratified, gc-r4 leak-fix C3): this crate's `BlockDirectory` owns raw
//! 16KB pages directly via `alloc_zeroed`/`dealloc` (block_directory.rs) — there is no
//! bmalloc-equivalent sub-allocator layer underneath it to run a Scavenger over. R1
//! collapses JSC's two-layer "delete the JSC block object, let the allocator's own
//! background scavenger eventually madvise the pages" pipeline into ONE synchronous
//! step: `BlockDirectory::shrink` madvises an empty block's pages directly and keeps
//! its `Vec` slot (marked decommitted) instead of freeing and later reallocating the
//! block object. The OS-facing effect is the same (an empty block's physical
//! footprint drops; the virtual reservation survives for instant reuse); the C++
//! object's free/realloc lifecycle is not literally replayed. EMPTY-block granularity
//! only (ratified) — JSC's finer-grained partial-block variants are a follow-up.
//!
//! SAFETY SCOPE: this is the ONLY module besides platform/unix_executable_memory.rs
//! that issues a raw `madvise` syscall. Every call here is on a whole, block-aligned
//! `[base, base+len)` range the caller (`BlockDirectory`) has already proven it
//! solely owns (contract C1/C2, marked_block.rs) with no live cell inside it.

#![allow(unsafe_code)]

use core::sync::atomic::{AtomicUsize, Ordering};
use std::ffi::{c_int, c_void};

unsafe extern "C" {
    fn madvise(addr: *mut c_void, len: usize, advice: c_int) -> c_int;
}

/// RELEASE-VISIBLE madvise-failure counters (one per direction). `debug_assert`
/// compiles out of release builds, so without these a persistently failing
/// `madvise` would let `committed_block_bytes()` silently diverge from the OS's
/// actual physical footprint — and the leak-fix C3 tests (and any future leak
/// claim) rest on that metric. bmalloc's own posture is the model: its `SYSCALL`
/// wrapper (Source/bmalloc/bmalloc/BSyscall.h:29-31) retries EAGAIN and otherwise
/// TOLERATES failure — "failing to return pages to the OS is preferable to
/// crashing when the allocator can still reuse them internally" (the JSC fixed-VM-
/// pool MADV_FREE rationale) — so failure here must never panic in release, only
/// count. AtomicUsize (not Cell) purely so the statics are Sync; the arena is
/// single-mutator (contract C6).
static DECOMMIT_FAILURES: AtomicUsize = AtomicUsize::new(0);
static RECOMMIT_FAILURES: AtomicUsize = AtomicUsize::new(0);

/// Times [`decommit`]'s madvise persistently failed (post-EAGAIN-retry). Nonzero
/// means `committed_block_bytes()` UNDER-reports the true resident footprint by up
/// to that many blocks — a leak claim based on the counter must check this first.
pub(crate) fn decommit_failure_count() -> usize {
    DECOMMIT_FAILURES.load(Ordering::Relaxed)
}

/// Times [`recommit`]'s madvise persistently failed (post-EAGAIN-retry). Benign on
/// Darwin (commit is a syscall no-op there — pages re-back on touch) but tracked
/// for symmetry and for the non-Darwin MADV_DONTNEED/MADV_NORMAL pairing.
/// (No caller yet — the load-bearing counter is `decommit_failure_count`, which
/// bounds `committed_block_bytes()` divergence; this one is kept alongside the
/// counter it mirrors rather than orphaning RECOMMIT_FAILURES as write-only.)
#[allow(dead_code)]
pub(crate) fn recommit_failure_count() -> usize {
    RECOMMIT_FAILURES.load(Ordering::Relaxed)
}

/// bmalloc `SYSCALL(madvise(...))` (Source/bmalloc/bmalloc/BSyscall.h:29-31):
/// retry while the failure is EAGAIN (the only transient case, per the JSC
/// fixed-VM-pool MADV_FREE handling), then report any persistent failure to the
/// given release-visible counter. Never panics in release; the debug assert keeps
/// real bugs (EINVAL-class misuse) loud during development.
///
/// SAFETY: forwarded — `addr..addr+len` must satisfy the calling wrapper's
/// ([`decommit`]/[`recommit`]) documented contract.
unsafe fn madvise_with_retry(addr: *mut c_void, len: usize, advice: c_int, failures: &AtomicUsize) {
    #[cfg(target_os = "macos")]
    const EAGAIN: i32 = 35; // Darwin errno (sys/errno.h)
    #[cfg(not(target_os = "macos"))]
    const EAGAIN: i32 = 11; // Linux asm-generic errno
    loop {
        // SAFETY: per fn contract — advisory-only syscall on a caller-validated range.
        let rc = unsafe { madvise(addr, len, advice) };
        if rc == 0 {
            return;
        }
        let errno = std::io::Error::last_os_error().raw_os_error();
        if errno == Some(EAGAIN) {
            continue; // bmalloc SYSCALL: EAGAIN is the one transient, retryable case
        }
        failures.fetch_add(1, Ordering::Relaxed);
        debug_assert!(
            false,
            "madvise(advice={advice}) failed non-transiently: {:?}",
            std::io::Error::last_os_error()
        );
        return;
    }
}

// Darwin madvise advice constants (sys/mman.h; verified against the MacOSX SDK
// header, not guessed): MADV_NORMAL=0, MADV_FREE_REUSABLE=7. Not exposed by any crate
// dependency (this crate has zero dependencies, Cargo.toml), so declared directly —
// same pattern as platform/unix_executable_memory.rs's PROT_*/MAP_* constants.
#[cfg(target_os = "macos")]
const MADV_DECOMMIT: c_int = 7; // MADV_FREE_REUSABLE

// MADV_NORMAL. Per the module doc, Darwin commit is a syscall no-op in bmalloc; this
// crate still issues it (harmless, mirrors the non-Darwin arm for symmetry), but the
// actual "recommit" is the caller's header/payload rewrite that touches the page.
#[cfg(target_os = "macos")]
const MADV_COMMIT: c_int = 0;

// Portable POSIX fallback for non-Darwin unix targets (Linux and friends): decommit
// via MADV_DONTNEED, commit via MADV_NORMAL (VMAllocate.h's non-Darwin `#else` arm,
// minus the Linux-only MADV_DONTDUMP/MADV_DODUMP pair, which has no portable value
// this crate can cite without a Linux header in this checkout).
#[cfg(not(target_os = "macos"))]
const MADV_DECOMMIT: c_int = 4; // MADV_DONTNEED
#[cfg(not(target_os = "macos"))]
const MADV_COMMIT: c_int = 0; // MADV_NORMAL

/// Return `[base, base+len)` physical pages to the OS while keeping the virtual
/// mapping intact (bmalloc `decommitAlignedPhysical`/`vmDeallocatePhysicalPages`,
/// cited in the module doc). Advisory-only: never faults, never unmaps.
///
/// SAFETY: `base..base+len` must be a live mapping this call's caller solely owns,
/// with no live (reachable) cell inside it — the caller must not read or write any
/// byte in this range again until a matching [`recommit`] documents it as safe.
pub(crate) unsafe fn decommit(base: usize, len: usize) {
    let ptr = std::ptr::with_exposed_provenance_mut::<c_void>(base);
    // SAFETY: forwarded by the caller's contract above; `madvise` is advisory and
    // cannot fault regardless of the range's actual commit state. Persistent
    // failure is counted release-visibly (see DECOMMIT_FAILURES), never a panic —
    // the block stays safely committed and internally reusable, exactly bmalloc's
    // failure posture.
    unsafe { madvise_with_retry(ptr, len, MADV_DECOMMIT, &DECOMMIT_FAILURES) };
}

/// Undo [`decommit`] before `[base, base+len)` is written again (bmalloc
/// `commitAlignedPhysical`/`vmAllocatePhysicalPages`, cited in the module doc). On
/// Darwin this issues `MADV_NORMAL` (harmless, matches the non-Darwin arm for
/// symmetry) but is NOT what actually re-backs the pages — per the cited bmalloc
/// comment, the kernel commits pages as they are touched, so the caller's own
/// header/payload rewrite immediately after this call is what recommits them.
///
/// SAFETY: `base..base+len` must be a range this call's caller solely owns that was
/// previously passed to [`decommit`] and not otherwise touched since.
pub(crate) unsafe fn recommit(base: usize, len: usize) {
    let ptr = std::ptr::with_exposed_provenance_mut::<c_void>(base);
    // SAFETY: forwarded by the caller's contract above; advisory-only syscall.
    // Persistent failure is counted release-visibly (see RECOMMIT_FAILURES).
    unsafe { madvise_with_retry(ptr, len, MADV_COMMIT, &RECOMMIT_FAILURES) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::{alloc_zeroed, dealloc, Layout};

    /// `decommit` then `recommit` a real page-aligned allocation and prove it is
    /// still writable/readable afterward with no crash — the syscall pairing itself
    /// is sound on this host, independent of `BlockDirectory`'s bookkeeping.
    #[test]
    fn decommit_then_recommit_then_write_is_sound() {
        let layout = Layout::from_size_align(16 * 1024, 16 * 1024).unwrap();
        // SAFETY: nonzero, page-aligned layout.
        let raw = unsafe { alloc_zeroed(layout) };
        assert!(!raw.is_null());
        let base = raw.expose_provenance();

        // SAFETY: `base..base+16KiB` is the just-allocated, solely-owned page; no
        // live data is read afterward before `recommit`.
        unsafe { decommit(base, layout.size()) };
        // SAFETY: matches the immediately preceding `decommit` on the same range.
        unsafe { recommit(base, layout.size()) };

        // SAFETY: `raw` is still a live allocation of `layout`'s size; recommit
        // documented it safe to touch again.
        unsafe {
            raw.write(0x42);
            assert_eq!(raw.read(), 0x42);
            dealloc(raw, layout);
        }
    }
}
