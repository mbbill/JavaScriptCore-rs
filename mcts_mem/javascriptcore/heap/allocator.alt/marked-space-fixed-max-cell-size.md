- MarkedSpace capped imprecise cells at maxCellSize=2048 bytes.
- Size classes covered fixed ranges up to the hard cap.
- Cells larger than the imprecise cutoff had no MarkedSpace allocation path.

## Moves

- 2012-09-11 (68effa91) replaced by [[allocator]]: Old MarkedSpace had a hard cap at maxCellSize=2048 bytes via fixed imprecise size classes; the new design adds a m_largeAllocator using mmap-backed oversized MarkedBlocks (sized to ceil(sizeof(MarkedBlock)+bytes, pageSize)) so arbitrarily large cells can be GC-managed without a separate allocator. (sourced)
