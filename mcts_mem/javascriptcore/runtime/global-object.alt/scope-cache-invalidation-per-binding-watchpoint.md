- Each added global lexical binding invalidates caches by scanning live code blocks.
- Resolve-scope bytecode entries are cleared individually when their identifier may be shadowed.
- Optimized code watches individual global-property identifiers for shadowing.

## Moves

- 2019-01-21 (b145cc48) replaced by [[global-object]]: The old mechanism iterated all live CodeBlocks and all op_resolve_scope instructions to clear per-entry caches whenever any lexical binding was added; the new epoch counter stored in op_resolve_scope metadata and compared inline in LLInt/JIT avoids the O(codeblocks × instructions) scan and also handles the case where a global property was deleted then re-shadowed without requiring property-specific watchpoints. (code)
