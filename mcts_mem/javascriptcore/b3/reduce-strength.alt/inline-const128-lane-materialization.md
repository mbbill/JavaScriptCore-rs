- Const128 lowering zeroed a vector and moved each 64-bit lane through a GPR.
- Non-zero vector constants were materialized inline rather than loaded from a data section.

## Moves

- 2022-12-15 (5ec60738) replaced by [[reduce-strength]]: Materializing Const128 takes many instructions, so B3 now loads non-zero vector constants from a data section like double and float constants. (sourced)
