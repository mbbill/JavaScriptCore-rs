- Varargs frame setup assumes execution begins from bytecode with a complete callee call frame.

## Moves

- 2015-02-10 (afa064cb) replaced by [[unlinked-code-sharing]]: Higher-tier and inlined varargs calls do not literally execute bytecode and may need to load arguments somewhere other than a full callee call frame. (sourced)
