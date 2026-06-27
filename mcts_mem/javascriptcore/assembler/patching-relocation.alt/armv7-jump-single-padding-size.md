- ARMv7 jumps reserved one maximum padding size for conditional and unconditional forms.
- Jump shrinking subtracted actual branch size from that shared padding amount.

## Moves

- 2010-10-14 (93647b9f) replaced by [[patching-relocation]]: Conditional and unconditional jumps require different maximum padding sizes (12 vs 10 bytes), making a single JumpPaddingSize constant too large for conditionals; per-type padding allows the IT instruction to be included in the conditional sequence, enabling B(T1)/B(T3) short conditional branch encodings that shrink code size. (sourced)
