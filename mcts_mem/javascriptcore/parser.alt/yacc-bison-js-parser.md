- JavaScript parsing entered a generated yacc/bison parser through a single `jscyyparse` call.
- The grammar pass built AST nodes directly and had no syntax-only tree-builder mode.

## Moves

- 2010-06-24 (10b06516) replaced by [[parser]]: The yacc/bison-generated parser (jscyyparse) was replaced by a hand-written recursive-descent parser (JSParser) modeled after V8 and SpiderMonkey, achieving greater than 2x improvement on SunSpider parsing tests; the recursive-descent form also enables a separate SyntaxChecker tree-builder that validates syntax without allocating AST nodes. (sourced)
