- Eval code uses one VM-global CodeCache keyed by source.
- Cached eval code does not encode the scope-depth assumptions under which var references were compiled.
- A global working set size governs all eval cache entries.

## Moves

- 2013-05-09 (4e61ee97) replaced by [[scope-chain-and-activation]]: The single global CodeCache for eval was unsound: it could return a cached unlinked code block containing var-reference offsets computed under one function's scope for execution under a different function's scope (or inside |with|/|catch|), producing bogus variable lookups; replaced with per-UnlinkedCodeBlock caches (NonGlobalCodeCache, 20x smaller limits) used only when the eval is at the top of the scope chain with no intervening activation objects. (code)
