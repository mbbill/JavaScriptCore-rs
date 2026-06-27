- Structure stored one boolean dictionary flag.
- Addition-overflow and property-removal dictionaries used the same dictionary predicate.
- Inline caches bailed out for all dictionaries under the single flag.

## Moves

- 2009-09-21 (07eb57bd) replaced by [[structure-shapes]]: A single bool m_isDictionary cannot distinguish dictionaries created by property removal (whose slot offsets may be reused) from those created by property addition overflow (whose existing slots are stable), so property-access caching was disabled for both; splitting into NoneDictionaryKind/CachedDictionaryKind/UncachedDictionaryKind allows IC caching on addition-overflow dictionaries while still skipping it on removal dictionaries. (sourced)
