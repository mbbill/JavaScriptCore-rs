- IPInt wasm-to-JS exits use one operation to decide whether multi-value results need unpacking and a second operation to iterate returned JS values.
- Single-result marshalling and multi-result unpacking are separate return paths.

## Moves

- 2025-10-24 (2c1627d7) replaced by [[js-boundary]]: The old IPInt exit path used separate operations to decide whether to unpack multi-value JS results and then iterate them, while the new return marshalling operation handles zero, one, and multi-result cases itself and writes results according to the wasm callee convention. (code)
