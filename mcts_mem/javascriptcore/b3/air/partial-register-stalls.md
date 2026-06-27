- Air inserts dependency-breaking moves before recent partial XMM-register updates when final instruction order makes a stall likely.
- Tmp width and zero-extension information is tracked separately from Tmp identity.
- Move-width canonicalization is architecture-specific rather than globally preferring one width.

## Facts

- 2015-12-02 (215f9484) rationale: the partial-register phase intentionally uses a cheap local heuristic rather than explicit dependency tracking; sourced notes say false-positive xorps instructions are cheap (sourced).
- 2015-12-02 (215f9484) measurement: a register reset before partial XMM-register updates was reported to make execution 20% faster (sourced).
- 2015-12-02 (215f9484) rationale: the phase runs after register allocation and as late as possible because it depends on final instruction order and register use (code).
- 2015-12-02 (215f9484) pitfall: hidden instructions inside calls or Air Specials can invalidate the local instruction-distance heuristic unless the phase accounts for them (sourced).
- 2015-12-21 (fbfbe179) pitfall: a 32-bit defining instruction assigned to a stack slot writes only 4 bytes, so a later 64-bit fill can observe garbage high bits unless the spiller tracks definition and use widths (sourced).
- 2015-12-21 (fbfbe179) measurement: the TmpWidth change fixed a V8/encrypt crash and left B3 about 10% behind LLVM on steady-state throughput for that test (sourced).
- 2016-01-22 (0a5e46a0) measurement: classifying CeilDouble as a partial-XMM-register update produced an 8% Kraken speed-up with B3 enabled, mostly from a 2.4x speed-up on audio-oscillator (sourced).
- 2023-05-22 (45c0547c) measurement: ARM64 did not benefit from x86_64's Move-to-Move32 preference; the sourced note says Move32 is more costly than Move on ARM64 and and-immediate forms are cheaper than uxt aliases (sourced).
- 2024-08-07 (2ea46ab9) rationale: width canonicalization is target-specific: x86_64 rewrites Move to Move32 when high bits are unobservable, while ARM64 can rewrite Move32 to Move when the destination is only 32-bit-defined (code).

## Moves

- 2015-12-21 (fbfbe179) replaced [[b3-zext32-copy-propagation-zero-extension-model]]: The instruction selector could not decide globally that every Int32 Value lowered to a zero-extending instruction, because spilled Add32 destinations write only 4 bytes while later fills may read 8 bytes, so zero-extension had to be modeled in Air where instruction definitions, uses, coalescing, and spill/fill choices meet. (code)
