- Global state save/restore copies the symbol table separately from the local-storage values.
- Cached pages keep a saved SymbolTable alongside saved property values.
- Back/forward cache restoration reconstructs names and values through two passes.

## Moves

- 2008-02-03 (13eb087e) replaced by [[scope-chain-and-activation]]: The old design saved the symbol table (name->index hash map) and local storage (index->value vector) as separate operations, requiring two full copies of the hash table during back/forward cache creation; the new design iterates the symbol table once and emits {name,value,attributes} tuples into SavedProperties, eliminating the separate symbol table copy and the need for CachedPage to hold a SymbolTable. (sourced)
