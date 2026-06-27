- OSR exit is the DFG deoptimization path for speculative checks, recording how to reconstruct baseline-visible state and resume at a bytecode origin.
- Exit value recovery is derived from a variable event stream plus a minified graph, not from eager per-exit recovery vectors.
- Exit stubs are normally generated lazily and patch the failed check site after first execution.
- OSR entry builds a target frame from baseline state and live-value expectations, using a scratch buffer before the assembly entry thunk installs the frame.
- Exit profiles stored with unlinked code let sibling CodeBlocks share deoptimization feedback.
- Exception exits are represented as ExitKind values, allowing exception checks and generic unwinds to participate in exit counting and jettison policy.

## Facts

- 2011-09-13 (d9f484e8) measurement: baseline-to-DFG OSR entry triggered at loop hints produced a 2.88x speedup on Kraken imaging-desaturate and was a win across SunSpider, V8, and Kraken when tiered compilation was enabled (sourced).
- 2011-11-10 (6471e6d7) measurement: lazy OSR exit generation was reported as about a 1% win on SunSpider and V8 in the author's harness, with no change in V8's harness or Kraken (sourced).
- 2016-01-29 (6a41d375) measurement: treating caught-exception exits as non-jettisoning gave about a 4-5% speedup on a high-frequency-throw v8-raytrace variant (sourced).
- 2017-09-14 (2d6ba10d) measurement: probe-mediated DFG OSR exit was rolled out after reported regressions of about 4% on Speedometer and 20% on Dromaeo CSS YUI (sourced).

## Moves

- 2011-11-10 (6471e6d7) replaced [[eager-dfg-osr-exit-compilation]]: The OSR exit code is now generated the first time it is executed, rather than right after speculative compilation, because most OSR exits are never taken. (sourced)
- 2012-07-03 (403f771e) replaced [[dfg-eager-osr-exit-value-recoveries]]: The DFG now saves a variable event stream and minified graph so DFG::OSRExitCompiler can reconstruct recoveries lazily instead of computing argument and variable ValueRecoveries at every speculation check. (code)
- 2014-02-17 (e9207932) replaced [[in-place-osr-entry-frame-rewrite]]: prepareOSREntry stopped directly editing the caller's stack frame and instead builds the target frame in a scratch buffer for an assembly thunk to copy into place, avoiding stack clobbering concerns and ASan crashes. (sourced)
- 2016-01-29 (6a41d375) replaced [[osr-exception-handler-side-flags]]: Exception-handling behavior needed to participate in exit-count and jettison policy, so encoding exception checks and generic-unwind arrivals as ExitKind values replaced separate per-exit boolean side flags. (code)
- 2017-09-08 (b6f7369c) replaced [[dfg-osr-exit-compiled-offramps]]: The JIT-probe thunk avoids OSR exit ramp compilation time and per-exit executable memory by executing OSRExit::executeOSRExit(), accepting a small per-exit slowdown because OSR exits are rare. (sourced)
- 2017-09-14 (2d6ba10d) replaced [[probe-mediated-dfg-osr-exit]]: Probe-mediated DFG OSR exit was rolled out because it regressed Speedometer by ~4% and Dromaeo CSS YUI by ~20%. (sourced)
- 2018-01-13 (3abf574c) replaced [[codeblock-local-dfg-exit-profile]]: Storing DFG exit profile data on UnlinkedCodeBlock lets all CodeBlocks backed by the same unlinked code, including those from the unlinked code cache, share OSR-exit feedback for earlier better compilation decisions. (sourced)
- 2022-04-29 (6eaf4a53) replaced [[linked-dfg-value-profile-handles]]: Unlinked DFG cannot store CodeBlock* and concrete profile pointers in OSR-exit metadata, so value-profile reporting now stores a CodeOrigin, kind, and operand and resolves the concrete profile when exit code is generated. (sourced)
- 2022-04-29 (6eaf4a53) replaced [[per-osr-exit-patchable-code-storage]]: Unlinked DFG needs OSR-exit code reachable from shared JITData by exit index instead of linked OSRExit objects with patchable jump locations, while linked DFG can still repatch jumps to the generated code. (sourced)
