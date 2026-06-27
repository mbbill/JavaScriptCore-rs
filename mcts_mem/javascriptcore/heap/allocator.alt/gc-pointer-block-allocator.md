- CollectorBlock stored an array of pointers to separately malloc'd JS values.
- The collector walked a linked list of blocks to find a free pointer slot.
- Collection pressure used an adaptive soft-limit increment heuristic.

## Moves

- 2002-11-20 (598c3ecd) replaced by [[allocator]]: Old GC stored pointers to malloc'd objects in linked CollectorBlocks (void** mem, linear scan for free slot); replaced with fixed-size 64-byte cells packed into 16 KB slab blocks with a bitmap to track live cells, eliminating per-object malloc overhead and improving cache locality. (sourced)
