- Per-context `JSClassRef` data is stored with the context-free class descriptor.
- Static-entry strings and prototype cache state are not separated by current global object.

## Moves

- 2008-07-23 (26073881) replaced by [[opaque-embedding]]: per-context `JSClassRef` data moved out of the context-free class descriptor because one `JSClassRef` can be used in multiple contexts and context groups; context-specific static-entry strings and prototype caches must be looked up through the current global object. (sourced)
