- VMTraps patched JIT code with architecture breakpoint instructions.
- Signal handling treated the trap as a breakpoint and adjusted PCs where the instruction form required it.

## Moves

- 2017-06-24 (df1555bc) replaced by [[patching-relocation]]: Using breakpoint instructions for VMTraps conflicted with lldb, while VMTraps only required an exceptioning instruction to stop execution. (sourced)
