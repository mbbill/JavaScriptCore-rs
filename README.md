# Rust JavaScriptCore — Octane Parity

A faithful C++→Rust rewrite of JavaScriptCore. **Goal:** match or beat local C++ `jsc`
on JetStream 3 Octane — `R = geomean(Rust)/geomean(C++ jsc) ≥ 1.0`, same machine, all
15 benchmarks passing first.

## Where we are

**R = UNDEFINED.** 13 of 15 Octane benches complete+validate; 2 don't yet (mandreel,
octane-zlib — asm.js, can't finish under the interpreter) — so the suite still produces no
geomean and parity can't be measured or claimed until all 15 pass. The remaining gap is the
**optimizing JIT**, and the measured scoreboard proves it (below).

**Latest (2026-06-30):** the baseline JIT now runs on the **native machine stack** (judge-panel-ratified
Option A — FP/SP unified, the faithful JSC-with-JIT model) and does **native JS→JS calls that beat the
interpreter** (~39× on a call-heavy probe; native ≪ interpreter). Real op_calls resolve the callee's
native entry and `blr` it — bypassing the old divergent route arbiter entirely. On the GC side,
**object- and string-cell GC are live** (the string leak is closed). This validates the native-call
*mechanism*, but has **not moved measured R yet** — R is gated on *coverage*: the native path engages
only for functions that tier up, and the allowlist still lacks property ICs, so property-heavy Octane
hot functions stay interpreted. The next R-lever is native-lowering **breadth**.

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

The native JIT + property/method/comparison/closure breadth are DONE, and the first **real-bench
measurement** (2026-06-30) showed the load-bearing truth: the native JIT **wins on numeric benches**
(navier +12%, crypto +7.6%) but **regresses on property/call-heavy ones** (richards/delta-blue) —
because Increment-1 **far-calls every property load + per-call callee resolve**, slower than the
interpreter's inline access. So the R-gate is **the inline versions**: **Increment 2 (inline machine-code
property load) gated on gc-r4 Batch 5** (the object-storage model: inline slots + a machine-addressable
butterfly) **+ the `CallLinkInfo` monomorphic cache** (skip the per-call resolve). Then property/call-heavy
benches win → **flip the JIT to the default** → **R lifts off the interpreter floor** → **DFG → FTL/B3**
take it to ≥ 1.0. (The 2 asm.js benches still need native execution just to finish.)

## For more detail
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — the plan: critical path, full % breakdown, settled decisions.
- [`docs/STATUS.md`](docs/STATUS.md) — per-subsystem status (the agent's working tracker).
- [`docs/design/`](docs/design/) — keystone designs (JSStack, the scoreboard, …).
- `CLAUDE.md` — the project contract · `git log` — the decision log.
