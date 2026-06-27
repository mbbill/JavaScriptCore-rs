- PropertyMap was a separate class owning a PropertyMapHashTable pointer.
- StructureID delegated property put, get, remove, empty, and storage-size queries through that separate map.
- Callers crossed an extra map indirection before reaching property storage metadata.

## Moves

- 2008-10-31 (875eadba) replaced by [[object-model]]: Merging PropertyMap into StructureID eliminates a layer of indirection so callers access property storage directly through StructureID, enabling future lazy-creation of the hash table on get; immediate result is 1% SunSpider and 0.5% v8-suite speedup. (sourced)
