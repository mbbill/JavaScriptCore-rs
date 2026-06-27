- Air has optimizing graph-coloring/coalescing allocation and lower-latency linear, greedy, and block-local allocation paths.
- Allocator data structures use dense Tmp indices and adaptive interference structures to control compile time and memory.
- Spill choices account for use temperature, dynamic frequency, rematerializable constants, and explicitly unspillable temporaries.
- Stack allocation can be fused into low-latency allocation paths when compile-time pressure is more important than optimal frame layout.

## Facts

- 2015-12-01 (ee30b7f3) rationale: Air Tmp liveness uses absolute Tmp values as flat-array indices and a sparse set because HashSet add/remove and collision costs were too high on large B3 graphs (sourced).
- 2015-12-01 (b2352c8c) measurement: the use-weighted spill heuristic was reported to eliminate all spilling inside the hot loop in Kraken/imaging-gaussian-blur (sourced).
- 2017-03-30 (abe97c60) measurement: optLevel=1 linear scan initially made B3 twice as fast with an 80% throughput regression, and linear scan itself ran 4.7x faster than graph coloring on average (sourced).
- 2017-04-12 (72e83448) measurement: combining O1 linear register and stack allocation was reported as a 21% speed-up on wasm -O1 compile times, with no significant -O1 throughput change and likely larger average stack frames (sourced).
- 2021-05-19 (30a045cc) measurement: the adaptive interference graph reportedly reduced maximum interference-graph memory from 16MB to 700KB on tsf-wasm and from 262MB to 20MB on mruby-wasm.aotoki.dev (sourced).
- 2024-07-29 (3a144e80) rationale: graph coloring tracks single constant definitions so spilled constants can be rematerialized at uses instead of treating every spill as stack-resident data (sourced).
- 2025-02-19 (f74305d8) rationale: Greedy was introduced to pursue one allocator for FTL/OMG configurations because the existing mix of O0, linear scan, IRC, and Briggs allocators was fragmented and graph coloring could be expensive with many temporaries (sourced).

## Moves

- 2015-11-11 (762819f8) replaced [[air-spill-everything-register-assignment]]: Air generation now uses a direct implementation of Appel's Iterated Register Coalescing allocator instead of spilling every tmp to a stack slot for non-testing code. (code)
- 2015-11-16 (5a3e4d8a) replaced [[air-coalescing-moves-as-inst-sets]]: For coalescing, the allocator only needs the abstract source and destination Tmps of a move, so replacing Inst* identity with dense move indices enables array and bit-vector based sets. (sourced)
- 2015-12-01 (b2352c8c) replaced [[air-irc-degree-only-spill-choice]]: Spill selection now scores candidates by interference degree divided by frequency-weighted warm uses and defs so hot values are less likely to be spilled while cold stackmap uses do not protect a value from spilling. (code)
- 2017-03-30 (abe97c60) replaced [[single-tier-air-graph-coloring-allocation]]: B3 opt levels were split so low opt levels use a faster linear-scan allocator while the full optimization level continues to use graph coloring for better generated code. (sourced)
- 2017-04-12 (72e83448) replaced [[o1-separate-linear-register-and-graph-stack-allocation]]: For B3 -O1, doing stack allocation inside linear scan reuses the liveness already computed for register allocation and skips the graph stack allocator's liveness/interference/coalescing/coloring pipeline, yielding a reported 21% wasm -O1 compile-time speed-up while accepting less optimal frames. (sourced)
- 2019-02-15 (af6d1676) replaced [[wasm-bbq-linear-scan-regalloc]]: Linear-scan register allocation for BBQ Wasm requires a separate IR editing pass to insert spills before code generation; the new block-local allocator fuses allocation with emission, eliminating that pass and achieving a reported 25% Wasm startup time speedup and ~1% JetStream2 improvement. (sourced)
- 2020-01-18 (be7e8364) replaced [[air-o0-one-stack-slot-per-tmp]]: Allocating a distinct spill slot for every Air Tmp produced huge O0 stack frames, while live-range-end reuse can assign dead Tmp slots to later Tmps. (sourced)
- 2020-10-23 (061db521) replaced [[wasm-omg-admission-by-function-byte-size]]: Graph-coloring register allocation memory grows with the square of the number of temporaries, while Wasm byte size was only an indirect proxy that rejected large functions that did not have many temporaries. (sourced)
- 2021-05-19 (30a045cc) replaced [[air-interference-hashset]]: The allocator's interference graph moved from one global HashSet of encoded Tmp pairs to a bit-vector for small graphs and per-Tmp likely-dense sets for larger graphs to reduce the allocator's peak memory footprint. (code)
- 2025-02-25 (4c15f539) replaced [[greedy-regalloc-spill-cost-unspillability]]: Unspillability had to be represented separately from use/def spill cost because coalesced groups should aggregate use/def cost without inheriting the unspillable property of short individual tmps. (sourced)
- 2025-04-21 (171b7a8e) replaced [[air-simd-fallback-register-allocators]]: SIMD code stopped falling back to graph coloring or linear scan because greedy allocation learned to distinguish high-64-only FP clobbers from full-width FP conflicts. (code)
