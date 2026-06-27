- Constructing a Map or Set iterator materialized collection storage eagerly.
- Empty or uninitialized collections could allocate storage before the iterator was advanced.

## Moves

- 2025-11-12 (bde9f45e) replaced by [[map-set-table]]: Iterator construction no longer has to materialize empty Map/Set storage or throw from storage allocation; nextWithAdvance can distinguish an uninitialized iterator storage field from the VM sentinel and acquire storage lazily. (code)
