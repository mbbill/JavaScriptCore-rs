- Typed-array get_by_val and put_by_val fall through to the generic C stub path.

## Moves

- 2012-10-10 (b04ba9ca) replaced by [[inline-caches]]: Typed array get_by_val/put_by_val in the baseline JIT always fell through to the generic C stub (cti_op_get_by_val_generic) because jitArrayModeForIndexingType only handled regular indexed storage; extending the dispatch to jitArrayModeForStructure covers typed array ClassInfo and emits inline typed array stubs, gaining ~40% on benchmarks that bail from DFG to baseline. (sourced)
