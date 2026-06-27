- WTF allocation helpers route selected classes and global operators through WebKit's fast allocation path without changing client object layout.
- Released allocator pages are returned to the operating system using platform-preserving decommit/reuse operations when remapping would risk correctness.
- Low-level spin locks used by allocation and GC infrastructure are WTF primitives rather than dependencies on removed allocator internals.

## Moves

- 2009-03-13 (81933366) replaced [[tcmalloc-system-release-mmap]]: On macOS Snow Leopard+, TCMalloc_SystemRelease switched from re-mmapping spans (MAP_FIXED|MAP_ANON) to madvise(MADV_FREE_REUSABLE)/madvise(MADV_FREE_REUSE) because the madvise pair allows the kernel to reclaim physical pages while keeping the virtual mapping intact, avoiding the silent correctness issues with MAP_FIXED re-mmap on newer Darwin. (sourced)
- 2015-03-13 (541755c0) replaced [[tcmalloc-spinlock-dependency]]: WebKit no longer uses TCMalloc and can replace its spinlock dependency with a WTF::SpinLock built on WTF::Atomic. (sourced)
