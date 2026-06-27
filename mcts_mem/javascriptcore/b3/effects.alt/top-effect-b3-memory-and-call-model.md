- Load and store effects used HeapRange::top rather than per-memory ranges.
- CCall and Patchpoint were modeled as top read/write effects with sideways exits and control dependence.

## Moves

- 2015-11-04 (b0789418) replaced by [[effects]]: Values with effects needed custom HeapRanges and effect summaries so memory operations, calls, and patchpoints would not all be modeled as reading or writing HeapRange::top(). (code)
