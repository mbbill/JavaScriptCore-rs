- fromDictionaryTransition cleared the dictionary kind when conversion was allowed.
- Structures with deleted offsets were left unchanged rather than made cacheable.
- Object property storage was not compacted or reordered during dictionary normalization.

## Moves

- 2009-11-10 (eecc52b6) replaced by [[structure-shapes]]: fromDictionaryTransition could not convert UncacheableDictionary structures (those with deleted slots causing non-contiguous offsets) into cacheable normal structures because it only cleared the dictionary flag without compacting storage; flattenDictionaryStructure adds a sort-and-repack pass that reassigns contiguous offsets and physically moves property values in the object's storage, enabling prototype chain caching even when the prototype was formerly an uncacheable dictionary. (sourced)
