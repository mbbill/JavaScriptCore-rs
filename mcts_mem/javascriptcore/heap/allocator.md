- Ordinary GC cells are carved from page-aligned MarkedBlock pages; each block belongs to one BlockDirectory and one size class.
- MarkedSpace maps requested cell sizes to directories, using dense small-object classes and coarser large-object classes up to the precise-allocation cutoff.
- Subspaces group directories by heap cell type and memory source; IsoSubspace narrows that model to one cell size and one logical object family.
- LocalAllocator owns the mutator fast path for a directory and refills from swept block free lists without forcing a full heap sweep.
- PreciseAllocation stores one cell behind a prepended allocation header and is distinguished from MarkedBlock-contained cells by its tagged container pointer.
- Incremental sweeping turns mark/sweep state back into allocation free lists between collections rather than making all sweeping part of the collection pause.
- AlignedMemoryAllocator variants select the backing region for a subspace, including normal fastMalloc, Gigacage-backed, and structure-specific memory.

## Facts

- 2002-11-20 (598c3ecd) measurement: fixed 64-byte cells packed into 16 KB slab blocks replaced per-object malloc and pointer-block scanning, removing malloc overhead and improving cache locality (sourced).
- 2002-11-24 (9f2a01bb) measurement: replacing a per-block bitmap allocation search with an embedded free list gave O(1) allocation and a 3% iBench gain (sourced).
- 2007-04-23 (68bf072e) measurement: removing the dead oversize-cell allocator simplified allocation, collection, and marking and gave a measured 0.66% speedup on 32-bit (sourced).
- 2007-10-31 (f12e3f63) measurement: a dedicated half-size Number cell class doubled number density per block and yielded a 0.5% SunSpider speedup, 7.1% on morph (sourced).
- 2019-11-09 (e6dbb891) measurement: letting IsoSubspace use a lower tier of precise allocations avoided a minimum 16 KB block for sparse types and reduced iOS memory by 0.6% (sourced).

## Moves

- 2002-11-20 (598c3ecd) replaced [[gc-pointer-block-allocator]]: Old GC stored pointers to malloc'd objects in linked CollectorBlocks (void** mem, linear scan for free slot); replaced with fixed-size 64-byte cells packed into 16 KB slab blocks with a bitmap to track live cells, eliminating per-object malloc overhead and improving cache locality. (sourced)
- 2002-11-24 (9f2a01bb) replaced [[gc-block-bitmap-allocator]]: Replaced per-block bitmap (uint32_t array) with an embedded singly-linked free-list inside CollectorCell, giving O(1) allocation and a 3% iBench gain; also added firstBlockWithPossibleSpace cursor and early-exit sweep when live count reached. (sourced)
- 2007-04-23 (68bf072e) removed: No JSCell subclass exceeded CELL_SIZE any more, so the oversize path was dead; removing it simplified the allocator, collector, and marker, and gave a measured 0.66% speedup on 32-bit. (sourced)
- 2012-09-11 (68effa91) replaced [[marked-space-fixed-max-cell-size]]: Old MarkedSpace had a hard cap at maxCellSize=2048 bytes via fixed imprecise size classes; the new design adds a m_largeAllocator using mmap-backed oversized MarkedBlocks (sized to ceil(sizeof(MarkedBlock)+bytes, pageSize)) so arbitrarily large cells can be GC-managed without a separate allocator. (sourced)
- 2019-11-09 (e6dbb891) replaced [[iso-subspace-marked-block-only]]: IsoSubspace previously required allocating a full MarkedBlock (16KB) even for object types instantiated rarely, imposing a minimum 16KB per type; adding a lower tier of up to 8 LargeAllocation cells per IsoSubspace avoids the MarkedBlock entirely for sparsely-allocated types, enabling IsoSubspace to be applied more aggressively across the object hierarchy with a measured 0.6% memory reduction on iOS. (sourced)
- 2022-02-18 (9b86c52d) replaced [[vm-owned-unified-iso-subspace]]: The unified IsoSubspace representation could not support the intended GlobalGC shape where many VM/client allocators are fed by one server Heap/IsoSubspace. (sourced)
