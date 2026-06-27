- A shared probe thunk executed OSRExit::executeOSRExit in C++ through Probe context state.
- OSRExit cached recovered state instead of compiling per-exit patchable off-ramps.

## Moves

- 2017-09-14 (2d6ba10d) replaced by [[osr]]: Probe-mediated DFG OSR exit was rolled out because it regressed Speedometer by ~4% and Dromaeo CSS YUI by ~20%. (sourced)
