- Speculative compilation immediately generated every OSR exit off-ramp before linking the main body.
- OSRExit carried the jump that was linked directly to its generated exit code.

## Moves

- 2011-11-10 (6471e6d7) replaced by [[osr]]: The OSR exit code is now generated the first time it is executed, rather than right after speculative compilation, because most OSR exits are never taken. (sourced)
