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
      [wip] splay, raytrace (running, slow interpreter, expected pass)
    [wip] Octane non-core (9 benchmarks)
      [done] non-ASCII string parsing fixed (unblocks code-load, pdfjs)
      [done] String.replace with function callback fixed
      [done] .5 numeric literal parsing fixed (unblocks box2d parsing)
      [wip] String.prototype.match implementation (unblocks regexp)
      [blocked] code-load, earley-boyer, gbemu, mandreel <= eval scope propagation
      [blocked] pdfjs <= non-ASCII strings fixed but may hit eval issue too
      [blocked] box2d <= __defineGetter__/__defineSetter__
      [blocked] regexp <= String.prototype.match missing
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
    [wip] TypeScript parser-prefix bytecode shape
    [missing] C++ ToNumeric/Inc update lowering
    [missing] full bytecode lowering parity audit

[wip] Runtime semantics
  [wip] objects, structures, properties, and prototypes
    [wip] basic object allocation and property storage
    [wip] prototype lookup and cacheability predicates
    [wip] property mutation planning and readiness
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
  [missing] typed arrays and ArrayBuffer breadth
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
  [missing] Date, Math, and standard-library completeness
  [missing] modules, jobs, microtasks, async ordering
  [deferred] Wasm

[wip] Execution tiers and JIT
  [done] generated baseline execution for accepted subset
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
    [wip] ToNumber slow-path continuation
    [wip] AddInt32 slow-path continuation/profiling
    [deferred] static generic rootless ToNumber admission
  [wip] property and call ICs
    [wip] property IC evidence and attachment
    [wip] call/direct-call telemetry and admission
    [missing] full C++ IC invalidation/watchpoint integration
  [missing] optimized JIT parity path
  [missing] loop tiering and OSR
  [missing] DFG/FTL-equivalent strategy or justified parity route

[wip] GC, rooting, barriers, and handles
  [done] bytecode root maps for current generated paths
  [done] targeted-root sync O(1) lookup + O(1) mutation (was O(instr x records^2))
  [risk] per-instruction eager targeted-root registry diverges from C++ JSC
         conservative-scan-at-safepoint; required by VM handoff UnresolvedRegisterRoot
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
