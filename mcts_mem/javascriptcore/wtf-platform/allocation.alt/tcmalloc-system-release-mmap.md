- Span release returns pages by remapping the virtual address range with `MAP_FIXED` anonymous memory.
- Releasing physical pages replaces the existing mapping rather than preserving it for reuse accounting.
- Commit and decommit tracking is tied to allocator-specific mmap behavior.

## Moves

- 2009-03-13 (81933366) replaced by [[allocation]]: On macOS Snow Leopard+, TCMalloc_SystemRelease switched from re-mmapping spans (MAP_FIXED|MAP_ANON) to madvise(MADV_FREE_REUSABLE)/madvise(MADV_FREE_REUSE) because the madvise pair allows the kernel to reclaim physical pages while keeping the virtual mapping intact, avoiding the silent correctness issues with MAP_FIXED re-mmap on newer Darwin. (sourced)
