- Interpreter, JIT stubs, slow paths, and operations helpers each implement their own scope-chain variable-resolution loops.
- Resolve variants are split across resolve, resolveSkip, resolveGlobal, resolveBase, resolveWithBase, and resolveWithThis functions.
- Scope objects do not share one base class for lookup policy.

## Moves

- 2012-08-28 (3aa4cd77) replaced by [[scope-chain-and-activation]]: Multiple copies of variable-resolution logic (resolve, resolveSkip, resolveGlobal, resolveBase, resolveWithBase, resolveWithThis) existed independently in Interpreter.cpp, JITStubs.cpp, and CommonSlowPaths.h; introducing JSScope as a shared base class for all scope-chain objects centralised these into one authoritative implementation to eliminate duplication and enable a forthcoming scope-chain optimisation. (sourced)
