- Dead-code elimination was driven by per-node reference counts and recursive child dereferencing.
- SetLocal was must-generate, and no Phi node represented inter-block variable liveness.

## Moves

- 2011-04-23 (406855ed) replaced by [[dfg]]: Reference-count-only DCE could not propagate liveness across basic-block boundaries; adding Phi nodes that link GetLocal uses to SetLocal definitions in predecessor blocks via an iterative work-queue enables true SSA-style inter-block dead-code elimination. (sourced)
