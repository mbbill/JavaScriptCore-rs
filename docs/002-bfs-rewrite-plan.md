# Priority-Managed BFS Rewrite Plan

This file is the active architect contract and scheduler for the Rust
JavaScriptCore rewrite. It should stay compact enough to be useful after
context compression.

Use `progress.md` only for sparse completed checkpoints. Historical detail is
not carried here.

## Durable Goal

The `/goal` reminder is long-lived. It should contain stable identity, role
boundaries, rewrite principles, and priority-management standards. Do not put
temporary batches, immediate rules, or stale failure labels into `/goal`.

Suggested durable `/goal` text:

```text
Act as architect and lead reviewer for the single-crate Rust JavaScriptCore
rewrite. This is a faithful rewrite of C++ JavaScriptCore, not a new
JavaScript engine. Use C++ JavaScriptCore as the source of truth for behavior,
algorithms, bytecode lowering, runtime invariants, interpreter/JIT structure,
GC/rooting, and benchmark semantics, while adapting those responsibilities to
safe Rust ownership, borrowing, rooting, frame, exception, runtime, GC, and
execution-tier contracts.

The current proof target is full JetStream 3 Octane at local C++
JavaScriptCore-level performance. Treat Octane as a correctness and performance
forcing function: first make Octane-core run honestly, then widen to full
Octane correctness, then close performance by comparing behavior, bytecode,
ICs, generated code, tiering, runtime calls, and allocation behavior against
C++ JavaScriptCore.

Own priority, dependency order, and parallelism across the whole rewrite. Keep
the rewrite breadth-first before depth-first: expand and validate major engine
boundaries before deep local tuning, and choose work by the most important
unblocked engine dependency rather than local test convenience.

Main agent role: maintain architecture, manage the dependency graph, delegate
large implementation, audit, and benchmark-investigation work to agents, review
their JSC evidence and patches, integrate accepted fixes, run gates, commit
clean checkpoints, and keep progress honest. Use sub-agents for large or
parallelizable batches. Implement locally only for trivial glue, review
corrections, temporary probes, or tightly bounded repairs; do not hand-edit
large feature/test migrations when that work can be delegated and reviewed.

Sub-agents must not blindly write features. Every non-trivial task must inspect
the corresponding C++ JavaScriptCore files first and explain what behavior was
borrowed or why a safe-Rust deviation is justified. For parallel work, prefer
separate git worktrees or isolated agent workspaces, then merge/rebase only
after main-agent review and verification.

Use `Source/JavaScriptCore/rust/docs/002-bfs-rewrite-plan.md` and
`Source/JavaScriptCore/rust/docs/progress.md` as the mutable scheduler and
checkpoint record between context compressions.
```

## Current Target

The active proof target is local JetStream 3 Octane:

```text
Octane-core correctness
  -> full Octane correctness
  -> comparable score output
  -> performance parity with local C++ JavaScriptCore on the same inputs
```

Do not use `PerformanceTests/JetStream3/Octane/run.js` as the Rust runner
contract. In this tree it is a stale legacy harness. The active benchmark
source of truth is `PerformanceTests/JetStream3/JetStreamDriver.js`.

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

This subset is not a tiny path. It covers prototype-style code, ES classes,
constructors, object graphs, arrays, strings, numeric loops, property access,
calls, allocation pressure, primitive math, and baseline-JIT pressure.

## Runner Contract

The first Rust runner is a synchronous, non-browser,
`DefaultBenchmark`-equivalent runner for the local Octane group.

Reference C++ JSC command shape from `PerformanceTests/JetStream3`:

```sh
/path/to/jsc --useDollarVM=1 -e 'testList="Octane"; dumpJSONResults=true' cli.js
```

Required runner behavior:

- Use one fresh JS global/realm per benchmark, not per iteration.
- Load each benchmark plan's files in `JetStreamDriver.js` order.
- Inject the shell globals needed by the driver and benchmark files:
  `isInBrowser = false`, `self`, `top`, `console`, `print`,
  `performance.now`, `load`, `readFile`/`loadString`/`runString` equivalents,
  and benchmark error compatibility such as `alert`.
- For plans marked `deterministicRandom`, install the driver-compatible seeded
  `Math.random` override and reset it before every measured iteration.
- Instantiate the benchmark's global `Benchmark` with the driver-selected
  iteration count.
- Per iteration, call optional `prepareForNextIteration()`, reset deterministic
  random when applicable, measure `runIteration()` with `performance.now()`,
  and clamp elapsed time to at least 1 ms.
- After iterations, call optional `validate()`.
- Compute `First`, `Worst`, `Average`, per-test score, and total score exactly
  like JetStream 3 `DefaultBenchmark`.
- Do not use legacy Octane `BenchmarkSuite`, reference-score throughput loops,
  or benchmark-source hacks.

## Current Baseline

The Rust tree is a single crate with module-level subsystem boundaries.

Last accepted clean checkpoint:

```text
9638776 Fix A1 program completion fidelity
```

Accepted gate at that checkpoint:

```text
cargo fmt --check
cargo clippy --lib --all-targets -- -D warnings
cargo build --lib
cargo test --lib -- --quiet
1932 tests passed
```

Accepted high-level capabilities:

- Parser -> bytecompiler -> interpreter execution for a broad JavaScript
  subset.
- VM-owned `CodeBlock`, frame, root, exception, call, construct, and tiering
  boundaries for the accepted execution spine.
- Heap/cell/root/barrier ownership scaffolding with targeted roots.
- One x86_64 baseline native tier for a narrow opcode subset, entered through
  VM-owned readiness with interpreter fallback.
- VM-owned native exits for runtime helpers, calls, property loads/stores, and
  loop backedges.
- VM-owned ordinary, derived, and super constructor entry for the accepted
  construction spine.
- Octane manifest, scoring math, source preparation, deterministic-random
  runner source generation, execution/failure classification, success records,
  score records, oracle-alert classification, and interpreter-vs-baseline
  comparison records under `shell::octane`.

Current evidence note:

- The Octane-core pass/fail matrix must be rerun from the clean baseline.
- A temporary post-checkpoint probe reached `richards` runner execution in
  interpreter-only mode and timed out after appending the generated runner
  source. Treat this only as a current investigation lead, not as accepted
  matrix evidence.

Known broad blockers:

- `load(path)` execution is not fully implemented.
- Primitive wrapper object construction for `new String`, `new Number`, and
  `new Boolean` remains incomplete.
- Full-Octane breadth still needs typed-array depth, `Function` constructor,
  eval/code-load behavior, deeper RegExp/Yarr behavior, Date/time
  compatibility, and older shell/browser shims.
- Performance parity still needs native fast paths, Math intrinsics in native
  paths, array indexed load/store and length fast paths, property/call IC
  quality, constructor/class/super fast paths, allocation/GC pressure work, and
  an optimizing-tier strategy or justified alternative.

## Roles

Project owner:

- Sets direction and rejects workflow drift.
- Decides whether a milestone boundary is acceptable.
- Clarifies scope when "real engine" has competing interpretations.

Main agent:

- Acts as architect and lead reviewer.
- Maintains the dependency graph and current priority queue.
- Decomposes broad work into parallel agent-owned batches.
- Requires C++ JSC evidence for every non-trivial change.
- Reviews code, tests, reports, and patches for architecture fit.
- Integrates accepted patches and runs gates.
- Implements only trivial glue, corrections, probes, or tightly bounded fixes.

Sub-agents:

- Own large implementation, audit, or benchmark-investigation batches.
- Read the relevant Rust and C++ JSC sources before editing.
- Work inside assigned file/module boundaries or isolated worktrees.
- Add tests for their batch.
- Report inspected JSC files, borrowed behavior, justified deviations, changed
  Rust files, verification, remaining gaps, and risks.
- Do not redefine project architecture.

Coding sub-agents should use GPT-5.5 xhigh when available.

## Operating Principles

This is a faithful safe-Rust rewrite of JavaScriptCore, not a new JavaScript
engine from scratch. Borrow JSC's tested behavior and algorithms wherever
possible, then reshape the types, ownership, borrowing, rooting, and mutation
boundaries so the design is safe Rust.

Deviation from JSC is allowed only when:

- Rust ownership, borrowing, rooting, or safety requires a different structure.
- The Rust design is demonstrably cleaner, faster, or safer while preserving
  observable semantics.

When the Rust implementation hits a bug or benchmark failure, first ask whether
C++ JSC has the same issue. If not, compare against the original implementation
and identify which JSC behavior or invariant the Rust rewrite failed to carry
over.

Priority rules:

- Shared architecture outranks local feature completion.
- Missing building blocks outrank tuning a small failing path.
- Dependency owners go first.
- Execution pressure matters: foundation work is valuable only when it moves
  Octane execution, fallback, roots, exceptions, calls, object/property
  behavior, JIT behavior, or benchmark harness compatibility forward.
- Correctness comes before optimization, but performance pressure must shape
  design from the beginning.

Parallelism rules:

- Delegate independent audits or implementation batches together when write
  sets do not overlap.
- Use isolated worktrees or workspaces for substantial independent patches.
- Do not parallelize implementation over an unresolved ownership boundary; use
  parallel audits first, then reconcile the contract.
- Avoid serially blocking on one small failure when other dependency-independent
  work can proceed.

## Octane BFS Map

Work moves by shared dependency layer, not by chasing one benchmark until it
passes.

Layer A: benchmark contract and runner boundary.

- Main agent keeps the runner aligned with JetStream 3 `DefaultBenchmark`.
- Sub-agents own manifest/load order, file provenance, prelude generation,
  deterministic random reset, result extraction, scoring, and telemetry.
- Current status: accepted runner scaffolding exists; current matrix must be
  rerun from the clean baseline.

Layer B: shared language/runtime blockers for Octane-core.

- Main agent prioritizes source-session and bytecode/runtime boundaries that
  unblock multiple Octane-core files.
- Sub-agents own failing feature families, not benchmark-local hacks.
- Current status: several parser/lowering/runtime blockers are accepted; rerun
  the current matrix before scheduling the next feature family.

Layer C: Octane-core correctness.

- Main agent runs the six-test core subset in interpreter-only and
  baseline-allowed modes and classifies every failure.
- Sub-agents investigate per benchmark or per failure family, inspect JSC first,
  and return evidence-backed fixes or blocker reports.
- Completion evidence: all six core tests complete with validated results and
  no benchmark source patches.

Layer D: Octane-core baseline performance.

- Main agent uses telemetry plus C++ JSC comparison to choose JIT widening.
- Sub-agents implement measured opcode/runtime families such as numeric loops,
  Math intrinsics, array indexed access, property/call ICs, constructors,
  allocation paths, and loop/tiering behavior.
- Completion evidence: core subset performance movement is measured against
  local C++ JSC and native coverage explains the remaining gap.

Layer E: full Octane correctness breadth.

- Main agent widens by shared full-Octane feature family.
- Sub-agents implement typed-array breadth, RegExp depth, eval and
  `Function` constructor behavior, code-load behavior, older shell/browser
  shims, Date/time compatibility, and large-program parser/bytecompiler
  pressure.
- Completion evidence: all fifteen JetStream 3 Octane tests complete with
  expected results and no source patches.

Layer F: full Octane performance parity.

- Main agent treats optimization as a rewrite of known JSC ideas, not
  rediscovery.
- Sub-agents own large optimization families with before/after evidence and
  clear fallback/telemetry explanations.
- Completion evidence: full Octane score reaches local C++ JSC level on the
  same machine and command inputs, or the remaining gap is quantified and
  attributed to specific missing tiers/features.

## Current Priority Queue

P0: Rerun and classify the Octane-core matrix from `9638776`.

- Main agent: define the probe contract, run or delegate the matrix, require
  phase classification, review JSC evidence, and select the next shared engine
  dependency from current results.
- Sub-agents: investigate assigned benchmarks or failure families in isolated
  workspaces where useful; inspect corresponding C++ JSC code before proposing
  fixes; report load order, phase reached, failure mode, suspected shared
  dependency, JSC evidence, and minimal fix recommendation.
- Completion evidence: `richards`, `delta-blue`, `crypto`, `splay`,
  `navier-stokes`, and `raytrace` each have current interpreter-only and
  baseline-allowed status: success, classified failure, or classified timeout.

P1: Fix the highest-shared Octane-core blocker revealed by P0.

- Main agent: choose the blocker that removes the most shared engine risk, not
  the easiest isolated test.
- Sub-agents: implement the selected feature family with JSC evidence, focused
  tests, and no benchmark source hacks.
- Completion evidence: focused tests pass, full gates pass, and the relevant
  Octane-core matrix entries move forward.

P2: Make the accepted baseline JIT cover Octane-core hot paths.

- Main agent: choose opcode-family widening from telemetry and JSC comparison.
- Sub-agents: implement disjoint JIT/runtime slices with before/after evidence.
- Completion evidence: baseline-enabled Octane-core is correct, native coverage
  is meaningful, and the remaining fallback/performance gap is understood.

P3: Widen from Octane-core to full Octane correctness.

- Main agent: schedule by shared feature dependency.
- Sub-agents: implement full-Octane feature families in parallel where write
  sets are disjoint.
- Completion evidence: all full Octane tests run correctly with no benchmark
  source patches.

P4: Support the official JetStream Octane harness path.

- Main agent: decide whether official driver compatibility requires async/job
  semantics, a host-side adapter, or both.
- Sub-agents: implement only the chosen harness compatibility pieces.
- Completion evidence: the Rust engine can run the JetStream 3 Octane selection
  through the official or accepted-equivalent harness and produce comparable
  score output.

P5: Close the performance gap against C++ JSC.

- Main agent: compare bytecode, ICs, generated code, tiering, runtime calls,
  and allocation behavior against C++ JSC before choosing optimization work.
- Sub-agents: own measured optimization families with source/JIT comparison.
- Completion evidence: full Octane score approaches local C++ JSC on the same
  machine, with remaining gaps explained.

## Delegated Batch Contract

Each delegated batch must specify:

- Objective.
- Why this is the current priority.
- Dependencies already satisfied.
- Dependencies still blocked.
- File/module ownership.
- Explicit non-goals.
- C++ JSC files/components to inspect before editing.
- Expected JSC behavior or algorithm to preserve.
- Allowed Rust-specific deviations.
- Required tests and gates.
- Expected final report format.

Each sub-agent final report must include:

- JSC files inspected.
- Rust files inspected or changed.
- Fidelity classification: faithful, intentional Rust deviation, or accidental
  deviation fixed.
- Borrowed JSC behavior or algorithm.
- Justified deviations, if any.
- Tests and gates run.
- Remaining risks or blockers.

The main agent accepts a batch only after reviewing:

- JSC fidelity.
- Ownership consistency.
- Dependency direction.
- Barrier/root/handle discipline.
- Avoidance of tiny-path shortcuts.
- Test coverage matching the objective.
- Whether the new evidence changes the priority queue.

## Scheduling Questions

Before starting any non-trivial batch, the main agent must answer:

- What is the most important engine gap right now?
- What does it depend on?
- Which prerequisites are architecture or ownership questions?
- Which parts are serial because they define shared contracts?
- Which parts can be implemented or audited in parallel?
- What counts as completion evidence?
- Which local failures are allowed to wait because a broader dependency is more
  important?

If these questions are not answered, do not start implementation.

## Stop Conditions

Stop a local task and re-evaluate priority when:

- It requires changing a shared ownership boundary.
- It creates a duplicate identity or lifetime model.
- It needs broad `Rc<RefCell<_>>` or panic-based placeholders.
- It spends effort making a small test pass while a missing subsystem contract
  is the real blocker.
- It requires touching unrelated modules without a reviewed integration plan.
- It adds foundation/provenance layers without bringing Octane execution or
  Octane performance closer.

## Quality Gates

Default code-batch gates:

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
