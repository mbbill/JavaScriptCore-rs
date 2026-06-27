- Long-running script interruption is mediated by VM trap and deadline state rather than by timeout checks owned by individual bytecodes or tiers.
- VM trap sites multiplex watchdog firing, asynchronous termination, and stop-the-world requests through shared trap bits and stack-limit manipulation.
- Optimized code may be interrupted by signal-assisted trap delivery when polling alone cannot reach a safe point promptly.
- Stop-the-world coordination covers both entered VMs and idle run-loop-driven VMs when debugger or Wasm coordination requires all VMs to stop.

## Facts

- 2026-01-27 (b5980d31) pitfall: Wasm debugger stop-the-world cannot rely only on trap checks because idle VMs processing RunLoop events may not execute code; a run-loop-dispatched stop handler covers those VMs. (sourced)

## Moves

- 2017-02-28 (9bd9e744) replaced [[watchdog-timer-poll]]: A VM-level trap bitfield can multiplex watchdog checks and asynchronous termination through the same poll sites, whereas the old watchdog poll bit could only represent watchdog timer firing. (code)
- 2017-03-09 (87de3e22) replaced [[polling-vm-traps]]: DFG and FTL code no longer have to rely on polling alone: SIGUSR1 asks the mutator to install breakpoint instructions at invalidation points, and SIGTRAP jettisons optimized code at a safe point before baseline op_check_traps handles the trap. (code)
- 2025-08-16 (37f1943f) replaced [[per-instance-soft-stack-limit-mirroring]]: StackManager mirrors replace per-instance soft-stack-limit updates so VMTraps can request stop-the-world by flipping trap-aware stack limits without needing VM apiLocks held by running mutators. (sourced)
- 2026-01-27 (b5980d31) replaced [[trap-only-wasm-debugger-stop-the-world]]: Idle VMs that are only processing RunLoop events do not execute code or check VM traps; Wasm debugger Stop-The-World needs a RunLoop-dispatched stop handler in addition to trap bits. (sourced)
