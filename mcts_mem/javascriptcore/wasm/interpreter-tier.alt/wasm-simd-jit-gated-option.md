- Disabling wasm JIT tiers also disables the Wasm SIMD feature flag.
- Wasm SIMD availability is coupled to the presence of a wasm JIT tier.

## Moves

- 2025-10-13 (cec82dae) replaced by [[interpreter-tier]]: Wasm SIMD can run without Wasm JIT when IPInt SIMD is enabled, so disabling all JIT options should preserve useWasmSIMD only under useWasmIPIntSIMD. (code)
