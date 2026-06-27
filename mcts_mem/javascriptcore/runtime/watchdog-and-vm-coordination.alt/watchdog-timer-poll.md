- Watchdog support emits a dedicated watchdog poll bytecode when a VM has a watchdog.
- LLInt and JIT tiers check a watchdog timer-fired bit and call a watchdog-specific operation.
- Asynchronous termination is simulated by manipulating watchdog deadline state.

## Moves

- 2017-02-28 (9bd9e744) replaced by [[watchdog-and-vm-coordination]]: A VM-level trap bitfield can multiplex watchdog checks and asynchronous termination through the same poll sites, whereas the old watchdog poll bit could only represent watchdog timer firing. (code)
