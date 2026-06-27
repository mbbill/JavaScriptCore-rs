- SH4 had a MacroAssembler backend, assembler support, JSC register metadata, and an offlineasm lowering target.
- The backend exposed SH4-specific loads, stores, arithmetic, calls, branches, argument registers, and offlineasm instructions.

## Moves

- 2017-01-03 (d6ead802) removed: SH4-specific JSC assembler, MacroAssembler, register metadata, and offlineasm backend code was removed because it had not compiled since at least r189884 and nobody maintained the architecture. (sourced)
