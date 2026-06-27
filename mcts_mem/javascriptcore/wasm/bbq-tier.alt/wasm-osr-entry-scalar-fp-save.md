- Wasm OSR entry saves live floating-point state through the scalar FP register probe layout.
- OSR scratch-buffer indexing reserves one 64-bit slot for each live value.

## Moves

- 2022-12-22 (a820d89a) replaced by [[bbq-tier]]: Wasm OSR entry selects a vector-saving probe and doubles scratch-buffer slots for SIMD functions so live V128 values are preserved instead of truncating FP registers to scalar doubles. (code)
