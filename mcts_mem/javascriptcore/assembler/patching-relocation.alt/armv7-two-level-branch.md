- ARMv7 branch linking chose between a four-byte short branch and a ten-byte long branch.
- The branch-size enum could not represent a two-byte unconditional branch form.

## Moves

- 2010-10-05 (c24ff4ce) replaced by [[patching-relocation]]: The old scheme had only ShortJump (T4, 4-byte) and LongJump (BX, 10-byte); adding T2 (2-byte unconditional branch, OP_B_T2) gives a third form the old enum could not represent, reducing JIT code size when the target is within ±2KB. (code)
