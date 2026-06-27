- Wasm control-flow result merging uses B3 variables and Set/Get operations for non-void block results.
- ResultList stores variables that branch and end paths assign into before reading them back.

## Moves

- 2017-04-14 (867a2ba7) replaced by [[omg-tier]]: Wasm control-flow result merging no longer needed B3 variables because the generated control-flow edges ensured each upsilon dominated its phi. (code)
