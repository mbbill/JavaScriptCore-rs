- DFG speculation failure emits and enters a full non-speculative JIT fallback path.

## Moves

- 2011-09-13 (f989259f) replaced by [[osr-tier-boundary]]: When DFG_OSR_EXIT is enabled (default with TIERED_COMPILATION), the NonSpeculativeJIT fallback path is no longer emitted; instead, speculation failures use OSR to jump directly into the pre-existing baseline JIT code at the matching bytecode index, avoiding the cost of maintaining a second full non-speculative code path. (sourced)
