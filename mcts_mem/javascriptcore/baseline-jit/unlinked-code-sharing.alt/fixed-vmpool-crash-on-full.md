- A full fixed VM pool causes a hard crash instead of releasing cached executable memory.

## Moves

- 2011-05-27 (9c17c959) replaced by [[unlinked-code-sharing]]: When the fixed VM pool is exhausted, crashing is replaced by releasing cached JIT-compiled regexp code via JSGlobalData::releaseExecutableMemory and retrying the allocation, turning a hard crash into a recoverable state. (sourced)
