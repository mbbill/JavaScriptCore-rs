- JSArray carried an ArrayStorage pointer and reached element zero through the storage header.
- shift and unshift moved element values rather than biasing the indexed pointer around reserved prefix space.

## Moves

- 2010-07-27 (9404802c) replaced by [[array-storage]]: JSArray changed from holding an ArrayStorage* pointer (requiring O(n) memmove of all elements for shift/unshift) to holding a JSValue* m_vector pointer directly to ArrayStorage.m_vector[0], plus an int m_indexBias field tracking pre-allocated JSValue slots before the ArrayStorage header, enabling O(1) memmove of only the header for shift and O(1) pointer-bump for unshift when bias space is available. (code)
