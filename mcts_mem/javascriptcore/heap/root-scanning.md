- Root enumeration covers exact VM roots, handle slots, marking constraints, conservative machine roots, and opaque roots.
- Conservative stack scanning copies suspended thread register and stack ranges before candidate filtering.
- Heap membership filtering uses block metadata, precise-allocation metadata, and Bloom-filter shortcuts.
- Marking constraints carry volatility and concurrency attributes.
- Collection entry supplies the current thread stack boundary explicitly; MachineThreads captures other threads.
- Opaque roots are visitation outputs fed back into the root set.

## Facts

- 2011-04-06 (144410fd) rationale: weak handles reachable only through DOM opaque roots require a two-pass root treatment that marks opaque-root-reachable handles to a fixpoint before finalizing remaining weak handles (sourced).
- 2011-11-01 (5e28ce2e) rationale: parallel marking stores opaque roots in shared visitor state so helper drainers and root discovery synchronize through the same marking work queues (code).
- 2017-01-18 (0d9d5577) pitfall: treating opaque roots as mutator-barriered roots is unsound because visitChildren can change its opaque-root outputs without an ordinary write barrier (sourced).

## Moves

