# JSC Rewrite Status Tree

Current compact status source for the Rust JavaScriptCore rewrite. Hard ceiling
~200 lines; update only affected lines in accepted batches. Detailed decisions,
evidence, and measurements belong in git commit messages, not this file.

Legend:

- [done] implemented and verified for the stated scope
- [wip] partially implemented or actively being expanded
- [missing] not implemented enough to rely on
- [blocked] blocked by the named dependency
- [risk] exists but needs fidelity or structure review
- [deferred] intentionally later than the current path
- [frozen] quarantined dead code retained as salvage; not on the active path

```text
ACTIVE ROADMAP (validated, profiling-earned dependency order; default execution path = InterpreterOnly):
  Phase A [done]    de-anchor + quarantine (commit c8e83ad) -- ENABLING/HYGIENE, score-neutral.
                 ARM64 admission-proof cluster + GC/JIT salvage gated behind
                 cfg(feature="arm64_native_entry_proof") off-by-default; #[cfg(test)] proof files
                 deleted. The 0.0458 figure was a --baseline-only probe artifact; default path is
                 InterpreterOnly (shell/octane.rs).
  Phase B [done]    per-bench subsystem profiling (WF2a). VERDICT (medium confidence, /usr/bin/sample
                 on 5 benches, live path): per-op GC bookkeeping dominates self-time -- ~79% richards,
                 ~49-65% crypto, ~99% splay, ~80% gbemu; codegen-addressable <=22% best / <5% on
                 three; real GC collection ~0% (never runs). EARNED: GC-first, baseline JIT deferred.
  Phase B2 [pending] local C++ JSC same-machine comparison harness (parity-gap number; jsc build by owner).
  Phase C [active]  real mark/sweep GC + safepoints + conservative stack scan + inline write barriers
                 + direct cell pointers -- retires the per-op targeted-root registry, the unbounded
                 write-barrier Vec (heap.rs:847-876), and the payload->cell HashMap identity bridge
                 (bind_object_to_heap). Gate: re-run the 5-bench profile; rooting buckets must collapse.
  Phase C2 [parallel] navier-stokes object-model fix (independent of GC/JIT): O(1) observation buffer
                 (vs Vec::remove(0) over 1024-entry ring, interpreter/mod.rs:3583-3588) + integer-indexed
                 dense butterfly storage (vs String(index.to_string())).
  Phase D [deferred] real machine-code baseline JIT (MacroAssemblerARM64 + ExecutableAllocator) --
                 codegen share becomes the bottleneck only after the rooting tax is removed.
  Phase E [planned] structural refactor: split vm/mod.rs (74k) + interpreter/mod.rs (42k).
  Phase F [blocked] DFG/FTL/B3 optimizing tier -- where suite-SCORE parity ultimately lives.
  Phase G [parallel] Yarr/RegExp.

[wip] JetStream 3 Octane parity
  [done] Runner/benchmark contract: JetStreamDriver load order, shell globals, iteration,
         validation, scoring, telemetry, probe command surface
  [done] All 15 Octane benchmarks RUN correctly (zero throwers/aborts); suite geomean is None
         until all 15 Succeed (shell/octane.rs:1996), score=5000/time_ms per bench
  [done] SCORES (3): octane-code-load, crypto, navier-stokes
  [wip]  FUNCTIONAL-BUT-SLOW (12; perf-gated, not a feature gap): Box2D, delta-blue,
         earley-boyer, gbemu, mandreel, pdfjs, raytrace, regexp, richards, splay (GC-stress),
         typescript, octane-zlib (asm.js). Gate to all-15-Succeed is throughput.
  [done] feature breadth: non-ASCII strings, replace-with-fn, String.match,
         __defineGetter__/Setter__, global Function, Math, apply/bind, globals
  [missing] Octane score parity with local C++ JSC (needs Phases C-F)

[frozen] ARM64 native-entry admission-proof state machine
  (src/vm/native_reentry/arm64_*, src/vm/arm64_native_entry/, gc proof cluster): never admits,
  zero bench movement, quarantined behind cfg(feature="arm64_native_entry_proof") off-by-default;
  gated GC/stub modules retained as baseline-JIT/GC salvage -- see git log.

[wip] C++ JSC structural fidelity
  [done] VM cluster extractions mapped to C++: call_link.rs (CallLinkInfo/JITCall),
         property_handoff.rs (JITPropertyAccess), generated_executor.rs (CodeBlock entry),
         jit/arm64_baseline.rs + submodules (MacroAssemblerARM64; behavior unchanged)
  [risk] existing Rust-only files/types need dedicated structure review
  [risk] vm/mod.rs (74k) + interpreter/mod.rs (42k) oversized -> Phase E split
  [done] compact status tree is current status source

[wip] Parser and bytecompiler
  [done] source session/identifier groundwork; labels, named break/continue, for-in, sequence
         exprs; string escape cooking; selected TypeScript syntax
  [wip]  large-program parser pressure; TypeScript parser-prefix bytecode shape
  [done] core statement/expression lowering; LoopHint at JSC loop-body OSR headers
  [missing] complete C++ parser parity; C++ ToNumeric/Inc lowering; full lowering parity audit

[wip] Runtime semantics
  [wip] objects/structures/properties/prototypes
    [done] shared add-property StructureTransitionTable (siblings share structure_id, mirrors
           C++ Structure transitions); offset-indexed Butterfly storage (out_of_line_storage,
           getDirect/locationForOffset) lockstep w/ authoritative HashMap; INLINE_CAPACITY=0
    [done] ordinary object ToPrimitive ordering for current relational path
    [missing] full structure/watchpoint invalidation; dictionary/override/static-class predicates
  [wip] property access and inline caches
    [done] interpreter LLInt monomorphic GetByName/PutByName IC (GetByIdModeMetadata mirror)
    [done] generated-entry depth-1 GuardedPrototypeData holder loads (C++ GetByIdPrototype DataIC)
    [done] resident self-load + prototype-chain get_by_id DataIC machine code (CORRECT but
           unreached: emitted on FUNCTION bodies that never run generated)
    [missing] full Get/Put/In AccessCase taxonomy (multi-hop, transition, megamorphic)
  [wip] calls, constructs, and function values
    [done] direct-call/generated-call paths, callee preparation (mirrors C++ linkFor
           prepareForExecution), auto-materialization/invalidation telemetry, sidecar projection
           cache, call-link setup payloads (C++ JITCall.cpp shape)
    [risk] baseline generated path is a bytecode RE-INTERPRETER, not machine code -> Phase C
    [missing] full CallLinkInfo/function-executable fidelity; constructor/new-target breadth
  [wip] arrays: basic indexed reads/writes + array profile; [missing] full ArrayProfile/ArrayMode,
        indexed IC breadth
  [wip] strings: selected TS helpers, UTF-16 subset, RegExp-backed replace/split; [missing] full
        String.prototype + rope/string representation
  [wip] RegExp/Yarr: TS AMD-dependency subset, simple executor; [missing] full Yarr
        parse/execute/Unicode, RegExp JIT -> Phase G
  [wip] typed arrays: 8 Number-content constructors (Int8..Float64); subarray/ArrayBuffer pending
  [wip] Number autoboxing: toString(radix)/valueOf/wrapper construction done; [missing] Boolean
        toString/valueOf
  [wip] Array.prototype for array-likes: slice.call/toString done; [risk] other methods may not
        support non-Array this
  [wip] functions/globals: call/apply/bind (BoundFunction), global Function ctor, isFinite/isNaN,
        NaN/Infinity/parseFloat; [missing] new Function(string) dynamic compile
  [wip] Math: abs/floor/log/max/min/pow/random/sqrt/trunc + trig + ceil/round/sign/exp family;
        [missing] clz32/fround/imul edge fidelity
  [missing] Date, modules/jobs/microtasks/async ordering; [deferred] Wasm

[deferred -> Phase C/F] Execution tiers and JIT
  [done] stable CodeBlock identity (C++ single CodeBlock*): memoized fingerprint + Rc-shared
  [done] baseline machine-code groundwork (number/move/jump subset, get_by_id DataIC, retained
         exits, LoopHint handoff) -- ALL on the re-interpreter shim path; CORRECT but unreached
  [risk] baseline is a Rust bytecode RE-INTERPRETER (no register allocation); real reg-alloc and
         the absent optimizing tiers own score parity
  [missing] CRITICAL PATH: call-dispatch-into-generated -> mc put_by_id/call/construct/get_by_val
         -> slow-case rejoin -> inline alloc -> real reg-alloc; then DFG/FTL/B3 (Phase F)

[wip] GC, rooting, barriers, and handles  <- Phase C, NOW THE ACTIVE PRIORITY (profiling: per-op
      rooting/barrier/heap-binding bookkeeping is ~50-99% of self-time on 4/5 profiled benches)
  [done] bytecode root maps; targeted-root sync gated on register cell-membership; register/stack
         stores not barriered (C++ barriers only heap fields); VM-owned interpreter root scope
  [risk] per-op targeted-root REGISTRY (HashMap insert per dispatch) vs C++ conservative-scan-at-
         safepoint -- the dominant self-time tax; unbounded write-barrier Vec (heap.rs:847-876)
         records into a Vec nothing consumes; payload->cell HashMap identity bridge (bind_object_to_heap)
  [missing] no GC collection runs (heap.rs:603-636 never collects, unbounded growth); safepoints +
            conservative stack scan; inline/elided write barriers; direct cell pointers; full
            moving/marking GC; finalization/weak/ephemerons; rooting audit

[wip] Verification and integration discipline
  [done] focused Rust gates; macOS arm64 bring-up gate (x86_64 P6 entries guarded non-callable on
         arm64); subagent reviewer flow; one-logical-commit boundary
  [missing] local C++ JSC same-machine comparison harness for parity claims -> Phase B
```
