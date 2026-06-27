- Air is the low-level layer below B3: a near-machine IR of instruction records over virtual temporaries, physical registers, stack slots, and target addressing modes. (`Air::Code`)
- The same operand vocabulary is used across instruction selection, register allocation, stack allocation, and final emission.
- Air Specials cover instructions whose constraints or emission cannot be represented by ordinary Arg forms.
- Register allocation, stack-slot assignment, partial-register repair, and final lowering are separate Air phases unless an O0 path deliberately fuses allocation with emission.

## Facts

- 2015-10-28 (a6816fc4) rationale: Air was introduced as a machine-like IR over Tmp virtual registers and Arg operands before MacroAssembler emission (code).
- 2016-06-08 (092543c8) measurement: an ES6 JSAir version of Air::allocateStack was reported almost exactly 100x slower than the C++ phase on the author's machine (sourced).
- 2016-08-10 (3aad125c) measurement: profiles showed graph node creation time in sparse indexed storage; constructing the unique_ptr in the empty slot and using unchecked vector overflow removed destructor/free and bounds-check overhead (sourced).
- 2016-08-24 (8c65fe54) rationale: indexed graph maps and sets moved from B3 into WTF because bytecode basic blocks and other indexed node/block graphs needed the same utilities (sourced).
- 2018-05-04 (194064d6) rationale: phase timing was unified so DFG, B3, and Air bottlenecks are reported through one per-phase/per-compiler timing mechanism (sourced).
- 2022-12-13 (189dce57) rationale: ARMv7 64-bit add/sub remain fused Air pseudo-instructions because Air does not track live processor flags (code).
- 2024-09-16 (f9085817) pitfall: when reconstructing an Int64 C call value from two pointer-width underlying arguments on ARM_THUMB2, each half must be loaded as Int32; recursively requesting Int64 halves creates invalid B3 (code).
