- fixSSA eagerly computed variable liveness and Phi placement for all remaining variables.
- Reaching definitions were queried by walking dominators without cached intermediate results.
- Dead Sets and sparse maps were not used to avoid global SSA work.

## Moves

- 2017-04-05 (001eb863) replaced by [[reduce-strength]]: Local SSA conversion, dead Set removal, sparse variable maps, cached reaching definitions, and lazy mapping reduce the amount of global SSA work needed after wasm made fixSSA a top compile-time cost. (sourced)
