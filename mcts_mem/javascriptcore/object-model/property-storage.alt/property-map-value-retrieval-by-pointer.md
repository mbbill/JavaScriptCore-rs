- PropertyMap::get returned JSValue pointers into the passed PropertyStorage array.
- PropertyMap::getLocation returned JSValue pointer locations for callers that would mutate storage.
- Lookup APIs required passing PropertyStorage into the map for storage indexing.

## Moves

- 2008-09-09 (b6b29e14) replaced by [[property-storage]]: PropertyMap::get and PropertyMap::getLocation returned JSValue* or JSValue** directly (requiring the PropertyStorage array to be passed in and indexed inside the map lookup), while getOffset returns only the integer slot index; callers then index PropertyStorage themselves, reducing coupling and allowing the extra indirection of passing PropertyStorage into every lookup to be eliminated. (code)
