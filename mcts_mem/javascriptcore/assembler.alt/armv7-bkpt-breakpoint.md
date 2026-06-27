- ARMv7 breakpoints used BKPT instructions in MacroAssembler and offlineasm break lowering.

## Moves

- 2023-10-18 (4a0ec7da) replaced by [[assembler]]: ARMv7 breakpoints switched to UDF because BKPT can hang under gdb instead of trapping. (sourced)
