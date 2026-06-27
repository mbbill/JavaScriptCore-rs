- Bounds-checking memory allocates and exposes only the active byte size. (`Wasm::Memory`)
- Growth reallocates an active-size buffer and copies old contents.
- Instances cache the active memory size for bounds checks.

## Moves

- 2020-11-18 (80581efa) replaced by [[memory-model]]: Shared WebAssembly.Memory can grow on one thread and become immediately accessible on other threads without updating their cached base pointer or bounds-checking size. (sourced)
