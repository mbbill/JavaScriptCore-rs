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

[wip] JetStream 3 Octane parity
  [done] Runner and benchmark contract
    [done] JetStreamDriver load order and shell globals
    [done] iteration, validation, scoring, and telemetry
    [done] benchmark/probe command surface for current investigations
  [wip] Current proof path — core 4/6 pass, full 4/15
    [wip] Octane core (6 benchmarks)
      [done] richards, navier-stokes, crypto, delta-blue (interpreter, scored)
      [risk] raytrace render path CORRECT (validated: checkNumber guard reached,
             monotonic w/ canvas size), but throughput-gated like the suite (30x30
             render ~57s; official = 15x 100x100/iter) -> NOT a cheap win; same
             generated-code throughput bottleneck as box2d
      [risk] splay functionally expected pass but pathologically slow (48min+,
             killed): it is the GC-stress benchmark and the engine runs no GC, so
             it allocates unboundedly -> reinforces GC is needed for splay perf
             (note: box2d RSS is flat, so box2d is NOT a GC problem -- different root)
    [wip] Octane non-core (9 benchmarks)
      [done] non-ASCII strings, replace-with-fn, .5 literals, String.match,
             __defineGetter__/__defineSetter__, global Function, standard Math
             (trig/round/sign/exp/hypot/...), apply/bind, isFinite/isNaN,
             NaN/Infinity/parseFloat globals
      [deferred] code-load, earley-boyer, gbemu, mandreel <= NO eval() support at all
                 (typeof eval === undefined). Real eval = runtime parse+compile in
                 caller scope + dynamic scope-chain resolution. Large subsystem, not
                 a quick capture fix. Verified empirically 2026-05-27.
      [blocked] box2d <= interpreter-COMPUTE-bound, NOT GC-bound (RSS flat ~1816kB
                 across a 64s Step; one Step ~53-60s interpreter CPU). Baseline tier
                 is currently a ~8.5x NET REGRESSION (MakeNewWorld 39s baseline vs
                 4.6s interpreter): generated code runs only ~2 bytecodes/entry then
                 exits+re-enters. Gate is generated-code RESIDENCY, capped by (1)
                 property-IC attachment in hot b2ContactSolver blocks (sidecar returns
                 None -> `Property` exit when no plan attached) and (2) user-JS `call`
                 exits (R.Clamp/SynchronizeTransform always terminate residency; only
                 native intrinsics stay resident). Non-cell property IC-hit resident
                 path ALREADY exists; LoopHint-handoff widening was the WRONG lever
                 (trigger already fires: 729 backedges/47 installs on MakeNewWorld;
                 residency is the gate, not the trigger). `MakeNewWorld(); true`
                 passes; `world.Step(1/60,10,3)` times out 120s. Next: measure
                 exit-reason breakdown, then fix IC attachment and/or resident call.
                 Object/BigInt bitwise and shift coercion remains a separate risk.
      [blocked] regexp <= complex Yarr patterns throw (match works, exec/replace
                 subset works; needs fuller Yarr execute)
      [blocked] pdfjs <= non-ASCII fixed; likely needs eval too
    [missing] full Octane correctness
    [missing] Octane score parity with local C++ JSC

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
    [missing] full C++ structure/watchpoint invalidation fidelity
    [missing] dictionary, override, and static-class-table predicates
  [wip] property access and inline caches
    [done] generated property handoff groundwork
    [done] property load/store observation flow for current hot path
    [done] named has/in dormant metadata and narrow generated sidecar
    [wip] access-case evolution and megamorphic policy
    [missing] full C++ Get/Put/In AccessCase taxonomy
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

[wip] Execution tiers and JIT
  [done] generated baseline execution for accepted (number/move/jump) subset
  [done] stable CodeBlock identity (faithful to C++ single CodeBlock*): memoized
         snapshot fingerprint + Rc-shared instance + interior-mutable runtime state
         (IC/value-profiles/tier); removed per-call O(N) re-fingerprint (~60-70%
         profiled self-time) and per-call deep clone
  [risk] baseline is still a Rust bytecode RE-INTERPRETER (~1.6x over interpreter,
         ~1.3M bytecodes/sec); top-level blocks get no generated execution. Parity
         needs machine-code dispatch: no native get_by_id/put_by_id/call lowering yet
  [wip] CRITICAL PATH to a real machine-code tier (in order): 1 StructureTransition
        Table [done] -> 2 fixed-offset Butterfly storage -> 3 inline mc GET/PUT_BY_ID
        -> 4 monomorphic call-link -> 5 per-opcode fast+slow slow-case rejoin ->
        6 inline alloc -> 7 real reg-alloc + retire the re-interpreter shim; GC deferred
  [wip] emitted native entry
    [done] number-number arithmetic fast path subset
    [wip] native-entry retained side-exit handling
    [blocked] remaining native-entry exits <= other owners and loop backedge proof
  [wip] retained exits and continuations
    [done] runtime-helper exits
    [done] property exits
    [done] JS-call exits
    [done] P6 side-exit native reentry
    [wip] opcode-specific side-exit cost reduction
  [wip] scanner/property-increment hot path
    [done] generated numeric load/inc/store sidecar
    [done] P10 native-exit combined increment sidecar
    [done] rootless admission for proven increment exits
    [done] producer-derived Int32 proof
    [done] hot scanner store-readiness coverage
    [done] interpreter store observation harvest
    [done] non-cell no-barrier store readiness proof
    [missing] C++ ToNumeric/Inc update lowering
  [wip] ToNumber/Add slow paths
    [done] interpreter arithmetic/bitwise primitive numeric coercion for
           current non-BigInt Number branch
    [wip] ToNumber slow-path continuation
    [wip] AddInt32 slow-path continuation/profiling
    [deferred] static generic rootless ToNumber admission
  [wip] property and call ICs
    [wip] property IC evidence and attachment
    [wip] call/direct-call telemetry and admission
    [missing] full C++ IC invalidation/watchpoint integration
  [missing] optimized JIT parity path
  [wip] loop tiering and OSR
    [done] LoopHint opcode, JSC loop-header placement, interpreter telemetry,
           LLInt LoopHint generated-artifact handoff skeleton,
           generated-baseline LoopHint counters, and native no-op lowering for
           current baseline subset
    [risk] LoopHint handoff TRIGGER already fires aggressively; residency (not the
           trigger) is the box2d gate -> further widening admission is counterproductive
    [missing] native inline LoopHint counter/operationOptimize flow matching
              JSC emit_op_loop_hint
    [missing] real loop OSR entry and optimized tier transition
  [missing] DFG/FTL-equivalent strategy or justified parity route

[wip] GC, rooting, barriers, and handles
  [done] bytecode root maps for current generated paths
  [done] targeted-root sync O(1) lookup + O(1) mutation + O(n) validate + buffer reuse
         (was O(instr x records^2) + per-instruction alloc; richards 12:13 -> 8:15,
         arith micro-bench 12.6s -> 8.0s)
  [risk] per-instruction eager targeted-root registry diverges from C++ JSC
         conservative-scan-at-safepoint; required by VM handoff UnresolvedRegisterRoot;
         interpreter still ~1.6us/op (architecture floor) -> parity must come from JIT
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
