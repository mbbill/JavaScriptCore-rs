# Main-agent principles — why this rewrite must be orchestrated, not single-threaded

This document is a standing reminder for the main agent. It is **not** a task
list. It records the operating principles that keep the Rust JavaScriptCore
rewrite faithful and finishable.

## 1. Hold the global view

The final goal is not a local benchmark win, not interpreter correctness, and not
"some native code runs." The final goal is:

- local C++ `jsc` **release** and the Rust engine **release, default config** run
  through the identical JetStream 3 Octane harness, same machine, same inputs,
  same scoring;
- all 15 Octane benchmarks complete and validate first (zero throws, zero
  oracle/wrong-answer failures), otherwise R is undefined;
- `R = geomean(Rust per-bench scores) / geomean(C++ jsc per-bench scores)`, plus
  every per-bench ratio `r_i = Rust_i / C++_i`;
- target R ≥ 1.0, effective-parity milestone R ≥ 0.90.

This makes the local C++ `jsc` build and comparison harness a **first-class
requirement**, not an optional measurement flourish. The local C++ `jsc` is the
measuring instrument. Without a release C++ `jsc` built on the same machine and
re-measured, there is no scoreboard and no definition of done.

The biggest missing part for score parity is the optimizing JIT: **DFG → FTL/B3**.
A baseline-JIT win over the interpreter is useful only insofar as it proves the
native substrate and supplies a bailout tier; it does not by itself move R close
to C++. Any focus that is not directly on the optimizing-JIT path must justify
itself as a hard dependency for that path (e.g. packed bytecode stream, profile
population, baseline-as-bailout soundness, GC/rooting/value representation).

## 2. Correct divergences; never optimize around them

This is a faithful rewrite of C++ JavaScriptCore, not a new engine. C++ JSC is the
source of truth for behavior, representation, invariants, tiering, profiling,
bytecode, ICs, GC/rooting, and runtime calls.

A Rust divergence is not a new baseline to build upon. It is a defect to correct
unless Rust-the-language makes a faithful shape impossible. The earlier a
load-bearing divergence is corrected, the cheaper it is: every feature built on
an accidental Rust-only shape becomes a dependent that makes the correction more
expensive.

Practical rule: before fixing a bug, timeout, crash, or performance issue, first
ask **"how does this differ from C++ JSC?"** Do not add caches, fast paths,
shortcuts, or local semantics on top of a known divergence. Move the Rust shape
toward the JSC shape.

## 3. Treat `mcts_mem/` as read-only JSC authority

`mcts_mem/` is distilled from the original JSC design history. Its purpose is to
teach agents what decisions JSC already made and which alternatives failed, so we
do not waste time reinventing methods that JSC already rejected.

Therefore:

- consult the relevant mcts_mem nodes before planning non-trivial work;
- inject the relevant JSC decisions and rejected alternatives into subagent
  prompts;
- follow the JSC decision unless Rust ownership/rooting/safety makes it
  impossible;
- **do not write Rust rewrite progress, status, or preferences into mcts_mem**.

A rewrite should not change JSC's design record. The only acceptable mcts_mem
write is a minimal note for a language-forced divergence: "C++ JSC does X; Rust
must do Y because of ownership/rooting/safety; Y preserves the same invariant."
Project status and Rust decisions belong in repo docs (`README.md`,
`docs/ROADMAP.md`, `docs/STATUS.md`, `docs/design/*.md`) and commit messages, not
in `mcts_mem/`.

## 4. Fan out massively

The C++ JSC source is enormous; the optimizing tiers alone are hundreds of
thousands of lines. A single main-agent thread will not finish the rewrite in a
reasonable time.

The main agent must make itself the integration bottleneck, not the throughput
bottleneck:

- use workflows/agents for substantial source reading, C++ archaeology,
  implementation, and first-pass verification;
- run many independent units concurrently where safe;
- keep subagent prompts self-contained: C++ source of truth, mcts_mem authority,
  exact unit scope, allowed files, required gates, and a structured return;
- use worktree isolation for parallel code-editing agents;
- do not ask one agent to make shared architecture decisions; parallel agents
  return blockers/questions, and the main agent resolves those serially;
- prefer structured summaries, file:line anchors, diffstats, and gate results over
  transcript dumps.

The main agent should not be the primary implementer for substantial work. Its
job is architect/orchestrator/integrator: choose the critical path, decompose the
work, inject authority, review outputs, resolve serial decisions, run/verify
final gates, commit, push, and keep the trackers accurate.

## 5. Let agents do editing and testing; main agent integrates

For non-trivial code changes, implementation agents should do the actual editing
and focused testing in isolated worktrees. This has three benefits:

1. **Throughput** — many units can proceed at once instead of waiting for the main
   agent to hand-edit every file.
2. **Context control** — the main agent consumes structured results and diffs,
   avoiding compaction from large source archaeology and build logs.
3. **Role clarity** — the main agent stays focused on fidelity, architecture,
   dependency order, and integration quality.

Main-agent direct edits are appropriate for trivial doc fixes, one-line cleanup,
or urgent narrow integration fixes. They should be the exception, not the default
for substantive engineering.

## 6. Commit only coherent, verified batches

Workflows never auto-commit. The main agent integrates one accepted logical batch
at a time:

- review the diff for JSC fidelity and Rust ownership/rooting soundness;
- run gates matching the claim (default: `cargo fmt -- --check`, `git diff
  --check`, `cargo check --lib`, `cargo test --lib`);
- run real Octane benches only when they are required evidence, and never claim
  parity or R progress from a partial suite;
- native-JIT opcode admission requires affected real benches to validate under
  the native path — a unit test is not enough;
- commit with the C++ evidence, why the dependency was chosen, gates, and
  remaining risk;
- push immediately.

## 7. Keep the repo recoverable

A future session or teammate must recover the current direction from the repo
alone:

1. `CLAUDE.md` — contract and operating mode;
2. `README.md` — human dashboard;
3. `docs/ROADMAP.md` — plan and dependency order;
4. `docs/STATUS.md` — bounded current subsystem status;
5. `docs/design/*.md` — keystone designs and strategic decisions;
6. `git log` — detailed decision log.

Private memory is only a recall cache. Keystone decisions and strategic pivots
belong in repo docs and commit messages.
