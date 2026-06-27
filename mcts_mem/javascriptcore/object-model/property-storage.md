- Named property storage is a two-level design: Structure-owned metadata maps keys to offsets, and each object stores values at those offsets.
- Property maps use hash lookup over uniqued keys rather than linear per-object scans or tree walks.
- Property offsets are the stable interface between lookup and storage; callers receive an offset and index storage themselves.
- Property tables preserve insertion order for enumerable names without sorting a separate index list on every enumeration.
- Common small property tables can use a compact entry representation while falling back to a wider representation when indices or offsets do not fit.
- Direct offset writes update storage only; callers that replace an existing property must separately notify the Structure.

## Facts

- 2008-09-09 (b6b29e14) measurement: returning slot offsets from PropertyMap instead of JSValue pointers improved SunSpider by 0.6% and reduced coupling between lookup and storage arrays (sourced).
- 2011-03-01 (e635e5f9) measurement: maintaining PropertyTable entries in insertion order avoided allocation and O(n log n) sorting during enumeration, improving SunSpider by 0.5-1% (sourced).
- 2022-05-27 (c21585eb) pitfall: direct offset writes only set storage; callers replacing an existing named property by offset must separately call Structure::didReplaceProperty(offset) (code).

## Moves

- 2002-03-22 (9491afaa) replaced [[linked-list-property-storage]]: Property storage changed from a singly-linked list (O(n) lookup) to an AVL balanced binary search tree (O(log n) lookup/insert/delete) to improve performance for objects with many properties. (code)
- 2002-11-19 (fcfb139a) replaced [[property-map-avl-tree]]: AVL tree gave O(log n) property lookup with per-node heap allocation and pointer chasing; open-addressing hash table keyed on UString::Rep* gives O(1) average lookup with a single flat array allocation, yielding ~7% improvement on iBench. (sourced)
- 2003-04-25 (f185265f) replaced [[property-map-linear-probing]]: Linear probing suffers from primary clustering when the table is heavily loaded; double hashing (step = 1 | (h % sizeMask)) distributes colliding keys more uniformly, yielding a measured 0.7% speedup on iBench JavaScript. (sourced)
- 2008-09-09 (b6b29e14) replaced [[property-map-value-retrieval-by-pointer]]: PropertyMap::get and PropertyMap::getLocation returned JSValue* or JSValue** directly (requiring the PropertyStorage array to be passed in and indexed inside the map lookup), while getOffset returns only the integer slot index; callers then index PropertyStorage themselves, reducing coupling and allowing the extra indirection of passing PropertyStorage into every lookup to be eliminated. (code)
- 2011-03-01 (e635e5f9) replaced [[property-table-index-tagged-sort-on-read]]: Old PropertyMapHashTable stored an ever-increasing 'index' integer in each PropertyMapEntry and a 'lastIndexUsed' counter in the table; getEnumerablePropertyNames sorted a pointer array by index before returning, incurring an allocation and O(n log n) sort per enumeration; the new PropertyTable class maintains entries in insertion order in the value array itself so ordered_iterator can walk them sequentially without sorting, measured at 0.5-1% sunspider improvement. (sourced)
- 2022-04-22 (29dc23b5) replaced [[single-layout-property-table]]: PropertyTable entries gained a compact uint8_t-index/uint8_t-offset representation for common small tables while preserving a non-compact representation for entries whose index or offset cannot fit. (code)
