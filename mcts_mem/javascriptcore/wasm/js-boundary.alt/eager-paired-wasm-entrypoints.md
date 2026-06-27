- Every internal wasm function is compiled with both a wasm entrypoint and a JavaScript-to-wasm wrapper.
- Wrapper generation is coupled to each function-body compilation.

## Moves

- 2017-06-08 (6d0ffd96) replaced by [[js-boundary]]: Only functions reachable from exports, element segments, or the start function need JavaScript-to-WebAssembly wrappers; internal-only functions keep only their wasm entrypoint. (code)
