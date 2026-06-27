- B3 values carry effect summaries over heap ranges, terminal/control behavior, sideways exits, fences, mutability, and local-state writes.
- Memory operations carry AbstractHeap ranges for alias and CSE reasoning across FTL and Wasm clients.
- C calls expose explicit effects, including side-effect-free calls, instead of a special pure-function tag.

## Facts

- 2015-12-02 (73609e0a) rationale: pure C calls are exposed to B3 because DFG lowering has many calls without side effects, and side-effect-free calls should not prevent optimizations (sourced).
- 2016-09-21 (12406d64) rationale: standalone fences use read/write HeapRange parameters because store-load and store-store fences are needed by concurrent GC, while load-store/load-load ordering should use fenced loads or dependencies (sourced).
- 2016-09-21 (12406d64) pitfall: a fence with an empty write heap would otherwise look side-effect-free enough to be killed, so Fence effects set writesLocalState even when they do not write modeled heap memory (code).
- 2025-09-23 (0eea29cd) rationale: AbstractHeapRepository moved from FTL to B3 so both FTL and OMG can decorate B3 memory operations with the same ranges and mutability metadata before optimization (code).
- 2025-09-23 (0eea29cd) pitfall: abstract heap decoration runs after OMG lowering because numbered, indexed, and absolute heaps are generated lazily and only heaps mentioned by lowering can be assigned final ranges (code).

## Moves

- 2015-11-04 (b0789418) replaced [[top-effect-b3-memory-and-call-model]]: Values with effects needed custom HeapRanges and effect summaries so memory operations, calls, and patchpoints would not all be modeled as reading or writing HeapRange::top(). (code)
- 2015-12-02 (af839507) replaced [[b3-purefunctiontag-ccall]]: Filip prefers explicit effects. (sourced)
- 2025-09-22 (570a3530) replaced [[b3-cse-mutable-memory-loads-only]]: The old memory-effect model could only invalidate prior loads on overlapping writes, while the new Mutability bit represents loads whose result is stable across clobbers and lets CSE keep them. (code)
