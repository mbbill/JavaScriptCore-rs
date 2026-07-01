# Rust JavaScriptCore — Octane Parity

A faithful C++→Rust rewrite of JavaScriptCore. **Goal:** match or beat local C++ `jsc`
on JetStream 3 Octane — `R = geomean(Rust)/geomean(C++ jsc) ≥ 1.0`, same machine, all
15 benchmarks passing first.

## Where we are

**R = UNDEFINED.** 13 of 15 Octane benches complete+validate; 2 don't yet (mandreel,
octane-zlib — asm.js, can't finish under the interpreter) — so the suite still produces no
geomean and parity can't be measured or claimed until all 15 pass. The remaining gap is the
**optimizing JIT**, and the measured scoreboard proves it (below).

**Latest:** the **DFG precursor set** (the pivot after the baseline JIT's measured net win over the
interpreter — still the ~1e-3 floor vs C++) has landed its core slices: baseline images now persist
the **JITCodeMap** (the bci→machine-code OSR landing map), the abstract Rust-only DFG node taxonomy
is **deleted** in favor of faithful `NodeType`/`NodeFlags`/`VariableAccessData` (the graph starts
`LoadStore`, as JSC's does), the packed mov/ret wedge is correctness-hardened (no mid-instruction
decode, constants placed by constant index, JSC-derived byte fixtures), and profile-slot derivation
now covers **all** profile-carrying opcodes plus Binary/Unary `ArithProfile` storage + record APIs.
Ratified: the first DFG OSR exit lands in the **interpreter** (JSC's `exitToLLInt` analog), so the
bailout hard gate sits before *speculative* DFG only — the first non-speculative parser is unblocked.
In flight: profile population (U1–U8, four parallel units) and the first DFG parser slice
(`src/dfg/parser.rs`, P2). The DFG itself stays 0% until that parser exists.

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
| **Baseline JIT** — per-opcode machine code + native calls *(now net-wins over interp; mixed)* | 10% | 35% | ███▌░░░░░░ |
| **DFG** optimizing tier | 18% | 0% | ░░░░░░░░░░ |
| **FTL + B3 + Air** top tier | 15% | 0% | ░░░░░░░░░░ |
| Final correctness + perf tuning to reach R ≥ 1.0 | 1% | 0% | ░░░░░░░░░░ |

The foundation, interpreter correctness (13/15), the whole codegen layer (the engine can
**execute machine code it generates**), the GC the JIT assumes (object + string cells now live
on a swept arena), and the JIT's **native-stack frame model** are done or well underway. The
baseline JIT now proves native execution is real and can beat the interpreter, but the
parity-bearing tiers are still the gap: **DFG (18%) + FTL/B3 (15%) = 33% of the project, at 0%**.
Those are what lift measured R from ~0.001 to ≥ 1.0. So: **~48% by effort, but still near the
start by measured R.**

## Scoreboard (measured 2026-06-28, both engines, identical harness)

| | benches | detail |
|---|---|---|
| ✅ pass | 13/15 | the 12 + **typescript** (Array-`length` fix → now `ok`, score 0.0075, interpreter-slow/JIT-gated; merged-main verified). Compute-bound `r_i = Rust/C++`: **5e-4 … 0.06** (full r_i re-measure pending) |
| ❌ fail | 2/15 | mandreel, octane-zlib (asm.js — don't finish under the interpreter; JIT-gated) |

Compute-bound benches run **~500–6000× slower** than C++; only parse-bound code-load reaches
0.06. The gap concentrates exactly where the JIT dominates — **parity is JIT-gated, with data,
not by assertion.** (Re-measure with `tools/octane-parity/run_{cpp,rust}_baseline.sh`.)

## What's next (the critical path)

The native baseline JIT is a net win over the interpreter but mixed, and a failed LoadCallee admission
proved that native-opcode unit tests are not enough — REAL benches must validate before widening the
allowlist. The strategic assessment after that milestone re-derived the highest-value next dependency:
**move toward the optimizing JIT, not more baseline-local wins.** Current critical path:

1. **Packed bytecode stream live cutover** — the #1 representation divergence: first live wedge landed
   (raw `op_mov`/`op_ret`, byte-offset PC + `Fits<VirtualRegister>` constants) and correctness-hardened;
   W1 widening **landed** (real generated opcode ids — wide16/32 = 128/130, not 0/1 — + sub/mul rows).
2. In parallel: **SpeculatedType canonicalization — done**; **profiles — storage + derivation done (F0),
   population running as 4 parallel units (U1–U8)**; **baseline-as-bailout — JITCodeMap landed, exit-target
   ratified (the first OSR exit lands in the interpreter), gating speculative DFG only**.
3. Then the first **single-basic-block non-speculative DFG parser** (`src/dfg/parser.rs`, in flight; the
   faithful NodeType skeleton is already landed), followed by DFG speculation + OSR exit, then FTL/B3.

The default flip is deferred: it would only define R at the ~1e-3 floor, and the flip-gate survey found
mandreel/octane-zlib still decline on missing opcodes under execoff. See `docs/design/dfg-path.md`.

## For more detail
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — the plan: critical path, full % breakdown, settled decisions.
- [`docs/STATUS.md`](docs/STATUS.md) — per-subsystem status (the agent's working tracker).
- [`docs/design/`](docs/design/) — keystone designs (DFG path, JSStack, scoreboard, main-agent principles, …).
- `CLAUDE.md` — the project contract · `git log` — the decision log.
