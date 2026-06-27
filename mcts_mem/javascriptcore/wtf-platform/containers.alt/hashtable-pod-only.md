- HashTable allocates buckets with zero-initialized memory and frees them without explicit element destruction.
- Rehashing copies values rather than moving or swapping them.
- Insert translation constructs values eagerly even when a lookup may fail.

## Moves

- 2005-12-23 (9d272aad) replaced by [[containers]]: The old table used calloc/free and value-copy semantics that required trivially-constructible/destructible types (POD); storing RefPtr<T> in a HashMap caused reference-count thrash on every rehash because values were copied instead of moved; the new implementation uses placement new for initialization, explicit destructor calls for teardown, a Mover template that swaps non-POD values during rehash, and a HashTranslator class approach to defer pair construction during insertion, enabling non-POD types without excess refcount operations. (code)
