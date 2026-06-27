- The fixed JIT pool uses a best-fit AVL tree keyed by free-chunk size.
- Free chunks are coalesced after allocation pressure builds.

## Moves

- 2011-01-31 (daf2fadd) replaced by [[unlinked-code-sharing]]: Best-fit via AVL tree (SizeSortedFreeTree) with deferred coalescing caused heavy external fragmentation under real JIT allocation patterns, leading to CRASH() when no suitable free chunk could be found even with available aggregate memory; first-fit via a two-level bitmap AllocationTable hierarchy (AllocationTableLeaf + AllocationTableDirectory) eliminates fragmentation by allocating at power-of-two block granularity with no coalescing needed. (sourced)
