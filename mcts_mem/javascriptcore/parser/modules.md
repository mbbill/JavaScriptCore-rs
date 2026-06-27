- Module parsing records imports, exports, and top-level lexical bindings separately from the later linked module environment.
- Module namespace objects expose sorted live bindings through custom immutable accessors.
- Star-export and namespace resolution maintain graph-local state and caches rather than resolving each name independently from scratch.
- Module loader and bytecode paths treat module code as a top-level form with no completion-value requirement.

## Facts

- 2015-09-01 (1917329d) rationale: The module environment symbol table lives on the unlinked module code block and is filled during bytecode generation so the module environment can be instantiated before the module body executes. (code)
- 2015-09-01 (1917329d) rationale: Unlinked module code blocks can be cached because they hold symbol-table layout without a concrete module environment; linked module code blocks cannot be cached because imports resolve to a specific module-environment set. (sourced)
- 2015-09-05 (cc5e7e75) rationale: Namespace exports are sorted by code point order, exposed through custom accessors that resolve live bindings, and made externally immutable despite descriptors appearing writable. (code)
- 2015-09-14 (6cd07f2f) rationale: Export resolution is cached because import linking, namespace lookup, and namespace construction repeatedly ask for the same pure module/export-name resolution. (sourced)
- 2026-04-30 (66c577c4) measurement: Namespace creation for 9000 names through about 1500 star edges dropped from 331 ms to 19 ms after switching to a single-walk star-resolution path. (sourced)

## Moves

- 2016-07-31 (a34b8996) replaced [[module-export-single-alias-map]]: A module local binding can be exported under multiple names, so export collection needs a per-module local-name to exported-name set instead of a single alias per local binding. (code)
- 2026-04-30 (66c577c4) replaced [[module-namespace-per-name-star-resolution]]: Building a module namespace by running ResolveExport once per exported name walked the star-export graph O(names * star-edges), so the new implementation walks the graph once to cache unique Local/Namespace bindings and falls back only for indirect or conflicting names. (code)
- 2026-05-12 (28112c01) replaced [[module-namespace-separate-name-vector-and-export-map]]: OrderedHashMap preserves namespace export order while letting construction populate and freeze the map before GC exposure, eliminating the separate names vector and the GC-marking lock. (sourced)
