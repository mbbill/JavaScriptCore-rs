- Baseline math inline caches specialize observed numeric operations via patchable inline code and shared slow fallbacks.
- Math thunks are retained only for operation shapes for argument representation or hardware instructions make the native-call boundary materially different.
- Numeric result profiling records when Baseline keeps values as Int32 or Double; higher tiers consume those arithmetic profiles.

## Facts

- 2010-04-29 (09270ac9) rationale: Math.pow's inline fast path is limited to positive integer exponents plus the x^-0.5 sqrt case because those shapes covered the cited high-frequency uses. (sourced)
- 2011-06-30 (a0ff9963) pitfall: pow's x^-0.5 path must use DoubleNotEqualOrUnordered so NaN exponents bail out instead of falling into sqrt. (code)
- 2011-09-28 (2a7eaf34) rationale: double division records non-int32 results so DFG can decide whether zero-remainder speculation is safe. (code)
- 2015-02-14 (769885b6) pitfall: a helper called from DFG JIT code must use JIT_OPERATION rather than a host-call convention copied from a runtime native function. (sourced)
- 2015-09-06 (078e587c) rationale: baseline modulo names the fixed IDIV registers directly and asserts value-register exclusions instead of hiding x86 modulo constraints behind platform conditionals. (sourced)
- 2016-05-23 (4fb6dab6) pitfall: Baseline op_div must not hand two constant operands to the arithmetic generator; at least one side must be materialized in a register. (sourced)

## Moves

- 2010-08-19 (2e397785) replaced [[pow-thunk-always-double]]: Math.pow() thunk unconditionally returned a double-backed JSValue; when the result fits in Int32 (e.g. 2^3=8), a double-backed value was extremely slow as an array subscript because it required unboxing; the fix attempts conversion to Int32 first and falls through to double only when needed. (sourced)
- 2011-06-30 (a0ff9963) replaced [[math-native-call-fallback]]: Calling Math.floor/ceil/round/abs/exp/log through the generic native call path required boxing/unboxing and full C calling convention overhead; profiling on real web content showed these functions matter enough to justify dedicated thunks that fast-path integer arguments and use XMM registers directly, roughly doubling performance. (sourced)
- 2016-09-23 (41f15cd2) replaced [[direct-op-negate-jit-fast-path]]: The inline cache won because delaying and profile-specializing op_negate code reduced generated code size from 147 to 125 bytes for pure integer negate and to 130 bytes for double negate while preserving slow-path fallback. (sourced)
- 2018-09-27 (fea5bfb0) replaced [[jitmathic-int-offsets]]: int32_t fields (m_inlineSize, m_deltaFromStartToSlowPathCallLocation, m_deltaFromStartToSlowPathStart) cannot carry ARM64E pointer authentication tags, so they were replaced with typed CodeLocation<Tag> smart pointers (m_inlineEnd, m_slowPathCallLocation, m_slowPathStartLocation) that encode the pointer tag in their type parameter. (code)
