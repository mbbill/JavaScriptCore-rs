- Wasm LLInt catch and catch_all handlers return LLInt code references tagged directly with ExceptionHandlerPtrTag.
- Catch entrypoints are selected from LLInt opcodes rather than generated JIT thunks.

## Moves

- 2021-12-11 (9005bd84) replaced by [[interpreter-tier]]: ExceptionHandlerPtrTag is only valid for JITCode, so Wasm LLInt catch entrypoints now route through generated JIT thunks when JIT is enabled instead of tagging LLInt code directly. (code)
