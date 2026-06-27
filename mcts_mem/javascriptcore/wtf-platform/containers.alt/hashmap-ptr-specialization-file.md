- Pointer-key HashMap behavior is handled by a separate specialization header.
- Pointer iterators wrap raw hash iterators with adapter types.
- Ref-counted pointer keys cannot choose raw-pointer backing storage through ordinary traits.

## Moves

- 2006-04-05 (1c33acee) replaced by [[containers]]: The old per-type specialization file (HashMapPtrSpec.h) could not express a hash table over RefPtr<StringImpl> that uses raw-pointer storage with -1 as deleted value without a global-initializer static; the new StorageTraits mechanism lets any type declare an underlying storage type sharing the same HashTable instantiation, eliminating the global initializer. (sourced)
