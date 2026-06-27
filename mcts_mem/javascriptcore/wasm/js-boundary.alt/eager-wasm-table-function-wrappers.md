- Active element segments eagerly materialize WebAssemblyFunction wrappers for function references stored into tables.
- Funcref table copying observes wrapper cells rather than preserving latent wasm-side function metadata.

## Moves

- 2026-05-17 (ca0c6bb1) replaced by [[js-boundary]]: Active element segments now install wasm-side function metadata and materialize WebAssemblyFunction wrappers only when JS observes the slot, avoiding wrapper/JSToWasmCallee allocation for entries used only by wasm call_indirect. (code)
