- Patchpoint lowering appended the patchpoint temporary as the only result operand for non-void patchpoints.
- PatchpointValue carried effects metadata but no explicit result constraint.
- Result representation was implicit in the lowered temporary.

## Moves

- 2015-12-04 (04a8cf94) replaced by [[stackmaps-and-patchpoints]]: Patchpoint users need to constrain results to arbitrary stackmap-style representations, such as JS calls requiring the result register while other patchpoints only require some register. (sourced)
