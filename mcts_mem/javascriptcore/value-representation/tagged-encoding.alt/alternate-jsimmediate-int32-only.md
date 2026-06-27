- The alternate 64-bit immediate format encoded int32 immediates and cell pointers.
- Doubles still required JSNumberCell allocation.
- Tags distinguished pointer and int ranges but did not reserve a NaN-boxed double range.

## Moves

- 2009-01-16 (eae07cf1) replaced by [[tagged-encoding]]: The ALTERNATE_JSIMMEDIATE 64-bit format previously encoded only int32 immediates (tag 0xFFFF) and pointers (tag 0x0000), leaving JSNumberCell heap allocation for all doubles; extending it to NaN-box doubles via +2^48 offset eliminates JSNumberCell entirely on x86-64, removing heap allocation on every floating-point result. (code)
