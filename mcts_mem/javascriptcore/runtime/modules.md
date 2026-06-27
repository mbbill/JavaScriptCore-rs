- Modules are represented as VM cells and records with host loader hooks, not as plain script source strings.
- Module namespace objects cache export resolutions and expose dedicated namespace-load behavior separate from ordinary object property lookup.
- Module loader internals are installed as private VM operations, keeping loader helper frames and methods out of the public JavaScript-visible surface.
- Module namespace storage is fixed-size when export metadata can live in table entries rather than in variable trailing cell storage.

## Facts


## Moves

- 2017-02-21 (24801203) replaced [[module-namespace-resolve-export-slowpath]]: Module namespace property access moved from per-access resolveExport with caching disabled to cached export resolutions plus a namespace-cell-guarded IC that loads directly from the module environment. (code)
- 2019-12-07 (dfcc6400) replaced [[module-namespace-object-variable-sized-cell]]: JSModuleNamespaceObject stored an embedded AbstractModuleRecord array at variable trailing offset requiring dynamic allocation sizing; moving moduleRecord into ExportEntry (stored in the HashMap) eliminated the variable-sized trailing array so the cell becomes fixed-size and eligible for IsoSubspace. (code)
- 2022-08-21 (254430a0) replaced [[module-loader-lut-public-methods]]: Internal module-loader functions were marked private so they would not be exposed in Error stacks. (sourced)
