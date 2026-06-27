- The low-level interpreter is generated from offlineasm into native assembly where supported and C_LOOP code where native executable code is unavailable or disabled.
- LLInt uses the JIT calling convention and the same entry thunks, VM entry records, slow-path calls, exception delivery, and tier-up boundaries as JIT code.
- Opcode dispatch is table-driven across narrow, wide16, and wide32 variants; bytecode PC state is kept in interpreter registers and spilled before C++ slow paths.
- LLInt slow paths return through explicit ABI protocols, including PC redirection for exceptions and OSR entry/loop/epilogue tier-up to Baseline or higher tiers.
- Opcode and configuration tables live in protected OS script configuration storage when available, with a JSC-owned fallback for older targets.

## Facts

- 2012-02-21 (c9fc3858) rationale: starting code in LLInt before Baseline JIT preserved benchmark neutrality while reducing JIT compilation enough to produce double-digit improvements on real-world websites. (sourced)
- 2012-02-21 (c9fc3858) rationale: LLInt tier-up can occur at prologue, loop, and epilogue replacement points, and loop OSR maps bytecode PC to the corresponding Baseline machine-code offset before jumping. (code)
- 2012-09-01 (a492d09d) rationale: the C_LOOP backend lets JSC run on targets without JIT memory or assembly backends by transpiling the same offlineasm source to portable C++. (code)
- 2012-09-01 (a492d09d) rationale: C_LOOP register conventions are modeled on ARMv7 LLInt plus selected x86-64 extensions to keep offlineasm source shared with native backends. (code)
- 2012-09-17 (31d8e7fe) pitfall: 32-bit LLInt get_by_val loaded vector length from the base-object register rather than the butterfly register, causing incorrect bounds comparison. (code)
- 2013-11-14 (f726de4f) pitfall: ARM cCall lowering must use ABI argument registers a0-a3 rather than t0-t3 because temporary register names do not consistently alias argument registers. (sourced)
- 2013-11-15 (54581bfd) pitfall: the callToJavaScript entry stub must save both caller frame and caller return PC into the sentinel frame, loading return PC from stack or link register according to architecture. (code)
- 2013-12-05 (b2ea0fe7) rationale: the C_LOOP bridge centralizes CallFrame setup and teardown in callToJavaScript/callToNativeFunction, receiving ProtoCallFrame and pushing/popping JSStack frames. (code)
- 2014-01-10 (4b0ded39) pitfall: on X86_64 the LLInt sentinel frame must read caller return PC and frame pointer from post-prologue stack slots before storing ReturnPC and CallerFrame. (code)
- 2014-02-19 (c2edd8b2) pitfall: Windows x86 LLInt needs its own backend because callToJavaScriptPrologue saves the original stack pointer, realigns to 16 bytes, and uses adjusted argument offsets. (code)
- 2014-06-25 (9c94603c) pitfall: Win64 slow-path calls require a 64-byte maxFrameExtentForSlowPathCall, unlike non-Windows x86_64. (code)
- 2015-03-25 (24b132ed) pitfall: LLInt watchdog polling must load VM::watchdog as a pointer and null-check before reading Watchdog::m_timerDidFire. (code)
- 2018-02-12 (a6aeff2a) rationale: pointer tagging prepared LLInt call and jump sites for pointer profiling by threading explicit tag/untag operations through offlineasm. (sourced)
- 2018-03-09 (ec70bd48) rationale: LLInt dispatch entries and call sites carry pointer-tag domains so pointer profiling distinguishes bytecode entries, code entries, exception handlers, native trampolines, and slow paths. (code)
- 2019-05-30 (bb678b97) pitfall: enabling wide16 on MSVC C_LOOP grew the CLoop::execute stack frame enough to overflow tests; a C_LOOP_WIN backend omitted wide16 until clang-cl made the workaround unnecessary. (sourced)
- 2020-01-16 (c76d0ad6) statement: with PB+PC bytecode addressing, PB lives in a callee-save register and C calls receive an Instruction*, requiring add/subtract of PB around calls. (code)
- 2021-08-16 (bb29688e) pitfall: LLInt op() entries whose wide variants intentionally emit no code must still emit breaks or accidental wide dispatch falls through into the next entry. (sourced)
- 2021-09-27 (518aa90b) pitfall: LLInt loop OSR that references Baseline JIT constant pools must stay separate from C_LOOP/non-JIT paths, which call the slow path and reload PC instead. (code)
- 2021-09-29 (879ade38) rationale: CodeBlock no longer caches a pointer to the LLInt execute counter because loading UnlinkedCodeBlock plus counter offset costs the same and saves a field. (sourced)
- 2022-06-09 (0e9b491f) rationale: VM entry/exit saves only registers that are callee-save in the C ABI but not in the JIT ABI. (sourced)
- 2022-08-30 (cc541135) measurement: non-temporal store-pair stack sanitization measured 0.32% better on Speedometer2 at 80% confidence and 0.58% better on React-TodoMVC at 95% confidence. (sourced)
- 2024-05-19 (2c77a224) rationale: Windows C_LOOP wide16 exclusion was removed once Windows C_LOOP only supported clang-cl, making the MSVC workaround unnecessary. (sourced)
- 2025-10-21 (6eb25666) statement: the offlineasm reference defines width suffixes and operand classes as the documentation contract for future instruction additions. (code)
- 2025-12-13 (07f79c93) pitfall: os_script_config_storage availability must be tested through deployment-target-aware configuration, not inferred from SDK headers during back-deployment. (sourced)

## Moves

- 2012-02-22 (7dc7faa4) replaced [[baseline-jit-first-execution]]: JSC starts execution in LLInt and only tiers up to the old JIT after code is proven hot, reducing JITing while preserving benchmark neutrality and improving real-world websites. (sourced)
- 2017-05-03 (09bc196f) replaced [[arm64e-as-arm64-native-backend]]: ARM64E was routed to the CLoop instead of the ARM64 native backend while JIT support was disabled for that CPU. (sourced)
- 2018-10-16 (08c63ef8) replaced [[llint-monolithic-offset-extractor]]: Configuration/settings extraction was separated from offset extraction so that the settings binary (LLIntSettingsExtractor) can be built and run before the offset extractor, enabling the assembler to generate correct code for each configuration combination independently. (sourced)
- 2018-11-22 (933424d6) dropped: ARM_TRADITIONAL LLInt/JIT backend: ARM_TRADITIONAL (non-Thumb2 ARM) LLInt and JIT are no longer maintained in JSC; ARM_TRADITIONAL targets will fall back to CLoop interpreter, eliminating ~4000 lines of architecture-specific JIT/LLInt code. (sourced)
- 2022-06-23 (79eb5e92) replaced [[arm64-offlineasm-single-load-store-spills]]: ARM64/ARM64E LLInt register spills and Wasm argument transfers can be encoded as ldp/stp pairs rather than repeated scalar ldr/str sequences when the offline assembler has explicit loadpair/storepair operations and pair-address validation. (code)
- 2024-05-21 (8e3653b4) dropped: x86-32 asm LLInt backend: This patch ensures that CLoop is enabled on x86 (32bit) and dropping asm LLInt support. (sourced)
- 2024-08-05 (f0f17a7e) replaced [[windows-separate-jsc-config-record]]: JSC stopped maintaining a Windows-only standalone JSC/WTF config path and made LLInt and C++ code address JSC configuration through the unified g_config offsets. (code)
- 2025-10-28 (231b11ba) replaced [[jsc-owned-llint-opcode-config-storage]]: When the SDK exposes os_script_config_storage, LLInt opcode configuration uses that OS-provided storage and keeps an in-tree allocation only as the fallback for SDKs without the SPI. (code)
- 2026-02-28 (5e96ef3f) replaced [[x86-ipint-manual-relative-pc-bases]]: Adding x86 pcrtoaddr let IPInt compute PC-relative label addresses directly like ARM64, eliminating per-entry manual relative-PC base setup and special x86 dispatch-base loads. (code)
