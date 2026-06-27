- Each DFG OSR exit compiled a unique off-ramp and stored executable exit code.
- The generation thunk compiled the off-ramp, restored registers, and repatched the site jump.

## Moves

- 2017-09-08 (b6f7369c) replaced by [[osr]]: The JIT-probe thunk avoids OSR exit ramp compilation time and per-exit executable memory by executing OSRExit::executeOSRExit(), accepting a small per-exit slowdown because OSR exits are rare. (sourced)
