- DFG compiles profiled bytecode into a typed data-flow graph whose nodes and edges carry the assumptions used by speculative native code. (`Graph`)
- The graph has explicit control-flow blocks and variable state at block boundaries, with Phi/value links used for inter-block liveness, DCE, and SSA conversion.
- The graph exists in distinct forms: a load/store or CPS form for parsing and CFG-rewriting phases, a threaded CPS form for CFA/regalloc-style reasoning, and SSA for later optimization and FTL input.
- DFG records the semantic code origin separately from the exit-target origin whenever code motion can move a check away from its bytecode resume point.
- DFG node semantics are centralized in effect/clobber models used by CSE, DCE, LICM, and backends rather than being rediscovered by each pass.
- Call, OSR, speculation, and tier-up decisions are split into [[call-dispatch]], [[osr]], [[speculation]], and [[tier-up]].

## Facts

- 2011-03-14 (08a80d90) rationale: the previous bytecode JIT generated code directly from bytecode and re-entered fast paths from slow paths mid-block, preventing type and liveness facts from propagating through the block; DFG IR was introduced to carry those facts across the whole block (sourced).
- 2011-03-14 (08a80d90) rationale: the first DFG deliberately accepted only single-basic-block functions on JSVALUE64/x86-64 and fell back to the existing JIT for jump targets and structure stub infos, making coverage widening incremental (sourced).
- 2011-09-18 (40e19fca) measurement: block-local DFG CSE was reported as an 80% speedup on Kraken imaging-gaussian-blur and 10% on Kraken geomean (sourced).
- 2013-02-21 (bd5859f8) measurement: encoding UseKind on edges was benchmark-neutral overall and about 8% faster on Octane/box2d while simplifying speculation dispatch into a UseKind switch (sourced).
- 2014-08-06 (22ee46af) measurement: clobberize-driven local/global CSE was reported as about 0.7% throughput improvement, with the local rewrite avoiding O(n^2) behavior and reducing compile time (sourced).

## Moves

- 2011-04-15 (3a4c6219) replaced [[dfg-variable-slot-as-nodeidx]]: A single NodeIndex per variable slot could not express the case where a GetLocal in one basic block needs to reference the most-recent SetLocal from a prior block; the VariableRecord{get,set} pair plus explicit GetLocal/SetLocal graph nodes makes the producer-consumer relationship explicit and persistent across block boundaries. (code)
- 2011-04-23 (406855ed) replaced [[dfg-dce-refcount-only]]: Reference-count-only DCE could not propagate liveness across basic-block boundaries; adding Phi nodes that link GetLocal uses to SetLocal definitions in predecessor blocks via an iterative work-queue enables true SSA-style inter-block dead-code elimination. (sourced)
- 2013-02-09 (39a8f3eb) replaced [[dfg-phi-threading-in-bytecode-parser]]: The old design built and maintained Phi data-flow links during bytecode parsing and required every subsequent phase to preserve them or redo the work itself, making it impossible to freely restructure CFG or data flow in phases; the new design introduces two explicit graph forms (LoadStore: implicit data flow, suitable for CFG transforms/CSE; ThreadedCPS: explicit linked Phi network, suitable for CFA/regalloc) and a dedicated CPSRethreadingPhase that any phase can invoke after dethreading, decoupling phase correctness from Phi maintenance. (sourced)
