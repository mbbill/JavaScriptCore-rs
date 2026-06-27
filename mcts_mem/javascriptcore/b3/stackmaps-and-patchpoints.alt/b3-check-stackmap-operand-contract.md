- CheckAdd, CheckSub, and CheckMul children were constrained to register ValueReps whose params.reps entries were meaningful to callbacks.
- CheckSpecial reconstructed arithmetic operands and expected clients to undo Add/Sub in the stackmap generator.
- ReduceStrength carried barriers around CheckAdd and CheckMul commutativity.

## Moves

- 2015-11-17 (a28f856a) replaced by [[stackmaps-and-patchpoints]]: The old contract exposed CheckAdd/Sub/Mul operands through params.reps and made clients undo arithmetic, which prevented commutativity and strength-reduction optimizations and failed for add-to-self and multiply input liveness. (sourced)
