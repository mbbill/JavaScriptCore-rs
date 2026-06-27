- Heap allocation is split between fixed-size MarkedBlock pages for ordinary cells and one-object PreciseAllocation blocks for cells that need precise sizing or exceed a block-class cutoff.
- Marking is a fixpoint over roots, object edges, weak references, and output constraints; helper visitors may run in parallel but must not make a cell black before all outgoing edges are accounted for.
- Collection scopes are generational: Eden collections reuse sticky marks and remembered sets while Full collections revisit the whole heap.
- Concurrent collection is coordinated by a collector/mutator phase connection rather than by one monolithic stop-the-world pass.
- Every cross-object reference mutation that can connect an old or black object to a young or white cell is expressed through a typed write-barrier path.
- External and embedder reachability enters the trace through explicit handles, opaque roots, conservative roots, or weak-owner callbacks rather than by exposing collector bitmaps.

## Facts

- 2007-04-23 (86344935) measurement: moving mark and main-thread-only bits from JSCell bitfields to per-block bitmaps reduced 32-bit cell size from 40 to 32 bytes and enabled block lookup by pointer masking, yielding a 0.8% iBench speedup (sourced).

## Moves

- 2011-11-01 (5e28ce2e) replaced [[serial-gc-mark-stack]]: Marking work can be split among marker threads by keeping per-thread local stacks plus a shared steal/donate stack and making mark-bit updates atomic. (code)
- 2014-01-09 (36fb03f0) replaced [[full-heap-marking]]: Re-marking the same objects over and over is a waste of effort, so the sticky mark bit algorithm uses EdenCollections to visit only new objects or objects added to the remembered set while FullCollections still visit all objects. (sourced)
- 2012-09-10 (6e39cc19) replaced [[markstack-plus-slotvisitor-two-class-gc-visitor]]: SlotVisitor was a thin subclass of MarkStack that added copying/parallel-drain logic; merging them into a single class eliminates the inheritance indirection and allows all GC visitor state to live in one place. (sourced)
- 2017-01-18 (0d9d5577) replaced [[opaque-root-mutator-barrier]]: Opaque roots can change when visitChildren changes its mind and they have no write barriers, so JSObject-to-OpaqueRoot edges must be evaluated as output constraints participating in the marking fixpoint rather than as mutator-barriered roots. (sourced)
