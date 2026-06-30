# Rust JavaScriptCore — Octane Parity

A faithful C++→Rust rewrite of JavaScriptCore. **Goal:** match or beat local C++ `jsc`
on JetStream 3 Octane — `R = geomean(Rust)/geomean(C++ jsc) ≥ 1.0`, same machine, all
15 benchmarks passing first.

## Where we are

**R = UNDEFINED.** 13 of 15 Octane benches complete+validate; 2 don't yet (mandreel,
octane-zlib — asm.js, can't finish under the interpreter) — so the suite still produces no
geomean and parity can't be measured or claimed until all 15 pass. The remaining gap is the
**optimizing JIT**, and the measured scoreboard proves it (below).

**Latest (2026-06-30):** the baseline JIT's stack model is settled and its foundation has landed.
A judge panel ratified **Option A — the native machine stack IS the JS stack** (FP/SP unified, the
faithful JSC-with-JIT model), and the cutover landed: the baseline JIT now runs on the native stack
with the faithful `push_pair(fp,lr); mov fp,sp` prologue. The **first JIT→JIT native call** (the
R-lever existence proof) is in flight. On the GC side, **object- and string-cell GC are live** (the
string leak is closed). None of this has moved measured R yet — it stays the interpreter floor until
native execution broadens and becomes the default — but it is the direct, faithful path to native code.

```
Overall: ~48% by effort  █████████▌░░░░░░░░░░░  (but the parity-bearing JIT tiers are ~0%)
```

## Progress (% of total project effort)

| Workstream | weight | done |  |
|---|---:|---:|---|
| Interpreter + parser + runtime/builtins (run all 15 correctly) | 27% | 90% | █████████░ |
| Faithful foundation (value · GC arena · Structure · strings · profiling · bytecode) | 13% | 95% | █████████▉ |
| Assembler codegen — **emit → relocate → execute machine code** | 3% | 100% | ██████████ |
| Scoreboard / measurement harness | 1% | 100% | ██████████ |
| JSStack execution substrate (frame model the JIT runs on) | 5% | 70% | ███████░░░ |
| GC / cell-identity cutover (the GC the JIT assumes) | 7% | 65% | ██████▌░░░ |
| **Baseline JIT** — per-opcode machine code + native calls *(R first moves here)* | 10% | 25% | ██▌░░░░░░░ |
| **DFG** optimizing tier | 18% | 0% | ░░░░░░░░░░ |
| **FTL + B3 + Air** top tier | 15% | 0% | ░░░░░░░░░░ |
| Final correctness + perf tuning to reach R ≥ 1.0 | 1% | 0% | ░░░░░░░░░░ |

The foundation, interpreter correctness (13/15), the whole codegen layer (the engine can
**execute machine code it generates**), the GC the JIT assumes (object + string cells now live
on a swept arena), and the JIT's **native-stack frame model** are done or well underway. The
parity-bearing tiers are still the gap: **DFG (18%) + FTL/B3 (15%) = 33% of the project, at 0%**,
and the baseline JIT (10%) is ~25% — these are the *only* things that lift measured R from
~0.001 to ≥ 1.0 (and the asm.js benches need them just to finish). So: **~48% by effort, but
near the start by measured R.**

## Scoreboard (measured 2026-06-28, both engines, identical harness)

| | benches | detail |
|---|---|---|
| ✅ pass | 13/15 | the 12 + **typescript** (Array-`length` fix → now `ok`, score 0.0075, interpreter-slow/JIT-gated; merged-main verified). Compute-bound `r_i = Rust/C++`: **5e-4 … 0.06** (full r_i re-measure pending) |
| ❌ fail | 2/15 | mandreel, octane-zlib (asm.js — don't finish under the interpreter; JIT-gated) |

Compute-bound benches run **~500–6000× slower** than C++; only parse-bound code-load reaches
0.06. The gap concentrates exactly where the JIT dominates — **parity is JIT-gated, with data,
not by assertion.** (Re-measure with `tools/octane-parity/run_{cpp,rust}_baseline.sh`.)

## What's next (the critical path)

The baseline JIT is what moves R, and its **stack model + foundation are now settled and landed**
(native machine stack = JS stack; faithful prologue; GC the JIT assumes is live). Next, in order:
**first JIT→JIT native call** (in flight — the R-lever existence proof) → **broaden native calls +
lowering** (a scoped JIT-frame GC scan so native-call args can carry cells; admit more opcodes;
delete the divergent generated-* call/route layer — see `docs/design/baseline-call-tier-divergence.md`)
→ **make the JIT a net speedup and flip it to the default** (today it is opt-in and a measured
regression on call-heavy code; native execution must beat the interpreter first) → **DFG → FTL/B3**
take R to ≥ 1.0. The 2 asm.js benches (mandreel, octane-zlib) need native execution just to finish.

## For more detail
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — the plan: critical path, full % breakdown, settled decisions.
- [`docs/STATUS.md`](docs/STATUS.md) — per-subsystem status (the agent's working tracker).
- [`docs/design/`](docs/design/) — keystone designs (JSStack, the scoreboard, …).
- `CLAUDE.md` — the project contract · `git log` — the decision log.
