- `JSVirtualMachine` exposes a shrink-footprint SPI that immediately asks the VM to delete code, collect, and scavenge memory.
- The API can run while JavaScript is still on the stack.

## Moves

- 2018-05-30 (645b08bf) replaced by [[objective-c-embedding]]: deleteAllCode frees less memory while JavaScript is on the stack because it is implemented to do work only when the VM is idle. (sourced)
- 2018-06-13 (284ea734) removed: The synchronous shrinkFootprint SPI was removed after clients moved to shrinkFootprintWhenIdle, leaving only the idle/asynchronous VM footprint-shrink entry point. (sourced)
