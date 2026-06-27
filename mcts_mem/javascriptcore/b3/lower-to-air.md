- LowerToAir uses B3 structure for local pattern selection and copy propagation before B3 values are discarded.
- Compare/branch lowering fuses comparisons into Air branches and Checks, flipping operands or conditions when target operand forms require it.
- Air opcode forms are target-qualified; reflective selection rejects unavailable architecture forms.
- Large or target-specific lowering problems are moved up into B3 macro passes when Air lowering would hide optimization opportunities.
- Memory and address operands cross the B3/Air boundary through checked offset and extended-address forms.

## Facts

- 2015-10-29 (818ad8f3) pitfall: LowerToAir must not duplicate Loads while pattern matching, because duplicating memory operations changes the lowered program rather than just selecting instructions (code).
- 2015-11-03 (400995ea) rationale: copy propagation belongs in LowerToAir because it still has both B3 use information and target Air forms available (code).
- 2016-02-26 (9d75fa32) pitfall: x86 add32(Imm32, src, dest) with zero immediate still has Add32 ZDef semantics, so lowering it to a pointer-width move preserves stale high bits (code).
- 2017-05-06 (98655afe) pitfall: extended-offset stack arguments must preserve logical FP/SP-relative offsets for patchpoints and stackmaps even when the final address needs a scratch register (sourced).
- 2021-06-11 (6ab1a8c5) rationale: ARM64 Sub32 defines high 32 bits as zero, so its Air result role must be ZD rather than D to let redundant ZExt32/Move32 operations disappear (code).
- 2021-06-21 (df228a08) rationale: ARM64 SMSUBL is a distinct Air opcode because it fuses sign-extends, multiply, and subtract into one instruction (code).
- 2021-06-22 (d6fa13e3) rationale: ARM64 SMADDL is a distinct Air opcode because it fuses sign-extends, multiply, and add into one instruction (code).
- 2021-07-20 (414eb809) rationale: ARM64 AIR selects EON/XorNot patterns so invert-and-xor lowers to one target opcode instead of separate Not/shift/Xor instructions (sourced).
- 2022-12-13 (189dce57) rationale: ARMv7 64-bit add/sub remain fused Air pseudo-instructions because Air does not track live processor flags (code).
- 2024-09-16 (f9085817) pitfall: on ARM_THUMB2, stitching an Int64 C call value from two pointer-width underlying arguments must request each half as Int32 rather than recursively requesting Int64 halves (code).
- 2026-03-19 (8528edac) rationale: canonical SIMD shuffle reduction is platform-gated: x86-64 enables only opcode/lane combinations with efficient AVX single-instruction mappings, while unsupported patterns remain generic shuffles (code).

## Moves

- 2015-11-05 (95975288) replaced [[truthiness-branchtest-lowering]]: Truthiness-only BranchTest lowering could not represent fused relational comparisons, operand/condition flipping, sub-32-bit load comparison branches, or keyed CheckSpecials for arbitrary fused branch opcodes. (code)
- 2015-12-14 (776ca177) replaced [[exclusive-compare-branch-fusion]]: B3-to-Air compare/branch fusion now duplicates shared comparisons because testing a previously materialized boolean is usually less efficient than redoing the fused compare, but it refuses load fusion once a shared comparison is involved because duplicating loads is wrong and inefficient. (sourced)
- 2015-12-14 (38688698) replaced [[air-universal-opcode-forms]]: Air opcodes and forms needed architecture masks so reflective queries and the instruction selector would reject unavailable architecture-specific address forms while keeping opcode names mentionable in C++. (sourced)
- 2015-12-23 (2e714c09) replaced [[air-x86-biased-two-address-forms]]: B3-on-ARM64 required Air to distinguish x86 two-address and memory forms from ARM64 three-address register forms, so lowering probes target-valid forms instead of always emitting the x86-biased sequence. (code)
- 2017-04-17 (7a86c519) replaced [[b3-memory-offset-int32-parameter-api]]: B3 adopted a signed-checked offset type boundary because implicit conversion of unsigned or oversized offsets into int32_t memory offsets could cause implementation-defined behavior. (sourced)
- 2017-05-06 (98655afe) replaced [[air-stack-addr-only-lowering]]: Stack arguments were replaced with an ExtendedOffsetAddr-capable lowering because WebAssembly can produce ARM64 stack frames whose FP/SP offsets do not fit normal Air Addr encodings while patchpoints and stackmaps still need logical FP/SP-relative offsets. (sourced)
- 2021-06-12 (7e0ab48d) replaced [[b3-procedure-retained-through-air]]: B3 drops most Procedure state after lowering to Air to reduce memory while preserving origins only for PC-to-origin mapping, dumping/disassembly, and origin-dependent Air operations. (sourced)
- 2023-03-18 (86bfe631) replaced [[arm64-vector-shift-air-direct-lowering]]: Lowering ARM64 vector shifts into B3 macro IR exposes the VectorSplat of a constant scalar shift amount to B3ReduceStrength so it can be constant-folded before Air lowering. (code)
- 2024-07-09 (e4b1106b) replaced [[air-integrated-int64-lowering-on-32-bit]]: The separate B3 pass can split Int64 values into explicit Int32 high/low values before Air lowering while temporarily stitching values back for Patchpoints, CCalls, and Add/Sub, avoiding generic B3/Air parallel-iteration and carry-tracking complications. (sourced)
- 2024-09-07 (6c002efb) replaced [[armv7-int64-tmp-pair-operand]]: Instructions like add64 that require a full int64 now extract their arguments from the Int64 input as if it were a tuple. (sourced)
- 2025-07-31 (5e200204) dropped: ARMv7 STRD pair-store lowering — The STRD pair-store lowering was dropped because some ARMv7 processors do not support unaligned STRD, so portable storePair32 must use separate 32-bit stores unless alignment is checked separately. (sourced)
- 2026-01-12 (2cd6a734) replaced [[arm64-branching-on-materialized-compare-booleans]]: ARM64 conditional-compare chains avoid the materialized boolean/control-flow shape for BitAnd/BitOr compare chains by preserving NZCV flags through CompareOnFlags, CompareConditionallyOnFlags, and BranchOnFlags. (code)
