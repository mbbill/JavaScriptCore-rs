- HandleHeap::updateWeakHandles finalized all unmarked weak handles in one pass.
- WeakHandleOwner supplied finalization but no opaque-root reachability callback.
- Marking carried no opaque-root set for weak-handle reachability fixpointing.

## Moves

- 2011-04-06 (144410fd) replaced by [[weak-references]]: A single-pass updateWeakHandles cannot support DOM-side reachability through opaque (GC-invisible) roots because it would finalize handles before all reachable handles had been marked; the two-pass design first marks all handles reachable from opaque roots (fixpoint until no new roots discovered) then finalizes the remaining unreachable ones. (sourced)
