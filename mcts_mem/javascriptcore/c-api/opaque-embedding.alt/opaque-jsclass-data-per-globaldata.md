- Opaque class context data is cached per VM/global-data object.
- Cached API prototypes can be shared across all globals in that VM.

## Moves

- 2013-03-22 (c1225a67) replaced by [[opaque-embedding]]: Sharing JSClassRef prototype cache across all JSGlobalObjects in a JSGlobalData caused the first GlobalObject that created the prototype to be retained indefinitely; moving the cache into JSGlobalObjectRareData scopes it to the owning context and fixes the leak. (sourced)
