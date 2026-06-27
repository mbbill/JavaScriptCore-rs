- DFG derives type assumptions from baseline profiles, value profiles, arithmetic result profiles, array profiles, and call link status before emitting checks.
- Each child edge carries a UseKind that states the contract the consumer needs, separating use-site requirements from producer predictions.
- SpeculatedType is a bitset lattice that includes integer, double, boolean, cell, object, string, array, and more precise numeric subcontracts.
- Abstract interpretation flows AbstractValue state through the graph, combining type bits, structure information, and constants.
- Structure speculation distinguishes current proofs from future possible structures guarded by watchpoints and clobber epochs.
- Speculation failures feed exit profiles used by later compilations to suppress or redirect assumptions that repeatedly fail.

## Facts

- 2011-09-15 (3343c8d4) measurement: requiring both comparison operands to be fully speculated yielded 75% on Kraken ai-astar, 8.5% on Kraken overall, 1% on V8, and neutral SunSpider (sourced).
- 2011-09-24 (658a7c39) measurement: ForceOSRExit for absent value profiles was reported as a slight benchmark-average speedup with about 5% swings and about a 1% SunSpider progression (sourced).
- 2013-02-21 (bd5859f8) measurement: UseKind-on-edge fixup was benchmark-neutral overall and about 8% faster on Octane/box2d (sourced).
- 2017-09-14 (2d6ba10d) measurement: compiled OSR exits were restored after the probe-mediated exit path regressed Speedometer by about 4% and Dromaeo CSS YUI by about 20% (sourced).
- 2023-07-11 (c3525c11) measurement: explicit NaN/Infinity handling recovered about 5% on JetStream2 octane-zlib by avoiding repeated ArithDiv overflow exits (sourced).

## Moves

- 2011-07-28 (99e030ef) replaced [[dfg-prediction-lattice-int32-cell-array]]: The old lattice had no PredictDouble value; arguments observed as doubles at compile time were typed as PredictNone (no information), causing the speculative JIT to attempt Int32 speculation on known-double arguments and fail; adding PredictDouble and PredictNumber allows the speculative JIT to skip Int32 speculation when the prediction says double. (code)
- 2011-09-29 (f3283a0d) replaced [[codeblock-wide-prediction-tracker]]: The old representation attached one prediction to each virtual register for the whole CodeBlock, while the new representation attaches prediction state to each variable access and unifies only aliased accesses, allowing distinct predictions for unaliased uses of the same virtual register. (code)
- 2012-02-06 (24882e71) replaced [[dfg-nodeindex-child-references]]: DFG child edges needed to carry per-use type information without making DFG::Node larger. (sourced)
- 2012-08-20 (7b747c92) replaced [[single-structure-proof-abstract-value]]: The old representation conflated structures proven right now by executed checks with structures bounded for future side effects by transition watchpoints, so it could not express the watchpoint-dependent future proof needed for sound watchpoint use and CheckStructure strength reduction. (code)
- 2015-08-21 (9a65a6f7) replaced [[boolean-use-as-speculative-boolean-check]]: Branch and LogicalNot fed by an effectful boolean-producing comparison needed to be representable as non-exiting uses, but BooleanUse implied speculation/type-check machinery. (code)
