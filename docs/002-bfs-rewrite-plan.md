# Priority-Managed BFS Rewrite Plan

This rewrite is breadth-first, but breadth-first is not enough by itself. The
main risk is local tuning: an agent can spend a long time making one small path
work while the surrounding engine infrastructure is still missing.

The rewrite must be managed by priority, dependency order, and parallelism. A
passing local test is useful only when it proves the intended engine boundary.

Use `progress.md` for sparse completed checkpoints. This file is the architect
operating contract and the current scheduler.

The `/goal` reminder is long-lived. It is the durable charter for the whole goal
session, not the current work queue. It should describe stable identity, role
boundaries, rewrite principles, and priority-management standards.

Milestone-specific priorities, blockers, next batches, temporary rules,
accepted checkpoints, and risks belong in this plan or in the sparse progress
record, not in `/goal`.

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

The current public proof target is the full JetStream 3 Octane group at local
C++ JavaScriptCore-level performance. Treat Octane as a correctness and
performance forcing function: first make the benchmark run honestly, then widen
to the full Octane set, then optimize by comparing behavior, bytecode, ICs,
generated code, tiering, and runtime decisions against C++ JavaScriptCore.

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

## Current Target

The active rewrite proof is local JetStream 3 Octane:

```text
PerformanceTests/JetStream3/JetStreamDriver.js Octane group
  -> first: Octane-core correctness subset under an accepted-equivalent runner
  -> then: full local Octane correctness under that runner
  -> then: comparable score output
  -> then: performance parity with local C++ JavaScriptCore on the same inputs
```

Do not use `PerformanceTests/JetStream3/Octane/run.js` as the Rust runner
contract. In this tree it is a stale legacy Octane harness that loads missing
`base.js` and `code-load.js`; the active benchmark source of truth is
`JetStreamDriver.js`.

The full local Octane group is:

```text
Box2D
octane-code-load
crypto
delta-blue
earley-boyer
gbemu
mandreel
navier-stokes
pdfjs
raytrace
regexp
richards
splay
typescript
octane-zlib
```

The initial Octane-core subset is:

```text
richards
delta-blue
crypto
splay
navier-stokes
raytrace
```

This subset is intentionally not a tiny path. It covers old-style function and
prototype code, ES classes, super constructors, object graphs, arrays, strings,
numeric loops, property access, calls, construction, allocation pressure,
primitive math, and baseline-JIT pressure.

Full Octane is the target. The core subset is the first correctness gate because
it requires fewer unrelated features than `gbemu`, `mandreel`, `zlib`,
`regexp`, `typescript`, and code-load style tests.

Accepted M1 runner contract:

- The first Rust runner is a synchronous, non-browser,
  `DefaultBenchmark`-equivalent runner for the local `Octane` group.
- The reference command shape for local C++ JSC, from
  `PerformanceTests/JetStream3`, is:

  ```sh
  /path/to/jsc --useDollarVM=1 -e 'testList="Octane"; dumpJSONResults=true' cli.js
  ```
- Use one fresh JS global/realm per benchmark, not per iteration.
- Load each benchmark plan's files in the same order as `JetStreamDriver.js`.
- Inject the shell globals needed by the driver and benchmark files:
  `isInBrowser = false`, `self`, `top`, `console`, `print`, `performance.now`,
  `load`, `readFile`/`loadString`/`runString` equivalents, and benchmark error
  compatibility such as `alert`.
- For plans marked `deterministicRandom`, install the driver-compatible seeded
  `Math.random` override and reset it before every measured iteration.
- Instantiate the benchmark's global `Benchmark` with the driver-selected
  iteration count.
- Per iteration, call optional `prepareForNextIteration()`, reset deterministic
  random when applicable, measure `runIteration()` with `performance.now()`,
  and clamp elapsed time to at least 1 ms.
- After iterations, call optional `validate()`.
- Compute `First`, `Worst`, `Average`, per-test score, and total score exactly
  like JetStream 3 `DefaultBenchmark`: `5000 / time`, drop the first iteration
  for worst/average, average the slowest `worstCaseCount` remaining times for
  `Worst`, then use geometric means.
- Do not use legacy Octane `BenchmarkSuite`, reference-score throughput loops,
  or benchmark-source hacks.

Accepted M1 feature map:

- `richards`: prototype-style functions, linked lists, allocation churn,
  polymorphic calls, integer counters, and throwing queue/hold-count oracle.
- `delta-blue`: `Object.defineProperty` on `Object.prototype`, prototype
  mutation, arrays, implicit global assignment, and `alert`-based failure
  reporting.
- `crypto`: deterministic random, bitwise-heavy numeric arrays,
  `Math.floor`/`pow`/`log`/`LN2`, `parseInt`, and string char-code APIs.
- `splay`: deterministic random, `performance.now`, tree rotations, recursive
  traversal, object payload churn, and GC pressure; the active driver does not
  call the old full teardown oracle.
- `navier-stokes`: dense numeric arrays, double arithmetic, callback
  boundaries, and `Math.sqrt`.
- `raytrace`: ES class syntax, `extends`/`super`, static methods, template
  literals, exponentiation, object-heavy vector/color/ray allocation, recursive
  reflection, and double math.
- Full Octane adds typed-array breadth, eval and `Function` constructor
  behavior, RegExp depth, large generated/bundled programs, JSON/string
  compiler workloads, mock browser/shell shims, promises/timeouts, asm.js-style
  zlib code, and heavier JIT/GC pressure.

## Current State

The Rust tree is a single crate with module-level subsystem boundaries.

Accepted green checkpoint:

- M2 source execution prerequisite slice: persistent source sessions,
  expression lowering, cross-source global bindings, file-backed source
  loading/provenance, and incremental session append support.
- Full accepted gate at that checkpoint: `cargo test --lib -- --quiet` with
  1813 passed.

Current git/code note:

- The current working tree may contain documentation or active-batch edits.
  Treat the 1813-test M2 prerequisite slice as the last accepted green code
  checkpoint unless a later progress entry records passing gates.
- Do not build benchmark work on a red baseline unless the batch is explicitly
  repairing that baseline.

Major accepted capabilities:

- Parser -> bytecompiler -> interpreter execution for a broad JavaScript subset.
- VM-owned `CodeBlock`, frame, root, exception, call, construct, and tiering
  boundaries for the accepted execution spine.
- Heap/cell/root/barrier ownership scaffolding with targeted roots.
- One honest x86_64 baseline native tier for a narrow opcode subset, entered
  through VM-owned readiness, with interpreter fallback.
- VM-owned native exits for runtime helpers, calls, property loads/stores, and
  loop backedges.
- VM-owned ordinary, derived, and super bytecode constructor entry for the
  accepted construction spine.
- Interpreter fallback and differential testing for the accepted native subset.

Known Octane run blockers:

- Runtime intrinsics used by Octane-core still need an explicit benchmark
  compatibility pass: `Math.floor`, `Math.sqrt`, `Math.random`, `Math.log`,
  `Math.LN2`, `String.prototype.charCodeAt`,
  `String.prototype.substring`, `String.fromCharCode`, and global `parseInt`.
- Shell and benchmark host names can now be declared for bytecompiler
  visibility, but their runtime behavior is not installed yet:
  `performance.now`, `load`, `readFile`, `print`, `console`, and
  error-reporting compatibility such as `alert`.
- Standard object/global ownership must be tightened before M3 implementation:
  several intrinsics are currently bytecompiler-local loads that allocate fresh
  objects instead of canonical global-object properties, which is wrong for
  benchmark-visible overrides such as deterministic `Math.random`.
- Benchmark telemetry and runner control: no Rust-side Octane manifest,
  load-order execution, iteration loop, validation policy, scoring, or
  tier-mode selection yet.

Known full-Octane blockers beyond the core subset:

- Typed-array breadth beyond the current basic `ArrayBuffer`, `Uint8Array`, and
  `DataView` slices.
- `Function` constructor and eval/code-load behavior.
- Deeper RegExp/Yarr behavior.
- More standard-library breadth, Date/time compatibility, and browser/shell
  shims expected by older Octane tests.
- Harness compatibility for JetStream's async driver. Do not implement
  async/await merely to unblock the first core run; use a synchronous Octane
  runner first, then support the official harness.

Known performance blockers after correctness:

- Native fast paths for Octane hot bytecode families, not only helper exits.
- Math intrinsics in native/JIT paths.
- Array indexed load/store and length fast paths.
- Property and call IC quality for prototype-heavy object graphs.
- Constructor, class, super, and instance-field fast paths.
- Allocation/GC pressure behavior.
- DFG/FTL-equivalent optimizing tiers or a clearly justified alternative.

## Roles

Project owner:

- Sets direction and rejects workflow drift.
- Decides whether a milestone boundary is acceptable.
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
the Octane proof forward or protects a shared ownership/runtime boundary.

Shared architecture outranks local feature completion.

Missing building blocks outrank tuning a small failing path.

Dependency owners go first. Runtime code must not invent ad hoc lifetimes,
roots, handles, or fallback paths while waiting for GC/VM contracts.

Parallelism is expected. Independent audits or implementation batches should be
delegated together when their write sets do not overlap.

Do not widen runtime, standard-library, module, or tooling breadth unless it
unblocks Octane execution, fallback, roots, exceptions, calls, object/property
behavior, JIT behavior, or benchmark harness compatibility.

Do not continue on a broken tree unless the current batch is explicitly the
repair or review of that broken layer.

Correctness comes before optimization, but performance pressure must shape the
design from the beginning. The goal is not "passes Octane slowly"; the goal is a
rewrite that can explain and close its performance gap against C++ JSC.

## Current Priority Queue

M0: Accepted - restore a clean accepted baseline.

- Main agent: reviewed the P18b constructor state and accepted it as a contract
  update rather than a rollback.
- Sub-agents: constructor audit was available in parallel; local repair kept the
  critical path moving.
- Completion evidence: explicit `super()` and default-derived forwarding now
  assert VM-owned `Construct` entry; nested default-derived chains,
  object-returning `super()` plus derived field initialization, and throwing
  `super()` cleanup are covered; the dead test-helper warning is gone; `cargo
  test --lib p18 -- --quiet`, `cargo test --lib construct -- --quiet`, `cargo
  test --lib derived -- --quiet`, and `cargo test --lib -- --quiet` passed with
  1791 lib tests.

M1: Accepted - freeze the Octane target and runner architecture.

- Main agent: define the benchmark contract before implementation: local C++
  JSC is the reference, Octane-core is the first correctness target, full
  Octane is the next correctness target, and performance parity is judged
  against the same benchmark inputs.
- Sub-agents: audit JetStream 3 Octane files, the official driver, and the Rust
  shell/runtime boundary to produce a feature gap matrix with disjoint write
  areas.
- Completion evidence: each Octane test is mapped to required syntax,
  intrinsics, shell APIs, VM/runtime behavior, and likely JIT pressure;
  `JetStreamDriver.js` is recorded as the source of truth; the stale
  `Octane/run.js` path is rejected; the synchronous `DefaultBenchmark` runner
  design is accepted without requiring the official browser/async harness.

M2: Accepted - build Octane-core execution prerequisites in parallel.

- Main agent: protect the accepted source-session, global-binding, and
  expression-lowering contracts while closing the remaining file-loading
  boundary. Keep the host `load`/`readFile` model serial enough that workers do
  not invent a second global, origin, or source append identity.
- Sub-agents: finish the remaining disjoint prerequisite slice:
  filesystem-backed source loading, source-origin records flowing into
  compiled sources, and incremental host append/merge support for future
  `load`/`readFile` execution.
- Completion evidence: multiple loaded sources share one benchmark global/host
  state without reinitializing VM-owned roots or dispatch state; shell globals
  can be declared without ad hoc intrinsic hardcoding; loaded files carry
  source-origin records into compiled sources; focused VM/source tests cover
  locals, properties, indexed elements, prefix/postfix value semantics,
  side-effect order, conditional branch behavior, loose equality cases used by
  Octane-core, and batch-vs-incremental source visibility.
- Accepted sub-slice: persistent batch source sessions now reuse one
  VM-owned global/root and one dispatch host across loaded sources while
  preserving one-shot `execute_source`; update expressions, compound
  assignments, conditional expressions, and an explicit loose-equality subset
  are parsed/lowered/executed; bytecompiler-visible global/host binding
  declarations and cross-load top-level `function`/`var` visibility are modeled
  through a real session global object; full gates passed with 1809 lib tests.
- Accepted final sub-slice: shell file reads now build loaded-source records
  with canonical path provenance plus bytecode-owned `SourceProviderId` and
  `SourceOriginId`; bytecompiler provenance flows into `SourceProvenance`; VM
  incremental source sessions can append and execute one source at a time while
  preserving the same global object, dispatch host, function table, identifier
  table, string table, and visible global bindings; full gates passed with 1813
  lib tests.
- Deferred by design: runtime behavior for `load`/`readFile` and a real global
  lexical environment for cross-source top-level `let`/`const`.

M3: Current - add Octane-core runtime intrinsics and shell globals.

- Main agent: first settle the canonical standard-object/global-object boundary
  so benchmark-visible mutation works. Existing bytecompiler-local intrinsic
  loads are acceptable for isolated tests but cannot be the final model for
  `Math.random` override/reset, `performance`, `console`, or host globals.
- Sub-agents: implement in ordered batches after the boundary is clear:
  Math runtime intrinsics (`floor`, `sqrt`, `log`, `LN2`, `random`); String and
  global runtime intrinsics (`charCodeAt`, `substring`, `fromCharCode`,
  `parseInt`); then shell host globals (`performance.now`, `load`, `readFile`,
  `print`, `console`, `alert`).
- Completion evidence: each API has focused tests, deterministic behavior where
  benchmark repeatability requires it, benchmark-visible overrides persist
  across loaded sources, and no duplicate host/global ownership model exists.
- Scheduling note: most executable native builtin code still lives in
  `src/interpreter/mod.rs`, so Math and String implementation batches should be
  serialized unless the main agent first splits builtin bodies into disjoint
  modules.

M4: Run Octane-core correctly in the Rust engine.

- Main agent: own the runner integration and failure triage. Failures should be
  classified as syntax, runtime semantic, shell API, VM boundary, GC/rooting, or
  JIT/tiering gaps before any local fix begins.
- Sub-agents: debug independent Octane-core failures by test or feature area,
  with strict file ownership and no local shortcuts in benchmark sources.
- Completion evidence: `richards`, `delta-blue`, `crypto`, `splay`,
  `navier-stokes`, and `raytrace` run to correct completion under the Rust
  shell/runner in interpreter-only mode and baseline-enabled mode.

M5: Make the accepted baseline JIT cover Octane-core hot paths.

- Main agent: use profiler/telemetry output to choose opcode-family widening,
  not isolated convenience tests. Compare Rust bytecode/JIT decisions with C++
  JSC before choosing the next widening.
- Sub-agents: implement disjoint JIT/runtime slices such as numeric operations,
  Math intrinsics, array indexed access, property ICs, call ICs, constructor
  paths, and allocation fast paths.
- Completion evidence: Octane-core is correct with baseline enabled, native
  execution covers meaningful hot loops/calls/properties, fallback telemetry is
  understood, and performance movement is measured against local C++ JSC.

M6: Widen from Octane-core to full Octane correctness.

- Main agent: select full-Octane work by feature dependency and shared engine
  value, not by the easiest remaining file.
- Sub-agents: implement larger missing feature families in parallel where
  possible: typed-array breadth, RegExp depth, `Function` constructor/eval,
  code-load behavior, Date/time compatibility, and older shell/browser shims.
- Completion evidence: every Octane test in JetStream 3 runs correctly in the
  Rust engine with a pre-registered expected result policy and no benchmark
  source hacks.

M7: Support the official JetStream Octane harness path.

- Main agent: decide whether official driver compatibility requires async
  function/job support, a host-side adapter, or both, then keep that decision
  separate from benchmark correctness.
- Sub-agents: implement the chosen harness compatibility pieces, including
  async/job semantics only if they are now the real dependency.
- Completion evidence: the Rust engine can run the JetStream 3 Octane selection
  through the official or accepted-equivalent harness and produce comparable
  score output.

M8: Close the performance gap against C++ JSC.

- Main agent: run the optimization loop as a rewrite, not rediscovery. For each
  gap, inspect C++ JSC bytecode, IC, tiering, runtime, and generated code, then
  decide which design should be ported/adapted to Rust.
- Sub-agents: own large optimization families with measured hypotheses and
  before/after evidence: IC specialization, native inline stubs, loop/tiering,
  allocation/GC pressure, constructor/class paths, and optimizing-tier
  architecture.
- Completion evidence: full Octane score approaches local C++ JSC on the same
  machine, remaining gaps have explanations, and optimizations are backed by
  source/JIT comparison rather than local tuning.

M9: Produce proof-quality benchmark evidence.

- Main agent: define the publication evidence standard and reject ambiguous
  measurements.
- Sub-agents: audit reproducibility, benchmark configuration, warmup policy,
  result collection, and comparison scripts.
- Completion evidence: clean repo state, documented commands, C++ JSC reference
  numbers, Rust numbers, correctness logs, score confidence, and a concise gap
  explanation.

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
- It adds more foundation/provenance layers without bringing Octane execution
  or Octane performance closer.

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
