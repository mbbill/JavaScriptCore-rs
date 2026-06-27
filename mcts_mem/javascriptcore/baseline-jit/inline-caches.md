- Baseline inline caches attach mutable IC metadata to property, element, delete, instanceof, private-name, and call-access sites, then patch or dispatch through generated handlers after observing shapes.
- Cacheability is expressed as access cases, conditions, watchpoints, handler chains, and explicit slow-path outcomes.
- Repatching increasingly moves site-specific state out of executable bytes and into StructureStubInfo, CallLinkInfo, DataIC, or InlineCacheHandler records; W^X, sharing, GC, and tier feedback remain tractable.
- IC data is part of the profiling contract for DFG/FTL and survives tier changes when the underlying target or handler can be relinked safely.

## Facts

- 2008-11-24 (42c2303a) measurement: direct hot-path relinking for get_by_id_chain produced a 3% deltablue progression. (sourced)

## Moves

- 2008-11-24 (42c2303a) replaced [[jit-get-by-id-chain-call-stub]]: The old form compiled a standalone stub and redirected the slow-case call to it; the new form (CTI_REPATCH_PIC) links the stub's failure path back to the original slow-case code in the hot patch and links success directly into the hot path's store sequence, eliminating a call/ret round-trip and yielding a 3% progression on deltablue. (sourced)
- 2009-07-31 (4f8fbdbc) replaced [[stub-deferred-optimization-code-patch]]: On ASSEMBLER_WX_EXCLUSIVE builds (W^X memory) the first-call code-patch (ctiPatchCallByReturnAddress to a _second stub) requires making executable memory writable, which is expensive; a data flag in StructureStubInfo eliminates the patch on first call and improves WX-exclusive performance by 2-2.5%. (sourced)
- 2012-04-13 (ff3a4437) replaced [[baseline-jit-property-patch-fixed-offsets]]: The baseline JIT no longer relies on platform-specific fixed offsets for get_by_id/put_by_id patch sites, and instead records the linked code-label deltas in StructureStubInfo. (code)
- 2012-10-10 (b04ba9ca) replaced [[baseline-jit-typed-array-generic-slowpath]]: Typed array get_by_val/put_by_val in the baseline JIT always fell through to the generic C stub (cti_op_get_by_val_generic) because jitArrayModeForIndexingType only handled regular indexed storage; extending the dispatch to jitArrayModeForStructure covers typed array ClassInfo and emits inline typed array stubs, gaining ~40% on benchmarks that bail from DFG to baseline. (sourced)
- 2015-09-10 (5481280a) replaced [[polymorphic-inline-cache-linked-stubs]]: The linked-list inline cache representation could not regenerate or remove a previously generated subsumed stub and scaled linearly with cases, while the new single regenerated PolymorphicAccess stub preserves metadata and can use BinarySwitch. (sourced)
- 2016-04-08 (dae57718) replaced [[inline-only-polymorphic-put-by-id-transition-allocation]]: The IC put_by_id transition path needed to cache reallocating transitions even when the butterfly had indexing storage, so those cases call JSObject reallocation operations while keeping inline allocation for non-indexing butterflies. (code)
- 2016-10-17 (f1503ba7) replaced [[domjit-custom-accessor-ic-c-call]]: Custom DOM accessors in inline caches needed the DOMJIT::Patchpoint environment so Baseline GetById ICs and DFG/FTL GetById cases could inline DOM access instead of always calling the opaque custom accessor. (sourced)
- 2019-12-11 (b26b45cc) replaced [[getter-setter-access-case-owns-call-link-info]]: GetterSetterAccessCase owned CallLinkInfo via unique_ptr and could be destroyed (on StructureStubInfo reset) while emitted stub code was still live on the stack and still held a pointer to that CallLinkInfo; moving ownership to MarkingGCAwareJITStubRoutine (via Bag<CallLinkInfo>) ensures CallLinkInfo lives exactly as long as the generated code. (code)
- 2021-05-18 (3de6f842) replaced [[call-ic-code-patching]]: Data Call ICs load the callee and code pointer from CallLinkInfo so relinking can update fields in CallLinkInfo instead of repatching generated JIT instructions. (code)
- 2024-06-13 (ad63895f) replaced [[handler-ic-combined-case-stub]]: Splitting Handler IC into one handler per AccessCase makes shared handler code reusable without considering combinations of AccessCases. (sourced)
