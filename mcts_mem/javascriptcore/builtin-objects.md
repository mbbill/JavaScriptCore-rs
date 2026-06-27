- Builtin objects are implemented through C++ host objects, generated self-hosted JS builtins, and private engine intrinsics.
- Internal builtin calls use private names and symbol-backed identifiers, keeping user mutation of public prototypes from perturbing engine algorithms.
- Numeric, callable, collection, string, array, typed-array, and JSON/RegExp surfaces keep representation-aware fast paths behind observable ECMAScript semantics.
- Intl-facing builtins delegate locale-sensitive behavior to `Intl` objects when the Intl feature is enabled.

## Facts

- 2014-02-12 (fa5f5a32) pitfall: Builtin parsing rejects unsafe global identifiers such as call, apply, eval, and Function unless they are addressed through private names mapped to engine-owned identifiers. (sourced)
- 2015-03-20 (67428581) rationale: The Function constructor wrapper uses a braced function declaration so the generated function name appears in source text without entering the function's own scope. (sourced)
- 2016-06-08 (d156b707) rationale: Builtins moved well-known symbol property access to symbol-backed identifiers so LLInt can use get_by_id rather than slower get_by_val symbol lookup before tier-up. (sourced)
- 2017-07-31 (e3e0e991) rationale: Private-name checks moved to an intrinsic SymbolImpl flag because user-created private fields make private symbols numerous enough that static builtin-name membership tables do not scale. (sourced)

## Moves

- 2007-10-24 (699e8f3c) replaced [[relational-op-unified-relation]]: The shared relation() function returned a tri-state int (-1/0/1) that each caller converted via branching on -1 into bool; replacing with two inline boolean functions (lessThan/lessThanEq) eliminates the tri-state encoding and per-call branching for a measured 0.5-0.6% SunSpider speedup. (sourced)
- 2012-06-08 (a97e3a24) replaced [[math-pow-system-libm]]: On iOS ARM_THUMB2, system pow is used only when neither input is denormal and the result is nonzero or an edge case; otherwise fdlibmPow handles cases where denormal support may be required. (code)
- 2014-09-27 (b59a1014) replaced [[function-dot-arguments-default-live-arguments]]: The support stayed behind a test-enabled option while default execution returned zero arguments because removing the compiler/runtime support outright was considered too risky until compatibility was known. (sourced)
- 2015-12-10 (58844ff4) replaced [[jsc-builtins-public-array-method-calls]]: Builtins use private @push/@shift so internal array operations cannot be disrupted by user scripts overriding public Array.prototype methods. (sourced)
- 2018-03-22 (292200f7) replaced [[scoped-arguments-inline-overflow-storage]]: ScopedArguments needed pointer poisoning and index masking, which the inline-tail storage representation could not provide for the overflow pointer and header without moving them into a poisonable auxiliary allocation. (code)
- 2025-06-12 (d95d6a80) replaced [[math-sumprecise-small-superaccumulator-only]]: Math.sumPrecise chooses the large superaccumulator only for arrays longer than PRECISE_SUM_THRESHOLD because measured large-input speedups were about 1.11x while small inputs were slightly slower. (sourced)
