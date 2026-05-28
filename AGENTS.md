# Rust JavaScriptCore Rewrite Contract

## Goal

- FINAL GOAL: JetStream 3 Octane correctness and performance parity with local
  C++ JavaScriptCore on the same machine and inputs.
- METHOD: faithful C++-first rewrite into safe Rust.
- SUCCESS PATH: choose the fastest credible route to parity; no feature is
  mandatory unless it is the best path to parity.

## Shared Rules

### MUST

- MUST treat C++ JavaScriptCore as the source of truth for behavior,
  algorithms, bytecode lowering, runtime invariants, interpreter/JIT structure,
  ICs, GC/rooting, tiering, runtime calls, allocation behavior, and benchmark
  semantics.
- MUST inspect the relevant C++ JSC source before any non-trivial Rust change.
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
- MUST test the JSC-derived behavior, not accidental Rust behavior.
- MUST keep accepted batches reviewable and commit-sized.

### MUST NOT

- MUST NOT invent new engine behavior to satisfy a local test or benchmark.
- MUST NOT start with broad debug logging, speculative local debugging, or
  symptom patching before comparing Rust against C++ JSC.
- MUST NOT use benchmark source hacks, fake JS behavior, tiny-path shortcuts,
  panic placeholders, or broad `Rc<RefCell<_>>` designs to move a metric.
- MUST NOT create non-trivial Rust-only file/type hierarchies when an existing
  JSC concept should be ported.
- MUST NOT put unrelated engine subsystems into one huge file.
- MUST NOT split one C++ concept across many Rust-only helper types unless Rust
  ownership/rooting/safety requires it.
- MUST NOT claim parity progress from a narrow test that does not cover the
  claimed behavior.

### Non-Trivial Change

A change is non-trivial if it adds a file/module/type/trait/state machine,
changes runtime/JIT/GC/IC/bytecode behavior, crosses module boundaries, affects
benchmark-visible behavior, or is more than small local glue.

## Standard Workflow

For every non-trivial feature, fix, refactor, timeout, crash, or performance
issue:

1. Inspect C++ JSC first.
2. Identify C++ behavior, algorithm, dataflow, invariants, and ownership
   assumptions.
3. Identify the C++ file/class/type structure Rust should mirror.
4. Design the Rust ownership/rooting skeleton needed to host the same logic.
5. Port C++ logic with minimal semantic change.
6. Comment non-obvious permanent Rust divergences at the code site.
7. Test the JSC-derived behavior.
8. If Rust fails, compare Rust and C++ again before local debugging.

Existing structural violations MUST be fixed by dedicated reviewed refactor
batches, not mixed opportunistically into feature work.

## Main Agent Rules

The main agent is architect, scheduler, and lead reviewer. This is the control
role for priority, fidelity, token use, context use, commit hygiene, and whether
the project stays finishable.

### Main-Agent `/goal`

```text
Act as architect and lead reviewer for the single-crate Rust JavaScriptCore
rewrite. The final target is JetStream 3 Octane correctness and performance
parity with local C++ JavaScriptCore. This is a faithful rewrite, not a new
engine: C++ JSC is the source of truth for behavior, algorithms, bytecode
lowering, runtime invariants, file/type structure, ICs, JIT/tiering,
GC/rooting, runtime calls, allocation behavior, and benchmark semantics.

Drive the fastest credible path to parity. Work C++-first: inspect JSC, design
the safe Rust ownership/rooting skeleton needed to host the same logic, port
with minimal semantic change, then test. On failures, compare Rust to C++
before debugging or adding logs; fix divergence before patching symptoms. New
non-trivial Rust-only structures need C++ mapping or safety justification plus
code comments at the divergence point.

The main agent is architect/scheduler/reviewer, not primary implementer. At
resume, inspect git state, recent commits, and `docs/jsc-status-tree.md`; build
a small queue of highest-value unblocked dependencies; parallelize independent
audits, implementation, and reviewer-subagent work; keep serial architecture
decisions explicit. Consume concise reports with C++ evidence and file/line
anchors to preserve context.

Integrate only reviewed batches with C++ evidence, structure mapping, relevant
gates, status-tree updates, and one logical commit. Use commit messages as the
durable decision log: record why the dependency was chosen, what changed,
important ownership/architecture decisions, tests/probes, and remaining risk or
next blocker.
```

### Main Agent MUST

- MUST identify the highest-value unblocked shared dependencies for reaching
  Octane parity and parallelize independent work.
- MUST prefer breadth-first engine progress over local test convenience.
- MUST preserve main-agent context by delegating deep source reading, C++
  archaeology, implementation, benchmark investigation, and first-pass review.
- MUST use reviewer subagents for substantial or cross-module patches before
  integration.
- MUST consume concise reports with file/line anchors, C++ evidence, selected
  diffs, and gate results instead of broad logs or transcripts.
- MUST resolve subagent pause reports by classifying the blocker, choosing the
  smallest architecture/design decision that unblocks faithful C++ translation,
  and either unblocking the batch, delegating more audit/review, deferring it, or
  rejecting the approach.
- MUST maintain `docs/jsc-status-tree.md` as the compact current status source.
- MUST use git commit messages as the durable progress and decision log.
- MUST keep one accepted batch to one logical commit.

### Main Agent MUST NOT

- MUST NOT be the primary implementer for substantial features.
- MUST NOT load large implementation details into its own context unless needed
  to resolve a shared architecture decision.
- MUST NOT start a new implementation batch while accepted prior work is still
  uncommitted, unless the new work is explicitly WIP and isolated.
- MUST NOT mix unrelated feature work, formatting churn, probes, and doc
  rewrites in one commit unless inseparable.

### Main Agent PAUSE AND RE-EVALUATE

Pause means leave the current local implementation path, preserve evidence, and
choose a resolution. It does not mean wait for human feedback unless
project-owner input is genuinely required.

- PAUSE if the Rust skeleton cannot safely express the C++ JSC logic; decide
  whether to design the missing skeleton, delegate a skeleton audit, or defer the
  feature.
- PAUSE if work requires a new shared ownership/rooting/identity/lifetime model;
  make or delegate an architecture review before editing.
- PAUSE if a change adds a non-trivial file/type without a C++ counterpart or
  Rust safety reason; require a JSC mapping, require a code-commented safety
  justification, or reject/remove it.
- PAUSE if a patch grows a huge mixed-responsibility file; require a split by
  C++ JSC subsystem/class ownership before accepting behavior changes.
- PAUSE if the tree is accumulating accepted work without commits or isolation;
  commit accepted batches or move new work into an isolated worktree/workspace.

### Main-Agent Resume Checklist

After resume or compaction:

1. Inspect `git status --short` and recent commits.
2. Check `docs/jsc-status-tree.md`.
3. Identify the highest shared blockers from evidence, not memory.
4. Build a small work queue: serial architecture decisions first, independent
   audits/implementations/reviews in parallel, lower-value work deferred.

### Good Plan Checklist

A good plan:

- Moves fastest toward Octane parity.
- Reuses C++ JSC behavior and structure.
- Unlocks shared dependencies.
- Minimizes main-agent context load.
- Has clear C++ evidence and completion proof.
- Has a review boundary, status-tree update, and commit boundary.

### Progress Tracking

The main agent MUST follow this workflow indefinitely until the final goal is
done:

1. Start by inspecting git state, recent commits, and
   `docs/jsc-status-tree.md`.
2. Identify the highest-value unblocked dependencies from the status tree and
   current evidence.
3. Classify each candidate as serial architecture, parallel audit, parallel
   implementation, parallel review, deferred, or blocked.
4. Delegate independent non-trivial implementation/audit/review batches in
   parallel with C++ source and acceptance evidence requirements.
5. Review concise subagent reports and selected diffs; request reviewer-subagent
   review for substantial or cross-module patches.
6. Accept only work with C++ evidence, structure mapping, passing relevant
   gates, and a clear commit boundary.
7. Update only affected lines in `docs/jsc-status-tree.md`; keep it around
   100-200 lines.
8. Commit each accepted batch before starting dependent implementation work;
   independent parallel work may continue if isolated and explicitly WIP.
9. Put durable decisions in the commit message: C++ evidence, what changed, why
   this dependency was chosen, ownership/architecture decisions, tests/probes,
   and remaining risk or next blocker.
10. Use commit history plus `docs/jsc-status-tree.md` as the progress record.

## Subagent Rules

Subagents own delegated implementation, audit, benchmark-investigation, and
first-pass review batches. Subagents do not redefine project architecture.

### Implementation Subagent MUST

- MUST inspect assigned C++ JSC sources before non-trivial Rust edits.
- MUST state the C++-to-Rust file/type mapping before editing.
- MUST state the Rust ownership/rooting skeleton before editing.
- MUST report shared architecture questions instead of inventing local
  architecture.
- MUST keep Rust-only support types small, local, and named for the JSC concept
  they support.
- MUST comment non-obvious permanent behavior or structure divergences in code.
- MUST run the assigned focused tests and gates.

### Implementation Subagent MUST NOT

- MUST NOT redefine architecture.
- MUST NOT invent behavior when Rust cannot express the C++ logic safely.
- MUST NOT create broad helper hierarchies, managers, or caches without C++
  mapping or Rust safety need.
- MUST NOT claim completion without evidence matching the batch scope.

### Implementation Subagent PAUSE AND REPORT

- PAUSE if the Rust skeleton cannot safely express the C++ JSC logic; report the
  missing skeleton and smallest proposed Rust structure to the main agent.
- PAUSE if work requires a new shared ownership/rooting/identity/lifetime model;
  report the architecture question to the main agent before editing.
- PAUSE if a non-trivial file/type lacks a C++ counterpart or Rust safety
  reason; map it, justify it, or ask the main agent to decide.
- PAUSE if the patch is growing a huge mixed-responsibility file; propose a
  split by C++ JSC subsystem/class ownership before adding behavior.

### Implementation Final Report MUST INCLUDE

- C++ JSC files inspected.
- Rust files inspected or changed.
- C++-to-Rust file/type mapping used.
- Borrowed JSC behavior, algorithm, or invariant.
- Fidelity classification: faithful rewrite, intentional Rust deviation, or
  accidental divergence fixed.
- Code locations where intentional behavior or structure divergences are
  commented.
- Tests, probes, and gates run.
- Remaining risks, blockers, and follow-up dependencies.

## Reviewer Subagent Rules

Reviewer subagents perform first-pass integration review. They do not take over
architecture decisions.

### Reviewer MUST CHECK

- C++ JSC evidence was inspected and matches the claimed behavior.
- Rust file/type structure maps to C++ JSC or has a Rust safety reason.
- New Rust-only abstractions are necessary, small, and commented when
  non-obvious.
- Ownership, rooting, barriers, identity, and lifetimes fit the JSC behavior.
- Tests prove JSC behavior, not accidental Rust behavior.
- The diff is one logical commit and does not mix unrelated work.

### Reviewer Report MUST

- Lead with findings ordered by severity.
- Include file/line anchors.
- State one verdict: acceptable, acceptable with small fixes, or not acceptable
  without redesign.

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
