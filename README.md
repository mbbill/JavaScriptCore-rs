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

```text
ACTIVE ROADMAP (settled 2026-05-29, strict order; see git log + memory):
  Phase 1 [wip]  Interpreter performance. Landed: per-op root sync gated on cell-membership
                 (3.6-4.9x); dropped per-register-write barrier; monomorphic GetByName/PutByName
                 IC (7dd0659). A wall profile (2026-05-30) corrected the cost model: the dominant
                 per-call cost is ROOTING + a wasted eager root snapshot, NOT IC safepoint passes
                 (~0 wall). Landed since: skip idempotent passes (e50f685, 90091ab), lazy
                 fallback-boundary-snapshot (3ff6095), in-loop root-sync narrowed to the dirtied
                 slot range (6ebfe66) -- richards 140s->112s, crypto/navier/code-load +30-60%;
                 VM-owned interpreter root scope keeps caller roots live across nested calls.
                 macOS arm64 richards after VM-root-scope: interpreter score=0.1219. Baseline
                 call-link telemetry now shows ~4.05M authorized direct-call transactions.
                 Host-callability fallback moves ~3.39M to GeneratedEntry; ready sealed native
                 direct-call callees now outrank GeneratedEntry when both artifacts exist, but the
                 broad blocker remains: generated baseline is still a Rust bytecode re-interpreter
                 and is slower than the optimized interpreter on richards. Capped telemetry now
                 shows generated dispatch heat dominated by GetByName, JumpIfFalse, PutByName,
                 and CallWithThis in the hot generated owners; generated-entry depth-1
                 GuardedPrototypeData loads now use the C++ GetByIdPrototype-shaped
                 receiver-structure + holder/offset fast path, with the 500k richards cap
                 still showing owner 110@32/76 heat and zero generated invalidations.
                 Generated Call/CallWithThis sidecars now carry same-dispatch setup payloads
                 into VM direct-call validation, avoiding duplicate argument register reads
                 while preserving the generated re-interpreter as the broad blocker.
  Phase 2 [done] Octane feature completeness: all 15 benchmarks RUN correctly. Ground-truth
                 2026-05-30: ZERO throwers/aborts -- 3 score, 12 functional-but-slow
                 (perf-gated, Phase 1). Landed: implicit-global store, indirect eval, catchable
                 TypeError + Array.prototype ToObject, putToPrimitive, RegExp getters, escape/
                 URI globals, ToPrimitive on object arithmetic. Deferred (not blocking any run):
                 @@Symbol.toPrimitive, BigInt+Number-mix TypeError, Yarr.
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
  [wip] Current proof path. GROUND-TRUTH run-state (2026-05-30, interpreter, 30s/2-iter):
        ZERO throwers/aborts across all 15. Suite geomean is None until ALL 15 Succeed
        (shell/octane.rs:1996); per-bench score=5000/time_ms (:2693); the 30s probe timeout
        is a budget, not a scoring cutoff.
    [done] SCORES (3): octane-code-load (10.0), crypto (0.88), navier-stokes (2.20) -- up
           ~30-60% from the call-perf batches (lazy snapshot + in-loop root-sync narrowing,
           confirmed 2026-05-30; the full-window root rescan was a broad per-op tax).
    [wip] FUNCTIONAL-BUT-SLOW (12; perf-gated -> Phase 1, not a feature gap): Box2D, delta-blue,
           earley-boyer, gbemu, mandreel, pdfjs, raytrace, regexp, richards (~56s/iter),
           splay (GC-stress), typescript, octane-zlib (asm.js, ~18-60M calls/iter). The gate to
           all 15 Succeeding (hence any suite geomean) is throughput; after the VM-stack root
           scope and call-link telemetry, richards is correctness-clean; generated direct-call
           residency now works for most hot calls but regresses score until generated execution
           stops re-interpreting bytecode.
    [done] feature breadth: non-ASCII strings, replace-with-fn, String.match,
           __defineGetter__/Setter__, global Function, Math, apply/bind, globals
    [missing] Octane score parity with local C++ JSC (needs call-dispatch + optimizing tiers)

[wip] C++ JSC structural fidelity
  [done] always-context rewrite contract in AGENTS.md
  [done] oversized-file guardrail in AGENTS.md
  [done] compact status tree is current status source
  [risk] pre-contract dirty tree needs logical commits or isolation
  [risk] existing Rust-only files/types need dedicated structure review
  [wip] dedicated C++-to-Rust structure audit/extraction batches
    [done] VM CallLinkInfo/generated direct-call execution cluster extracted to
           src/vm/call_link.rs (maps to C++ CallLinkInfo/LLIntSlowPaths/JITCall)
    [done] VM generated property IC exit/handoff cluster extracted to
           src/vm/property_handoff.rs (maps to C++ JITPropertyAccess/
           JITOperations/PropertyInlineCache)
    [done] VM generated CodeBlock entry executor extracted to
           src/vm/generated_executor.rs (maps to C++ ScriptExecutable/CodeBlock/JITCode entry)
    [done] JIT ARM64 return-seed emitter extracted to src/jit/arm64_baseline.rs
           (maps to C++ JIT/MacroAssemblerARM64; behavior unchanged)
    [done] JIT ARM64 control-flow/direct-branch/semantic-byte-builder cluster
           extracted to src/jit/arm64_baseline/control_flow.rs (includes dormant
           direct branches, retained side-exit stubs, and primitive JumpIfFalse
           byte slice; maps to C++ MacroAssemblerARM64/ARM64Assembler branch
           linking plus JIT Jump/JumpList/JumpTable/SlowCaseEntry; behavior unchanged)
    [done] JIT ARM64 GPRInfo/JSRInfo/AssemblyHelpers register materialization
           contract skeleton added to src/jit/arm64_baseline.rs (metadata only)
    [done] JIT ARM64 dormant virtual-register frame materialization helpers and
           private JumpIfFalse branch-aware callable encoder skeleton added to
           src/jit/arm64_baseline.rs (public branch-aware callable emission
           remains disabled)
    [done] JIT/VM ARM64 callable side-exit payload bridge added: ARM64 stubs return
           retained P6 payloads and the VM decodes them before JSValue wrapping
           (Rust native-entry ABI/rooting bridge only; C++ jfalse truthiness remains
           valueIsFalsey/LLInt slow-path work)
    [done] JIT/VM retained side-exit records, VM retained-return tables,
           src/vm/side_exit.rs resolver metadata, and src/vm/native_reentry.rs
           request bridge preserve JumpIfFalse taken/fallthrough native reentry
           targets; x86/private truthiness exits now single-dispatch and resume
           native through exact two-label metadata, dormant public ARM64 proof
           now reuses the decoded JumpIfFalse shape before descriptor-range
           checks, retained ARM64 fallback can privately reenter a resolved P6
           target, and public ARM64 callable admission still rejects to the
           existing x86 semantic artifact.
    [done] VM native call-frame publication skeleton added to src/vm/entry.rs:
           C++ TopCallFrameSetter/NativeCallFrameTracer-shaped publish/restore
           records for future public ARM64 admission; src/vm/native_reentry.rs
           now separates missing top-call-frame publication from symbolic
           publication-without-conservative-root proof; src/vm/call_frame_storage.rs
           now adds a C++ CallFrame.h-shaped boxed header store whose stable
           caller-slot address is the future FrameAddress source, with
           VM-owned active/retired storage handles and storage-derived native
           publication proofs while the raw native ABI frame-base stays
           separate; src/vm/entry_frame_storage.rs adds the dormant
           EntryFrame.h/VMEntryRecord.h-shaped previous-top-pair storage/proof
           skeleton, and src/vm/entry.rs now has a dormant storage-backed
           VM-entry guard that validates/restores the distinct topCallFrame /
           topEntryFrame pair and is the only reachable entry-guard path for
           native call-frame publication; src/vm/native_reentry.rs now derives
           the publication proof from those storage-backed entry/call-frame
           guards before rejecting at the conservative-root blocker;
           src/gc/machine_stack_marker.rs and src/wtf/stack_bounds.rs add the
           dormant MachineStackMarker/RegisterState/WTF StackBounds-shaped
           crate-internal, heap/epoch-bound current-thread gather+ingest
           closure with real macOS/aarch64 stack-bounds and x19-x28 capture,
           without treating boxed VM frame storage as machine-stack evidence;
           src/gc/heap/run_current_phase.rs adds the dormant
           Heap::runCurrentPhase / collectInMutatorThread NeedCurrentThreadState
           handshake that reruns mutator Fixpoint with the exact captured
           CurrentThreadState; src/gc/conservative_roots.rs adds the dormant
           ConservativeRoots descriptor with exact published-payload validation
           only, src/gc/visitor.rs adds the descriptor-only
           SlotVisitor::append(ConservativeRoots) boundary, and
           src/gc/heap/conservative_scan.rs adds the heap conservative-scan
           append receipt that scopes the visitor reason locally;
           src/gc/heap/marking.rs and src/gc/visitor/conservative_marking.rs
           add heap-owned conservative-root test-and-set evidence plus the
           SlotVisitor JSCell queue / Auxiliary live-note action split;
           src/gc/visitor/collector_effects.rs adds the SlotVisitor
           mark-stack/cell-state/container-effect proof, src/jit/stub_routines.rs
           adds the JITStubRoutineSet may-be-executing trace proof,
           src/vm/vm_roots.rs adds VM scratch-buffer / checkpoint side-state
           root gather evidence, and src/vm/native_reentry.rs consumes them
           before rejecting at the missing verifier append / real native-rooting
           blocker;
           native execution unchanged.
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
           locationForOffset) written lockstep w/ the authoritative HashMap; interpreter
           and generated hot data loads read by offset (machine-code mov target). INLINE_CAPACITY=0
    [missing] full C++ structure/watchpoint invalidation fidelity
    [missing] dictionary, override, and static-class-table predicates
  [wip] property access and inline caches
    [done] interpreter LLInt monomorphic GetByName/PutByName IC (GetByIdModeMetadata
           mirror): per-site {structure,offset,state} cache, own-data GET read +
           replace-existing PUT from out_of_line_storage; warmup-gated so it does
           not starve the observation pipeline; PUT ~5x, GET access ~2x on micro
    [done] generated property handoff groundwork
    [done] property load/store observation flow for current hot path
    [done] generated-entry depth-1 GuardedPrototypeData holder loads: receiver
           structure guard + holder/offset read mirrors C++ GetByIdPrototype DataIC
           while preserving Rust sidecar priority and pending-invalidation fallback
    [done] named has/in dormant metadata and narrow generated sidecar
    [wip] access-case evolution and megamorphic policy
    [done] resident monomorphic self-load + prototype-chain (holder) get_by_id DataIC
           machine code (structure-guarded, watchpoint-validated holder ptr); CORRECT
           but does NOT move richards -- emitted on FUNCTION bodies that never run generated
    [missing] full C++ Get/Put/In AccessCase taxonomy (multi-hop chains, Put/transition,
              megamorphic stubs)
    [missing] proxy/indexed in activation and call-link status
  [wip] calls, constructs, and function values
    [wip] direct-call and generated-call paths
    [done] disabled native-entry metadata on macOS arm64 no longer blocks portable
           generated baseline residency when the generated subset can represent the callee
    [done] macOS arm64 P6 return-seed native entry: no-call/no-heap callable return shapes
           install real ARM64 C-ABI bytes; arithmetic/unsupported bodies keep the existing
           x86_64 semantic artifact or generated/interpreter fallback path, and direct-call
           continuation PC metadata remains deferred
    [done] host-noncallable P6 x86_64 auto-installs also materialize generated
           residency on arm64; richards top direct calls now route to GeneratedEntry
    [done] direct-call callee preparation path: mirrors C++ linkFor()
           prepareForExecution for host-blocked and missing-native-gate callees; the 50k
           richards probe now routes the former 110@80 -> 146 MissingArtifact fallback through
           GeneratedEntry, with unsupported generated callees recording one failed install
    [done] generated auto-materialization/invalidation telemetry preserves install
           stage, validator detail, and generated-code invalidation source/outcome
           through Octane benchmark summaries
    [done] generated property-handoff current-metadata validation: warmed bytecode ICs
           no longer make generated install or invalidated-artifact exits apply cold
           PropertyHandoffPlan cache checks; focused tests cover bytecode-0 host-blocked
           callee auto-materialization and invalidated GetByName slow-path dispatch
    [done] generated property sidecar projection cache: property load/store/has tables are
           retained per owner/snapshot under CodeBlock-registry, plan-generation, and
           megamorphic-projection epochs; terminal guarded misses now retire only the guarded
           candidate and refresh projection without jettisoning the generated owner artifact
    [done] generated executor per-invocation dispatch cap: Rust-only diagnostic guard
           makes the bytecode re-interpreter shim honor DispatchConfig for one generated
           invocation while default helper callers stay unbounded
    [done] generated executor source-entry dispatch budget: VM/source-entry scoped budget
           is shared across generated-entry interpreter fallbacks and generated resumes; 50k
           capped richards runner now returns DispatchStepLimitExceeded instead of staying
           in the runner
    [done] rootless direct-call admission: full validated monomorphic generated-entry
          handoffs no longer require an extra Rust hot-slot hit before rootless dispatch;
          missing generated artifacts stay on the rooted CallLinkInfo::linkFor-style slow path
          and become rootless only after that path prepares the callee artifact
    [done] generated Call/CallWithThis setup payload: sidecars carry the C++ JITCall.cpp-shaped
           same-dispatch this+argument frame setup snapshot into VM CallLinkInfo validation;
           malformed payloads fall back, and plain Call preserves implicit undefined this
    [done] generated direct-call route-opportunity telemetry: generated-entry successes now
           report the selected/preferred route and native-entry miss reason per site/target,
           exposing macOS arm64 native blockers without changing route selection
    [done] generated direct-call MissingGate preparation: the CallLinkInfo/linkFor-style
           slow path now attempts native baseline entry materialization before publishing
           a generated callee fallback, so route telemetry reports concrete native failures
    [done] generated direct-call backend-contract detail: native auto-materialization telemetry
           now preserves backend contract variants; capped richards confirms hot failures are
           ARM64 return-seed UnsupportedOpcodeSubset, not an undifferentiated native gate
    [done] native-entry readiness body-capability metadata: enabled readiness now records
           install-proof-derived subset/effect evidence, and rootless native admission consumes
           that body evidence instead of callable-kind coverage diagnostics
    [done] ARM64 return-seed subset fallback policy: seed UnsupportedOpcodeSubset now keeps
           the existing prepared x86 semantic/generated artifact path; capped richards moves
           hot native misses to HostBlockedX86_64, making broader ARM64 backend coverage next
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
  [risk] baseline is a Rust bytecode RE-INTERPRETER (no register allocation); macOS richards now
         proves call-link admission (~4.05M direct-call transactions) and generated callee
         residency (~3.39M GeneratedEntry, ~0.66M NestedInterpreterFallback); a narrow native
         entrypoint preference now bypasses the generated callee re-interpreter when sealed native
         readiness exists, but score regresses to ~0.0458 because general generated execution still
         re-dispatches bytecode. A 500k capped probe reports top generated opcode heat as owner
         110 GetByName=47,408, then owner 146/185 GetByName, JumpIfFalse, PutByName, and
         CallWithThis. Real reg-alloc and the absent optimizing tiers own score parity.
  [missing] CRITICAL PATH (deferred, in order): call-dispatch-into-generated -> mc
         put_by_id/call/construct/get_by_val -> slow-case rejoin -> inline alloc ->
         real reg-alloc; then DFG/FTL/B3-equivalent optimizing tier (where parity lives)

[wip] GC, rooting, barriers, and handles
  [done] bytecode root maps for current generated paths
  [done] targeted-root sync now gated on register cell-membership (Phase 1 cut 1):
         non-cell hot loops skip the per-op recompute (3.6-4.9x); register/stack
         stores no longer barriered (cut 2, faithful: C++ barriers only heap fields)
  [done] VM-owned interpreter root scope mirrors C++ live call-frame stack lifetime:
         caller roots survive nested calls, and frame pop cleans only the popped window
  [risk] still a per-op targeted-root registry (vs C++ conservative-scan-at-safepoint);
         full safepoint rewrite is GC-coupled -> Phase 3. Cell-churning code still pays
         dirty-slot syncs when cell membership changes.
  [wip] targeted roots around helper exits
  [wip] write-barrier evidence for current property stores
  [missing] no GC collection runs during execution (unbounded heap growth)
  [missing] full moving/marking GC fidelity
  [missing] finalization, weak references, and ephemerons
  [missing] complete handle/rooting model audit against C++ JSC

[wip] Verification and integration discipline
  [done] focused Rust gates for current accepted slices
  [done] macOS arm64 bring-up gate: x86_64 P6 emitted native entries are guarded
         as non-callable on arm64, while generated/interpreter fallbacks keep tests green
  [done] release octane_probe evidence for VM-stack rooting on macOS arm64: nested-call eval
         works with zero fallbacks; richards via ../WebKit/PerformanceTests/JetStream3 returns
         score=0.1219 avg=0.1059 with zero fallbacks
  [done] baseline-mode richards evidence: score=0.1121, fallbacks=4, baseline_installs=35,
         generated_artifacts=5, generated_direct_call_transactions=4,046,563, all
         NestedInterpreterFallback
  [done] macOS arm64 disabled-native fallback evidence: focused tests prove a
         ReadyButExecutionDisabled native artifact auto-materializes generated baseline code and
         ordinary CallLinkInfo can route to GeneratedEntry, but richards stays score=0.1119 with
         ~4.05M NestedInterpreterFallback direct-call transactions
  [done] macOS arm64 host-callability fallback evidence: richards baseline now records
         generated_artifacts=40, generated_direct_call_generated_entries=3,389,270,
         nested_interpreter_fallbacks=657,293, hot_slot_hits=2,731,836, but score regresses to
         0.0458 because generated baseline is still the re-interpreter shim
  [done] direct-call fallback evidence: top remaining richards nested fallback is
         caller=110@80 target=146 count=657,293 with generated_entry_miss=MissingArtifact and
         native_entry_miss=HostBlockedX86_64
  [done] direct-call callee auto-materialization evidence: focused arm64 test proves
         supported host-blocked native callees publish a generated entry; earlier richards
         probe showed target 146 blocked by PropertyHandoffPlan malformed bytecode cache
  [done] generated property-handoff current-metadata evidence: C++ baseline JIT creates
         fresh property IC slots from profiled CodeBlock metadata; Rust install now derives
         from current metadata and focused tests cover warmed bytecode-0 and bytecode-1 ICs
  [done] generated dispatch-budget evidence: focused tests prove a capped generated infinite
         loop returns DispatchStepLimitExceeded, default generated execution remains unbounded,
         and a source-run fixture spends one budget across generated-entry fallback/resume
  [done] richards bounded dispatch-guard release probe: 50k macOS arm64 baseline probe reaches
         the runner then fails at DispatchStepLimitExceeded with tiering summary after the
         guarded-IC terminal miss reset; generated-code invalidations=0, but unbounded richards
         throughput remains a performance blocker, so benchmark progress is not claimed
  [done] rootless direct-call hot-slot gate evidence: C++ CallLinkInfo fast path has no second
         hot-slot proof after monomorphic linking; focused test proves the second generated-entry
         direct call skips entry root sync, and 50k richards rootless rejections drop 212->31->0
         after guarded IC reset while hot_slot_miss stays 0
  [done] generated opcode-heat evidence: 500k capped richards baseline probe still fails at
         DispatchStepLimitExceeded with generated-code invalidations=0 and rootless rejections=0,
         but now reports owner/opcode plus owner/site/opcode/readiness heat for the next C++
         JIT::emit_op_* selection (owner 110 GetByName=47,408; first surfaced GetByName site
         owner 110@32 GuardedPrototypeData=7,611)
  [done] generated call-link setup-payload evidence: reviewer accepted extraction + payload
         after the plain-Call implicit-this guard; focused payload/hot-slot/rootless tests and
         full gates pass; 500k capped richards still exits at DispatchStepLimitExceeded with
         invalidations=0, rootless_rejections=0, sidecar_hot_slot_hits=28,548, and owner
         110@36 CallWithThis heat=7,612
  [done] macOS arm64 return-seed native evidence: focused tests prove constants, moves,
         frame/argument, and callee returns enter real ARM64 native entry without generated
         execution; arithmetic fallback keeps the existing x86_64 semantic artifact path
  [missing] local C++ JSC comparison harness for parity claims
  [done] subagent reviewer flow used for current substantial patch
  [done] one logical commit boundary restored for current accepted batch
```
