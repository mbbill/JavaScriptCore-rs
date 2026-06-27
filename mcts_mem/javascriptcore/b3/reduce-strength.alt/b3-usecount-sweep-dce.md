- ReduceStrength deleted zero-use values one fixpoint iteration at a time.
- UseCounts counted direct users of all Values.
- Upsilon/Phi liveness was not modeled as a separate relation.

## Moves

- 2015-11-01 (5bcc8fe4) replaced by [[reduce-strength]]: The old sweep deleted never-referenced values one fixpoint at a time and did not eliminate cycles, while the new pass presumes all values dead, marks must-execute roots and their children live, and iterates Upsilons whose Phis are live. (code)
