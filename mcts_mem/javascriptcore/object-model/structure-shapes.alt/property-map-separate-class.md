- PropertyMap was a standalone metadata object reached from StructureID.
- StructureID property lookup paid an extra pointer indirection through PropertyMap.
- The property map table was not directly embedded in the shape object.

## Moves

- 2008-10-31 (875eadba) replaced by [[structure-shapes]]: Merging PropertyMap into StructureID eliminates a layer of indirection so callers access property storage directly through StructureID, enabling future lazy-creation of the hash table on get; immediate result is 1% SunSpider and 0.5% v8-suite speedup. (sourced)
