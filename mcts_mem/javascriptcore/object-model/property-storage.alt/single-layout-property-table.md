- PropertyTable stored one index-vector and PropertyMapEntry layout for all table sizes.
- PropertyMapEntry carried key, property offset, and attributes as direct fields.
- Callers iterated and mutated PropertyMapEntry records through direct pointers.

## Moves

- 2022-04-22 (29dc23b5) replaced by [[property-storage]]: PropertyTable entries gained a compact uint8_t-index/uint8_t-offset representation for common small tables while preserving a non-compact representation for entries whose index or offset cannot fit. (code)
