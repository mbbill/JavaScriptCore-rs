- WasmIPIntGenerator exposes SIMD tier support as a compile-time false constant.
- SIMD parser paths use constexpr branching and emit crash paths when the IPInt context lacks SIMD support.

## Moves

- 2025-09-02 (c9959901) replaced by [[interpreter-tier]]: IPInt SIMD support needed a runtime feature flag while under development, so tierSupportsSIMD could no longer be a compile-time false constant for the IPInt generator. (sourced)
