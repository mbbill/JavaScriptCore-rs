- FTL relied on LLVM static branch-weight estimation after lowering and OSR-entrypoint creation.
- DFG did not store per-block execution counts before FTL entrypoint cloning.

## Moves

- 2014-02-21 (8cc643ca) replaced by [[tier-up]]: DFG estimates branch weights before OSR entrypoint creation because later CFG perturbations make LLVM's static estimates less accurate for the original graph. (sourced)
