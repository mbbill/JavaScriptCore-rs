- CollectorHeap tracked oversize cells in a separate allocation list.
- Oversize cells were allocated through fastMalloc outside ordinary fixed cells.
- Marking and conservative scanning had separate oversize-cell scan paths.

## Moves

- 2007-04-23 (68bf072e) removed: No JSCell subclass exceeded CELL_SIZE any more, so the oversize path was dead; removing it simplified the allocator, collector, and marker, and gave a measured 0.66% speedup on 32-bit. (sourced)
