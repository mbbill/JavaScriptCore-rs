- FTL is JavaScriptCore's top JavaScript tier, compiling DFG SSA graphs into B3 procedures before shared backend optimization and code generation.
- FTL lowering translates DFG nodes directly to B3 values through a single lowering pass; FTL itself does not emit machine code.
- FTL state ties one DFG graph to one generated B3 procedure for the duration of compilation.
- FTL OSR exits describe frame reconstruction as ExitValue lists and lower to backend patchpoint/stackmap records.
- Slow paths are deferred and linked after the main B3/Air pipeline finishes.
- FTL native-call inlining via external LLVM bitcode is not part of the live design.

## Facts

- 2013-07-25 (760b8f14) rationale: LLVM-era FTL OSR exits were modeled as conditional branches to no-return exit thunks so the backend could treat exit paths as dead code and avoid reserving registers for them; constants, dead variables, and flushed arguments were communicated through OSRExit metadata (sourced).
- 2016-01-25 (8ecc9ff6) measurement: switching the x86-64/Mac FTL backend from LLVM to B3 was reported performance-neutral on major tests while cutting FTL compile time by about 5x-10x and avoiding stackmap section parsing (sourced).
- 2016-06-13 (71b74136) measurement: outlining most FTL::Output methods reduced LowerDFGToB3 code size because the template-based B3 API generated non-trivial inline bodies, with no reported FTL performance change (sourced).

## Moves

- 2014-08-06 (661ece0e) dropped: FTL native-call inlining via runtime LLVM bitcode: FTL native call inlining requires Clang-emitted bitcode for the native libraries, so the engine removed that dependency from builds without ENABLE(FTL_NATIVE_CALL_INLINING). (sourced)
- 2016-01-25 (8ecc9ff6) replaced [[ftl-llvm-backend]]: The X86_64/Mac FTL backend switched from LLVM to B3 because B3 was performance-neutral on major tests while cutting FTL compile time by about 5x-10x and avoiding stackmap section parsing. (sourced)
