- Every collection used one full-heap operation.
- Each collection cleared all mark bits and traversed all live objects.
- Allocation limits were recomputed after a single collection kind rather than after Eden and Full scopes.

## Moves

- 2014-01-09 (36fb03f0) replaced by [[heap]]: Re-marking the same objects over and over is a waste of effort, so the sticky mark bit algorithm uses EdenCollections to visit only new objects or objects added to the remembered set while FullCollections still visit all objects. (sourced)
