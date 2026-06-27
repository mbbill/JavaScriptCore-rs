- VM trap firing only sets trap bits under a lock.
- Trap delivery depends on generated or interpreted code polling and consuming pending trap bits.
- Optimized code has no signal-assisted path to force a safe trap point.

## Moves

- 2017-03-09 (87de3e22) replaced by [[watchdog-and-vm-coordination]]: DFG and FTL code no longer have to rely on polling alone: SIGUSR1 asks the mutator to install breakpoint instructions at invalidation points, and SIGTRAP jettisons optimized code at a safe point before baseline op_check_traps handles the trap. (code)
