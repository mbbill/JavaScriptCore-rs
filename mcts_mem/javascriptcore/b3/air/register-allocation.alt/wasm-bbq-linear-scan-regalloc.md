- BBQ Wasm used linear scan through the existing Air O2-style pipeline.
- Spill insertion happened as a separate IR-editing pass before emission.
- Each temporary received a globally live-ranged register assignment.

## Moves

- 2019-02-15 (af6d1676) replaced by [[register-allocation]]: Linear-scan register allocation for BBQ Wasm requires a separate IR editing pass to insert spills before code generation; the new block-local allocator fuses allocation with emission, eliminating that pass and achieving a reported 25% Wasm startup time speedup and ~1% JetStream2 improvement. (sourced)
