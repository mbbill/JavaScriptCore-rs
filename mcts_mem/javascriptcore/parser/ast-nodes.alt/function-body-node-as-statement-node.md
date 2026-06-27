- Function body metadata was modeled as a statement node.
- The node exposed statement-like linkage and bytecode hooks despite being metadata for a function body.

## Moves

- 2015-08-10 (d44fa932) replaced by [[ast-nodes]]: Function metadata can appear in expression context and has no next statement, so modeling it as a StatementNode was the wrong AST type. (sourced)
