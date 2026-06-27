- Each linked OSRExit owned its generated exit code and patchable jump location.
- Exit code lookup was tied to concrete linked OSRExit objects.

## Moves

- 2022-04-29 (6eaf4a53) replaced by [[osr]]: Unlinked DFG needs OSR-exit code reachable from shared JITData by exit index instead of linked OSRExit objects with patchable jump locations, while linked DFG can still repatch jumps to the generated code. (sourced)
