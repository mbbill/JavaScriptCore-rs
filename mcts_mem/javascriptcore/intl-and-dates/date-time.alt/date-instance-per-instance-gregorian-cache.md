- Each DateInstance lazily allocated and owned its own cache of GregorianDateTime expansions.
- Distinct Date objects with the same millisecond value did not share cached expansion data.

## Moves

- 2009-10-27 (8a1d0723) replaced by [[date-time]]: Per-instance heap-allocated Cache was allocated lazily on first access and owned by DateInstance (delete in destructor); replaced by a 64-entry fixed-size hash table in JSGlobalData shared across all DateInstance objects because benchmark patterns access many distinct DateInstance objects with the same ms value, so a cross-instance cache hits where per-instance caches cold-miss. SunSpider reports ~0.5% speedup. (sourced)
