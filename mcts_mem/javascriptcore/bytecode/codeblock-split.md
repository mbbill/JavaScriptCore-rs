- Bytecode compilation produces a realm-independent UnlinkedCodeBlock containing instructions, constants, jump tables, source/expression metadata, and unlinked metadata layout.
- Linking binds an UnlinkedCodeBlock to a scope and global object, producing a live CodeBlock with resolved link-time constants, runtime metadata, executable tier state, and GC ownership.
- CodeBlock-local side data is exact-sized, lazily allocated, or moved back to the unlinked form when the final counts are known before execution.
- Source-position, liveness, quick-tiering, and bytecode-cache data live beside the code only when reuse, debugging, profiling, or cache restoration require them.
- Unlinked code may be shared, cached, compressed, lazily decoded, or jettisoned according to context, but linked CodeBlocks carry execution lifetime and tier policy.

## Facts

- 2008-08-06 (09af41bc) measurement: replacing per-use op_load constant loads with upfront constant preload into call-entry registers improved SunSpider by 2.6% (sourced).
- 2008-09-08 (c741f1b8) rationale: eval caching was limited to short source strings and simple variable-object scopes to avoid caching results that depend on dynamic scope shape while capping memory exposure (code).
- 2012-11-07 (b79c05f3) rationale: separating context-free UnlinkedCodeBlock from execution-context-bound CodeBlock allows bytecode reuse across JSGlobalObject instances; linking is a fast linear pass instead of parse plus codegen (sourced).
- 2013-08-23 (da52af11) measurement: zlib-backed compression of cold expression-range info saved about 200KB on Google Maps (sourced).
- 2014-01-27 (5132367b) measurement: packing the unlinked instruction stream compressed it to about 60-61% and saved 27.5MB on Membuster3, about 2% total memory (sourced).
- 2016-06-08 (27fcadfc) rationale: StackFrame stores CodeBlock and computes source id, URL, function name, and line/column on demand from the authoritative CodeBlock instead of eagerly copying source metadata (code).
- 2016-11-04 (431c607c) pitfall: eval cache keys that retain StringImpl pointers must compare characters, not pointer identity, because equal eval source text may have distinct backing storage (code).
- 2019-06-10 (3af19608) measurement: in VM mini mode, age-gated UnlinkedCodeBlock jettisoning reduced a target daemon from 6.5MB to 5.9MB and gave larger Gmail reductions while avoiding browser-mode Speedometer2 regressions (sourced).
- 2020-02-04 (3922bd4a) measurement: sharing ValueProfile and ArrayProfile summaries through UnlinkedCodeBlock was measured as about a 0.5% Speedometer2 speedup (sourced).

## Moves

- 2008-08-06 (09af41bc) replaced [[per-use-op-load-constant-loading]]: Emitting an op_load instruction for every constant use added instruction-dispatch overhead at each use site; pre-copying all constants into the register file once at function entry eliminates the per-use opcode entirely, yielding a 2.6% speedup on SunSpider. (sourced)
- 2013-08-23 (da52af11) replaced [[expression-range-plain-vector]]: UnlinkedCodeBlock expression-range data is rarely accessed at runtime; using CompressibleVector (zlib-backed) saves ~200k on Google Maps by compressing cold bytecode metadata that otherwise stays live in memory. (sourced)
- 2013-09-23 (3df588ee) replaced [[expression-info-compressible-vector]]: CompressibleVector<ExpressionRangeInfo> was reverted to plain Vector<ExpressionRangeInfo> because it caused a CodeLoad performance regression that the team could not immediately resolve. (code)
- 2014-01-21 (b1b14ab9) replaced [[pointer-sized-unlinked-watchable-variable-operands]]: This makes UnlinkedCodeBlocks use 32-bit instruction streams again. (sourced)
- 2016-11-04 (431c607c) replaced [[eval-code-cache-portable-context-key]]: Keying cached eval code by call-site location avoids relocating eval code across different surrounding scopes, so strict and lexical-scope evals can be cached without the old scope-shape exclusions. (code)
- 2019-09-09 (73a006a1) replaced [[bytecode-property-access-instruction-vector]]: The propertyAccessInstructions vector required explicit registration at each bytecode emit site and was easily missed (op_create_promise was missing its registration); the MetadataTable::forEach<Op> API can enumerate all metadata for a specific opcode directly without a side vector, removing a class of omission bugs. (code)
- 2019-03-07 (77eff8b8) replaced [[eager-cached-bytecode-decode]]: Eager decode of all UnlinkedCodeBlocks at cache restore time was replaced by storing byte offsets in a union with the WriteBarrier slots and decoding on first call to unlinkedCodeBlockFor, matching lazy-parsing's block-boundary pause strategy to avoid unnecessary work for never-called functions. (code)
- 2018-07-09 (2256116f) replaced [[regexp-side-buffer-in-unlinked-codeblock]]: RegExp no longer needs a special RareData vector because JSCells can reside in the bytecode constant buffer. (sourced)
