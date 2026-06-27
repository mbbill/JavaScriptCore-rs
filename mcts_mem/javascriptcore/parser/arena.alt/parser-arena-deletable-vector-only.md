- ParserArena tracked deletable and ref-counted parser objects in vectors.
- Destructor-free AST nodes still allocated individually through the general allocator.
- Identifiers were owned by lexer-side storage rather than the arena.

## Moves

- 2009-08-22 (c2becfdb) replaced by [[arena]]: The previous ParserArena only tracked objects needing deletion (ParserArenaDeletable) and ref-counted objects; most Node subclasses have no destructors worth calling, yet paid the cost of individual FastMalloc allocations. Adding a bump-pointer 'freeable pool' for destructor-free nodes avoids per-node malloc overhead, yielding 0.6% SunSpider speedup. (sourced)
