- Call and construct nodes use variable-arity child storage for callee, `this`, and argument operands, while fixed-arity nodes keep inline child storage.
- Call linking is specialized by call kind, with call and construct sharing the same dispatch machinery instead of separate hard-coded paths.
- Native/host calls can be represented as intrinsics when the callee identity is known, allowing graph nodes to replace ordinary calls.
- Inlining records the inline call frame and arguments metadata needed to reconstruct callee, scope, arity, and tail-call shape at OSR exit.
- Call slow paths are emitted out-of-line through deferred generators and thunks rather than interleaved with the main speculative path.

## Facts

- 2011-09-16 (6e15bf2a) measurement: inlining Math.abs through a DFG intrinsic was reported as a 61% speedup on imaging-gaussian-blur and about a 13% net Kraken win (sourced).
- 2012-05-23 (aa50b20a) rationale: exact-arity inlining was relaxed because surplus actual arguments can be ignored when linking argument positions (code).
- 2012-07-12 (9fe8f27c) rationale: call/construct thunks can tail-call directly to generated JIT code after shape and arity checks, which the old C slow-path call/link sequence could not express (code).
- 2017-09-02 (2c847953) rationale: arity-fixup inlining keeps the actual argument count separate from synthesized missing slots, allowing optional pre-ES6 arguments to be inlined (code).
- 2025-07-20 (b704d28c) rationale: when exactly one observed CallVariant fails to inline, DFG still uses DirectCall because DirectCall is faster than the ordinary Call inline cache (sourced).

## Moves

- 2011-07-06 (2ea36dbf) replaced [[dfg-node-fixed-three-children]]: Call nodes require a variable number of child operands (one per argument) which cannot be expressed in the fixed three-child (child1/child2/child3) node representation; the new design adds a NodeHasVarArgs flag and a union of fixed-children and variable-children (firstChild+numChildren index into a separate child array) to handle both cases in one Node type. (code)
- 2011-07-13 (36f0c9c8) replaced [[dfg-call-only-link-dispatch]]: The original DFG call dispatch path hardcoded CodeForCall throughout (dfgLinkCall, operationVirtualCall, operationLinkCall) and had no path to select construct code blocks; op_construct requires CodeForConstruct selection which the call-only path could not express — an expressivity wall — so Call and Construct were unified under CodeSpecializationKind. (code)
- 2011-09-16 (6e15bf2a) replaced [[dfg-host-function-call-unoptimized]]: DFG could not inline host (native) functions because it had no mechanism to identify which native function a Call node targeted; adding intrinsic annotations to NativeExecutable and hash table entries lets DFG detect calls to e.g. Math.abs at parse time and substitute ArithAbs nodes, enabling full DFG optimization on those paths. (sourced)
- 2012-05-23 (be575a09) replaced [[dfg-inlining-without-reflective-arguments]]: Inlining functions that use arguments reflectively required arguments creation, tear-off, length, and indexed access to be addressed through the relevant InlineCallFrame rather than only the current CodeBlock's arguments register. (code)
- 2014-08-25 (888178b2) replaced [[monomorphic-call-link-status-inlining]]: A single executable/callee status could not express multiple likely callees at one callsite, so FTL adopted precise call-edge profiles and a callee switch to inline several alternatives. (code)
- 2017-09-15 (20af43a9) replaced [[one-phase-inline-arity-fixup-sets]]: Inline arity fixup changed to a two-phase commit because exiting from caller-origin SetLocals after argument memcpy could expose a clobbered caller frame, while delayed callee-origin SetLocals exit with the callee frame already set up. (code)
- 2017-11-08 (3894ac30) replaced [[dfg-tail-call-dispatch]]: Recursive tail calls are converted in DFGByteCodeParser into jumps after op_enter so the resulting loop can be optimized, while limiting entry-block splitting to functions with tail calls because unconditional splitting hurt Octane/raytrace. (sourced)
- 2023-02-23 (34e32f76) replaced [[dfg-bound-function-inlining-as-regular-call]]: DFG bound-function inlining now preserves tail-call form with a BoundFunctionTailCall inline frame and terminal inlining result, because a bound function that is erased during inlining must still reconstruct OSR-exit frames as if its target returns to the original tail-call caller. (sourced)
