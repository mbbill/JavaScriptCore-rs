- ARM64 Baseline modulo routes int32 modulo through the generic op_mod slow path.
- No inline divide/multiply-subtract sequence is emitted for op_mod.

## Moves

- 2026-02-12 (7cda7d7a) replaced by [[bytecode-specialization]]: ARM64 Baseline can compute int32 modulo inline with divide and multiply-subtract instead of routing every op_mod through the generic slow path. (sourced)
