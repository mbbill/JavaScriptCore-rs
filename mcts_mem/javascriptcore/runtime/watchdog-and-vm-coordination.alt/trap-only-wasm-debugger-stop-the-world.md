- Wasm debugger stop-the-world requests set VM trap bits and count entered VMs.
- Idle VMs are not stopped unless they re-enter executing code and reach a trap check.
- Stop notification depends on executing code observing the VM trap request.

## Moves

- 2026-01-27 (b5980d31) replaced by [[watchdog-and-vm-coordination]]: Idle VMs that are only processing RunLoop events do not execute code or check VM traps; Wasm debugger Stop-The-World needs a RunLoop-dispatched stop handler in addition to trap bits. (sourced)
