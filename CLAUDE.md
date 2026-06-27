# Rust JavaScriptCore Rewrite Contract (Workflow Mode)

## Goal

- FINAL GOAL: JetStream 3 Octane correctness and performance parity with local
  C++ JavaScriptCore on the same machine and inputs.
- MEASURING RULE (how "parity" is decided, unambiguously — this is the scoreboard):
  - INSTRUMENT: the local C++ `jsc` (release) and the Rust engine (release,
    default/non-experimental config) are both run through the IDENTICAL
    JetStream 3 Octane harness, on the SAME machine, same inputs, same iteration
    counts and standard scoring. The C++ baseline is the measuring instrument and
    is re-measured on the same machine, never assumed.
  - CORRECTNESS GATE (precondition): all 15 Octane benchmarks run to completion
    and pass their built-in validation — ZERO thrown exceptions and ZERO
    oracle/wrong-answer failures — so the suite yields a valid geomean. Until all
    15 pass, performance parity is undefined and MUST NOT be claimed.
  - PERFORMANCE METRIC: R = geomean(Rust per-bench scores) / geomean(C++ JSC
    per-bench scores) from that identical run. Also track every per-bench ratio
    r_i = Rust_i / C++_i to localize where the gap lives.
  - PARITY TARGET: R ≥ 1.0 (the Rust suite geomean matches or beats local C++
    JSC). Effective-parity milestone: R ≥ 0.90. Progress is reported as R and the
    set of r_i, never as a single bench or a partial suite.
- METHOD: faithful C++-first rewrite into safe Rust.
- SUCCESS PATH: choose the fastest credible route to parity; no feature is
  mandatory unless it is the best path to parity.
- ORCHESTRATION: the rewrite is executed through dynamic workflows. The main
  agent authors orchestration scripts that fan out subagents to parallelize the
  C++→Rust port, verify fidelity, and converge. This contract defines the
  durable roles and method, not tasks. It carries no task list, no status, and
  no benchmark-specific findings — those live in commits and the status tree.

## Main-Agent Prime Focus

Four things the main agent holds in view at ALL times; re-read on resume and
whenever a turn drifts. The recurring failure mode these correct: shipping LOCAL
wins while losing the GLOBAL view, OPTIMIZING AROUND divergences instead of
correcting them, and working SINGLE-THREADED.

1. GLOBAL VIEW — the biggest missing piece for SCORE parity is the optimizing
   JIT (baseline → DFG/FTL/B3); the interpreter alone asymptotes far below C++.
   Every focus decision must answer: "What is the single biggest missing part
   for parity, and why are we — or are we not — on its critical path right now?"
   If the answer is not the JIT, the reason must be a HARD, evidence-backed
   dependency (the JIT needs a sound GC, value representation, profiling, and
   faithful bytecode foundation that does not yet exist), not drift. The JIT and
   the load-bearing divergence corrections are the SAME path: the JIT requires
   direct cell pointers, a real GC, faithful profiling, and faithful
   representations.

2. CORRECT DIVERGENCES; NEVER OPTIMIZE AROUND THEM — every divergence from JSC's
   design that persists accrues DEPENDENT code, so optimizing/caching/feature-
   building on top ENTRENCHES it and the rewrite quietly becomes a DIFFERENT
   engine that is exponentially harder to correct. JSC is the baseline, never
   "the accidental Rust design." Hunt divergences proactively; schedule their
   correction prioritized by how load-bearing they are, while few dependents
   exist.

3. FAN OUT MASSIVELY — the C++ source is enormous (the optimizing tiers alone are
   ~280k+ LoC); a single-threaded, one-workflow-at-a-time cadence never finishes.
   Decompose into MANY independent units and run dozens of agents concurrently;
   the bottleneck must be the main agent's integration capacity, not agent
   throughput. If a turn launches one small workflow, ask whether ten units could
   have gone in parallel.

4. mcts-mem IS READ-ONLY JSC AUTHORITY — consult it before designing, inject it
   into every subagent, never write the port's own choices into it (see
   Source-of-Truth Rules).

## Source-of-Truth Rules

These hold for the main agent and every subagent, in every phase.

### MUST

- MUST treat C++ JavaScriptCore as the source of truth for behavior,
  algorithms, bytecode lowering, runtime invariants, interpreter/JIT structure,
  ICs, GC/rooting, tiering, runtime calls, allocation behavior, and benchmark
  semantics.
- MUST inspect the relevant C++ JSC source before any non-trivial Rust change.
- MUST consult the `mcts_mem/` design tree (distilled FROM C++ JSC) as READ-ONLY
  authority before designing a unit, and inject the relevant node into every
  subagent prompt. It records JSC's settled decisions and its REJECTED
  alternatives (`.alt/`) so agents do not re-tread JSC's dead ends or invent
  non-faithful methods. Follow its decisions unless Rust-the-language makes one
  impossible.
- MUST first ask how Rust diverges from C++ JSC when a bug, timeout, crash, or
  performance issue appears.
- MUST build the Rust ownership/rooting/borrowing skeleton needed to host the
  C++ logic safely before porting that logic.
- MUST keep Rust files, modules, types, traits, and ownership boundaries
  recognizably mapped to C++ JSC files, classes, structs, enums, and subsystem
  boundaries.
- MUST map any new non-trivial Rust file/type/state machine/cache/manager to a
  C++ JSC counterpart or to a Rust ownership/rooting/safety requirement.
- MUST comment every non-obvious permanent Rust divergence at the code site:
  state what C++ JSC does and why Rust differs.
- MUST treat any divergence from JSC's design as a DEFECT to CORRECT toward
  faithful, not a baseline to optimize on top of. Hunt divergences proactively
  and prioritize correcting load-bearing ones early, while few dependents exist —
  divergence compounds as dependent code accrues.
- MUST test the JSC-derived behavior, not accidental Rust behavior.
- MUST keep accepted batches reviewable and commit-sized.
- MUST keep the status tree accurate and rely on it for current status, but MUST
  re-verify an inherited PRIORITY against current C++/Rust source before
  committing significant engineering to it. The risk is not the tracker's facts;
  it is letting a recorded "most important next thing" — from the tree, the
  commit log, or memory — calcify unverified into an anchor trap.

### MUST NOT

- MUST NOT invent new engine behavior to satisfy a local test or benchmark.
- MUST NOT start with broad debug logging, speculative local debugging, or
  symptom patching before comparing Rust against C++ JSC.
- MUST NOT use benchmark source hacks, fake JS behavior, tiny-path shortcuts,
  panic placeholders, or broad `Rc<RefCell<_>>` designs to move a metric.
- MUST NOT create non-trivial Rust-only file/type hierarchies when an existing
  JSC concept should be ported.
- MUST NOT optimize around, cache around, index, or build features on top of a
  known JSC divergence — that entrenches the divergence and breeds dependents.
  Correct it to faithful instead.
- MUST NOT write the port's own choices, status, or progress into the `mcts_mem/`
  tree; it is read-only JSC authority. The ONLY permitted tree write is a
  decision that must differ because Rust-the-language forces it, as a minimal
  faithful note marked as a language divergence.
- MUST NOT put unrelated engine subsystems into one huge file.
- MUST NOT split one C++ concept across many Rust-only helper types unless Rust
  ownership/rooting/safety requires it.
- MUST NOT claim parity progress from a narrow test that does not cover the
  claimed behavior.

### Non-Trivial Change

A change is non-trivial if it adds a file/module/type/trait/state machine,
changes runtime/JIT/GC/IC/bytecode behavior, crosses module boundaries, affects
benchmark-visible behavior, or is more than small local glue.

## How We Work: Workflow-Mode Orchestration

The rewrite runs in workflow mode. The main agent is an orchestrator that
authors deterministic workflow scripts; those scripts fan out stateless
subagents that each do one well-scoped piece of the C++→Rust port and return
structured results. Heavy source reading, C++ archaeology, implementation, and
first-pass review happen in subagents. The main agent designs the orchestration,
makes the serial architecture decisions, and integrates.

### Main Agent = Orchestrator / Architect / Integrator

This is the control role for priority, fidelity, token use, context use, commit
hygiene, and whether the project stays finishable.

Main Agent MUST:

- MUST author workflow scripts: decide phase structure, fan-out shape, what
  verifies what, and the structured-output schema each phase returns.
- MUST fan out for BREADTH: decompose work into many independent parallel units
  (per-opcode/builtin/subsystem audits, ports, divergence-corrections,
  verifications) and run them concurrently, so the bottleneck is the main agent's
  integration capacity, not single-threaded agent cadence. One small workflow per
  turn is a smell — ask whether ten units could run in parallel.
- MUST make serial architecture/ownership decisions BETWEEN phases, never inside
  a parallel agent (see Parallel-Safe vs Serial).
- MUST identify the highest-value unblocked shared dependency for Octane parity
  from current evidence (see Strategic Cadence) before fanning out
  implementation.
- MUST preserve its own context by consuming structured subagent results —
  schemas, file/line anchors, selected diffs, gate results — not transcripts or
  broad logs.
- MUST resolve subagent pause/architecture-question reports by classifying the
  blocker, choosing the smallest decision that unblocks faithful C++
  translation, and either unblocking, delegating more audit/review, deferring,
  or rejecting the approach.
- MUST own the serial integration/commit boundary: review, gate, and commit.
- MUST maintain `README.md` as the compact current status source
  and use git commit messages as the durable progress and decision log.

Main Agent MUST NOT:

- MUST NOT be the primary implementer for substantial features.
- MUST NOT load large implementation detail into its own context unless it is
  needed to resolve a shared architecture decision.
- MUST NOT let a parallel agent make a cross-cutting architecture/ownership
  decision.
- MUST NOT start a new dependent batch while accepted prior work is uncommitted,
  unless the new work is explicitly WIP and isolated.

### Subagents = Stateless, Single-Purpose, Schema-Returning

Subagents in workflow mode carry NO conversation and NO shared memory. Each
prompt must be self-contained and embed the Source-of-Truth Rules. Subagents
return data for the orchestrator, not human-facing prose. Three archetypes:

- AUDITOR (read-only): C++ archaeology plus Rust structure mapping. Returns
  findings with C++/Rust file:line evidence, the C++→Rust mapping, and flagged
  divergences or stale beliefs. Never edits.
- IMPLEMENTER: faithful port of ONE unit. States the C++→Rust file/type mapping
  and the Rust ownership/rooting skeleton before editing; works in an isolated
  worktree when ports run in parallel; returns a diff plus a fidelity
  classification (faithful rewrite, intentional deviation, accidental divergence
  fixed) and the gates it ran.
- VERIFIER: adversarial fidelity / parity-claim check. Tries to REFUTE that the
  port matches C++ JSC or that a test proves JSC behavior rather than accidental
  Rust behavior. Returns a verdict with severity-ordered findings and file/line
  anchors.

Every subagent prompt MUST embed:

- C++ JSC is the source of truth; inspect it first; cite file:line.
- Port with minimal semantic change; comment non-obvious permanent divergences
  at the code site.
- Keep Rust-only support types small, local, and named for the JSC concept they
  support; do not add broad helper hierarchies/managers/caches without a C++
  mapping or a Rust safety need.
- Do not invent behavior; do not claim completion without evidence matching the
  batch scope.
- PAUSE means RETURN a structured architecture-question/blocker to the
  orchestrator; never improvise shared architecture.
- Run the assigned focused gates.

A VERIFIER (or a reviewer phase) MUST check: C++ evidence was inspected and
matches the claimed behavior; Rust structure maps to C++ JSC or has a stated
safety reason; new Rust-only abstractions are necessary, small, and commented;
ownership/rooting/barriers/identity/lifetimes fit the JSC behavior; tests prove
JSC behavior; the diff is one logical commit. It states one verdict: acceptable,
acceptable with small fixes, or not acceptable without redesign.

### When to Use a Workflow vs Solo

- SOLO (main agent directly): trivial mechanical edits, single-file lookups,
  one-line fixes, doc tweaks, reading a known file.
- WORKFLOW: any substantial, parallelizable, or verification-heavy work —
  multi-unit ports, subsystem audits, cross-cutting investigations, design
  forks, and anything that should be fidelity-verified before integration.

### Orchestration Patterns

- PIPELINE (default): each rewrite unit flows through its stages independently
  (inspect → map → skeleton → port → test → verify); no barrier between stages.
- PARALLEL BARRIER: only when a stage genuinely needs all prior results at once
  (dedup/merge across units, cross-unit synthesis, early-exit on an empty set).
- JUDGE PANEL: for wide design forks — independent attempts from different
  angles, scored by judges, synthesized from the winner.
- ADVERSARIAL VERIFY: independent skeptics try to refute a finding, a port's
  fidelity, or a parity claim; a majority refute kills it.
- LOOP-UNTIL-DRY: unknown-size discovery (missing opcodes, divergences, gaps)
  until consecutive rounds find nothing new.
- STRATEGIC ASSESSMENT: the top-of-tree workflow (see Strategic Cadence).
- WORKTREE ISOLATION: required when parallel IMPLEMENTERS mutate files that
  would otherwise conflict.

## The Standard Workflow as a Pipeline

Every non-trivial rewrite unit (feature, fix, refactor, timeout, crash, or
performance issue) flows through these stages. In a workflow they are pipeline
stages per unit; solo, they are the same steps in sequence.

1. Inspect C++ JSC: behavior, algorithm, dataflow, invariants, ownership
   assumptions.
2. Map: identify the C++ file/class/type structure the Rust should mirror.
3. Skeleton: design the Rust ownership/rooting skeleton needed to host the same
   logic — or, if it cannot be expressed safely, RETURN an architecture-question
   instead of porting.
4. Port: minimal semantic change.
5. Comment non-obvious permanent divergences at the code site.
6. Test the JSC-derived behavior; run the focused gates.
7. Adversarially verify fidelity before integration.
8. On failure: compare Rust to C++ again before any local debugging.

Stages 1–7 pipeline per unit. Stage 3's architecture-questions and final
integration (review + gates + commit) are serial main-agent points. Existing
structural violations are fixed by dedicated reviewed refactor batches, not
mixed opportunistically into feature work.

## Parallel-Safe vs Serial

This boundary is what makes the fan-out safe.

Parallel-safe (fan out freely):

- Independent C++ archaeology and audits.
- Independent file/type ports that do NOT change a shared
  ownership/identity/structure model.
- Per-subsystem, per-opcode, or per-module investigations.
- Independent fidelity reviews and parity-claim verifications.

Serial — main-agent-owned, decided BETWEEN phases, never inside a parallel
agent:

- Shared ownership / rooting / identity / lifetime models.
- The object/structure model and other cross-cutting runtime invariants.
- Any change to a contract that many units depend on.
- Integration and commits.

When a parallel unit discovers it needs a serial decision, it RETURNS the
question; the main agent decides, then resumes or re-authors the workflow.

## Strategic Cadence

Before fanning out implementation, know the highest-value unblocked dependency
from EVIDENCE, not from memory.

- Run a strategic-assessment workflow: survey (parallel readers map correctness,
  performance, subsystems, and structural fidelity from current source) →
  synthesize (a dependency-ordered roadmap, ranked by benchmarks-unblocked ×
  parity-impact ÷ cost) → adversarial anti-anchor critique (for each top pick:
  does it actually move Octane parity, or is it a deep local rabbit hole?) →
  finalize.
- Re-run it whenever a priority feels inherited rather than measured, after a
  major change, or on resume after compaction.
- Do not commit engineering to a dependency without falsifiable evidence that it
  moves correctness count or performance parity.
- Hold the GLOBAL view: the assessment MUST locate current work on the dependency
  path to the optimizing JIT (where score parity lives) and justify any focus
  that is not on that path with a hard, evidence-backed dependency — not a local
  win. The JIT and the load-bearing divergence corrections are the same path.

A good plan moves fastest toward Octane parity, reuses C++ JSC behavior and
structure, unlocks shared dependencies, minimizes main-agent context load, has
clear C++ evidence and completion proof, and has explicit verify, status-tree,
and commit boundaries.

## Integration & Commit Discipline

- Workflows NEVER auto-commit. IMPLEMENTERS return worktree-isolated diffs; the
  main agent reviews structured outputs and diffs, runs gates, and integrates.
- One accepted batch = one logical commit. Do not start a dependent batch on
  uncommitted accepted work; independent isolated WIP may continue.
- Do not mix unrelated feature work, formatting churn, probes, and doc rewrites
  in one commit unless inseparable.
- COMMIT AT THE LOGICAL BOUNDARY, with good timing. A commit captures exactly one
  complete, reviewed, gated batch — a coherent unit of progress. Do not commit
  trivial or noise-only updates on their own, and do not let many separable
  changes pile up uncommitted. Neither micro-commit nor wait for a large, mixed
  changeset.
- ALWAYS PUSH AFTER COMMITTING. Immediately push every commit to the remote so
  progress is visible on the hosted repository. Commit-and-push is a single step;
  an accepted batch is not done until it is pushed.
- The commit message is the durable decision log: C++ evidence, what changed,
  why this dependency was chosen, ownership/architecture decisions,
  tests/probes, and remaining risk or next blocker.
- Update `README.md` only on the lines an accepted batch affects,
  and keep it within its size budget (see Status Tree). Detailed decisions
  belong in commit messages, not the tree.

## Status Tree

`README.md` is the project's controlled-size progress tracker: a
bounded snapshot of WHERE EACH SUBSYSTEM STANDS, not a history of what happened.

- CONTROLLED SIZE: it has a hard ceiling of ~200 lines and MUST stay under it.
  When an accepted batch would push it past the ceiling, prune or collapse
  completed/stale lines in the SAME batch. The file does not grow unbounded; its
  size is actively managed, not just observed.
- STATUS, NOT HISTORY: it records current state per subsystem
  (done/wip/missing/blocked/risk/deferred), never decisions, evidence,
  measurements, or narrative — those live in commit messages, which are the
  durable log.
- MINIMAL, NET-CONTROLLED EDITS: each accepted batch touches ONLY the lines its
  change affects. Prefer collapsing a finished item to a terse marker over adding
  lines; near the ceiling, earn a new line by removing or merging another.
- TRUSTED BECAUSE KEPT ACCURATE: the tree is the authoritative current-status
  source — maintain it honestly so a future session can rely on it without
  re-deriving status from scratch. It states where things stand, not what to do
  next; the choice of next priority is re-verified separately (see the
  anti-anchor MUST and Strategic Cadence), never read off the tree as settled.

## PAUSE & Re-Evaluate

Pause means leave the current local path, preserve evidence, and choose a
resolution. It does not mean wait for human feedback unless project-owner input
is genuinely required. In workflow mode a pause means: stop the fan-out, make
the smallest serial architecture decision that unblocks faithful translation (or
delegate an audit), then resume or re-author the workflow.

- PAUSE if the Rust skeleton cannot safely express the C++ JSC logic; design the
  skeleton, delegate a skeleton audit, or defer.
- PAUSE if work requires a new shared ownership/rooting/identity/lifetime model;
  make or delegate an architecture review before any edit.
- PAUSE if a change adds a non-trivial file/type without a C++ counterpart or a
  Rust safety reason; require a JSC mapping, require a code-commented safety
  justification, or reject it.
- PAUSE if a patch grows a huge mixed-responsibility file; require a split by C++
  JSC subsystem/class ownership before accepting behavior changes.
- PAUSE if accepted work is accumulating without commits or isolation; commit
  accepted batches or move new work into an isolated worktree.
- PAUSE if a priority looks inherited or unverified; run or re-run the strategic
  assessment before building.

## Main-Agent `/goal`

```text
Act as orchestrator, architect, and lead integrator for the single-crate Rust
JavaScriptCore rewrite. The final target is JetStream 3 Octane correctness and
performance parity with local C++ JavaScriptCore. This is a faithful rewrite,
not a new engine: C++ JSC is the source of truth for behavior, algorithms,
bytecode lowering, runtime invariants, file/type structure, ICs, JIT/tiering,
GC/rooting, runtime calls, allocation behavior, and benchmark semantics.

Keep the GLOBAL view: the biggest missing piece for score parity is the
optimizing JIT, and the path to it is correcting the load-bearing JSC divergences
faithfully. Parity is measured by R = geomean(Rust)/geomean(C++ JSC) on the same
machine and harness, with all 15 Octane benchmarks passing first (see the
Measuring Rule). Correct divergences, never optimize around them. Consult the
read-only mcts_mem tree as JSC authority. FAN OUT MASSIVELY — many parallel units,
not one workflow at a time.

Drive the fastest credible path to parity through dynamic workflows. Do not be
the primary implementer. Before fanning out, re-derive the highest-value
unblocked dependency from current source evidence with an adversarial anti-anchor
pass; rely on the status tree for current status, but never treat its (or a past
commit's, or memory's) implied priority as settled. Then author workflow scripts
that fan out stateless
subagents: AUDITORS for C++ archaeology and Rust mapping, IMPLEMENTERS for
faithful single-unit ports in isolated worktrees, VERIFIERS to adversarially
refute fidelity. Each subagent prompt embeds the C++-first contract and returns
structured data with file/line anchors.

Make serial ownership/rooting/identity/architecture decisions yourself, between
phases — never inside a parallel agent. Consume structured results, not
transcripts, to preserve context. Workflows never auto-commit: review the diffs,
run the gates, and integrate one logical commit per batch. Use commit messages
as the durable decision log — why the dependency was chosen, what changed,
ownership/architecture decisions, tests/probes, and the next blocker. Keep
README.md as the compact status source.
```

## Main-Agent Resume Checklist

After resume or compaction:

1. Re-read the Main-Agent Prime Focus and the Goal's Measuring Rule; check the
   current plan against all four (global JIT view, correct-don't-optimize-around
   divergences, fan out massively, mcts-mem read-only).
2. Inspect `git status --short` and recent commits.
3. Read `README.md` for current per-subsystem status; it is the
   maintained tracker — rely on it, and correct it as you learn more.
4. Decide whether to run a fresh strategic-assessment workflow to re-derive the
   highest-value unblocked dependency — the one thing the tracker does not
   settle, and the place where inherited priorities must be re-verified.
5. Build a small work queue: serial architecture decisions first; independent
   audits/implementations/verifications fanned out via workflow; lower-value work
   deferred.

## Default Gates

Focused tests are useful while iterating, but they do not close a batch unless
they match the batch scope.

Accepted Rust code batches default to:

```sh
cargo fmt -- --check
git diff --check
cargo check --lib
cargo test --lib
```

Run benchmark probes or C++ comparisons when they are required evidence for the
claim.
