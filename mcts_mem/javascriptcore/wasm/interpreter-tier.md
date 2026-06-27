- Wasm interpreter execution uses IPInt bytecode and metadata, with LLInt-era entry/catch paths folded into the in-place interpreter substrate. (`IPIntCallee`)
- IPInt callees retain signature, locals, stack layout, call-profile, exception, SIMD, and OSR metadata for validation, execution, and higher-tier replacement.
- The in-place interpreter owns wasm wrapper entry assembly, thunked catch entries, and slow paths that can tier to BBQ/OMG.
- IPInt prologue, loop, and epilogue OSR are separately gated, but only loop OSR performs a running replacement lookup.
- SIMD interpretation is runtime-gated and can keep Wasm SIMD enabled even when wasm JIT tiers are disabled.

## Facts

- 2019-11-22 (56546744) statement: delayed local/constant expressions are allowed on the active stack, but they must be materialized when splitting control stacks, entering loops or branches, and before setLocal overwrites a local that an existing stack expression still denotes. (code)
- 2019-11-22 (56546744) statement: caller and callee call-layout computation is split; the callee reuses fixed result registers, while the caller reserves stack space and commits returned values to expression-stack slots after the call. (code)
- 2019-12-10 (12bbe89e) measurement: after the Wasm LLInt landed (r251886) JSC binary size grew significantly; three specific template-expansion hotspots were identified and fixed: (1) dumpBytecode was instantiated 2x at 30 KB each — moved to a generated .cpp file; (2) computeUsesForBytecodeIndex/computeDefsForBytecodeIndex were instantiated 3x at 11 KB each — switch body extracted to non-template *Impl functions; (3) emit_compareAndJump(Slow) had 8 instantiations at 8 KB each — bulk extracted to *Impl. Total recovery ~200 KB. (sourced)
- 2023-12-12 (4c063193) pitfall: for br_on_null, the LLInt temporary null-test result must not reuse the stack slot of the reference that remains on the non-branch path; reserve a hole for that reference and pop both temporary slots after testing. (code)
- 2024-10-25 (46432960) pitfall: addBranchNull uses stack pushes as a reserved reference slot and a temporary null-test condition, so those pushes must bypass expression-stack consistency checks and the real invariant should be checked only before and after the branch sequence. (code)
- 2025-02-14 (987c0aa8) pitfall: LLInt gate initialization cannot rely only on runtime Options::useJIT(); JIT thunk creation paths and Wasm gate entries also need compile-time ENABLE(JIT) and ENABLE(WEBASSEMBLY) guards so platforms with those features disabled do not build or initialize unreachable code. (code)
- 2025-06-16 (1f3b9d9b) pitfall: IPInt exception handler targets must be represented as LLInt/JIT thunk code references and then retagged to ExceptionHandlerPtrTag, rather than manually retagging raw C-function pointers, so JITCage-enabled configurations treat them as JIT code. (code)
- 2025-10-03 (e728c9d8) pitfall: zero-initializing only the low pointer-sized half of a default local leaves stale high bytes for v128 locals, so IPInt must clear the full 16-byte stack slot. (code)
- 2025-10-09 (7ddc4ad1) pitfall: for Wasm SIMD fmin/fmax on X86_64, a single vmin/vmax plus unordered mask is insufficient because signed-zero and NaN behavior are asymmetric; IPInt now computes both operand orders and canonicalizes NaNs after combining the results. (code)

## Moves

- 2019-11-22 (56546744) replaced [[wasm-llint-registerid-expression-stack]]: The LLInt generator needed the parser expression stack to match virtual registers directly so constants and locals could avoid redundant temporaries while still being materialized at control-flow boundaries. (code)
- 2021-12-11 (9005bd84) replaced [[direct-llint-wasm-catch-handler-pointer]]: ExceptionHandlerPtrTag is only valid for JITCode, so Wasm LLInt catch entrypoints now route through generated JIT thunks when JIT is enabled instead of tagging LLInt code directly. (code)
- 2023-01-23 (47d91b3b) replaced [[wasm-catch-callee-side-table]]: The catch path no longer swaps a JSCell into the callee slot because LLInt and JIT catch code can get the VM from a wasm callee via the Instance stored in the codeBlock slot. (code)
- 2025-01-17 (6ee9b349) replaced [[wasm-llint-default-interpreter]]: After fixing the known bugs, IPInt should be stable enough to re-enable again. (sourced)
- 2025-06-17 (fc8cbf62) dropped: separate useWasmJIT option: JSC dropped the independent useWasmJIT switch because JITCage makes interpreter tiers rely on JIT thunks whenever JS JIT is enabled, so wasm thunk availability cannot be controlled separately from Options::useJIT. (sourced)
- 2025-08-27 (9104872c) replaced [[wasm-llint-separate-webassembly-asm]]: The separate WebAssembly.asm path was folded into InPlaceInterpreter.asm because only IPInt remained available and keeping a separate wasm LLInt file left duplicate or slightly divergent definitions. (sourced)
- 2025-09-02 (92f56d6e) replaced [[wasm-ipint-prologue-osr-replacement-probe]]: Wasm compilation and installation finish on the compiler thread, so an IPInt prologue that is already running cannot discover a main-thread-finalized replacement the way JS prologue OSR can. (sourced)
- 2025-09-02 (c9959901) replaced [[compile-time-wasm-ipint-simd-support]]: IPInt SIMD support needed a runtime feature flag while under development, so tierSupportsSIMD could no longer be a compile-time false constant for the IPInt generator. (sourced)
- 2025-10-13 (cec82dae) replaced [[wasm-simd-jit-gated-option]]: Wasm SIMD can run without Wasm JIT when IPInt SIMD is enabled, so disabling all JIT options should preserve useWasmSIMD only under useWasmIPIntSIMD. (code)
