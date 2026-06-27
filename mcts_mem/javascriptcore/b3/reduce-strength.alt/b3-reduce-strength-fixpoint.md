- ReduceStrength used an outer fixpoint loop while changes continued at high optimization levels.
- Each value was walked once per pass without an inner retry loop.
- The pass depended on repeated whole-procedure passes for convergence.

## Moves

- 2026-06-16 (0ba6297d) replaced by [[reduce-strength]]: The fixpoint loop was wasteful: preliminary analysis showed 98% of B3 values converge in the first run and remaining passes are near-no-ops; replacing with a bounded single-pass (at most two passes) reduces compile time for large graphs while per-value inner retry loops (maxReductionAttempts=8) ensure local convergence within a single walk. (sourced)
