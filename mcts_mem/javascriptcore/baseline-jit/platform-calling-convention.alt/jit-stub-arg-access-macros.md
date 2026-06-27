- Stub arguments are read through raw void** arrays and numbered ARG_* macros.

## Moves

- 2009-05-07 (77f3ce7f) replaced by [[platform-calling-convention]]: Raw void** array access via numbered-index macros (ARG_src1, ARG_callFrame) was replaced with a typed JITStackFrame struct giving named, typed fields so the compiler can type-check stub argument access instead of relying on unsafe casts. (code)
