- JSObject::visitChildren scanned a butterfly using one structure/butterfly pair.
- Dictionary structures were protected by Structure locking during visits.
- Indexed-property scanning used cell locks and load-load fences to avoid seeing a mismatched structure and butterfly.

## Moves

- 2016-12-08 (12e75c3d) replaced by [[object-model]]: The new protocol makes structure/butterfly transitions detectable as BEFORE, AFTER, or IGNORE by inserting a nuked StructureID between structure-size and butterfly updates and by having the collector read structure and lastOffset both before and after reading the butterfly. (code)
