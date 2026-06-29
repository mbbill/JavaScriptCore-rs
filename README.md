# Rust JavaScriptCore — Octane Parity

A faithful C++→Rust rewrite of JavaScriptCore. **Goal:** match or beat local C++ `jsc`
on JetStream 3 Octane — `R = geomean(Rust)/geomean(C++ jsc) ≥ 1.0`, same machine, all
15 benchmarks passing first.

## Where we are

**R = UNDEFINED.** 13 of 15 Octane benches now complete+validate; 2 don't yet (mandreel,
octane-zlib — asm.js, can't finish under the interpreter) — so the suite still produces no
geomean and parity can't be measured or claimed until all 15 pass. The remaining gap is the
**optimizing JIT**, and the measured scoreboard proves it (below).

```
Overall: ~40% by effort  ███████▌░░░░░░░░░░░░░  (but ~0% of the parity-bearing JIT tiers)
```

## Progress (% of total project effort)

| Workstream | weight | done |  |
|---|---:|---:|---|
| Interpreter + parser + runtime/builtins (run all 15 correctly) | 27% | 90% | █████████░ |
| Faithful foundation (value · GC arena · Structure · strings · profiling · bytecode) | 13% | 95% | █████████▉ |
| Assembler codegen — **emit → relocate → execute machine code** | 3% | 100% | ██████████ |
| Scoreboard / measurement harness | 1% | 100% | ██████████ |
| JSStack execution substrate (frame model the JIT runs on) | 5% | 55% | ██████░░░░ |
| GC / cell-identity cutover (the GC the JIT assumes) | 7% | 20% | ██░░░░░░░░ |
| **Baseline JIT** — per-opcode machine code *(R first moves here)* | 10% | 12% | █▎░░░░░░░░ |
| **DFG** optimizing tier | 18% | 0% | ░░░░░░░░░░ |
| **FTL + B3 + Air** top tier | 15% | 0% | ░░░░░░░░░░ |
| Final correctness + perf tuning to reach R ≥ 1.0 | 1% | 0% | ░░░░░░░░░░ |

The foundation, interpreter correctness (12/15), and the whole codegen layer — the engine
can already **execute machine code it generates** — are done. The remaining ~60% is the
JIT: baseline (10%) + DFG (18%) + FTL/B3 (15%) = **43% of the project, all at 0%**. That
block is the *only* thing that lifts measured R from ~0.001 to ≥ 1.0 (and the asm.js benches
need it just to finish). So: **~40% by code, but near the start by measured R.**

## Scoreboard (measured 2026-06-28, both engines, identical harness)

| | benches | detail |
|---|---|---|
| ✅ pass | 13/15 | the 12 + **typescript** (Array-`length` fix → now `ok`, score 0.0075, interpreter-slow/JIT-gated; merged-main verified). Compute-bound `r_i = Rust/C++`: **5e-4 … 0.06** (full r_i re-measure pending) |
| ❌ fail | 2/15 | mandreel, octane-zlib (asm.js — don't finish under the interpreter; JIT-gated) |

Compute-bound benches run **~500–6000× slower** than C++; only parse-bound code-load reaches
0.06. The gap concentrates exactly where the JIT dominates — **parity is JIT-gated, with data,
not by assertion.** (Re-measure with `tools/octane-parity/run_{cpp,rust}_baseline.sh`.)

## What's next (the critical path)

A running **baseline JIT** is the next milestone that moves R. It needs, in order:
**JSStack substrate** (B1–B4 done: arena is the live register window; B5–B7 next) → **GC / R4 cell identity** → **wire per-opcode
codegen** through the proven encoder. Then **DFG → FTL** take R to ≥ 1.0. In parallel, two
correctness items protect the gate: **StringImpl** (and the now-fixed **typescript**
Array-`length` value-divergence).

## For more detail
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — the plan: critical path, full % breakdown, settled decisions.
- [`docs/STATUS.md`](docs/STATUS.md) — per-subsystem status (the agent's working tracker).
- [`docs/design/`](docs/design/) — keystone designs (JSStack, the scoreboard, …).
- `CLAUDE.md` — the project contract · `git log` — the decision log.
