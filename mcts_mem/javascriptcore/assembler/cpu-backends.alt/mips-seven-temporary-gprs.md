- MIPS GPRInfo exposed seven temporary registers mapped to v0, v1, and t0-t4.
- Register-hungry DFG intrinsic and call-lowering paths carried MIPS-specific fallbacks for that smaller temporary set.
- CCallHelpers treated MIPS like ARM EABI for dummy argument alignment.

## Moves

- 2018-05-08 (b6ddc8dd) replaced by [[cpu-backends]]: MIPS DFG paths could use the generic register-hungry code once caller-save argument registers a0-a3 were admitted as temporary registers. (sourced)
