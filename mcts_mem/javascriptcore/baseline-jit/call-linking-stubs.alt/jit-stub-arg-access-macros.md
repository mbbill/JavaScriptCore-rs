- JIT stubs read arguments through raw void** arrays and numbered ARG_* macros.
- Stub signatures rely on casts rather than named typed fields.

## Moves

- 2009-05-07 (77f3ce7f) replaced by [[call-linking-stubs]]: Raw void** array access via numbered-index macros (ARG_src1, ARG_callFrame) was replaced with a typed JITStackFrame struct giving named, typed fields so the compiler can type-check stub argument access instead of relying on unsafe casts. (code)
