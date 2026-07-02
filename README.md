# Rust JavaScriptCore — Octane Parity

A faithful C++→Rust rewrite of JavaScriptCore. **Goal:** match or beat local C++ `jsc`
on JetStream 3 Octane — `R = geomean(Rust)/geomean(C++ jsc) ≥ 1.0`, same machine, all
15 benchmarks passing first.

## Where we are

**R = UNDEFINED.** 13 of 15 Octane benches complete+validate; 2 don't yet (mandreel,
octane-zlib — asm.js, can't finish under the interpreter) — so the suite still produces no
geomean and parity can't be measured or claimed until all 15 pass. The remaining gap is the
**optimizing JIT**, and the measured scoreboard proves it (below).

**Latest:** the arena is now **leak-free end-to-end** — Symbol cells (`c9c3227`) close the last
leaking cell store, joining Object (R4b) / String (U0–U1) / BigInt (`354cb89`), plus a faithful
weak-finalization seam with WeakMap/WeakSet ephemeron semantics (`3ad0ab7`) and a CodeBlock
constant-pool rooting fix that closed a latent UAF (`f213265`) — all adversarially verified. In
parallel, the interpreter-tier **profile population round completed** (`c650d48`/`8a2b5e7`/
`1f53724`/`5f45ab9`): value, array, and binary/unary arith profiles now record live at every
LLInt-faithful site, closing the DFG's `SpecNone→ForceOSRExit` hazard for the wired opcode set. And
the DFG got its **first real component**: `src/dfg/parser.rs` (`c164345`) lowers one non-inlined,
non-speculative basic block (`op_enter {mov|add|sub|mul}* op_ret`) into a faithful `DfgBasicBlock`
— the DFG is no longer 0%, though nothing executes yet (no plan/phases/speculation/codegen). The
generated 193-opcode table (G1–G3: `833592d`/`ee174a7`/`7accf10`) is now the crate's live
`OPCODE_TABLE` — the packed stream can decode every JSC bytecode. In flight: the negative-zero
value fix, getter-resume value profiling, U8 argument profiles, and a DFGPlan parse-only analog.
**R is still UNDEFINED** — no bench-pass count or measured performance changed this round; this
closes foundational GC/profiling/bytecode dependencies the DFG/JIT need.

```
Overall: ~55% by effort  ███████████▌░░░░░░░░░  (but the parity-bearing JIT tiers are still ~0-4%)
```

## Progress (% of total project effort)

| Workstream | weight | done |  |
|---|---:|---:|---|
| Interpreter + parser + runtime/builtins (run all 15 correctly) | 27% | 90% | █████████░ |
| Faithful foundation (value · GC arena · Structure · strings · profiling · bytecode) | 13% | 98% | █████████▊ |
| Assembler codegen — **emit → relocate → execute machine code** | 3% | 100% | ██████████ |
| Scoreboard / measurement harness | 1% | 100% | ██████████ |
| JSStack execution substrate (frame model the JIT runs on) | 5% | 70% | ███████░░░ |
| GC / cell-identity cutover (the GC the JIT assumes) | 7% | 90% | █████████░ |
| **Baseline JIT** — per-opcode machine code + native calls *(now net-wins over interp; mixed)* | 10% | 35% | ███▌░░░░░░ |
| **DFG** optimizing tier | 18% | 4% | ▍░░░░░░░░░ |
| **FTL + B3 + Air** top tier | 15% | 0% | ░░░░░░░░░░ |
| Final correctness + perf tuning to reach R ≥ 1.0 | 1% | 0% | ░░░░░░░░░░ |

The foundation, interpreter correctness (13/15), the whole codegen layer (the engine can
**execute machine code it generates**), and the JIT's **native-stack frame model** are done or
well underway. The GC row moves to 90%: the arena is now **leak-free end-to-end** — all four cell
kinds (object/string/bigint/symbol) reclaim, plus faithful weak-collection semantics; what's left
is the scoped native-stack conservative scan and generational/incremental collection. The DFG row
ticks up off 0% for the first time: it has a real (if tiny) parser, but nothing executes yet.
The baseline JIT proves native execution is real and can beat the interpreter, but the
parity-bearing tiers are still the gap: **DFG (18%) + FTL/B3 (15%) = 33% of the project, still
~2% done overall**. Those are what
lift measured R from ~0.001 to ≥ 1.0. So: **~55% by effort, but still near the start by measured
R** — R itself did not move this round.

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
   W1 widening **landed** (real generated opcode ids — wide16/32 = 128/130, not 0/1 — + sub/mul rows); the
   generated 193-opcode table is now the crate's live `OPCODE_TABLE` (G1–G3 **done**) — the packed stream
   can **decode every JSC bytecode**. G4 (the CoreOpcode identity cutover, ~8k refs) remains, separately.
2. In parallel: **SpeculatedType canonicalization — done**; **profiles — storage/derivation/population all
   done** (value + array + binary/unary arith all record live at LLInt-faithful sites; U8 argument profiles
   + getter-resume + construct-result remain); **baseline-as-bailout — JITCodeMap landed, exit-target
   ratified (the first OSR exit lands in the interpreter), gating speculative DFG only**.
3. The first **single-basic-block non-speculative DFG parser landed** (`src/dfg/parser.rs`, `c164345`):
   lowers `op_enter {mov|add|sub|mul}* op_ret` into a faithful `DfgBasicBlock`. Next: a DFGPlan analog
   (graph creation + identity stamping), then DFG speculation + OSR exit, then FTL/B3.

The default flip is deferred: it would only define R at the ~1e-3 floor, and the flip-gate survey found
mandreel/octane-zlib still decline on missing opcodes under execoff. See `docs/design/dfg-path.md`.

## For more detail
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — the plan: critical path, full % breakdown, settled decisions.
- [`docs/STATUS.md`](docs/STATUS.md) — per-subsystem status (the agent's working tracker).
- [`docs/design/`](docs/design/) — keystone designs (DFG path, JSStack, scoreboard, main-agent principles, …).
- `CLAUDE.md` — the project contract · `git log` — the decision log.
