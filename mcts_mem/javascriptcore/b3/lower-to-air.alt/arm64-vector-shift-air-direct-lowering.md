- ARM64 vector shifts were expanded during Air lowering.
- The sequence built vector splats of scalar shift amounts after B3 ReduceStrength had already run.
- Constant scalar shift amounts could not expose a foldable VectorSplat to B3.

## Moves

- 2023-03-18 (86bfe631) replaced by [[lower-to-air]]: Lowering ARM64 vector shifts into B3 macro IR exposes the VectorSplat of a constant scalar shift amount to B3ReduceStrength so it can be constant-folded before Air lowering. (code)
