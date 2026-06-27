- SIMD extmul high/low is lowered as an extend-low/high operation followed by a generic vector multiply.

## Moves

- 2025-06-05 (34349713) replaced by [[bbq-tier]]: Wasm SIMD extmul_high/extmul_low map directly to VectorMulHigh/VectorMulLow, avoiding the previous extend-low/high plus generic VectorMul sequence whose VectorMul operation was significantly more costly, especially on ARM64. (code)
