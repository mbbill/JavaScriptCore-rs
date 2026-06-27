- Every HashTable uses one hard-coded minimum capacity.
- Shrinking and expansion compare against the same fixed floor regardless of key or value traits.
- Tiny hash-table users pay the same initial bucket allocation as larger maps.

## Moves

- 2011-08-29 (5c165f17) replaced by [[containers]]: Hard-coding m_minTableSize=64 inside HashTable prevented individual key types from choosing a smaller initial capacity, forcing all hash tables to allocate at least 64-slot arrays even for collections that are almost always tiny; moving minimumTableSize into HashTraits lets callers specialize it per key type. (sourced)
