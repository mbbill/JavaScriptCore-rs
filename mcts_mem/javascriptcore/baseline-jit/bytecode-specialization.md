- Baseline bytecode templates specialize common opcode shapes inline and leave uncommon or invalid cases to slow paths.
- Opcode-local specializations may use last-result register caching, value profiles, array profiles, metadata-table slots, or type-specific snippets, but each site remains anchored to its bytecode instruction.
- Allocation and iterator fast paths are kept only where Baseline can profit without obscuring instrumentation, tier feedback, or width-specific correctness.

## Facts

- 2008-09-23 (dc1c11b6) measurement: inlining the fast cases of op_nstricteq into CTI, sharing the parameterized strict-equality emitter, yielded a 2.9% speedup on EarleyBoyer. (sourced)
- 2008-11-13 (88efbfd5) measurement: last-result register caching, reusing eax across adjacent temporary-producing opcodes, yielded 1.0% on SunSpider and 6.3% on V8. (sourced)
- 2009-03-10 (b68a9542) pitfall: last-result register caching must be killed at forward-jump targets because stale cached values at ?: or || join points produced wrong results. (sourced)
- 2012-02-21 (86de1a8d) rationale: the inline allocation fast path uses CopiedAllocator internals only for simple bump-pointer storage allocation and bails out when the copied-space block lacks room or the requested array storage is oversize. (code)
- 2016-04-02 (61217008) pitfall: profiler disassembly must be option-gated because large programs can exhaust memory if baseline disassembly is always allocated for profiling. (code)
- 2019-08-30 (f930b07c) pitfall: DFG must copy only the non-JIT part of a profiled SimpleJumpTable because Baseline JIT can concurrently expand its JIT-allocated ctiOffsets vector. (code)
- 2021-08-23 (31407a67) rationale: array profiling sites store only the last-seen structure ID; loading indexingType is an explicit caller responsibility, not a side effect of profiling a cell. (code)
- 2021-10-11 (a7eb93bd) pitfall: on JSVALUE32_64, baseline metadata-profile writes must use the metadata table and store payload and tag separately at the bucket offset. (code)
- 2024-03-22 (a50b7868) rationale: the hot fast-array iterator-next path calls a JITOperation with direct operands and a metadata pointer on supported Baseline targets, leaving bytecode-width CommonSlowPath calls for unsupported targets. (code)
- 2024-06-19 (2cf423ad) measurement: profiled ClosureVar resolve_scope unrolls fewer than eight known scope-next loads and keeps the counted loop only for larger depths. (code)

## Moves

- 2010-05-10 (af9962ba) replaced [[regexp-literal-as-emitLoad-jsvalue]]: r57955 replaced op_new_regexp with emitLoad(RegExpObject as JSValue constant) to cache regexp instances, but the spec requires each regexp literal evaluation to produce a new object (ES3/ES5 differ but the cached approach caused test failures), so the caching was rolled back. (sourced)
- 2012-10-20 (4b067bc2) dropped: baseline JIT inline array allocation — Baseline JIT array allocation inlining was dropped because hot allocations are handled by DFG JIT (which still inlines), making baseline inlining dead weight that blocked instrumentation; no performance regression was observed. (sourced)
- 2016-01-05 (6ebc10cf) replaced [[ordered-result-profile-vector]]: ResultProfiles needed to be creatable at any time, including from slow paths during runtime execution, instead of only in bytecode order at baseline compilation time. (code)
- 2026-02-12 (7cda7d7a) replaced [[baseline-arm64-op-mod-slow-path]]: ARM64 Baseline can compute int32 modulo inline with divide and multiply-subtract instead of routing every op_mod through the generic slow path. (sourced)
