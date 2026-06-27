- Module namespace objects are destructible objects with variable-sized trailing storage.
- Each namespace object embeds an array of module-record write barriers after the object header.
- Allocation size depends on the number of module records referenced by the namespace.

## Moves

- 2019-12-07 (dfcc6400) replaced by [[modules]]: JSModuleNamespaceObject stored an embedded AbstractModuleRecord array at variable trailing offset requiring dynamic allocation sizing; moving moduleRecord into ExportEntry (stored in the HashMap) eliminated the variable-sized trailing array so the cell becomes fixed-size and eligible for IsoSubspace. (code)
