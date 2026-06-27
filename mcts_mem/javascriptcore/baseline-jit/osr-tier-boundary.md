- Baseline is the lower-tier machine-code target for tier-up, OSR entry, OSR exit, and replacement.
- Loop hints, entry counters, replacement TTLs, and worklist policies decide when Baseline code is compiled, jettisoned, or handed to DFG/FTL.
- OSR boundaries preserve bytecode PC, metadata, call-frame shape, and baseline code locations; optimized code uses those records for lower-tier entry and exit.

## Facts

- 2012-02-21 (86de1a8d) rationale: LLInt tier-up maps bytecode PC to the corresponding Baseline machine-code offset before jumping at loop OSR points. (code)
- 2021-06-25 (37b6b281) measurement: sharing saved/restored register emission reduced cumulative LinkBuffer profile size from 188266956 to 185773296 bytes, with no significant Speedometer2 and JetStream2 changes. (sourced)
- 2026-05-21 (371f7ad0) rationale: JIT helper threads use QOS_UTILITY only on small devices to avoid P-core contention and thermal throttling; larger devices keep default QoS because lowering priority regresses them. (sourced)

## Moves

- 2011-09-13 (f989259f) replaced [[dfg-speculation-failure-nonspeculative-fallback]]: When DFG_OSR_EXIT is enabled (default with TIERED_COMPILATION), the NonSpeculativeJIT fallback path is no longer emitted; instead, speculation failures use OSR to jump directly into the pre-existing baseline JIT code at the matching bytecode index, avoiding the cost of maintaining a second full non-speculative code path. (sourced)
- 2011-09-15 (c15bf62a) dropped: DFG non-speculative bailout tier — The non-speculative JIT as a bailout destination for DFG speculation failure was superseded by OSR exit to the baseline JIT (ENABLE_DFG_OSR_EXIT), making the entire non-speculative code path dead; the commit message states it 'is no longer used and should be removed'. (sourced)
- 2013-11-07 (8e5fe7bd) replaced [[dedicated-call-frame-register]]: Using the architected frame pointer as callFrameRegister frees the previously dedicated call-frame register for the DFG register allocator. (sourced)
- 2016-08-05 (71672e5b) replaced [[disabled-old-age-codeblock-jettison]]: Old-age CodeBlock jettisoning was restored because the earlier crash causes appeared to have been fixed, with per-JIT-tier TTLs and stress options to exercise the policy. (sourced)
- 2019-09-02 (c0b9c686) replaced [[op-check-traps-as-separate-bytecode]]: op_check_traps was always being emitted unconditionally after a previous change made it non-conditional, making a separate bytecode instruction unnecessary; folding the check into op_enter and op_loop_hint eliminates one bytecode dispatch overhead per function entry and back-edge. DFG nodes (CheckTraps / InvalidationPoint) are kept separate because per-configuration node selection is easier from one bytecode. (sourced)
- 2021-06-07 (d76d00e3) replaced [[baseline-jit-inline-specialized-entry-code]]: Moving Baseline JIT prologue and op_loop_hint bodies into shared thunks cut Speedometer2 LinkBuffer size from 188.379295 MB to 179.728931 MB with neutral Speedometer2 and JetStream2 performance. (sourced)
- 2025-04-24 (6d26b6d6) replaced [[jitworklist-plan-per-thread-wakeup]]: The worklist now scales compiler threads from weighted per-tier queue and in-flight load instead of waking one thread per enqueued plan, because extra compiler threads impose wakeup, synchronization, cache, contention, and scheduler overhead when the queue is too small. (sourced)
