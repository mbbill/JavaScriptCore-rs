- Property patch sites are found by platform-specific fixed instruction offsets.

## Moves

- 2012-04-13 (ff3a4437) replaced by [[inline-caches]]: The baseline JIT no longer relies on platform-specific fixed offsets for get_by_id/put_by_id patch sites, and instead records the linked code-label deltas in StructureStubInfo. (code)
