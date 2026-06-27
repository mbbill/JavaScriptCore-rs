- Fixed-pool executable allocation delegated to WTF::MetaAllocator or per-region RegionAllocator subclasses.
- ExecutableMemoryHandle was a MetaAllocator handle and memory-pressure statistics came from MetaAllocator accounting.

## Moves

- 2021-07-13 (b6d532a7) replaced by [[executable-memory]]: JSC executable allocation switched from WTF::MetaAllocator handles to libpas jit_heap so the allocator could use bitfit/large-heap allocation, approximate first-fit over supplied ranges, no in-managed-memory metadata, bounded allocation/deallocation behavior, fine-grained locking, and libpas scavenging policy. (sourced)
