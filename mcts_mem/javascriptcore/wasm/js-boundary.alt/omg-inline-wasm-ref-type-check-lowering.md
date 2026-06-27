- OMG lowers ref.cast and ref.test directly into null/type control flow, patchpoints, phis, RTT loads, and subtype-display checks.
- No single B3 value represents a wasm reference type check for later CSE or strength reduction.

## Moves

- 2026-01-22 (ea11204f) replaced by [[js-boundary]]: Wasm ref.cast/ref.test became explicit B3 values so B3 data-flow analysis, CSE, and ReduceStrength can reason about WasmGC type checks before they are lowered to branches, checks, patchpoints, and loads. (code)
