# JSC Rewrite Status Tree

Current compact status source for the Rust JavaScriptCore rewrite. Keep this
file around 100-200 lines. Update only affected lines in accepted batches.
Detailed decisions belong in git commit messages, not this file.

Legend:

- [done] implemented and verified for the stated scope
- [wip] partially implemented or actively being expanded
- [missing] not implemented enough to rely on
- [blocked] blocked by the named dependency
- [risk] exists but needs fidelity or structure review
- [deferred] intentionally later than the current path

ACTIVE ROADMAP (settled 2026-05-29, strict order; see git log + memory):
  Phase 1 [mostly done] Interpreter performance for faster iteration. Cut 1: gate
                 per-op register-root sync on cell-membership (3.6-4.9x; arith 20M
                 245s->49s). Cut 2: drop per-register-write barrier (no C++
                 counterpart for stack stores). Residual 2nd tier (HashMap props,
                 CoreObjectCell, match dispatch, the sync itself on cell-churning
                 code like richards) deferred to Phase 3; full safepoint rewrite is
                 GC-coupled. Iteration is now fine for feature-dev (cargo test 0.8s).
  Phase 2 [wip]  Octane feature completeness (perf-independent): RUN all benchmarks
                 correctly. CORRECTED by ground-truth run-state (2026-05-29): the
                 4 "eval-blocked" benchmarks were actually COMPILE-blocked by a
                 store-to-unresolved-identifier bug -- FIXED (38ce3a4), now past
                 prepare. Remaining: indirect eval (code-load + zlib runtime throw),
                 the delta-blue regression, regexp/pdfjs hangs, splay(GC). eval is a
                 defined binding; only its INVOCATION throws.
  Phase 3 [deferred] JIT as the perf path toward parity. NOTE: suite-SCORE parity is
                 structurally owned by the DEFERRED optimizing tiers (DFG/FTL/B3
                 ~283k LoC) + real GC; a baseline-only JIT asymptotes ~10-25%. Do NOT
                 resume the per-opcode baseline-JIT residency grind until phases 1-2
                 are done -- it is correct groundwork but not on the parity path alone.

[wip] JetStream 3 Octane parity
  [done] Runner and benchmark contract
    [done] JetStreamDriver load order and shell globals
    [done] iteration, validation, scoring, and telemetry
    [done] benchmark/probe command surface for current investigations
  [wip] Current proof path. GROUND-TRUTH run-state (2026-05-29, interpreter, 60s budget):
    [done] SCORES: crypto (0.604), navier-stokes (1.355); richards scores given time
           (~79s/iter, 0.062). First real non-zero scores achieved.
    [wip] PAST-PREPARE BUT SLOW/TIMEOUT (functional, perf-gated -> Phase 1/3, not feature):
           richards, raytrace, splay (GC-stress), pdfjs, Box2D, typescript; regexp
           (hangs in regexp.js top-level, does NOT throw -- not a Yarr-throw as old
           tree claimed). delta-blue REGRESSED: old tree says scored, now times out
           even at 180s/2-iter -- investigate (Phase 2 correctness).
    [wip] RUNTIME THROW needing indirect eval: octane-code-load (indirect eval on a
           whole program) and octane-zlib (asm.js blob invokes eval). eval EXISTS as
           a binding; only invocation throws. Indirect-eval-as-global is the next
           Phase 2 target (the whole Eval parse/bytecompile pipeline already exists;
           one serial decision: native-call -> compile-pipeline re-entrancy).
    [done] COMPILE-BLOCK on implicit-global store FIXED (38ce3a4): gbemu, earley-boyer,
           mandreel now run past prepare; code-load now compiles (-> eval throw above).
    [done] feature breadth: non-ASCII strings, replace-with-fn, String.match,
           __defineGetter__/Setter__, global Function, Math, apply/bind, globals
    [note] Phase 3 (JIT) context: richards' hot FUNCTIONS don't run generated code
           (calls run callee in interpreter, generated_direct_call=0); the real JIT
           gate is call-dispatch-into-generated-code, not opcode coverage. Deferred.
    [missing] Octane score parity with local C++ JSC (needs optimizing tiers -- Phase 3)
    [note] box2d/regexp/pdfjs/splay specifics are perf/throughput-gated (Phase 1/3) or
           GC (splay); box2d Object/BigInt bitwise+shift coercion is a separate risk.

[wip] C++ JSC structural fidelity
  [done] always-context rewrite contract in CLAUDE.md
  [done] compact status tree is current status source
  [risk] pre-contract dirty tree needs logical commits or isolation
  [risk] existing Rust-only files/types need dedicated structure review
  [missing] dedicated C++-to-Rust structure audit batches
  [missing] commit-message decision log discipline for new batches

[wip] Parser and bytecompiler
  [done] source session and stable identifier groundwork
  [wip] parser fidelity for Octane language surface
    [done] labels, named break/continue, for-in, sequence expressions
    [done] string escape cooking and selected TypeScript syntax blockers
    [wip] large-program parser pressure
    [missing] complete C++ parser feature parity
  [wip] bytecode lowering fidelity
    [done] core statement/expression lowering for current Octane path
    [done] LoopHint placement at JSC loop body OSR headers for current loop forms
    [wip] TypeScript parser-prefix bytecode shape
    [missing] C++ ToNumeric/Inc update lowering
    [missing] full bytecode lowering parity audit

[wip] Runtime semantics
  [wip] objects, structures, properties, and prototypes
    [wip] basic object allocation and property storage
    [wip] prototype lookup and cacheability predicates
    [done] ordinary object ToPrimitive ordering for current relational path
    [wip] property mutation planning and readiness
    [done] shared add-property StructureTransitionTable: same-(kind,prototype)
           siblings share one structure_id (mirrors C++ Structure transitions),
           so structure-keyed ICs hit cross-instance incl. the new-Foo() path
    [done] offset-indexed Butterfly storage (out_of_line_storage Vec, getDirect/
           locationForOffset) written lockstep w/ the authoritative HashMap; hot
           load reads by offset (batch-3 machine-code mov target). INLINE_CAPACITY=0
    [missing] full C++ structure/watchpoint invalidation fidelity
    [missing] dictionary, override, and static-class-table predicates
  [wip] property access and inline caches
    [done] generated property handoff groundwork
    [done] property load/store observation flow for current hot path
    [done] named has/in dormant metadata and narrow generated sidecar
    [wip] access-case evolution and megamorphic policy
    [done] resident monomorphic self-load + prototype-chain (holder) get_by_id DataIC
           machine code emitted: receiver guarded by STRUCTURE not identity (mirrors C++
           generateGetByIdInlineAccessBaselineDataIC Self/Prototype + AccessCase::Load
           guardedByStructureCheckSkippingConstantIdentifierCheck); holder is a baked
           pinned cell ptr validated by StructureTransition watchpoints. Machine-code
           CORRECT (executes in tests), but see CRITICAL gate below: it does NOT move
           richards -- the emitted code is on FUNCTION bodies that never run generated.
    [missing] full C++ Get/Put/In AccessCase taxonomy (multi-hop chains, Put/transition,
              megamorphic stubs)
    [missing] proxy/indexed in activation and call-link status
  [wip] calls, constructs, and function values
    [wip] direct-call and generated-call paths
    [wip] rootless direct-call admission
    [missing] full CallLinkInfo/function executable fidelity
    [missing] constructor and new-target breadth audit
  [wip] arrays and indexed storage
    [wip] basic arrays and indexed reads/writes
    [wip] array profile observations for current in-by-val path
    [missing] full ArrayProfile/ArrayMode parity
    [missing] indexed IC breadth and storage-mode predicates
  [wip] strings
    [done] selected TypeScript string helpers
    [wip] UTF-16/code-unit semantics
    [wip] RegExp-backed replace/split subset
    [missing] full String.prototype and rope/string representation fidelity
  [wip] RegExp and Yarr
    [done] TypeScript AMD-dependency pattern subset
    [wip] simple Yarr executor subset
    [missing] full JSC Yarr parse/execute/Unicode semantics
    [missing] RegExp JIT and full String regex methods
  [wip] typed arrays — 8 Number-content constructors ported (Int8..Float64,
        faithful adaptor coercion); subarray, form edge cases, ArrayBuffer breadth pending
  [wip] Number autoboxing and prototype methods
    [done] Number.prototype.toString with radix (faithful C++ port)
    [done] Number.prototype.valueOf
    [done] number/boolean autoboxing in property access
    [done] Number/Boolean/String wrapper object construction
    [missing] Boolean.prototype.toString/valueOf
  [wip] Array.prototype completeness for array-like objects
    [done] Array.prototype.slice.call on arguments/array-likes
    [done] Array.prototype.toString calls join
    [risk] other Array methods may not support non-Array this
  [wip] functions and globals
    [done] Function.prototype.call/apply/bind (bind = CoreObjectKind::BoundFunction
           mirroring C++ JSBoundFunction); global Function constructor (non-constructible,
           no dynamic compile); isFinite/isNaN; NaN/Infinity/parseFloat globals
    [missing] new Function(string) dynamic compilation; bound-function construct args
  [wip] Math standard library
    [done] abs/floor/log/max/min/pow/random/sqrt/trunc + trig (sin/cos/tan/asin/
           acos/atan/atan2/sinh/cosh/tanh/asinh/acosh/atanh) + ceil/round/sign/exp/
           expm1/cbrt/log2/log10/log1p/hypot (JS round/sign semantics)
    [missing] clz32/fround/imul/log... edge fidelity audit
  [missing] Date and remaining standard-library completeness
  [missing] modules, jobs, microtasks, async ordering
  [deferred] Wasm

[deferred -> Phase 3] Execution tiers and JIT
  [done] stable CodeBlock identity (C++ single CodeBlock*): memoized fingerprint +
         Rc-shared instance + interior-mutable runtime state; no per-call re-fingerprint
  [done] baseline machine-code groundwork: number/move/jump subset, get_by_id self +
         prototype-chain DataIC (structure-guarded), retained exits (helper/property/
         JS-call/P6 reentry), increment sidecar, LoopHint handoff skeleton. ALL on the
         re-interpreter shim path; CORRECT but unreached on hot functions.
  [risk] baseline is a Rust bytecode RE-INTERPRETER (no register allocation); the real
         gate is call-dispatch-into-generated-code (generated_direct_call=0), then real
         reg-alloc. Score is owned by the absent optimizing tiers.
  [missing] CRITICAL PATH (deferred, in order): call-dispatch-into-generated -> mc
         put_by_id/call/construct/get_by_val -> slow-case rejoin -> inline alloc ->
         real reg-alloc; then DFG/FTL/B3-equivalent optimizing tier (where parity lives)

[wip] GC, rooting, barriers, and handles
  [done] bytecode root maps for current generated paths
  [done] targeted-root sync now gated on register cell-membership (Phase 1 cut 1):
         non-cell hot loops skip the per-op recompute (3.6-4.9x); register/stack
         stores no longer barriered (cut 2, faithful: C++ barriers only heap fields)
  [risk] still a per-op targeted-root registry (vs C++ conservative-scan-at-safepoint);
         full safepoint rewrite is GC-coupled -> Phase 3. Cell-churning code (richards)
         still pays the sync when it runs.
  [wip] targeted roots around helper exits
  [wip] write-barrier evidence for current property stores
  [missing] no GC collection runs during execution (unbounded heap growth)
  [missing] full moving/marking GC fidelity
  [missing] finalization, weak references, and ephemerons
  [missing] complete handle/rooting model audit against C++ JSC

[wip] Verification and integration discipline
  [done] focused Rust gates for current accepted slices
  [wip] release octane_probe evidence for active bottlenecks
  [missing] local C++ JSC comparison harness for parity claims
  [missing] standard subagent reviewer flow for every substantial patch
  [missing] one logical commit per accepted batch going forward
