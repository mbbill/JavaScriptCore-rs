- Wasm catch handling temporarily swaps a JSCell module into the callee slot and restores the real wasm callee from a VM side table.
- Interpreter catch code derives the VM through JS-cell allocation metadata.

## Moves

- 2023-01-23 (47d91b3b) replaced by [[interpreter-tier]]: The catch path no longer swaps a JSCell into the callee slot because LLInt and JIT catch code can get the VM from a wasm callee via the Instance stored in the codeBlock slot. (code)
