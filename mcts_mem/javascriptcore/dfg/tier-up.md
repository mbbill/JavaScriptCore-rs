- DFG compilation is packaged as a Plan that can run graph-building and optimization away from final installation.
- Plan finalization links code, installs watchpoints, registers identifiers, and handles success or fail paths on the owning VM thread.
- A CompilationKey distinguishes DFG, FTL replacement, and FTL OSR-entry compiles for the same CodeBlock.
- LoopHint-driven tier-up injects counters and OSR-entry metadata at loop backedges, with FTL placement using DFG loop hierarchy and execution-count estimates.
- FTL compiles can run while their DFG graph is exposed as GC-scannable safepoint state.
- FTL OSR entries and exits are stackmap-backed; logical exit descriptors are resolved to concrete stackmap records after backend optimization.

## Facts

- 2013-09-04 (c82c46db) measurement: initial DFG-to-FTL tier-up reported a 70% speedup on imaging-gaussian-blur, while replacement compilation was still slowed by incomplete LLVM calling-convention integration (sourced).
- 2014-04-08 (4ec35e1b) rationale: LLVM initialization became optional; dlopen or symbol lookup failure now sends the FTL plan to FailPath instead of asserting or crashing (code).
- 2015-10-19 (9335f1ca) pitfall: LLVM can duplicate or remove stackmap intrinsics, so lowering-time OSR exit descriptors cannot be treated as one-to-one stackmap IDs (sourced).

## Moves

- 2013-07-25 (76a8f465) replaced [[dfg-driver-monolithic-compile]]: The monolithic DFGDriver compile() function combined all compilation phases and finalization (linking, watchpoint installation, identifier registration) in one synchronous call that must run on the main thread, making it impossible to run the compilation phase concurrently on a background thread while requiring finalization on the main thread. (sourced)
- 2013-08-29 (3aa21ae9) replaced [[dfg-worklist-codeblock-key]]: A single worklist is reused for DFG, FTL-replacement, and FTL-OSR-entry compilations; keying by CodeBlock* alone cannot distinguish two concurrent plans for the same block with different compilation modes, causing false positive 'already compiling' checks and dropped OSR-entry triggers. (sourced)
- 2014-02-10 (11ca79ff) replaced [[ftl-compile-under-worklist-right-to-run-lock]]: FTL compilation could unlock the worklist rightToRun mutex during long LLVM initialization/optimization/backend work if the in-progress DFG graph registered itself as scannable GC state. (code)
- 2014-02-21 (8cc643ca) replaced [[llvm-static-branch-weight-estimation]]: DFG estimates branch weights before OSR entrypoint creation because later CFG perturbations make LLVM's static estimates less accurate for the original graph. (sourced)
- 2015-10-19 (9335f1ca) replaced [[ftl-osr-exit-by-stackmap-id]]: LLVM can duplicate or remove OSR-exit stackmap intrinsics, so one logical lowering-time descriptor may correspond to zero, one, or more concrete stackmap records and each generated OSRExit must point at a specific record index. (sourced)
- 2016-02-26 (8bddc539) replaced [[sequential-ftl-replacement-before-osr-entry-compile]]: A DFG function used only for OSR entry could waste 8-10 ms waiting for full-function FTL compilation, so triggerOSREntryNow now starts both replacement and OSR-entry FTL compiles when the DFG entry flag says entry never ran. (sourced)
