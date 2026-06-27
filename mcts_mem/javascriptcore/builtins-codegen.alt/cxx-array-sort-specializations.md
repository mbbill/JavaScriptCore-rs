- Array.prototype.sort used C++ numeric, compacted-vector, qsort, merge, and AVL-tree specializations.
- Bytecode generation pattern-matched numeric comparators to route arrays into native sorting paths.

## Moves

- 2015-04-24 (d42943a4) replaced by [[builtins-codegen]]: Array.prototype.sort moved from C++ specializations to a JavaScript builtin because JavaScript made the operation simpler and less error-prone while providing memory safety, exception safety, and recursion safety. (sourced)
