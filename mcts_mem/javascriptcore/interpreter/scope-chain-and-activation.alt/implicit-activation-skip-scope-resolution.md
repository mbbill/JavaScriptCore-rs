- Scope resolution manually skips the top scope when the CodeBlock needs activation.
- LLInt, Baseline, and DFG encode special skip-top-scope paths for a function's own activation.
- Closed-variable lookup can bypass activation through tier-specific logic.

## Moves

- 2014-08-20 (46386079) replaced by [[scope-chain-and-activation]]: This is ground work for ensuring that all closed variable access is made through the function's activation. (sourced)
