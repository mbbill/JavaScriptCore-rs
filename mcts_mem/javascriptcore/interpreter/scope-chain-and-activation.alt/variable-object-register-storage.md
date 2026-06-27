- JSVariableObject symbol-table storage points at raw Register arrays.
- symbolTablePut writes directly into Register slots without a write barrier.
- Copying variable-object storage copies Register values by raw array operations.

## Moves

- 2011-03-03 (fcac5b75) replaced by [[scope-chain-and-activation]]: Raw Register* in JSVariableObject symbol-table property storage bypassed the GC write barrier, allowing the GC to miss pointer updates when variables were written; replacing with WriteBarrier<Unknown>* makes all symbol-table writes go through the barrier. (sourced)
