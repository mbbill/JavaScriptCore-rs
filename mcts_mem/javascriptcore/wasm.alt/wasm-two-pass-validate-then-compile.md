- Module validation runs as a serial pass before concurrent function compilation. (`WasmValidate`)
- A generated validation helper layer implements the same function-parser interface used by compilation.

## Moves

- 2019-12-05 (da014010) replaced by [[wasm]]: Merged serial validation pass into the concurrent bytecode-generation pass so that all functions are validated and compiled in one concurrent traversal instead of a serial validate step followed by a concurrent compile step, yielding a 1.5x compile-time speedup on ZenGarden. (sourced)
