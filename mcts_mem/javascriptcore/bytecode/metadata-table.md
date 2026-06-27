- Mutable per-instruction state lives outside the bytecode stream in a MetadataTable indexed by opcode kind and occurrence count.
- UnlinkedMetadataTable records counts, offsets, and layout; the linked MetadataTable allocates the live storage and is reclaimed with the linked CodeBlock.
- Value, array, allocation, arithmetic, call, and rare-case profiles are bytecode-owned feedback channels consumed by tiering and optimizing compilers.
- Metadata layout is compacted by alignment ordering, offset-width selection, exact entry counts, and colocating value profiles where the execution tiers can address them cheaply.
- Profile updates are designed for mostly main-thread writes plus concurrent compiler reads, using locks, racy fields, weak buckets, or publication fences according to each profile's safety needs.

## Facts

- 2011-08-20 (35603d77) rationale: ValueProfile uses pseudo-random bucket stepping instead of sequential indexing so hot code keeps distinct recent samples in its eight buckets without adding a branch at the profiling site (sourced).
- 2011-09-14 (da318cb2) rationale: DFG tiering waits for sufficiently full profiles, reheats sparse profiles, and merges predictions across cycles so rare stable values observed early still inform specialization (code).
- 2012-09-19 (68867402) rationale: ArrayProfile tracks indexed-access interception so DFG does not Arrayify typed arrays, strings, or other structures whose indexed access is observable (code).
- 2012-11-08 (225306d0) rationale: array allocation profiling is bytecode-level per allocation opcode so repeated allocations at the same site converge before DFG compilation (code).
- 2013-07-25 (4caf7d97) rationale: profile operations split lockless main-thread writes, locked prediction synthesis, and locked concurrent reads; ArrayAllocationProfile stays lock-free because races at worst cause excess OSR exits (code).
- 2018-07-25 (2c998915) measurement: recording copy-on-write array status in ArrayProfile improved stanford-crypto-aes by about 6-7% by eliminating an OSR exit from CoW misspeculation (sourced).
- 2018-11-08 (8a87b524) rationale: MetadataTable uses a non-standard RefCounted pattern because the object address is the buffer start; destroying the table directly also notifies the unlinked layout that it is no longer linked (code).
- 2021-09-26 (d1cb45f8) rationale: arithmetic bytecodes carry profile table indices instead of metadata pointers so each instruction can find its profile while saving metadata memory (code).
- 2021-09-28 (7c1887d2) pitfall: MetadataTable iteration must use the next opcode's unaligned end offset and align only the current opcode start, or the previous opcode can appear to have extra metadata entries (code).
- 2022-11-03 (7c2bd95e) rationale: ArrayProfile observation bits are stored in one OptionSet so LLInt and JIT code OR extensible flags at a single metadata offset (code).
- 2023-09-05 (41169e7e) rationale: the MetadataTable layout was butterflied so the existing metadata table register can address ValueProfiles at negative indices and ordinary metadata at positive indices, avoiding a new LLInt/Baseline register (sourced).

## Moves

- 2011-09-03 (c38b9660) replaced [[value-profile-strong-bucket]]: ValueProfile buckets stored live GC cells via WriteBarrier (strong refs), making it unsafe to read profiling data after GC completed a collection that did not mark those cells; the WeakBucket approach lets the GC harvest surviving structure/classinfo lazily after the mark phase without keeping profiled cells alive. (code)
- 2011-09-14 (59ad8d44) replaced [[predicted-type-uint8-bitmap]]: The uint8 PredictedType representation could only hold 5 type bits plus a strong-prediction tag; adding JSFinalObject, ObjectOther, ObjectUnknown, String, CellOther, and Other distinctions required 15 value bits plus the tag, necessitating expansion to uint16. (code)
- 2012-04-08 (7c57afe0) replaced [[osr-exit-rate-reoptimization-trigger]]: Inadequate-coverage OSR exits indicate code that may be profitably optimized after enough executions, so they are counted separately and trigger reoptimization by count rather than by the ordinary success/fail ratio. (code)
- 2013-07-25 (6dc567a6) replaced [[array-profile-monotone-accumulation]]: When an ArrayProfile goes polymorphic (two or more array mode bits set) for the first time, forcibly monomorphizing it to the latest-seen structure (controlled by m_didPerformFirstRunPruning) eliminates unnecessary Arrayify nodes and makes loops effect-free; measured 5% speedup on Kraken/imaging-gaussian-blur with FTL enabled. (sourced)
- 2019-05-23 (18546474) replaced [[unlinked-metadata-table-u32-offsets]]: Gmail had 21979-24727 live UnlinkedMetadataTable instances each paying 204 bytes for a full uint32_t offset table; switching to uint16_t offset table (with 0 as sentinel indicating a spilled uint32_t table) reduces per-instance overhead for small tables and should save ~2 MB in Gmail steady state. (sourced)
- 2021-09-27 (8e47e3c2) replaced [[arithmetic-profile-pointers-in-bytecode-metadata]]: Arithmetic bytecodes carry profile table indices instead of metadata pointers so each instruction can find its BinaryArithProfile/UnaryArithProfile while saving metadata memory. (code)
- 2023-10-05 (d62f981d) replaced [[locked-codeblock-profile-updates]]: Profile updates on 64-bit no longer take CodeBlock::m_lock because ArrayProfile updates are intentionally racy and LazyOperandValueProfile additions are published from the mutator to compiler threads with ConcurrentVector plus storeStoreFence. (code)
