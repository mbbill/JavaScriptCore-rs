- CacheMap stores entries in a hash map plus a fixed side array.
- Eviction chooses a WeakRandom-generated side-array slot.
- The cache carries an extra randomness source solely for eviction.

## Moves

- 2013-02-15 (e99e2204) replaced by [[containers]]: The old CacheMap used a side FixedArray indexed by a WeakRandom-generated slot number for eviction, requiring both a HashMap<key,index> and the FixedArray; the commit notes that hash tables are already pseudo-random so a second randomness source adds complexity without benefit, and the new implementation simply removes the first HashMap entry (FIFO) when the map is full. (sourced)
