- Weak reachability uses WeakSet storage owned by each MarkedBlock or PreciseAllocation plus collection-time callbacks.
- External strong and weak handles are GC-visible slots in HandleSet/HandleHeap storage.
- Weak processing is part of the marking fixpoint with callbacks and opaque-root reachability.
- Finalization is separate from weak marking.
- Ephemeron-like maps and WeakGCMap entries are pruned by GC reachability.

## Facts

- 2011-11-15 (0e4f3b5f) pitfall: weak reference harvesters may append more marking work, so weak harvesting must repeat until the visitor work stack is empty (code).
- 2016-07-19 rationale: per-block WeakSet storage keeps weak cells colocated with the allocation container that owns their liveness metadata (code).
- 2020-04-29 rationale: WeakGCMap entries are swept by GC reachability so keys that die in the JS heap do not keep embedder-side state alive (code).

## Moves

- 2011-04-06 (144410fd) replaced [[weak-handle-single-pass-update]]: A single-pass updateWeakHandles cannot support DOM-side reachability through opaque (GC-invisible) roots because it would finalize handles before all reachable handles had been marked; the two-pass design first marks all handles reachable from opaque roots (fixpoint until no new roots discovered) then finalizes the remaining unreachable ones. (sourced)
- 2011-11-15 (0e4f3b5f) replaced [[single-pass-weak-reference-harvesting]]: Weak reference harvesters can add new mark work, so weak handles and harvesters are repeated until the visitor stack is empty. (code)
