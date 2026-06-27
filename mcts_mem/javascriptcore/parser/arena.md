- AST nodes allocate from a parse-owned slab arena; ordinary nodes are bulk-freed and nodes with destructors are tracked for ordered teardown.
- Source ownership is decoupled from parse lifetime through source-provider objects and value-type source ranges.
- Function parameters, destructuring patterns, and function-body metadata live in the same parse lifetime domain as the function they describe.
- Identifier allocation is attached to parser lifetime instead of global or per-token ownership.

## Facts

- 2011-02-11 (149f27e5) rationale: `SourceProviderCache` exposes clear so WebCore can release cached parser data when cached script decoded data is destroyed. (sourced)
- 2011-11-07 (3271973e) rationale: Passing `StringImpl` to the lexer preserves the backing string width for identifier and string allocation. (code)
- 2015-12-13 (061ccdc3) pitfall: Source-provider hashing cannot take a `StringImpl` from a temporary `StringView`; hashing moved onto the provider itself when source access became view-based. (code)

## Moves

- 2009-08-22 (c2becfdb) replaced [[parser-arena-deletable-vector-only]]: The previous ParserArena only tracked objects needing deletion (ParserArenaDeletable) and ref-counted objects; most Node subclasses have no destructors worth calling, yet paid the cost of individual FastMalloc allocations. Adding a bump-pointer 'freeable pool' for destructor-free nodes avoids per-node malloc overhead, yielding 0.6% SunSpider speedup. (sourced)
- 2014-12-03 (579e5edd) replaced [[vm-owned-global-parser-arena]]: There's no need to keep a global arena. We can create a new arena each time we parse. (sourced)
- 2014-12-05 (086e0611) replaced [[parser-arena-refcounted-roots]]: Once each parse tree had a clear root node type, parse-tree ownership no longer needed a type that could be either refcounted or arena-allocated and could instead be managed with unique_ptr and normal C++ destructors. (sourced)
- 2015-07-17 (7a073296) replaced [[separate-function-parameter-parse-arena]]: A function's parameters are now parsed in the same arena as the function itself so destructuring AST nodes and FunctionParameters can be arena allocated and ES6 default parameter values can be implemented sanely. (sourced)
