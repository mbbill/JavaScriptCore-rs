- BBQ compiles wasm directly to machine code from the function parser and skips the full OMG B3 graph. (`BBQCallee`)
- The tier records callsites, stack maps, exception handlers, and loop entrypoints for callee patching and OSR entry.
- Single-pass BBQ is the default baseline backend, with fallback to Air/B3 for unsupported extension paths.
- SIMD, exceptions, tail-call probes, and platform-specific codegen are lowered in the baseline tier only when the option and architecture gates allow a correct replacement tier.
- Baseline code uses IPInt metadata, call profiles, and stack maps as shared tiering substrate rather than optional side data.

## Facts

- 2017-06-06 (184a9951) measurement: the shared-stub patchpoint form was reported as a 5-10% compile-time speedup for BBQ tier-up checks. (sourced)
- 2022-01-11 (e38fc881) rationale: the commit states that Air loop tier-up uses EntrySwitch loop-header entrypoints instead of separate OSR-entry callees like BBQ-to-OMG because it may trade some throughput for avoiding different compilations for loop and call entrypoints. (sourced)
- 2023-02-08 (50c7aaec) pitfall: Wasm BBQ/Air stack result moves must validate the concrete Air Arg form for the move opcode and value width; large offsets or misaligned vector addresses need a materialized pointer temporary before the move. (code)
- 2023-02-14 (46375fbc) rationale: the new single-pass BBQ backend landed behind a default-off useSinglePassBBQJIT flag while the existing B3/Air BBQ paths remain available for loop OSR entrypoints and unsupported cases. (code)
- 2023-03-12 (98bc2206) pitfall: tier-up checks should use nonPreservedNonArgumentGPR0 as the probe result/function-index scratch; clobbering argumentGPR0/argumentGPR1 on the non-tier-up path forces otherwise unnecessary argument spills. (code)
- 2024-03-20 (e0954318) pitfall: Wasm loop OSR must check the target JIT frame's stack requirement before destroying the interpreter frame, and the target OSR entry should crash rather than throw if its supposedly prechecked stack limit still fails. (code)
- 2024-12-10 (45c1b152) rationale: IPInt SIMD functions are routed through a distinct SIMD prologue that preserves vector arguments and immediately requests BBQ compilation instead of entering IPInt, because IPInt does not implement SIMD stack-height semantics and the generated code is never meant to run. (code)
- 2025-09-02 (92f56d6e) rationale: the retained replacement lookup is loop OSR via tryGetBBQCalleeForLoopOSR; epilogue returns no replacement, and prologue only checks a replacement when useWasmIPInt is disabled. (code)
- 2025-11-11 (c977165d) pitfall: for 32-bit BBQ i64 binary ops, computing the destination high half before the low half can clobber an input high register when resultLo aliases it; compute the low half into a scratch first when destLo aliases an input high register. (code)
- 2025-11-13 (08ba4dce) measurement: on JetStream3 tfjs-wasm.js, folding constant stores, equal storePair constants, and constant pointers reduced ARMv7 BBQ code size by 9,676 bytes, or 2.23%. (sourced)

## Moves

- 2017-06-06 (184a9951) replaced [[wasm-omg-tier-up-b3-call-block]]: BBQ tier-up checks switched from an explicit B3 branch and CCall block to a patchpoint plus shared thunk so out-of-line call code is generated once instead of in each function. (code)
- 2022-01-08 (0c3a8460) replaced [[wasm-air-exceptions-b3-fallback]]: Air gained native try/catch/throw/rethrow/delegate lowering, so BBQ no longer forces B3 for functions that use Wasm exceptions. (code)
- 2022-12-22 (a820d89a) replaced [[wasm-osr-entry-scalar-fp-save]]: Wasm OSR entry selects a vector-saving probe and doubles scratch-buffer slots for SIMD functions so live V128 values are preserved instead of truncating FP registers to scalar doubles. (code)
- 2024-03-26 (4322c3bd) replaced [[wasm-osr-stack-check-size-as-unsigned-bytes]]: Zero was both the unsigned field's unset value and the computed size for leaf functions where OMG omitted stack checks, so the representation changed to signed sentinels that distinguish unset from not-needed. (code)
- 2025-06-05 (34349713) replaced [[wasm-simd-extmul-as-extend-then-vectormul]]: Wasm SIMD extmul_high/extmul_low map directly to VectorMulHigh/VectorMulLow, avoiding the previous extend-low/high plus generic VectorMul sequence whose VectorMul operation was significantly more costly, especially on ARM64. (code)
- 2025-08-20 (0a2b0683) replaced [[optional-wasm-ipint-runtime-metadata]]: IPInt metadata and callees became a required wasm validation/tier substrate even when execution immediately tiers to BBQ, so users of stack maps and callee state no longer branch on useWasmIPInt. (code)
