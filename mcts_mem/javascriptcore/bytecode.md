- JavaScriptCore executes JavaScript through a register-based bytecode VM: bytecode is the shared program representation handed to LLInt, Baseline JIT, DFG, and FTL.
- Bytecode keeps language-semantics lowering in the generator, not in later tiers: scope, exception, finally, tail-call, and private/super/property-operation distinctions are encoded as explicit instructions or operands.
- The root bytecode node owns only cross-cutting VM bytecode decisions; storage/layout decisions live in [[codeblock-split]], instruction encoding in [[instruction-format]], profiling state in [[metadata-table]], and cache feedback in [[inline-cache]].

## Facts

- 2008-05-22 (e6946ec0) rationale: the SquirrelFish bytecode VM landed while preserving the old tree-walking interpreter as OldInterpreterExecState for not-yet-ported nodes, allowing incremental migration with both paths in one binary (code).
- 2008-06-11 (34e48d88) measurement: fusing op_less plus op_jtrue into op_jless improved SunSpider by 3.6% (sourced).
- 2008-06-30 (e8cf67c7) measurement: fusing op_less plus op_jfalse into op_jnless improved SunSpider by 2.4%, while the generator rewind helper had to remain inlined to avoid a regexp-dna regression (sourced).
- 2012-09-25 (afac70bd) rationale: after LLInt became the non-JIT baseline, the classic C interpreter could not build alongside LLInt and was unreachable on all targets, making removal safe (code).
- 2012-09-27 (957ae448) rationale: Special::Pointer keeps bytecode from embedding raw function pointers; LLInt pays an extra indirection to resolve the enum through JSGlobalObject, with no benchmark cost reported (sourced).
- 2016-12-22 (c8db412b) measurement: completion-record finally de-duplication was reported performance-neutral on ES6SampleBench and JSC benchmarks (sourced).
- 2024-09-24 (74d23321) measurement: fusing the instanceof bytecode sequence cut emitted bytecode from 50 bytes to 7 bytes and improved the instanceof microbenchmark geometric mean 1.0055x (sourced).
- 2026-03-24 (9b8f2a4b) pitfall: when eq_null and neq_null stop materializing operands through a temporary move, LLInt handlers must load operands with loadConstantOrVariable rather than assuming a frame slot (code).

## Moves

- 2015-08-07 (4588c578) replaced [[runtime-exception-scope-depth-unwind]]: The bytecode generator knows every local scope it creates and can assign the correct catch scope directly, so the exception runtime no longer has to rediscover it by walking scope depth. (sourced)
- 2016-07-02 (59a7a2d5) replaced [[tdz-variable-environment-stack]]: A stack that only stored variables currently under TDZ could not represent intervening lexical scopes whose bindings are known not to need TDZ checks, so TDZ lifting could pass through those scopes to an outer declaration; a per-name necessity map can represent NotNeeded as a blocker alongside Optimize and DoNotOptimize. (code)
- 2016-12-22 (c8db412b) replaced [[bytecode-finally-duplication]]: Completion-record threading replaced finally-body duplication because duplicated finally code caused exponential bytecode generation for deeply nested finallys while the new scheme emits each finally body once and dispatches on saved completion type. (sourced)
- 2019-03-07 (2a75b559) replaced [[finally-completion-shared-global-registers]]: A single pair of m_completionTypeRegister/m_completionValueRegister shared across all FinallyContext instances was clobbered when an inner finally ran (e.g. a continue inside a nested try-finally), destroying the outer try block's saved completion and producing wrong results for nested try-finally. (code)
- 2020-07-17 (ed327a18) replaced [[generic-is-undefined-bytecode-with-html-dda]]: Only typeof/equality/ToBoolean should see `[[IsHTMLDDA]]` as undefined, so the bytecode was renamed and confined to typeof while emitIsUndefined emits strict equality with jsUndefined. (code)
- 2021-09-27 (8e47e3c2) replaced [[for-in-null-end-sentinel]]: For-in iteration now returns a preallocated sentinel JSString cell at end-of-iteration so EnumeratorNextUpdatePropertyName remains string-typed instead of being polluted by null/Other. (code)
