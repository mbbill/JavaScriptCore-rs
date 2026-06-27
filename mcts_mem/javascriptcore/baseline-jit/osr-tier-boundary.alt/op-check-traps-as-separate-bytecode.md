- Trap checks are represented as a separate op_check_traps bytecode.

## Moves

- 2019-09-02 (c0b9c686) replaced by [[osr-tier-boundary]]: op_check_traps was always being emitted unconditionally after a previous change made it non-conditional, making a separate bytecode instruction unnecessary; folding the check into op_enter and op_loop_hint eliminates one bytecode dispatch overhead per function entry and back-edge. DFG nodes (CheckTraps / InvalidationPoint) are kept separate because per-configuration node selection is easier from one bytecode. (sourced)
