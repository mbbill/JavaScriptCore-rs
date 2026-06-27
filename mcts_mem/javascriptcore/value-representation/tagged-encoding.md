- Tagged encoding defines JSValue predicate, C++ call-boundary, LLInt, Baseline, DFG, FTL, and Wasm-to-JS bit patterns.
- JSVALUE32_64 uses a high tag word and low payload word with ordered null, boolean, int, and number tests.
- JSVALUE64 uses NaN-boxing with biased doubles, explicit non-cell masks, and purified NaNs.
- Boolean immediates share one tag with a payload value.
- Encoded-value sentinels are representation-dependent.
- Concurrent encoded-value writes publish a valid old value, a valid new value, or an explicit invalid-tag state.

## Facts

- 2009-08-02 (39566916) rationale: JSVALUE32_64 can represent all value types with one tag/payload ABI, avoiding the special immediate paths required by JSVALUE32 (sourced).
- 2011-04-07 (810b982f) measurement: merging true and false into BooleanTag with a payload produced a 1.007x SunSpider speedup by simplifying jtrue/jfalse checks (sourced).
- 2011-09-30 (a638dbaf) pitfall: code must not cast literal sentinel values such as 0 to EncodedJSValue because the empty-value bit pattern differs between JSVALUE64 and JSVALUE32_64 (code).
- 2019-09-19 (b3814f6e) rationale: NumberTag and NotCellMask are named for the masking operation that distinguishes cells from immediates, and static constexpr tag constants avoid out-of-line storage hazards (code).
- 2024-01-31 (55815dc7) measurement: the invalid-tag protocol for concurrent JSVALUE32_64 updates carries fence cost but avoids unusable spliced observations in value profiles (sourced).

## Moves

- 2009-01-16 (eae07cf1) replaced [[alternate-jsimmediate-int32-only]]: The ALTERNATE_JSIMMEDIATE 64-bit format previously encoded only int32 immediates (tag 0xFFFF) and pointers (tag 0x0000), leaving JSNumberCell heap allocation for all doubles; extending it to NaN-box doubles via +2^48 offset eliminates JSNumberCell entirely on x86-64, removing heap allocation on every floating-point result. (code)
- 2009-07-30 (74d77a72) replaced [[jsvalue-pointer-immediate-encoding]]: The old single-pointer encoding on 32-bit could not store a 64-bit double directly as an immediate (only integers and special-value sentinels fit in 32 bits); the new 32+32 tag/payload union holds doubles natively and removes the need for heap-allocated JSNumberCell for every non-integer number on 32-bit. (code)
- 2010-10-27 (1de93bac) replaced [[jsvalue32-64-tag-layout-int-first]]: Placing NullTag (0xffffffff) and UndefinedTag (0xfffffffe) as the two highest unsigned tag values lets op_jeq_null / op_jneq_null be compiled as a single AboveOrEqual/Below unsigned comparison instead of two equality checks ORed together, reducing 4 instructions to 1 on ARM 32-bit. (sourced)
- 2011-04-07 (810b982f) replaced [[jsvalue32-64-separate-true-false-tags]]: Merging TrueTag (0xfffffffb) and FalseTag (0xfffffffa) into a single BooleanTag (0xfffffffe) with boolean value in the payload word (same pattern as Int32Tag+payload) lets jfalse/jtrue use a single ranged branch (BooleanTag..Int32Tag) to fast-path both booleans and integers, eliminating two separate tag comparisons per conditional-branch opcode; combined with a payload boolean convention, this keeps boolean materialization simple. (sourced)
- 2014-04-16 (b0026adb) replaced [[quiet-nan-as-tag-safe-double-nan]]: A single quiet-NaN category could not express whether a NaN was safe to encode as a JSValue or needed purification before tagging. (code)
- 2024-01-31 (55815dc7) replaced [[direct-pair-jsvalue32-64-stores]]: Direct tag/payload stores were replaced for concurrent 32_64 JSValue locations because ARMv7 cannot atomically update the pair and tolerating spliced JSValues produced unusable observations in value profiles. (code)
