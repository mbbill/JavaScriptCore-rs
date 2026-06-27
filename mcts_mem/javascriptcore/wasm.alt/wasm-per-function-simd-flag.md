- Per-function feature tracking records only whether a function uses SIMD. (`FunctionData`)
- Module metadata exposes SIMD-specific query and marking helpers.

## Moves

- 2023-03-01 (43f182b3) replaced by [[wasm]]: A single SIMD-only per-function flag could not represent the additional exception and atomic feature predicates needed to switch behavior by wasm function feature. (code)
