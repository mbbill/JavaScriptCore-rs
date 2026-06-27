- First execution compiles bytecode directly to the old Baseline JIT when JIT is available.
- Executable preparation invokes Baseline JIT as the bottom tier before code is proven hot.
- DFG-optimized code keeps the Baseline CodeBlock as its lower-tier alternative.

## Moves

- 2012-02-22 (7dc7faa4) replaced by [[llint]]: JSC starts execution in LLInt and only tiers up to the old JIT after code is proven hot, reducing JITing while preserving benchmark neutrality and improving real-world websites. (sourced)
