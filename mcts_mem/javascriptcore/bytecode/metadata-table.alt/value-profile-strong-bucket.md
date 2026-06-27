- WriteBarrierBase<Unknown> buckets[numberOfBuckets] — strong write-barrier slot array, one per sample bucket
- numberOfSamples() checked !!buckets[i]
- numberOfInt32s/Doubles/Cells read buckets[i].get() directly as JSValue
- no distinction between live and dead cell references

## Moves

- 2011-09-03 (c38b9660) replaced by [[metadata-table]]: ValueProfile buckets stored live GC cells via WriteBarrier (strong refs), making it unsafe to read profiling data after GC completed a collection that did not mark those cells; the WeakBucket approach lets the GC harvest surviving structure/classinfo lazily after the mark phase without keeping profiled cells alive. (sourced)
