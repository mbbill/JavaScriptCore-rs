- The JIT reserves a dedicated register as callFrameRegister rather than using the architected frame pointer.

## Moves

- 2013-11-07 (8e5fe7bd) replaced by [[osr-tier-boundary]]: Using the architected frame pointer as callFrameRegister frees the previously dedicated call-frame register for the DFG register allocator. (sourced)
