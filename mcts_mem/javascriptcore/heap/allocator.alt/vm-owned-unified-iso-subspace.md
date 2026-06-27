- JSC::IsoSubspace directly owned its BlockDirectory, LocalAllocator, allocator base, lower-tier free list, and IsoCellSet list.
- VM exposed common IsoSubspace instances through its embedded Heap.
- IsoSubspacePerVM mapped each VM directly to one IsoSubspace instance.

## Moves

- 2022-02-18 (9b86c52d) replaced by [[allocator]]: The unified IsoSubspace representation could not support the intended GlobalGC shape where many VM/client allocators are fed by one server Heap/IsoSubspace. (sourced)
