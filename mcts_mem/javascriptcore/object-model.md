- Object identity and dispatch are represented by JSCell headers plus shared Structure shapes, not by per-object virtual metadata alone.
- Named property lookup maps a uniqued property key to attributes and a numeric offset; the offset selects inline storage or out-of-line Butterfly storage.
- Structure transitions are the normal representation for adding or changing properties; dictionary forms are used when transition sharing no longer preserves efficient or stable offsets.
- The Butterfly joins named out-of-line storage and indexed storage behind one object pointer, with named slots growing left and indexed elements growing right.
- Property enumeration preserves insertion/key semantics through Structure-owned property tables rather than by scanning object storage order.
- Shape speculation is invalidated by watchpoints and cache conditions attached to Structures, prototype chains, dictionaries, and property tables.

## Facts

- 2002-11-19 (fcfb139a) measurement: replacing an AVL property map with an open-addressing hash table improved iBench by about 7% by removing per-node allocation and pointer chasing (sourced).
- 2008-10-31 (875eadba) measurement: merging PropertyMap into StructureID eliminated a lookup indirection and produced 1% SunSpider and 0.5% v8-suite speedups (sourced).
- 2008-11-14 (6d9f96e8) measurement: lazy materialization of Structure property maps saved about 15 MB on a 30-page Membuster test by stealing predecessor maps instead of copying them on every transition (sourced).

## Moves

- 2002-11-19 (fcfb139a) replaced [[property-map-avl-tree]]: AVL tree gave O(log n) property lookup with per-node heap allocation and pointer chasing; open-addressing hash table keyed on UString::Rep* gives O(1) average lookup with a single flat array allocation, yielding ~7% improvement on iBench. (sourced)
- 2008-10-31 (875eadba) replaced [[property-map-separate-class]]: Merging PropertyMap into StructureID eliminates a layer of indirection so callers access property storage directly through StructureID, enabling future lazy-creation of the hash table on get; immediate result is 1% SunSpider and 0.5% v8-suite speedup. (sourced)
- 2008-11-14 (6d9f96e8) replaced [[structureid-propertymap-eager-copy]]: Every addPropertyTransition always copied the PropertyMap into the new StructureID; the new design steals the PropertyMap from the predecessor and reconstructs it on demand via materializePropertyMap(), saving ~15MB on a 30-page Membuster test. (sourced)
- 2012-09-13 (0400d283) replaced [[jsobject-unidirectional-out-of-line-storage]]: A single m_butterfly pointer allows named out-of-line properties to be placed to the left and indexed properties to the right of the pointed-to location with no space overhead vs m_outOfLineStorage, enabling all JSObjects (not just JSArray) to have O(1) indexed property access and allowing indexed storage to morph over time. (sourced)
- 2016-12-08 (12e75c3d) replaced [[jsobject-concurrent-visit-lock-and-doublecheck]]: The new protocol makes structure/butterfly transitions detectable as BEFORE, AFTER, or IGNORE by inserting a nuked StructureID between structure-size and butterfly updates and by having the collector read structure and lastOffset both before and after reading the butterfly. (code)
- 2026-03-28 (55783b94) replaced [[all-jsobjects-carry-butterfly-slot]]: Wasm GC objects cannot have properties, prototypes, or structure transitions, so their inherited m_butterfly slot was always null and wasted one pointer per allocation. (code)
