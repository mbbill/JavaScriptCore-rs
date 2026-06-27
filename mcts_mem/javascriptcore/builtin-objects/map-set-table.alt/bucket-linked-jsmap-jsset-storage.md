- Map and Set storage used linked bucket cells under HashMapImpl.
- Iteration survival was encoded through bucket links rather than flat ordered storage with obsolete-table forwarding.

## Moves

- 2024-07-08 (b451fca3) replaced by [[map-set-table]]: JSMap and JSSet storage was changed from HashMapImpl<HashMapBucket<Data>> to a flattened CloseTable OrderedHashTable to reduce memory use, improve cache locality, and measured about 1.14x faster geometrically on map microbenchmarks. (code)
