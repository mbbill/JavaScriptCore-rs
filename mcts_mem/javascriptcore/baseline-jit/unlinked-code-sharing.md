- Baseline can generate machine code against UnlinkedCodeBlock-owned bytecode and reuse that unlinked machine code across linked CodeBlocks for the same source.
- Per-CodeBlock and per-global state, including constants, metadata tables, stub infos, call links, exception-handler native PCs, and profiler registration, is installed through linked data at finalization.
- Sharing baseline code pushes mutable IC and metadata state into indexed pools, constant buffers, JITData, handler records, or shared stub sets rather than embedding per-CodeBlock pointers in machine code.
- Background baseline compilation may link code off the main thread, but global table updates and CodeBlock installation remain main-thread finalization work.

## Facts

- 2021-09-29 (879ade38) rationale: CodeBlock no longer caches a pointer to the LLInt execute counter because loading UnlinkedCodeBlock plus counter offset costs the same and saves a field. (sourced)
- 2021-10-30 (24e25cc7) rationale: Baseline compilation needs stable UnlinkedStructureStubInfo identity while emitting code, but retained storage can be a compact fixed vector indexed through the constant pool. (sourced)
- 2021-11-22 (d9197df9) rationale: shared CTI thunks are the sole implementation for resolve_scope and get_from_scope because unlinked baseline made inline scope-access slow paths too large on memory-constrained JSVALUE32_64 targets. (sourced)
- 2023-09-08 (8d43611a) rationale: the shared inline-cache ABI makes slow paths load CallSiteIndex and Identifier from StructureStubInfo so handler ICs can share slow-path call code. (sourced)
- 2024-05-27 (5397a281) rationale: enumerator ICs no longer need a separate isEnumerator bit because the megamorphic cases it protected against no longer exist. (sourced)

## Moves

- 2011-01-31 (daf2fadd) replaced [[jit-exec-pool-best-fit-avltree]]: Best-fit via AVL tree (SizeSortedFreeTree) with deferred coalescing caused heavy external fragmentation under real JIT allocation patterns, leading to CRASH() when no suitable free chunk could be found even with available aggregate memory; first-fit via a two-level bitmap AllocationTable hierarchy (AllocationTableLeaf + AllocationTableDirectory) eliminates fragmentation by allocating at power-of-two block granularity with no coalescing needed. (sourced)
- 2011-05-27 (9c17c959) replaced [[fixed-vmpool-crash-on-full]]: When the fixed VM pool is exhausted, crashing is replaced by releasing cached JIT-compiled regexp code via JSGlobalData::releaseExecutableMemory and retrying the allocation, turning a hard crash into a recoverable state. (sourced)
- 2015-02-10 (afa064cb) replaced [[bytecode-shaped-varargs-frame-setup]]: Higher-tier and inlined varargs calls do not literally execute bytecode and may need to load arguments somewhere other than a full callee call frame. (sourced)
- 2018-04-11 (a033ca46) replaced [[compact-jit-code-map]]: Baseline bytecode-to-machine-code maps stopped storing delta-compressed bytecode and machine-code offsets and instead stored CodeLocationLabel entries directly, so OSR exits could retrieve a code label without decoding an offset vector and reconstituting an executable address. (code)
- 2021-06-07 (d76d00e3) replaced [[baseline-jit-inline-specialized-entry-code]]: Moving Baseline JIT prologue and op_loop_hint bodies into shared thunks cut Speedometer2 LinkBuffer size from 188.379295 MB to 179.728931 MB with neutral Speedometer2 and JetStream2 performance. (sourced)
- 2021-09-27 (bfd44c5c) replaced [[per-codeblock-baseline-jit-code]]: Baseline machine code is generated against UnlinkedCodeBlock and per-CodeBlock state is loaded through a linked constant pool so all CodeBlocks for the same UnlinkedCodeBlock can share the compiled code. (code)
- 2023-09-19 (b577e3e9) replaced [[baseline-dataic-per-site-slow-path-generation]]: Baseline DataIC slow paths were consolidated into shared thunks once register usage was aligned, so both generated IC stubs and baseline IC sites could jump to one slow path and return via StructureStubInfo::doneLocation. (sourced)
- 2023-09-19 (5ed23f1c) replaced [[baseline-dataic-shared-slow-path-jump-thunks]]: The shared jump-thunk slow-path design was reverted because it caused a 0.3% Speedometer 3 regression. (sourced)
- 2024-05-04 (13455c7a) replaced [[per-site-polymorphic-call-code-stubs]]: A shared polymorphic thunk walking CallSlot trailing data replaced per-call-site BinarySwitch stub generation for DataIC so Baseline polymorphic calls no longer allocate new JIT code. (code)
- 2024-06-17 (db4158a3) replaced [[baseline-mixed-handler-and-polymorphic-ic]]: ByVal ICs could use Handler IC once Int32, String, and Symbol property checks were emitted inside handlers, so Baseline no longer needed the polymorphic compile path for Handler IC. (sourced)
