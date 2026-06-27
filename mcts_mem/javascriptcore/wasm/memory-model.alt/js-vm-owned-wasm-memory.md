- Wasm memory construction and growth require the JavaScript VM. (`Wasm::Memory`)
- The JavaScript WebAssembly memory object caches base and size state for generated-code access.
- Grow failure is reported through the JavaScript-facing out-of-memory path.

## Moves

- 2017-10-03 (26ecac57) replaced by [[memory-model]]: Wasm::Memory stopped requiring VM/JS so non-JS embedders can supply their own memory-pressure, synchronous-reclamation, and growth-success behavior while keeping Memory as the generated-code source of truth. (sourced)
