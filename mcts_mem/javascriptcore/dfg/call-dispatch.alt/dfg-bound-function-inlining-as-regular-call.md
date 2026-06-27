- Bound-function inlining lowered the bound target as a regular call inline frame.
- Intrinsic inlining could not report a terminal inlining result.

## Moves

- 2023-02-23 (34e32f76) replaced by [[call-dispatch]]: DFG bound-function inlining now preserves tail-call form with a BoundFunctionTailCall inline frame and terminal inlining result, because a bound function that is erased during inlining must still reconstruct OSR-exit frames as if its target returns to the original tail-call caller. (sourced)
