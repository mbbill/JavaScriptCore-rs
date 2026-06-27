- ARM64E follows the ARM64 offline-assembly backend path.
- ARM64E LLInt/JIT code dumping and native entry handling do not require a separate CPU-specific backend gate.

## Moves

- 2017-05-03 (09bc196f) replaced by [[llint]]: ARM64E was routed to the CLoop instead of the ARM64 native backend while JIT support was disabled for that CPU. (sourced)
