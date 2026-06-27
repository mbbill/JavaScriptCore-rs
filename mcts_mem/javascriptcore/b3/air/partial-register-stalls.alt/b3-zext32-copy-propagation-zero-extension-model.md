- LowerToAir classified B3 Values to decide whether high bits were zero.
- B3::ZExt32 copy propagation depended on that B3-level high-bits analysis.
- Air roles did not express upper-bit zeroing, and spills used full-width slots and moves.

## Moves

- 2015-12-21 (fbfbe179) replaced by [[partial-register-stalls]]: The instruction selector could not decide globally that every Int32 Value lowered to a zero-extending instruction, because spilled Add32 destinations write only 4 bytes while later fills may read 8 bytes, so zero-extension had to be modeled in Air where instruction definitions, uses, coalescing, and spill/fill choices meet. (code)
