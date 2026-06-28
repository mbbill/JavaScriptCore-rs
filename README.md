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
ACTIVE ROADMAP (validated, profiling-earned; default path = InterpreterOnly; status 2026-06-27):
  Phase A [done]   de-anchor + quarantine (c8e83ad): ARM64 admission cluster gated off-by-default.
  Phase B [done]   per-bench profiling -> GC-first earned (per-op bookkeeping was 50-99% self-time).
  Phase C [wip]    GC-first tax removal -- DONE: barrier elision (db23cbd), safepoint register-rooting
                 (830b686) + frame-rooting (9fdc938) = richards ~7.5x; O(1) cell-record lookup + FxHash
                 maps (3f8ee9b); JSCell type-header = Route B S1 (6e96182). Abstract-equality fix lands
                 pdfjs (e499fd9). 8/15 score (was 3).
  Phase C-BLOCKED  Route B cell-deref (S2) + the real mark/sweep collector are BLOCKED on S4 (a
                 provenance-stable Heap-owned arena): the carried int->ptr cell deref is miri-proven UB
                 (Stacked+Tree Borrows) in the current Vec<Pin<Box>> skeleton, and a sweeping collector
                 breaks the dense CellId index. S4 = the irreversible, pervasively-unsafe gate -> OWNER.
  Phase B2 [pending] local C++ JSC same-machine comparison harness (parity-gap; jsc build by owner).
  Phase D [deferred] real machine-code baseline JIT (MacroAssemblerARM64 + ExecutableAllocator).
  Phase E [wip]   megafile split by JSC runtime/ boundaries: interpreter/mod.rs 41k->33k, ALL 4
                 runtime-class stores extracted to interpreter/{string,bigint,symbol,object}_store.rs
                 (B1-B4 done, pure byte-exact code-motion, gates green). Stores isolated -> the cutovers
                 (Structure-wire + R3 in object_store, StringImpl-swap in string_store) can now run in
                 parallel. Remaining: interpreter-core split (E.2, non-blocking); vm/mod.rs (74k).
  Phase F [blocked] DFG/FTL/B3 optimizing tier -- where suite-SCORE parity ultimately lives.
  Phase G [parallel] Yarr/RegExp (regexp throw on lookahead + \b correctness).
  NEAR-TERM LEVERS (profiling-earned, faithful, owner-overseen): (1) call-link tiering PER-CALLSITE
                 refactor -- bound the unbounded O(N^2) logs to JSC CallLinkInfo -> lands earley-boyer +
                 typescript (-> 10/15); (2) string rep UTF-8 -> WTF::StringImpl Latin-1/UTF-16 -> ~10x
                 pdfjs + all string-heavy benches; (3) gbemu/Box2D value-divergence BUGS (need jsc compare).

[wip] Faithful foundation rebuild (JIT-anchored; all UNWIRED behind dead_code until Phase E cutovers)
  [done] value -> JSVALUE64 NaN-boxing (lossless double + immediates); raw-cell cfg-fork s4_raw_cell
  [done] S4 cell arena: MarkedSpace/MarkedBlock/BlockDirectory/FreeList/PreciseAllocation (miri-proven)
  [done] SlotVisitor STW marking core (mark-stack drain + visitChildren) -- collector RUN-gated R3/R4
  [done] Structure: leaf ports (PropertyOffset/IndexingType/TransitionTable/PropertyTable) + Structure
         cell (StructureID/StructureIdTable/TypeInfoBlob) -- NEW module beside the live DSL
  [done] StringImpl Stage A (8/16-bit Latin-1/UTF-16, O(1) index)
  [done] profiling fuel: ArithProfile + ExecutionCounter (faithful packed bitfields, profiling.rs) +
         SpeculatedType uint64 bitset (new module) -- counter/speculation canonicalization is serial
  [done] assembler: AbstractMacroAssembler operands + RegisterID + ARM64 instruction encoder (new
         src/assembler/*, byte-oracle-proven vs the known-good prologue bytes) -- not yet emitting
  [done] bytecode: faithful packed instruction-stream core (Vec<u8>, byte-offset index, Narrow/Wide16/
         Wide32 width, size()-advance) -- replacement-in-waiting for the typed-Vec-by-ordinal divergence
  [missing] WIRING is gated on Phase E (now unblocked) -> R3/R4 arena cutover -> Structure-wire; the
         baseline JIT additionally needs arm64_baseline to emit via the encoder + the W^X unsafe keystone

[wip] JetStream 3 Octane parity
  [done] Runner/benchmark contract: JetStreamDriver load order, shell globals, iteration,
         validation, scoring, telemetry, probe command surface
  [wip]  Run-state (2026-06-27, interpreter, iter=2): 8 SCORE / 4 too-slow / 3 throw.
         Suite geomean is None until all 15 Succeed (shell/octane.rs:1996), score=5000/time_ms.
  [done] SCORES (8, up from 3 pre-GC-waves; richards ~7.5x): octane-code-load 89, navier 5.3,
         crypto 3.5, splay 1.0, richards 0.91, pdfjs 0.88 (slow ~194s), delta-blue 0.62, raytrace 0.23
  [wip]  TOO-SLOW (4; perf-gated, >90s): earley-boyer, typescript, mandreel (asm.js),
         octane-zlib (asm.js). mandreel/zlib likely need the JIT (Phase F).
  [missing] THROW (3): Box2D + gbemu (value-divergence BUGS needing data-flow debugging, not
         feature gaps), regexp (full Yarr gap). pdfjs FIXED -- was a feature gap (abstract
         equality object==primitive ToPrimitive), now scores.
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
  [wip]  vm/mod.rs (74k) still oversized; interpreter/mod.rs 41k->33k (Phase E B1-B4 done: all 4
         runtime-class stores split to interpreter/*_store.rs by JSC runtime/ boundary)
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
