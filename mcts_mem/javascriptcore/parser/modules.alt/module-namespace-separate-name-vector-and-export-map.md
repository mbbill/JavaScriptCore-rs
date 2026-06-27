- Module namespace objects kept an export map and a separate ordered vector of export names.
- GC marking took a lock while walking export records exposed through that separate storage.

## Moves

- 2026-05-12 (28112c01) replaced by [[modules]]: OrderedHashMap preserves namespace export order while letting construction populate and freeze the map before GC exposure, eliminating the separate names vector and the GC-marking lock. (sourced)
