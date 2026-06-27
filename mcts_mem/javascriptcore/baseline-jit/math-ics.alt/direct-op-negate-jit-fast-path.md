- op_negate emits a fixed direct fast path at compile time rather than a type-observed MathIC.

## Moves

- 2016-09-23 (41f15cd2) replaced by [[math-ics]]: The inline cache won because delaying and profile-specializing op_negate code reduced generated code size from 147 to 125 bytes for pure integer negate and to 130 bytes for double negate while preserving slow-path fallback. (sourced)
