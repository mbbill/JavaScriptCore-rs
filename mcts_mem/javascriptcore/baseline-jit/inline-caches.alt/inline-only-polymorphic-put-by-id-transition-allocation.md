- Polymorphic put_by_id transition ICs only cache transitions whose storage allocation can be emitted inline.

## Moves

- 2016-04-08 (dae57718) replaced by [[inline-caches]]: The IC put_by_id transition path needed to cache reallocating transitions even when the butterfly had indexing storage, so those cases call JSObject reallocation operations while keeping inline allocation for non-indexing butterflies. (code)
