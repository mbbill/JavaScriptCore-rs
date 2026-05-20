# Priority-Managed BFS Rewrite Plan

This rewrite is breadth-first, but breadth-first is not enough by itself. The
main risk is local tuning: an agent can spend a long time making one small path
work while the surrounding engine infrastructure is still missing.

The rewrite must be managed by priority, dependency order, and parallelism. A
passing local test is useful only when it proves the intended engine boundary.

Use `progress.md` for sparse completed checkpoints. This file is the architect
operating contract and the current scheduler.

The `/goal` reminder is long-lived. It is the durable charter for the whole
goal session, not the current work queue. It should describe only stable
identity, role boundaries, rewrite principles, and priority-management
standards for the whole rewrite.

Stage-specific priorities, blockers, next batches, temporary rules, accepted
checkpoints, and risks belong in this plan or in the sparse progress record, not
in `/goal`.

Do not put immediate rules, the current tactical batch, or any short-term
instruction into `/goal`. The goal text will be repeated for the whole goal
session, so anything that should expire after the next checkpoint must stay in
the scheduler section of this document.

Suggested durable `/goal` text:

```text
Act as architect and lead reviewer for the single-crate Rust JavaScriptCore
rewrite. Preserve JavaScriptCore's real engine responsibilities while adapting
them to Rust ownership, rooting, frame, exception, runtime, GC, and execution
tier contracts.

Own priority, dependency order, and parallelism across the whole rewrite. Keep
the rewrite breadth-first before depth-first: expand and validate major engine
boundaries before deep local tuning, and choose work by the most important
unblocked engine dependency rather than local test convenience.

Main agent role: maintain architecture, manage the dependency graph, delegate
large implementation and audit work to agents, review and integrate their
patches, run gates, and keep progress honest. Use sub-agents for large or
parallelizable batches; implement locally only for trivial glue or tightly
bounded fixes.

Use `Source/JavaScriptCore/rust/docs/002-bfs-rewrite-plan.md` and
`Source/JavaScriptCore/rust/docs/progress.md` as the mutable scheduler and
checkpoint record between context compressions.
```

## Current Phase

We are moving from foundation scaffolding to the real execution spine.

The next major milestone is an honest two-mode engine:

```text
canonical bytecode
  -> LLInt/reference interpreter as semantic oracle
  -> VM-owned baseline JIT tier for a narrow opcode subset
  -> shared frame/root/exception/runtime/GC contracts
  -> interpreter fallback for unsupported bytecode
```

The first baseline JIT can be narrow. It must not be fake. It must enter through
VM-owned `CodeBlock` and tiering state, use the shared value/frame/root/fallback
contracts, and prove that generated execution and interpreter execution agree at
the VM boundary.

## Current State

The Rust tree is a single crate with module-level subsystem boundaries.

Accepted progress includes:

- parser -> bytecompiler -> interpreter execution for a broad JavaScript subset;
- many VM/runtime semantic tests;
- staged heap/cell/root/barrier ownership contracts;
- VM-owned CodeBlock identity and baseline install/readiness machinery;
- typed generated baseline stand-in and metadata/proof layers;
- P6 operand-aware lowering;
- P6 backend/value-frame/ABI contract;
- P6 symbolic instruction selection and semantic non-callable x86_64 byte
  emission;
- P6 VM-owned semantic baseline materialization behind disabled native-entry
  readiness and interpreter fallback;
- P6 callable-byte/platform substrate: non-authoritative C-ABI-shaped x86_64
  bytes with reserved-payload side-exit return stubs, and a sealed platform
  call helper that keeps executable addresses private;
- P6 narrow emitted native entry for the accepted constants/moves/return/int32
  arithmetic subset, including exact VM callable readiness, guarded frame-base
  borrowing, platform-compartment execution, VM-owned reserved-payload side-exit
  tables with precise interpreter fallback PCs, stale snapshot fallback, and
  descriptor-only non-publication;
- P4a frame/side-exit alignment: copied register-window descriptors must be
  revalidated against the active top frame before generated/native access, and
  emitted native side exits now return identity payloads consumed only through
  VM-owned readiness/artifact/snapshot tables;
- P4b reference-interpreter call/return continuation spine: ordinary `Call`
  and `CallWithThis` now create explicit continuation records, attach them to
  callee frames, finish returns through a single pop/validate/write path, and
  reject forged generated-call continuation metadata at the VM handoff boundary.
- P4c generated runtime-helper exit transactions: runtime-helper handoffs now
  revalidate current CodeBlock-derived helper proofs at the VM boundary,
  materialize only exact root-map-filtered temporary VM register roots, suspend
  and resume no-GC across helper return/fail/throw/reject paths, roll back
  disallowed helper throws, and reject stale or forged helper metadata before
  dispatch.
- P4d generated property handoff boundary hardening: property handoffs now
  revalidate active frame, registered CodeBlock snapshot, installed artifact
  property plan, and derived property site metadata at the VM boundary, enforce
  no-GC and may-throw parity, validate property-load destination-root targets
  exactly, and reject stale or forged property metadata before dispatch.
- P4e exception/fallback cleanup: handler-consumed exceptions now complete
  unwind state and drop unwind-pending roots while preserving last exception,
  generated single-dispatch failure paths restore and sync exception state
  before returning `Failed`, and unmatched native side-exit payloads reject
  instead of replaying from an imprecise PC.
- P5a VM-owned generated JS direct-call boundary: attached call-link metadata
  can authorize ordinary monomorphic bytecode `Call`/`CallWithThis` sidecar
  hits as VM transaction requests only; the VM revalidates the active caller
  frame, CodeBlock snapshot, candidate table, target executable/CodeBlock/
  callee, boundary, `this`, arguments, destination, and resume PC before
  pushing the callee frame, suspends and restores generated no-GC while the
  bytecode callee runs, completes return through the continuation spine,
  propagates throws with exception roots, and rejects forged direct-call
  metadata before dispatch.
- P6 emitted-native differential matrix: the current x86_64 emitted subset now
  has VM-level interpreter-vs-native coverage for constants, moves, return,
  `AddInt32`, `SubInt32`, and `MulInt32`, plus precise side-exit coverage for
  overflow, non-int32 operands, negative zero, unsupported-bytecode install
  rejection, stale snapshot fallback, and launch/readiness side-effect guards.
- P7a VM-retained runtime-helper native-exit authority: emitted-native reserved
  payload routing now has a separate VM-owned runtime-helper site table, helper
  payloads dispatch through the existing generated runtime-helper transaction,
  owner/frame authority comes from active native dispatch, and synthetic
  `NewObject` helper exits reject stale or forged metadata before dispatch.
- P7b real emitted-native `NewObject` helper exit: callable P6 semantic
  materialization can now derive the `NewObject` runtime-helper plan, lower it
  as a helper-native-exit instruction, emit a reserved payload return, retain
  helper-site metadata in the VM table, dispatch through the existing
  runtime-helper transaction, and resume through interpreter fallback.
- P7c emitted-native runtime-helper exit matrix: `NewArray`, `LoadString`,
  `LoadBigInt`, and `TypeOf` now use the same helper-native-exit path as
  `NewObject`, with literal snapshot binding, exact destination/source root
  filtering, stale metadata rejection, no arithmetic fallback telemetry, and
  generated-helper/interpreter equivalence coverage.
- P8a emitted-native forward control flow: `Jump` and `JumpIfNotNullish` now
  execute in the native tier through a separate bytecode-branch lane, with
  proof-owned forward targets, branch records distinct from side exits and
  helper stubs, shared normal-return handling, and VM-level native/interpreter
  equivalence coverage for taken and fallthrough paths.
- P8b emitted-native primitive truthiness control flow: `JumpIfFalse` now uses
  the same proof-owned forward branch lane for primitive undefined/null/bool/
  int32 decisions, while double/cell/unknown truthiness leaves native execution
  through VM-owned side-exit fallback metadata at the branch bytecode index.
- P9 emitted-native JS-call exits: ordinary `Call` and `CallWithThis` now
  leave native code through a distinct VM-owned `0xfe` payload, revalidate
  retained call-site metadata against the active frame/CodeBlock/register state,
  observe and attach call-link metadata through the precise generated handoff
  path, and route authorized monomorphic calls through the existing generated
  direct-call transaction with no-GC/root/exception cleanup.
- P10 emitted-native `GetByName` property exits: native code now leaves through
  a distinct VM-owned `0xfd` payload, revalidates retained property handoff
  metadata plus decoded destination/base registers against the active
  frame/CodeBlock/snapshot, and routes through the existing property handoff
  transaction to record property-load observations without native lookup or IC
  mutation.
- P11 emitted-native `GetByName` property-load sidecar probes: native property
  exits now project current VM-owned own-data and guarded candidate tables at
  dispatch time, reuse the generated property-load probe helper, sync cell
  destination roots through the VM, record existing miss lifecycle metadata, and
  fall back to the P10 handoff path on miss or no candidate.
- P12 emitted-native `PutByName` property-store exits: native property exits now
  reuse the `0xfd` property-native namespace with opcode-specific retained
  operands, revalidate active frame/CodeBlock/snapshot/site/base/value metadata,
  route ordinary stores through the existing VM-owned property handoff
  transaction, record property-store observations/plans plus barrier and
  throwing-setter evidence, and avoid native store sidecar mutation.
- P13 emitted-native `PutByName` property-store sidecar hits: native store exits
  now reuse VM-projected store mutation candidates and host-owned probe/commit
  authority for replace and transition stores, record store probe misses and
  mutation rejections before falling back through P12, reject forged retained
  metadata before probing, and preserve the no-direct-native-store/no-IC-mutation
  boundary.
- P14 emitted-native loop backedges: backward `Jump`, `JumpIfNotNullish`, and
  `JumpIfFalse` targets now require exact VM-derived backedge safepoint
  authority, leave native code through retained loop-backedge payloads instead
  of raw in-code loops, validate frame/CodeBlock/readiness/artifact/snapshot/
  source/target metadata at the VM boundary, record VM-owned loop/tiering
  evidence, enforce a native backedge budget, and re-enter native at checked
  allocation-relative target offsets.
- P15 tiering-selected baseline auto-materialization: interpreter-entry tiering
  now separates plan selection from committed entry decisions, can
  synchronously install eligible accepted emitted-native `CodeBlock`s through
  the existing VM-owned native install transaction, recomputes the entry gate,
  enters native in the same execution when readiness becomes valid, preserves
  interpreter execution for unsupported or stale code without native side
  effects, and never auto-installs from loop-backedge safepoints.
- P16a VM-owned generated/P9 direct-call callee entry: already-authorized
  generated and P9 direct JS-call transactions now execute bytecode callees
  through a VM-owned nested callee-entry spine with `Function` entry-kind
  tiering, P15 auto-install/readiness recomputation, native-or-interpreter
  dispatch, existing continuation return/throw cleanup, and forged metadata
  rejection before any callee entry decision.
- P16b VM-owned ordinary interpreter-call callee entry: ordinary interpreter
  `Call` and `CallWithThis` now use typed deferral requests in VM-owned
  interpreter execution, keeping public/raw interpreter execution direct while
  allowing registered function callees to enter the nested `Function` tiering
  spine, P15 auto-install, native-or-interpreter dispatch, continuation
  cleanup, and no-GC/root cleanup without giving `CoreOpcodeDispatchHost` raw
  VM authority.
- P16c VM-owned non-direct generated/P9 JS-call callee entry: cold generated
  and emitted-native JS-call exits now use the typed ordinary-call deferral
  boundary for bytecode callees, drain call observations before nested callee
  entry, record the real post-callee outcome, attach call-link metadata, route
  validated callees through the nested `Function` tiering/P15-native spine, and
  preserve direct interpreter fallback for unvalidated callees.
- P17a VM-owned function-value tail-call callee entry: runtime-internal
  bytecode function-value calls now have a distinct typed request boundary for
  tail-style completions, allowing `Function.prototype.call`, `Reflect.apply`,
  and proxy-apply propagation to route validated registered callees through the
  nested `Function` tiering/P15-native spine while preserving direct/raw
  interpreter behavior for unregistered callees and continuation-heavy runtime
  flows.
- P17b VM-owned function-value property-operation continuations: property
  accessors, setters, selected proxy traps, Reflect get/set/has/delete
  composition, proxy no-trap forwarding, and generated property observation
  finalization now route validated bytecode callees through the nested
  `Function` tiering/P15-native spine while preserving direct/raw interpreter
  behavior for unregistered or unsupported callees.
- P18a VM-owned ordinary bytecode constructor entry: ordinary base
  `Construct` now has a distinct typed construct request and nested
  `Construct` entry kind, validates construct-specialized CodeBlock/executable
  mappings before VM-owned entry, normalizes constructor returns through the
  allocated receiver, and preserves direct fallback for unsupported,
  unregistered, native, proxy, and derived constructor cases.

The last accepted green checkpoint is P18a VM-owned ordinary bytecode
constructor entry with `cargo test --lib -- --quiet` reporting 1788 passed.

P0, P1, P2, P3a, P3b, P4, P5a, P6, P7a, P7b, P7c, P8a, P8b, P9, P10, P11,
P12, P13, P14, P15, P16a, P16b, P16c, P17a, P17b, and P18a are accepted. The
narrow native tier is still only an initial proof: any widening must be chosen
by missing execution spine contracts, not by convenient local opcode tests.

Large areas remain shallow or incomplete: full GC semantics, weak/ephemeron
behavior, host API compatibility, module/job/microtask ordering, standard
library breadth, RegExp/Yarr depth, Wasm, debugger/inspector/profiler, native IC
stubs, optimizing tiers, JSC test-suite parity, and performance parity.

## Roles

Project owner:

- Sets direction and rejects workflow drift.
- Decides whether a stage boundary is acceptable.
- Clarifies scope when "real engine" has competing interpretations.

Main agent:

- Acts as architect and lead reviewer.
- Maintains the dependency graph and current priority queue.
- Decomposes broad work into parallel agent-owned batches.
- Reviews code, tests, and reports for architecture fit.
- Integrates patches and runs gates.
- Implements only trivial glue, corrections, or tightly bounded fixes.

Sub-agents:

- Own large implementation or audit batches.
- Read the relevant Rust and JSC sources before editing.
- Work inside assigned file/module boundaries.
- Add tests for their batch.
- Report changed files, verification, remaining gaps, and risks.
- Do not redefine project architecture.

Coding sub-agents should use GPT-5.5 xhigh when available.

## Operating Principles

Execution pressure now matters. Foundation work is valuable only when it moves
the engine spine forward or protects a shared ownership/runtime boundary.

Shared architecture outranks local feature completion.

Missing building blocks outrank tuning a small failing path.

Dependency owners go first. Runtime code must not invent ad hoc lifetimes,
roots, handles, or fallback paths while waiting for GC/VM contracts.

Parallelism is expected. Independent audits or implementation batches should be
delegated together when their write sets do not overlap.

Do not widen runtime, standard-library, module, or tooling breadth unless it
unblocks execution, fallback, roots, exceptions, calls, or object/property
behavior needed by the execution spine.

Do not continue on a broken tree unless the current batch is explicitly the
repair/review of that broken layer.

## Current Priority Queue

P0: Accepted - restore a clean accepted tree.

- Main agent: review the partial P6 instruction-selection edits and choose
  accept-after-repair or rework.
- Sub-agents: audit the selection layer for ownership, proof binding, operand
  validity, side-exit correctness, and test coverage.
- Completion evidence: formatting, compile, focused selection tests, and
  broader lib gates pass.

P1: Accepted - accept P6 symbolic instruction selection.

- Main agent: define the selection contract boundary from backend contract to
  symbolic machine instructions.
- Sub-agents: implement or repair selection for constants, moves, returns, and
  int32 add/sub/mul with explicit side exits.
- Non-goal: byte emission, callable authority, VM readiness, platform execution,
  native ICs, direct JS calls.
- Completion evidence: selected instructions bind to the backend contract,
  validate against tampered contracts, preserve no-byte/no-callability
  authority, and have focused golden/negative tests.

P2: Accepted - encode semantic P6 machine bytes.

- Main agent: approve byte encoding only after symbolic selection is accepted.
- Sub-agents: implement x86_64 byte encoding for the accepted symbolic subset
  behind a non-callable/non-ready boundary first.
- Completion evidence: bytes are derived only from accepted selection records,
  have relocation/range/provenance tests, and still cannot bypass readiness.

P3a: Accepted - materialize semantic bytes behind disabled VM readiness.

- Main agent: review that semantic bytes flow through VM-owned CodeBlock proof,
  byte-evidenced link/finalization, W^X residency, disabled readiness, and
  interpreter fallback.
- Sub-agents: connect the accepted byte path to VM materialization without
  opening unsupported opcodes or callable authority.
- Completion evidence: VM materialization is derived from semantic byte images,
  descriptor-only shortcuts are rejected, stale CodeBlock snapshots are
  rejected before side effects, disabled readiness has no callable authority,
  unsupported bytecode falls back to the interpreter, and full gates pass.

P3b: Accepted - open a narrow sealed callable native entry.

- Main agent: define the unsafe boundary and reject any path that lets raw
  execution bypass VM-owned CodeBlock/tiering state, snapshot checks, frame
  layout, no-GC/root contracts, side exits, or interpreter fallback.
- Sub-agents: implement the minimal x86_64 callable authority and platform
  trampoline for the accepted P6 constants/moves/return/int32-arithmetic subset,
  plus negative tests for unsupported bytecode, stale snapshots, descriptor-only
  residency, callable forgery, and disabled policy.
- Completion evidence: baseline-enabled mode enters the narrow native tier for
  eligible bytecode through sealed VM readiness, interpreter-only and
  baseline-enabled runs compare at the VM boundary, unsupported or stale code
  falls back to the interpreter, and raw execution remains limited to the
  accepted no-call/no-heap P6 subset.

P4: Accepted - audit and align the LLInt/reference interpreter.

- Main agent: keep the interpreter as the semantic oracle and identify where it
  must expose frame, root, exception, call/return, and fallback state for JIT.
- Sub-agents: audit dispatch, frame layout, call/return, exception propagation,
  and fallback resume against the JIT contracts.
- Completion evidence: interpreter-only and baseline-enabled runs compare at
  the VM boundary for the narrow execution spine.

P5: Accepted - open the VM-owned generated JS direct-call boundary.

- Main agent: define the authority boundary for generated `Call`/`CallWithThis`
  direct calls and reject any path that skips VM-owned executable/codeblock
  identity, call-link revalidation, continuation completion, no-GC suspension,
  roots, exceptions, or fallback.
- Sub-agents: implement the monomorphic bytecode-function call path in bounded
  slices across call-link readiness/candidate projection, generated sidecar
  outcome vocabulary, VM call transaction, and tests.
- Scope: ordinary bytecode function targets only; exact target executable,
  target CodeBlock, caller bytecode index, callee value/object, `this`,
  argument count, destination, and resume PC must be revalidated.
- Non-goals: constructors, native calls, bound/proxy calls, varargs,
  `CallDirect`, super calls, accessors, polymorphic call targets, or property
  getter/setter direct calls.
- Completion evidence: generated sidecar hits can enter a bytecode callee
  through a VM-owned transaction, return through the caller continuation, clean
  callee/caller temporary roots, suspend and restore generated no-GC while the
  callee executes, propagate throws with correct unwind roots, and match
  interpreter-only execution for simple nested calls and cell-returning calls.

P6: Accepted - prove the current emitted x86_64 subset before widening.

- Main agent: define the differential matrix and review it as a gate before any
  opcode-family widening.
- Sub-agents: implement interpreter-vs-native matrix coverage for the accepted
  constants/moves/return/int32 arithmetic subset and its side exits without
  adding new opcodes or new runtime features.
- Non-goal: widening the native subset, adding native IC stubs, optimizing tiers,
  or broadening runtime/standard-library behavior.
- Completion evidence: the current emitted x86_64 subset has end-to-end
  interpreter-vs-native differential coverage for constants, moves, returns,
  `AddInt32`, `SubInt32`, and `MulInt32`, plus side-exit coverage for overflow,
  negative zero, non-int32 operands, and unsupported-bytecode fallback without
  launch/readiness side effects.

P7a: Accepted - open the VM-owned runtime-helper native-exit authority layer.

- Main agent: define the authority boundary for emitted native code to exit to
  the existing VM-owned runtime-helper transaction without letting native code
  allocate, root, throw, or call helpers directly.
- Sub-agents: audit the current runtime-helper handoff contract and implement
  the VM-retained native-exit site layer before emitting helper bytes.
- Completion evidence: synthetic `NewObject` retained helper-exit payloads route
  through the existing runtime-helper transaction, arithmetic side exits remain
  on the existing fallback path, helper payloads cannot use arithmetic fallback
  authority, and stale/forged helper metadata rejects before dispatch.

P7b: Accepted - make the first helper-backed opcode reach the bridge from real
emitted native bytes.

- Main agent: define the minimal byte/emitter/install contract that lets a
  helper-backed opcode leave native execution as an opaque VM payload while
  preserving the accepted helper transaction boundary.
- Sub-agents: implement `NewObject` as the first emitted-native helper-exit
  family across lowering, semantic byte emission, retained-site metadata,
  materialization/readiness, and VM differential tests.
- Scope: reuse the accepted generated runtime-helper transaction for helper
  opcodes, starting with `NewObject`; emitted native should return a VM-owned
  payload token that is revalidated against the active CodeBlock/helper proof
  before dispatch, then resume through interpreter fallback after the single
  helper dispatch.
- Non-goal: direct native calls to runtime helpers, broad standard-library
  widening, property IC native stubs, direct JS calls from native code, or
  optimizing-tier work.
- Completion evidence: one helper-backed opcode family can be reached from the
  emitted native tier through a VM-owned exit transaction, with exact root-map
  synchronization, generated no-GC suspend/resume, throw/fail cleanup, stale or
  forged metadata rejection before helper dispatch, interpreter equivalence, and
  no raw helper/native-call authority.

P7c: Accepted - complete the emitted-native runtime-helper exit matrix for the
already accepted helper opcode families.

- Main agent: define a small differential matrix that covers destination-only
  helpers, literal-backed helpers, and destination/source helpers without
  weakening the native no-heap subset.
- Sub-agents: widen the helper-native-exit lowering/emission/retention path from
  `NewObject` to the remaining existing runtime-helper proof families:
  `NewArray`, `LoadString`, `LoadBigInt`, and `TypeOf`.
- Scope: reuse the same VM-owned opaque payload and generated runtime-helper
  transaction; prove CodeBlock literal snapshot binding for literal helpers and
  exact destination/source root-map filtering for `TypeOf`.
- Non-goal: new runtime helper families, direct native helper calls, native
  allocation, standard-library breadth, property/call IC native stubs, or
  optimizing tiers.
- Completion evidence: each existing generated runtime-helper family can be
  reached from emitted native through retained helper payloads with
  interpreter/generated-helper equivalence, stale literal/root/proof rejection,
  no arithmetic fallback telemetry, no-GC suspend/resume, and throw/fail cleanup
  through the existing transaction.

P8a: Accepted - add emitted-native forward control flow for the first modeled
branch subset.

- Main agent: define the native control-flow contract before widening more
  runtime behavior: branch targets must be CodeBlock/proof-owned, stay inside
  the validated bytecode range, preserve frame/root/no-GC invariants, and remain
  separate from side-exit/helper payload authority.
- Sub-agents: audit the existing generated/interpreter jump semantics and
  implement a narrow emitted x86_64 branch subset for `Jump` and
  `JumpIfNotNullish`.
- Scope: branch within one CodeBlock only, no OSR, no loops requiring profiling,
  no exception-handler edge targets, no broad truthiness of heap objects, and no
  new runtime helper families.
- Non-goal: optimizing-tier CFG work, loop optimizations, polymorphic
  truthiness, property/call native IC stubs, or standard-library breadth.
- Completion evidence: emitted native executes forward `Jump` and
  `JumpIfNotNullish` CodeBlocks with interpreter-vs-native differential coverage
  for taken and fallthrough paths, rejects invalid/stale/tampered targets before
  native side effects, patches rel32 targets only to normal instruction starts,
  and preserves helper, side-exit, and direct-call boundaries.

P8b: Accepted - define and implement the first native `JumpIfFalse` truthiness
contract.

- Main agent: define the branch/fallback boundary before implementation:
  native code may decide only the primitive cases it can prove from encoded
  values, and cell/unknown truthiness must leave through a precise VM-owned
  fallback/side-exit path at the branch bytecode index.
- Sub-agents: audit the existing interpreter and typed-generated
  `JumpIfFalse` semantics, then implement the emitted-native primitive
  truthiness branch path plus fallback metadata/tests.
- Scope: keep targets proof-owned and forward-only for this batch; preserve
  no-GC/root/frame authority; do not add object truthiness, ToBoolean runtime
  calls, loops, OSR, or optimizing-tier CFG.
- Completion evidence: emitted native covers `JumpIfFalse` primitive
  taken/fallthrough cases against the interpreter, rejects malformed/stale
  targets before native side effects, returns precise fallback metadata for
  unsupported cell/unknown values, and keeps existing P7 helper exits and P8a
  bytecode branches green.

P9: Accepted - connect emitted-native code to the existing VM-owned JS call
transaction.

- Main agent: define the emitted-native call-exit authority boundary before
  bytes are widened: native code may request a VM transaction for already
  attached monomorphic `Call`/`CallWithThis` metadata, but must not push frames,
  inspect executable internals, invoke callees, allocate, root, or handle throws
  directly.
- Sub-agents: audit the accepted P5a generated direct-call transaction and the
  current emitted-native payload machinery, then implement the narrow call-exit
  bridge only after the main agent reconciles resume-PC, argument, `this`,
  return-continuation, no-GC, and throw propagation evidence.
- Scope: ordinary bytecode `Call`/`CallWithThis` with existing attached
  monomorphic call-link metadata; VM-owned validation remains authoritative;
  emitted native returns an opaque payload and resumes through the established
  continuation/fallback path.
- Non-goal: constructors, bound/proxy/native host calls, varargs spreading,
  polymorphic call links, native inline call stubs, property IC widening,
  optimizing tiers, or broader standard-library behavior.
- Completion evidence: a real emitted-native call site reaches the existing
  VM-owned direct-call transaction, validates active frame/CodeBlock snapshot/
  call-link candidate/callee/arguments/`this`/destination/resume PC, preserves
  no-GC/root/exception cleanup, rejects stale or forged call metadata, and keeps
  P7 helper exits plus P8 branch side exits green.

P10: Accepted - connect emitted-native code to the VM-owned property access
handoff and observation boundary.

- Main agent: define the emitted-native property-exit authority boundary before
  widening bytes: native code may request the existing VM property transaction
  for a `GetByName` site, but must not inspect object structure, perform lookup,
  mutate inline caches, allocate, root, call getters, or handle exceptions
  directly.
- Sub-agents: audit the accepted P4d generated property handoff hardening and
  the current property-load sidecar/probe machinery, then implement the narrow
  emitted-native property-exit bridge only after the main agent reconciles
  property key, IC slot, base/destination registers, no-GC, root sync, getter
  throw, and fallback evidence.
- Scope: ordinary bytecode `GetByName` with existing CodeBlock-owned property
  metadata and VM-owned property-load observation state; emitted native
  returns an opaque retained payload and resumes through the established
  property handoff/fallback path.
- Non-goal: `PutBy*`, indexed/private/symbol access, proxy semantics, setter
  calls, broad prototype-chain native stubs, watchpoint mutation from native
  code, polymorphic StructureStub patching, optimizing tiers, or standard
  library widening.
- Completion evidence: a real emitted-native `GetByName` site reaches the
  existing VM-owned property handoff transaction, validates active frame,
  CodeBlock snapshot, property metadata, base/destination operands, no-GC and
  may-throw policy, records property-load observations/plans through the slow
  path, preserves root/exception cleanup for getter/fallback paths, rejects
  stale or forged property metadata, and keeps P7 helper exits, P8 branches, and
  P9 call exits green.

P11: Accepted - connect emitted-native property exits to the VM-owned
property-load sidecar probe boundary.

- Main agent: define the native property-probe authority boundary before
  widening behavior: emitted native may request VM-owned own-data or guarded
  `GetByName` probes, but must not cache structure/offset truth, perform object
  lookup, mutate ICs, install watchpoints, allocate, or root directly.
- Sub-agents: factor or reuse the existing generated property-load sidecar
  probe path for a single emitted-native `GetByName` exit, preserving the P10
  handoff fallback when no current VM-owned probe candidate is valid.
- Scope: VM-projected current property-load access-case tables and guarded
  candidate tables for `GetByName`; sidecar hits may write the destination only
  through existing VM/register/root-sync authority, and misses must record the
  same lifecycle metadata as generated sidecars.
- Non-goal: `PutBy*`, indexed/private/symbol access, direct native StructureStub
  patching, watchpoint mutation from native code, broad prototype-chain native
  stubs, polymorphic stub compilation, optimizing tiers, or standard-library
  widening.
- Completion evidence: emitted-native `GetByName` can consume current own-data
  and guarded property-load probe candidates through VM-owned validation, sync
  cell destination roots, record non-terminal/terminal misses with the existing
  lifecycle behavior, fall back to the P10 property handoff transaction when no
  valid candidate exists, reject stale or forged probe metadata, and keep P7,
  P8, P9, and P10 green.

P12: Accepted - connect emitted-native `PutByName` to the VM-owned property store
handoff boundary.

- Main agent: define the emitted-native property-store authority boundary before
  widening bytes: native code may request the existing VM property-store
  transaction for an ordinary `PutByName` site, but must not perform the store,
  mutate structures or inline caches, run barriers, allocate, root, call setters,
  or handle exceptions directly.
- Sub-agents: audit the accepted generated property-store handoff and store
  observation/barrier contracts, then implement the narrow emitted-native
  `PutByName` store-exit bridge only after the main agent reconciles property
  key, IC slot, base/value operands, no-GC, barrier/root requirements,
  setter/throw behavior, and fallback evidence.
- Scope: ordinary bytecode `PutByName` with existing CodeBlock-owned property
  metadata and VM-owned property-store observation state; emitted native returns
  an opaque retained payload and resumes through the established property
  handoff/fallback path.
- Non-goal: indexed/private/symbol stores, `PutByVal`, proxy/setter native
  shortcuts, direct native StructureStub patching, property-store sidecar
  mutation hits, watchpoint mutation from native code, optimizing tiers, or
  standard-library widening.
- Completion evidence: a real emitted-native `PutByName` site reaches the
  existing VM-owned property handoff/store transaction, validates active frame,
  CodeBlock snapshot, property metadata, base/value operands, no-GC and
  may-throw policy, records property-store observations/plans through the slow
  path, preserves barrier/root/exception cleanup for setter/fallback paths,
  rejects stale or forged store metadata, does not mutate bytecode ICs from
  native glue, and keeps P7, P8, P9, P10, and P11 green.

P13: Accepted - connect emitted-native `PutByName` to existing VM-owned
property-store sidecar probe/commit authority.

- Main agent: define the store-sidecar mutation authority boundary before
  native store hits are allowed. Native code may request a VM-projected store
  candidate, but base/value projection, barriers, structure-transition
  validation, mutation commit, miss/rejection telemetry, and fallback remain
  VM/host-owned.
- Sub-agents: audit the accepted generated property-store sidecar path and
  implement the narrow emitted-native bridge only after the main agent
  reconciles candidate-table projection, write-barrier evidence, no-GC
  suspend/resume, structure transition/replacement distinction, and stale
  attachment rejection.
- Scope: ordinary `PutByName` own-data replace and transition candidates already
  represented by VM-owned property-store access-case plans and attachments.
- Non-goal: direct native stores, new native IC stubs, proxy/private/symbol/
  indexed stores, setter fast paths, watchpoint mutation from native code,
  broad StructureStub patching, optimizing tiers, or standard-library widening.
- Completion evidence: emitted-native `PutByName` can hit existing VM-owned
  store sidecar candidates, commit replace/transition stores through the host
  mutation API with barrier evidence, record probe misses and mutation
  rejections, fall back through P12 handoff on miss/reject/stale metadata,
  preserve no-GC/root/exception cleanup, avoid bytecode IC mutation from native
  glue, and keep P9, P10, P11, and P12 green.

P14: Accepted - emitted-native backward control flow for loops with VM-owned
loop/tiering safepoint authority.

- Main agent: define the loop/backedge authority boundary before adding more
  branch-shaped opcodes. Native code may execute proof-owned backward branches
  for the accepted primitive-control subset, but loop accounting, safepoint
  checks, interrupt/tiering decisions, and fallback remain VM-owned.
- Sub-agents: audit the accepted P8 forward branch lane, interpreter loop
  dispatch semantics, baseline fallback records, and any existing tiering or
  safepoint counters before implementing backedge byte emission and VM
  validation.
- Scope: backward `Jump`, `JumpIfFalse`, and `JumpIfNotNullish` targets for the
  already accepted primitive/native branch subset, with exact CodeBlock
  snapshot validation and interpreter equivalence coverage for finite loops.
- Non-goal: OSR into optimizing tiers, arbitrary truthiness, exception-handler
  loops, host interrupt delivery, debugger/profiler integration, new arithmetic
  widening, or broad stdlib/test-suite work.
- Completion evidence: emitted-native loops execute backward branches without
  falling back on every iteration, preserve frame/root/no-GC invariants, honor
  existing primitive branch side-exit policies, fall back through precise PCs on
  unsupported truthiness or stale/tampered targets, record VM-owned loop/tiering
  safepoint evidence without giving native code independent tiering authority,
  and keep P8 through P13 green.

P15: Accepted - connect tiering-selected baseline plans to VM-owned native
materialization for the accepted emitted-native subset.

- Main agent: define when a tiering entry decision may request native baseline
  materialization, where the VM may mutate CodeBlock/executable entry state,
  and how rejected or stale compilation attempts fall back without making local
  execution chase a small test path.
- Sub-agents: audit `observe_interpreter_entry`, existing P6/P14 semantic
  native install transactions, CodeBlock/executable publication, and entry-gate
  behavior before implementing automatic install.
- Scope: synchronous VM-owned materialization/install at interpreter entry for
  already accepted emitted-native subsets, using existing `CodeBlock`
  snapshots, baseline readiness, platform residency, retained payload tables,
  and interpreter fallback on unsupported bytecode.
- Non-goal: background compilation threads, optimizing tiers, OSR into DFG/FTL,
  broad opcode widening, native IC patching, loop-backedge installation, host
  API compatibility, or standard-library/test-suite expansion.
- Completion evidence: a hot eligible `CodeBlock` reaches a selected baseline
  plan, installs the emitted-native entry through the same VM-owned transaction
  used by explicit tests, publishes launch metadata only after readiness is
  valid, enters native in the same execution without manual test installation,
  records rejection/fallback evidence for unsupported or stale code without
  side effects, does not auto-install from loop backedges, and keeps P8 through
  P14 green.

P16a: Accepted - route already-authorized generated/P9 direct bytecode calls
through a VM-owned tiered callee-entry spine.

- Main agent: split the generated/native side from the ordinary interpreter
  call side so the first nested native-entry proof lands without forcing an
  unsafe generic host callback rewrite.
- Sub-agents: factor VM entry execution enough to reuse P15 tier selection,
  auto-install, readiness recomputation, and native-or-interpreter dispatch for
  a callee frame whose continuation is already installed by the VM direct-call
  transaction.
- Scope: generated baseline direct calls and P9 emitted-native JS-call exits
  that have passed existing VM authority checks and converge through the
  generated direct-call transaction.
- Non-goal: ordinary interpreter `Call`/`CallWithThis`, non-direct generated
  call exits, proxy/native/constructor calls, new call IC mutation, OSR, or
  opcode widening.
- Completion evidence: an eligible bytecode callee reached through generated/P9
  direct-call authority records a `Function` entry decision, P15-installs and
  enters the accepted emitted-native tier, returns through the existing
  continuation spine, leaves no leaked no-GC/root/frame state, rejects forged
  direct-call metadata before callee entry, and keeps P9 and P15 green.

P16b: Accepted - route ordinary interpreter bytecode calls through a VM-owned
tiered callee-entry boundary.

- Main agent: decide the callable execution boundary before widening native
  opcodes. Top-level entry and generated/P9 direct-call entry can now
  auto-install and enter native, but ordinary interpreter bytecode calls still
  bypass tiering, CodeBlock readiness, root/frame accounting, and continuation
  cleanup at the VM boundary.
- Sub-agents: design and implement a safe host/VM ownership boundary for
  interpreter `Call` and `CallWithThis` without giving generic
  `CoreOpcodeDispatchHost` raw VM authority.
- Scope: ordinary bytecode `Call` and `CallWithThis` callees reached from the
  interpreter should enter through a VM-owned path that observes tiering, may
  use the accepted P15 auto install at entry, preserves continuation
  return/throw behavior, keeps standalone raw-interpreter tests possible, and
  falls back to interpreter execution for unsupported callees.
- Non-goal: constructors, bound/proxy/native host calls, varargs/spread,
  arity-specialized entry stubs, OSR, broad opcode widening, new call IC
  mutation, recursion optimization, background compilation, or direct native
  calls from generated code.
- Completion evidence: an eligible bytecode callee can auto-install and enter
  the accepted emitted-native tier when called from ordinary interpreter
  `Call`/`CallWithThis`, returns through the existing continuation spine,
  leaves no leaked frames/roots/no-GC state, preserves unsupported-callee
  interpreter behavior, avoids duplicate installs, keeps generated/P9 direct
  call behavior intact, and keeps P9 through P16a green.

P16c: Accepted - route non-direct generated/P9 JS-call exits through the
VM-owned ordinary-call deferral boundary.

- Main agent: close the remaining nested bytecode-call bypass in generated and
  emitted-native JS-call exits after P16a covered authorized direct calls and
  P16b covered full interpreter execution.
- Sub-agents: audit `execute_single_dispatch`, generated JS-call handoff
  observation/attachment, P9 cold call exits, and the P16b ordinary-call request
  plumbing before changing generated single-dispatch behavior.
- Scope: when generated/P9 JS-call exits dispatch an ordinary bytecode
  `Call`/`CallWithThis` that is not yet authorized as a direct call, the VM
  should consume the typed ordinary-call request, execute the bytecode callee
  through the nested `Function` entry spine when validated, preserve call-link
  observation and attachment side effects, and resume the caller through the
  existing single-dispatch continuation path.
- Non-goal: broad call IC mutation, direct native calls from generated code,
  constructors, proxy/native/bound calls, `execute_function_value`, OSR,
  background compilation, or opcode widening.
- Completion evidence: first/cold generated and P9 JS-call exits can route a
  registered eligible bytecode callee through VM-owned nested entry without
  losing call-observation/attachment metadata, while unsupported or unvalidated
  callees preserve the old direct interpreter behavior, forged metadata still
  rejects before callee entry, no-GC/root/frame cleanup remains clean, and P9
  through P16b stay green.

P17a: Accepted - route runtime-internal bytecode function-value tail calls
through a VM-owned callee-entry boundary.

- Main agent: close the tail-style function-value call bypass before widening
  opcodes or taking on continuation-heavy runtime flows.
- Sub-agents: audit `execute_function_value` callers, separate semantic runtime
  helper behavior from callee-entry authority, and implement a typed VM-owned
  request path without handing a raw `Vm` reference to generic dispatch code.
- Scope: `Function.prototype.call`, `Reflect.apply`, and proxy-apply
  propagation when the callee result is the current bytecode/native call result.
  Validated registered bytecode targets enter the nested `Function` tiering
  spine, while unregistered/stale targets and direct raw-interpreter execution
  preserve existing behavior.
- Non-goal: getters, setters, property observation finalization, array
  callbacks, promises, constructors, field initializers, bound-function
  optimization, native host-call acceleration, generated direct calls, new IC
  mutation, OSR, background compilation, or opcode widening.
- Completion evidence: `Function.prototype.call` and `Reflect.apply` route
  eligible VM-registered bytecode callees through nested `Function` entry with
  P15 auto-materialization, unregistered callees use direct fallback with no
  native side effects, throws and no-GC cleanup are preserved, and P15 through
  P16c stay green.

P17b: Accepted - extend runtime-internal function-value deferral to accessor,
setter, and proxy-trap completions.

- Main agent: choose the next runtime continuation family by execution-spine
  value, not local ease. Property accessors, setters, and proxy traps are the
  next shared boundary because `GetByName`, `PutByName`, `Reflect.get/set`, and
  generated property exits can all call JavaScript through this path.
- Sub-agents: audit getter/setter/proxy-trap `execute_function_value` callers,
  identify the smallest shared completion enum expansion that preserves
  property lookup/store observations, truthy/prototype conversions, exception
  mapping, and direct/raw interpreter compatibility, then implement the VM-owned
  deferral path for the selected family.
- Scope: ordinary getters, primitive prototype getters, setters, and proxy
  traps that have single-call completion transforms such as identity return,
  ignored setter return, truthy boolean conversion, prototype conversion, and
  property-observation finalization. Each selected completion must carry enough
  state to resume the runtime operation without keeping borrowed slices or raw
  VM authority in `CoreOpcodeDispatchHost`.
- Non-goal: array iteration callbacks, promise reaction/executor continuations,
  constructor/field-initializer continuations, bound-function optimization,
  native host-call acceleration, new IC mutation, OSR, background compilation,
  or opcode widening.
- Completion evidence: selected accessor/setter/proxy-trap paths route
  validated registered bytecode callees through nested `Function` entry with
  correct return transforms and property observation records, preserve
  unregistered/proxy/native direct behavior, preserve throws and handler
  unwinding, leave no frame/root/no-GC leaks, and keep P15 through P17a green.

P18a: Accepted - close the ordinary bytecode constructor-entry bypass.

- Main agent: move from call-like function execution to construction because
  `Construct` currently reaches bytecode bodies through the direct interpreter
  path even when ordinary calls, generated calls, and function-value calls have
  VM-owned nested entry. The first constructor slice must define the shared
  construct-entry contract before any generated `Construct` exit or construct
  IC work.
- Sub-agents: audit `dispatch_construct`, `dispatch_function_index_call`,
  constructor return-value handling, instance-field initialization, executable
  call-vs-construct specialization, and tiering entry decisions before
  implementation. Then implement the smallest typed constructor-call deferral
  that keeps `CoreOpcodeDispatchHost` free of raw VM authority.
- Scope: ordinary bytecode `Construct` with the existing allocated
  `constructor_this`, constructor return-value normalization, caller
  destination/resume continuation, registered CodeBlock/liveness/snapshot
  validation, no-GC/root/exception cleanup, and direct fallback for
  unregistered or unsupported constructor targets.
- Non-goal: `ConstructSuper`, default-derived constructor forwarding,
  constructor field-initializer deferral, native constructors, proxy/bound
  constructors, varargs/spread construction, generated/native `Construct`
  exits, construct IC mutation, OSR, background compilation, or opcode
  widening.
- Completion evidence: a registered eligible bytecode constructor can enter
  through a VM-owned nested constructor entry without raw interpreter execution,
  P15 tiering decisions distinguish constructor entry authority from ordinary
  function calls, object/explicit-return constructor semantics remain correct,
  unregistered constructors preserve direct fallback with no native side
  effects, throws leave no frame/root/no-GC leaks, and P15 through P17b stay
  green.

P18b: Current - close the derived and super constructor-entry bypass.

- Main agent: keep construction as the priority before widening generated
  opcodes. `ConstructSuper` and default-derived constructor forwarding still
  decide the core class-construction spine, including when `this` becomes
  initialized and when derived instance fields run.
- Sub-agents: audit explicit `ConstructSuper`, default-derived constructor
  forwarding, `dispatch_function_index_call` constructor paths, existing
  post-super field initialization, and constructor return normalization before
  implementation. Then implement only the typed continuation/request boundary
  needed for registered bytecode super constructors.
- Scope: explicit `ConstructSuper` and default-derived forwarding to registered
  bytecode super constructors, continuation state for caller destination/resume,
  derived `this`/constructor receiver handling, existing post-super instance
  field initialization sequencing, construct-specialized CodeBlock validation,
  no-GC/root/exception cleanup, and direct fallback for native, proxy,
  unregistered, stale, or unsupported targets.
- Non-goal: proxy `construct` traps, bound constructors, native constructor
  acceleration, varargs/spread construction, new field-initializer architecture,
  generated/native `Construct` exits, construct IC mutation, OSR, background
  compilation, or opcode widening.
- Completion evidence: explicit and default-derived super construction can
  route validated registered bytecode super constructors through nested
  `Construct` entry while preserving derived-constructor semantics; unsupported
  targets fall back without P15 side effects; throws leave no frame/root/no-GC
  leaks; and P15 through P18a stay green.

## Scheduling Questions

Before starting any non-trivial batch, the main agent must answer:

- What is the most important engine gap right now?
- What does it depend on?
- Which prerequisites are still architecture or ownership questions?
- Which parts are serial because they define shared contracts?
- Which parts can be implemented or audited in parallel?
- What would count as completion evidence for this batch?
- What local test failures are allowed to wait because a broader dependency is
  more important?

If these questions are not answered, do not start implementation.

## Work Item Types

Architecture batch:

- Defines ownership, mutation, unsafe boundary, dependency direction, and test
  expectations for a broad subsystem.
- May edit Rust contracts and comments.
- Should not chase local feature behavior.

Implementation batch:

- Fills behavior behind an existing contract.
- Has bounded file ownership.
- Adds tests at the correct layer.
- Must report whether the implementation exposes a missing upstream contract.

Audit batch:

- Reads current Rust code and, when needed, corresponding JSC source.
- Produces a gap map and next-batch recommendation.
- Does not edit files unless explicitly assigned as a worker task.

Integration batch:

- Connects two already-shaped subsystems.
- Requires main-agent review for ownership, barriers, rooting, and API
  direction.
- Usually runs broader tests than an isolated implementation batch.

## Batch Template

Each delegated batch should be assigned with:

- Objective.
- Why this is the current priority.
- Dependencies already satisfied.
- Dependencies still blocked.
- File/module ownership.
- Explicit non-goals.
- Required tests and gates.
- Expected final report format.

The main agent reviews each batch for:

- Ownership consistency.
- Dependency direction.
- Barrier/root/handle discipline.
- Avoidance of tiny-path shortcuts.
- Test coverage matching the actual objective.
- Whether new gaps change the priority queue.

## Parallelization Rules

Parallelize when write sets are disjoint and the result does not depend on a
pending shared contract.

Do not parallelize implementation over an unresolved ownership boundary. Use
parallel audit agents first, then implement after the main agent reconciles the
contract.

Prefer several broad subsystem audits over one deep local debugging task when
the next priority is unclear.

## Stop Conditions

Stop a local task and re-evaluate priority when:

- It requires changing a shared ownership boundary.
- It creates a new duplicate identity or lifetime model.
- It needs broad `Rc<RefCell<_>>` or panic-based placeholders.
- It spends effort making a small test pass while a missing subsystem contract
  is the real blocker.
- It requires touching unrelated modules without a reviewed integration plan.
- It adds more foundation/provenance layers without bringing execution closer.

## Quality Gates

Before closing a code batch, run the gates appropriate to its scope. The default
gate set is:

```sh
cargo fmt --manifest-path Source/JavaScriptCore/rust/Cargo.toml --check
cargo clippy --manifest-path Source/JavaScriptCore/rust/Cargo.toml --lib --all-targets -- -D warnings
cargo test --manifest-path Source/JavaScriptCore/rust/Cargo.toml --no-run
cargo test --manifest-path Source/JavaScriptCore/rust/Cargo.toml --lib
```

Focused gates are acceptable while iterating, but they do not close a batch.

Forbidden-marker scans should check for:

- `TODO`
- `FIXME`
- `todo!(`
- `unimplemented!(`
- `panic!(`
- `Rc<RefCell`
- `minimum working`
- `MVP`
- `tiny path`
- `fake JS`

Naming drift scans should check for accidental JavaScriptCore-style shorthand
prefixes and unnecessary duplicate identity types.
