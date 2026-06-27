- Indirect switches that became constants in DFG remained linear B3 Branch chains.
- ReduceStrength simplified CFG and dead code but did not synthesize SwitchValues from branch chains.

## Moves

- 2016-07-21 (b07597bd) replaced by [[reduce-strength]]: Chains of branches that test equality on the same value can be inferred into a B3 Switch, turning O(n) dispatch into O(log n) or O(1) when cases are dense. (sourced)
