- Map and Set share ordered storage while keeping Map value slots and Set key-only semantics as trait choices.
- The backing table is a flat ordered hash table with bucket metadata, data entries, and obsolete-table links for live iterators.
- Rehash, clear, and lazy materialization preserve iterator progress without tracking every iterator object.
- Collection constructors and Set-method builtins use direct storage fast paths only under watchpoints that prove iteration and add/has calls are non-observable.

## Facts

- 2013-08-30 (4821a89d) rationale: ES6 Map originally used separate string-key and JSValue-key hash maps plus a flat entry array because strings compare by value while other keys use identity/SameValueZero, with the entry array preserving insertion order. (code)
- 2016-09-06 (faa62bee) rationale: Removed buckets pointed forward to later buckets or tail so iterators could recover without the map tracking every iterator. (code)
- 2017-05-27 (d0b6a1be) measurement: Map/Set constructor clone fast paths improved ARES-6 Air steady state by about 5.3%. (sourced)

## Moves

- 2015-03-12 (4ce89cb4) replaced [[separate-jscell-mapdata-storage]]: Embedding specialized MapData/SetData into JSMap/JSSet removes two object allocations per collection and lets SetData omit the dummy value field, halving set entry storage. (code)
- 2017-05-20 (dd0087fd) replaced [[map-set-wrapper-owned-hashmapimpl]]: JSMap and JSSet can directly inherit HashMapImpl, eliminating one indirection when accessing the map implementation and one allocation per Map or Set. (code)
- 2024-07-08 (b451fca3) replaced [[bucket-linked-jsmap-jsset-storage]]: JSMap and JSSet storage was changed from HashMapImpl<HashMapBucket<Data>> to a flattened CloseTable OrderedHashTable to reduce memory use, improve cache locality, and measured about 1.14x faster geometrically on map microbenchmarks. (code)
- 2025-11-12 (bde9f45e) replaced [[eager-map-set-iterator-storage]]: Iterator construction no longer has to materialize empty Map/Set storage or throw from storage allocation; nextWithAdvance can distinguish an uninitialized iterator storage field from the VM sentinel and acquire storage lazily. (code)
