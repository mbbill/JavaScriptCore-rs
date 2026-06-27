- Every import record embeds DataOnlyCallLinkInfo directly.
- Wasm-to-JS fast paths compute the call-link-info address by offsetting into the instance import record.

## Moves

- 2025-01-17 (717c7964) replaced by [[js-boundary]]: The import record stopped embedding DataOnlyCallLinkInfo because not all imports are JS calls and JIT-less calls through FuncRefTable/WebAssemblyFunctionBase need a maintained CallLinkInfo pointer to the original instance slot. (code)
