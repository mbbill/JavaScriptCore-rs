- A JavaScript value is a compact tagged machine representation, not an allocated wrapper for every primitive.
- On 64-bit JSVALUE64 platforms, non-double values occupy tag patterns outside the biased IEEE double range, and cell pointers remain distinguishable by a cell-prefix range.
- On JSVALUE32_64 platforms, the value is a tag word plus a payload word; doubles occupy the full pair while other types use tag constants and payloads.
- Empty, deleted, null, undefined, boolean, int32, cell, and BigInt32 cases have representation-specific sentinel or tag patterns that callers must not invent as raw literals.
- EncodedJSValue is the ABI-facing transport type for returning and storing raw encoded values across C++ and JIT boundaries.
- JSValue exposes representation operations through inline predicates and constructors; callers do not reach into the storage descriptor.

## Facts

- 2011-04-11 (4c7f9e1b) measurement: defining JSVALUE64 tag constants as static const integral members caused a measurable regression, so the constants stayed as preprocessor definitions at that time (sourced).
- 2015-03-30 rationale: JSVALUE64 NaN tagging must distinguish pure NaNs that are safe to encode from impure NaNs that could be mistaken for non-double JSValue tags (code).
- 2019-09-19 (b3814f6e) rationale: JSVALUE64 uses 15-bit tags and a 2^49 double offset because smaller tag widths collide with valid double encodings or negative pure NaN patterns (code).
- 2024-01-31 (55815dc7) pitfall: concurrent updates to JSVALUE32_64 locations cannot rely on direct tag/payload stores on ARMv7, because observers can see spliced pairs; an invalid-tag protocol is required (code).

## Moves

- 2009-07-30 (74d77a72) replaced [[jsvalue-pointer-immediate-encoding]]: The old single-pointer encoding on 32-bit could not store a 64-bit double directly as an immediate (only integers and special-value sentinels fit in 32 bits); the new 32+32 tag/payload union holds doubles natively and removes the need for heap-allocated JSNumberCell for every non-integer number on 32-bit. (code)
- 2009-08-02 (39566916) replaced [[jsvalue-encoding-jsvalue32-default]]: JSVALUE32_64 (64-bit JSValue with type-tag in high 32 bits and payload in low 32 bits) became the default on all non-x86_64 platforms, replacing JSVALUE32 (immediate-encoded JSValue that bit-packs type into pointer tag bits), because 32_64 can represent all value types without special-casing immediates and enables a simpler value ABI. (sourced)
- 2010-10-27 (1de93bac) replaced [[jsvalue32-64-tag-layout-int-first]]: Placing NullTag (0xffffffff) and UndefinedTag (0xfffffffe) as the two highest unsigned tag values lets op_jeq_null / op_jneq_null be compiled as a single AboveOrEqual/Below unsigned comparison instead of two equality checks ORed together, reducing 4 instructions to 1 on ARM 32-bit. (sourced)
- 2011-04-07 (810b982f) replaced [[jsvalue32-64-separate-true-false-tags]]: Merging TrueTag (0xfffffffb) and FalseTag (0xfffffffa) into a single BooleanTag (0xfffffffe) with boolean value in the payload word (same pattern as Int32Tag+payload) lets jfalse/jtrue use a single ranged branch (BooleanTag..Int32Tag) to fast-path both booleans and integers, eliminating two separate tag comparisons per conditional-branch opcode; combined with a payload boolean convention, this keeps boolean materialization simple. (sourced)
- 2011-04-11 (4c7f9e1b) removed: JSImmediate and JSNumberCell on JSVALUE64 contained only uncalled/dead methods and JSValue constructors split across unnecessary layers; collapsing them into JSValue.h and JSValueInlineMethods.h removes indirection while keeping JSVALUE32_64 and JSVALUE64 implementations unified in one header. (sourced)
- 2024-01-31 (55815dc7) replaced [[direct-pair-jsvalue32-64-stores]]: Direct tag/payload stores were replaced for concurrent 32_64 JSValue locations because ARMv7 cannot atomically update the pair and tolerating spliced JSValues produced unusable observations in value profiles. (code)
