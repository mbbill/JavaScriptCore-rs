- JIT thunks are generated lazily through a generator-keyed hash map.

## Moves

- 2023-11-28 (b1de44f2) replaced by [[call-linking-stubs]]: Frequently used thunks are always needed when JIT is enabled, so pre-generating them at JITThunks initialization makes lookup an enum-indexed array access instead of a lazy ThunkGenerator hash-map lookup. (sourced)
