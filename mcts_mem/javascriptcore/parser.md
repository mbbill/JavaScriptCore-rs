- JavaScript parsing is a hand-written recursive-descent pipeline parameterized by a tree-builder, allowing the same grammar walk to build an AST or validate syntax without allocation.
- The lexer is specialized by source character width and owns token-side allocations across success and error exits.
- Parse modes and parser savepoints carry the context needed for functions, eval, modules, classes, destructuring, and speculative cover-grammar parsing.
- JSON and SloppyJSON literal parsing use explicit parser modes rather than separate engines for eval pre-parsing and strict JSON parsing.

## Facts

- 2002-11-24 (bf6078c5) pitfall: Building argument lists right-to-left in grammar actions caused parser stack overflow on very long argument lists; switching to left-recursive grammar and reversing the list kept source order without native stack growth. (sourced)
- 2010-06-24 (10b06516) measurement: Replacing the yacc-generated parser with recursive descent was reported as more than 2x faster on SunSpider parsing tests. (sourced)
- 2011-11-07 (3271973e) rationale: Passing source as `StringImpl` lets the lexer select Latin-1 or UTF-16 backing storage before scanning rather than widening every source to UTF-16. (code)

## Moves

- 2003-10-30 (b88c4f2b) replaced [[lexer-token-ownership-by-grammar]]: Grammar action ownership (delete yyvsp[0].ustr / delete yyvsp[0].ident in each production) leaked on parse error paths because error recovery skipped cleanup; moving ownership to the lexer via doneParsing() ensures cleanup on all exit paths including errors. (sourced)
- 2007-10-15 (418c2a12) dropped: nested function scope hack — Legacy code from a large merge placed nested function declarations as named properties of their enclosing function object and pushed the enclosing function into the nested function's scope chain; this contradicted Firefox, IE, and the ECMA spec, incurred a parse-time performance penalty (processing nested declarations recursively during parent parse), and had no documented rationale in SVN history. (sourced)
- 2009-06-13 (c69b558f) replaced [[literal-parser-recursive-descent]]: Recursive descent with a depth-limit StackGuard (depth<10) can stack-overflow on deeply nested JSON; replacing it with a hand-rolled PDA (explicit stateStack/objectStack vectors) eliminates native call-stack growth entirely. (sourced)
- 2010-06-24 (10b06516) replaced [[yacc-bison-js-parser]]: The yacc/bison-generated parser (jscyyparse) was replaced by a hand-written recursive-descent parser (JSParser) modeled after V8 and SpiderMonkey, achieving greater than 2x improvement on SunSpider parsing tests; the recursive-descent form also enables a separate SyntaxChecker tree-builder that validates syntax without allocating AST nodes. (sourced)
- 2016-01-22 (c143d94a) replaced [[parser-lexer-only-savepoint]]: The old SavePoint name implied whole-parser rollback while it only restored lexer position, so parser state mutated by speculative parsing had to be saved separately and was easy to misuse. (code)
- 2025-03-03 (65ae8f7a) replaced [[sloppy-json-eval-iterative-literal-parser]]: SloppyJSON eval preprocessing moved onto the recursive JSON parser by parameterizing recursive descent with ParserMode so it can accept SloppyJSON property keys and parenthesized literals while preserving strict JSON behavior. (code)
