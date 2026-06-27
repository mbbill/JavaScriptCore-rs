- Module namespace construction collected exported names and ran full export resolution once for each name.
- Star-export graphs were traversed repeatedly for names in the same namespace.

## Moves

- 2026-04-30 (66c577c4) replaced by [[modules]]: Building a module namespace by running ResolveExport once per exported name walked the star-export graph O(names * star-edges), so the new implementation walks the graph once to cache unique Local/Namespace bindings and falls back only for indirect or conflicting names. (code)
