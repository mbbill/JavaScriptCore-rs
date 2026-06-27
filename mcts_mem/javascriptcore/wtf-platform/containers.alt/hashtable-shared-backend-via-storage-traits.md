- Pointer and RefPtr hash tables reinterpret their keys as integer storage.
- A shared integer HashTable instantiation backs multiple pointer-keyed table types.
- HashKeyStorageTraits redirects pointer hashing and traits through integer storage aliases.

## Moves

- 2008-04-28 (35810d6f) replaced by [[containers]]: The StorageTraits/HashKeyStorageTraits mechanism reinterpreted pointer and RefPtr keys as integers so one HashTable<int> back-end could serve all pointer-keyed tables, but reinterpreting pointer storage through integer aliases violates C99/C++03 strict-aliasing rules and broke with GCC 4.2 -fstrict-aliasing; each key type now gets its own HashTable instantiation. (sourced)
