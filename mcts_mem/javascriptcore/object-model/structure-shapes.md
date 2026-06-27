- Structure records are shared shapes with prototype, type info, inline capacity, indexing mode, and named-property metadata.
- Property additions normally allocate or reuse transition Structures.
- Structure property metadata may be materialized lazily from the transition chain.
- Dictionary Structures distinguish cacheable addition-overflow dictionaries and uncacheable deletion dictionaries.
- Dictionary flattening compacts property offsets and physically reorders object storage.
- Property tables may be GC-managed cells whose unpinned instances can die while their owning Structure remains alive.

## Facts

- 2008-11-14 (6d9f96e8) measurement: lazy property-map materialization saved roughly 15 MB on a 30-page Membuster test by avoiding eager map copies on every Structure transition (sourced).
- 2010-01-15 (6b068ef4) pitfall: cache code that flattens a dictionary must recompute the property offset afterward because storage repacking can make the pre-flatten offset point at the wrong slot (code).
- 2013-02-26 (f7da71f2) measurement: making PropertyTable a GC-managed cell allowed unpinned Structure tables to be collected and removed a 14 MB waste on Membuster3 (sourced).

## Moves

- 2008-10-31 (875eadba) replaced [[property-map-separate-class]]: Merging PropertyMap into StructureID eliminates a layer of indirection so callers access property storage directly through StructureID, enabling future lazy-creation of the hash table on get; immediate result is 1% SunSpider and 0.5% v8-suite speedup. (sourced)
- 2008-11-14 (6d9f96e8) replaced [[structureid-propertymap-eager-copy]]: Every addPropertyTransition always copied the PropertyMap into the new StructureID; the new design steals the PropertyMap from the predecessor and reconstructs it on demand via materializePropertyMap(), saving ~15MB on a 30-page Membuster test. (sourced)
- 2009-09-21 (07eb57bd) replaced [[structure-single-dictionary-flag]]: A single bool m_isDictionary cannot distinguish dictionaries created by property removal (whose slot offsets may be reused) from those created by property addition overflow (whose existing slots are stable), so property-access caching was disabled for both; splitting into NoneDictionaryKind/CachedDictionaryKind/UncachedDictionaryKind allows IC caching on addition-overflow dictionaries while still skipping it on removal dictionaries. (sourced)
- 2009-11-10 (eecc52b6) replaced [[dictionary-to-normal-transition-fromDictionary]]: fromDictionaryTransition could not convert UncacheableDictionary structures (those with deleted slots causing non-contiguous offsets) into cacheable normal structures because it only cleared the dictionary flag without compacting storage; flattenDictionaryStructure adds a sort-and-repack pass that reassigns contiguous offsets and physically moves property values in the object's storage, enabling prototype chain caching even when the prototype was formerly an uncacheable dictionary. (sourced)
- 2013-02-26 (f7da71f2) replaced [[property-table-heap-allocated]]: Unpinned Structure property tables were never freed while the Structure was alive even when no longer needed (14 MB waste on Membuster3); making PropertyTable a GC-managed JSCell allows Structure::visitChildren to null out m_propertyTable for unpinned tables so the GC can collect them. (sourced)
