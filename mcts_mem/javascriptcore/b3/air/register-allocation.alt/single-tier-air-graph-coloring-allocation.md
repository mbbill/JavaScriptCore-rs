- Air always used graph-coloring allocation for non-spill-everything generation.
- Low B3 optimization levels still paid graph-coloring allocator cost.
- There was no low-latency Air allocation path for optLevel 0 or 1.

## Moves

- 2017-03-30 (abe97c60) replaced by [[register-allocation]]: B3 opt levels were split so low opt levels use a faster linear-scan allocator while the full optimization level continues to use graph coloring for better generated code. (sourced)
