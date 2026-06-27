- get_by_id_chain compiles a standalone stub and redirects the slow-case call to it.
- Success returns to the caller through call/ret rather than linking into the hot path.

## Moves

- 2008-11-24 (42c2303a) replaced by [[inline-caches]]: The old form compiled a standalone stub and redirected the slow-case call to it; the new form (CTI_REPATCH_PIC) links the stub's failure path back to the original slow-case code in the hot patch and links success directly into the hot path's store sequence, eliminating a call/ret round-trip and yielding a 3% progression on deltablue. (sourced)
