- Async module completion walks parent module graphs using native recursion.
- Rejection propagation recursively calls into each async parent.
- Deep async module graphs depend on native stack depth instead of an explicit worklist.

## Moves

- 2026-06-10 (5c64352c) replaced by [[promises-and-microtasks]]: GatherAvailableAncestors and AsyncModuleExecutionRejected are infallible microtask operations over potentially deep async module graphs, so native recursion is replaced with explicit worklists to avoid hard stack overflows without throwing RangeError. (sourced)
