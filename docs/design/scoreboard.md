# Design — the R scoreboard (the measuring instrument)

`R = geomean(Rust per-bench scores) / geomean(C++ jsc per-bench scores)` on JetStream 3
Octane, same machine/inputs/scoring. The local C++ `jsc` comparison is a **first-class
requirement** — without it there is no scoreboard, so "done" is undefined. The C++
baseline is **re-measured on the machine, never assumed**.

**Correctness gate (precondition):** all 15 benches must complete + pass their built-in
validation (zero throws, zero oracle/wrong-answer failures) before the suite yields a
geomean. Until then **R is undefined and parity must not be claimed.** Report R and the
full set of `r_i = Rust_i / C++_i` — never a single bench or a partial suite.

## The instruments (identical harness, both engines)

- **C++ side:** `tools/octane-parity/run_cpp_baseline.sh [iters] [wc] [timeout] [bench…]`
  — drives `octane_driver.js` (replicates `shell/octane.rs` scoring) under local `jsc`,
  fresh process per bench, per-bench timeout.
- **Rust side:** `tools/octane-parity/run_rust_baseline.sh [iters] [wc] [timeout] [bench…]`
  — drives the release `octane_probe --benchmark <name>` per bench, portable watchdog.
- **jsc binary:** `/Users/bytedance/Dev/WebKit/WebKitBuild/Release/jsc`, built via
  `Tools/Scripts/build-jsc --release`. Run it with the build dir on the framework path or
  it dyld-fails against the system framework:
  `DYLD_FRAMEWORK_PATH=…/Release DYLD_LIBRARY_PATH=…/Release …/Release/jsc <script>`.
- Both run at the same `iters/wc` (e.g. 2/1 for the slow interpreter) so `r_i` is
  apples-to-apples. Octane benches: `/Users/bytedance/Dev/WebKit/PerformanceTests/JetStream3`.

## 2026-06-30 — the faithful native JIT call path WORKS + beats the interpreter (mechanism validated)

After the native-stack cutover (A1.0–A1.5: the baseline JIT runs on the native machine stack; the
first JIT→JIT native call; cell rooting; stack-overflow check) + **broad engagement** (live op_calls
resolve the callee's installed native entry per call and `blr` it when the callee is baseline-JIT'd):
- **Synthetic call-heavy probe** `caller(n){s=0; for i<n s+=callee(i)} , callee(x)=x+1`, both JIT'd,
  each op_call taking the native `blr`: **native 4.56ms vs interpreter 178ms ≈ 39×** (engagement
  counter == n, proven taken). HONEST CAVEAT: the 39× is INFLATED by the pre-GC interpreter's heavy
  per-call path (fresh interpreter VM per rep to bound the leak) — NOT a steady-state parity figure;
  the load-bearing result is **native ≪ interpreter, decisively.** native == interpreter == oracle.
- This is the FAITHFUL path (CallLinkInfo-style per-call resolve → native `blr`); it **bypasses the
  generated-* route arbiter entirely** — the 06-29 regression source. So the native call mechanism is
  validated as a real speedup.

**BUT this does NOT yet move R, and re-measuring real Octane would still not, because of COVERAGE:** the
native path engages only for functions that TIER UP, and the S4 allowlist is narrow (int/double arith,
typed-array element get/put, op_call, LoadDouble — **NO property `get_by_id`/`put_by_id` ICs**). Real
Octane hot functions are PROPERTY-heavy, so they fail the allowlist, stay interpreted, and never reach
the native call path. The mechanism is proven on allowlist-covered code; real benches need breadth.

**NEXT R-LEVER (evidence-derived): native-lowering BREADTH — the baseline property-access ICs
(`get_by_id`/`put_by_id`, the deferred K2) + more opcodes**, so real Octane hot functions tier up and
run native. THEN re-measure the JIT-default (now bypassing the route arbiter) → if a net speedup, flip
the default (held since 06-29) → **R finally moves above the interpreter floor.** Re-measure on the same
harness; report R + all r_i.

## 2026-06-29 — the baseline JIT MEASURED: a REGRESSION, not a speedup yet

First apples-to-apples measurement after the GC track + the baseline gate-capability
set landed (op_call/typed-array/LoadDouble all execute). navier-stokes (typed-array
+ arith, the best fit for the current allowlist), iters=2/wc=1, same release binary:
- **--interpreter score = 4.26** ; **--baseline (JIT) score = 0.42** → the baseline
  JIT is **~10× SLOWER than the interpreter**. It VALIDATES (`ok`) and the live GC
  keeps it memory-bounded (no OOM) — a real CORRECTNESS milestone — but it is a
  PERFORMANCE REGRESSION, not a speedup.
- **The DEFAULT config is InterpreterOnly** (the JIT is opt-in via --baseline). So
  the contract's R is the INTERPRETER score; the JIT regression does NOT corrupt
  measured R, but the JIT has NOT moved it either.
- Cause (from --tiering-summary): of 8 hot CodeBlocks only **3 lower to real native;
  5 FALL BACK to the slow "generated" re-interpreter** — native lowering fails on
  UnsupportedOpcode (StrictEqual, a LoadDouble form) + the native call route is
  deferred (op_call/typed-array are slow-call far-calls). Tiering-up currently ADDS
  overhead without fast native execution.

DEEPER VALIDATION (2026-06-29, full-bench): the "generated executor OFF" win does NOT
generalize — it holds only for LOW-CALL loop kernels. Full-bench default-vs-interp
(executor off, JIT default): navier-stokes 1.39x (win), crypto 1.00x, richards 0.33x
(3x REGRESSION), delta-blue 0.37x, raytrace/earley-boyer/regexp DNF. **Geomean ~0.64x —
the JIT default is a ~1.5x NET REGRESSION even with the executor off.** ZERO correctness
failures (every bench validates ok) — purely perf. ROOT CAUSE: this host is **arm64**,
but the baseline CALL-TARGET native entries are **x86_64 byte sequences** (rejected by
can_execute_baseline_native_entry_kind → `HostBlockedX86_64`); only a NARROW arm64
return-seed exists (config.rs:30-36). So ALL ~3.6M generated-direct-call transactions take
route NestedInterpreterFallback (generated_direct_call_native_entries=0) **while still
paying the per-call route-selection + per-transaction accounting**. The slow-call op_call
(B5-first-cut: far_call operation_call → interpreter) + this per-call overhead regress
call-heavy benches ~3x; only low-call loops net a win.

IMPLICATION (the JIT is machinery-complete + CORRECT but perf-negative on arm64): to MOVE R
the baseline must become a NET SPEEDUP first. THE LOAD-BEARING BLOCKER (arm64): **B5-full —
an arm64-callable native baseline CALL entry** (the native bl-chain + direct-link via
CallLinkInfo, replacing the x86_64-host-blocked entry so a JS→JS call jumps native instead
of falling back to the nested interpreter) **+ stop paying the per-call route/accounting
when the route is always interpreter-fallback** (link the site to a cheap direct interpreter
call). Then: keep the generated-executor-off policy, admit StrictEqual + fix the LoadDouble
form (native breadth), land the inline typed-array stub — THEN flip the default. HOLD the
default flip until B5-full lands (it moves R DOWN today). Broadening tier-up before the call
path is fast makes it WORSE.

## Latest measurement (2026-06-28, iters=2/wc=1) — R UNDEFINED

Gate not met: 2/15 fail — mandreel + octane-zlib (asm.js DNF/too-slow, JIT-gated; NOT throw,
NOT OOM). typescript value-divergence FIXED SINCE this snapshot (Array-length; now completes,
interpreter-slow ~0.0075), so 13/15 complete. C++ jsc baseline (all 15 complete): crypto
1611.6, richards 1240.1, navier 1184.5,
delta-blue 1072.3, code-load 962.0, regexp 750.0, splay 699.5, raytrace 689.9, earley-boyer
662.8, Box2D 462.2, pdfjs 261.0, mandreel 198.9, gbemu 136.5, octane-zlib 37.6, typescript
36.4.

Rust per-bench `r_i` (the 12 completing in this 2026-06-28 snapshot; typescript is now the
13th completer at ~0.0075, not re-measured here): code-load 0.060 (parse-bound, the only one above
~3e-3), pdfjs 3.3e-3, regexp 2.4e-3, navier 2.0e-3, crypto 1.3e-3, gbemu 9.7e-4, splay
9.2e-4, richards 7.4e-4, Box2D 6.8e-4, earley-boyer 5.4e-4, delta-blue 5.2e-4, raytrace
1.7e-4. Partial geomean over the completing benches ≈ 1.3e-3.

**Conclusion:** compute-bound `r_i` cluster at 5e-4–2e-3 (≈500–6000× slower); only the
parse-bound bench approaches 0.06. The gap concentrates exactly where the JIT dominates →
**parity is JIT-gated**, with data. R cannot become defined (let alone ≥1.0) until the JIT
runs (and the asm.js benches need it just to complete).

Re-run both baselines after any change that could move a bench across the
complete/validate line, and treat the result — R if all 15 pass, else the `r_i` set with
R explicitly undefined — as the only progress report.

## 2026-06-30 LATE — THE REAL-BENCH RE-MEASURE: the harness uses the WRONG JIT path (load-bearing)

First real-bench re-measure after all the native-stack + broad-engagement + property/method/comparison/
closure breadth: **navier-stokes --interpreter = 4.27 ; --baseline (JIT) = 0.42 -> the JIT is still ~10x
SLOWER** (same as 06-29; both validate ok -- purely perf). The synthetic probes (39x call-heavy, 2.3x
method-heavy) were UNIT TESTS of the ARM64 `emit_baseline_function` path in ISOLATION; they did NOT
measure the octane harness.

ROOT CAUSE (from navier --baseline --tiering-summary): **`octane_probe --baseline` routes through the OLD
generated-* divergent machinery, NOT the ARM64 native path where the whole session's work lives.** Evidence:
3.44M baseline_generated_executions (the byte-blob generated EXECUTOR runs the hot functions); the hot
CodeBlocks fail "native lowering" on StrictEqual + LoadDouble -- opcodes the ARM64 emit_baseline_function
ADMITS (unit tests green) -- so the octane lowering is a DIFFERENT emitter (x86-64 P6 / generated-direct-
call), not emit_baseline_function; native_entry_miss=HostBlockedX86_64 (x86-64 entries can't run on this
arm64 host -> generated-executor fallback).

So the breadth was necessary but the harness never used it. **THE R-LEVER: wire the live BaselineAllowed
tier-up (the octane path) to the ARM64 `emit_baseline_function` + native-call path (broad engagement +
the property/method/comparison/closure breadth), replacing the generated-* machinery** -- the STEP 2/3
cutover of docs/design/baseline-call-tier-divergence.md, now confirmed as the gate to R. Diagnostic in
flight to map the dispatch (BaselineAllowed -> which emitter) + the wiring.

## 2026-06-30 LATE-2 — the R-gate, measured: FAR-CALLS regress property/call-heavy benches (Batch 5 / Increment 2)

Two findings from the real-bench measurement (octane_probe, bounded, memory-safe ~0.8GB RSS):

(1) **The 10x navier "regression" was ENTIRELY the generated EXECUTOR** (the byte-blob re-interpreter
that declined functions fall to). The wiring is CORRECT: execute_code_block_with_entry_kind (vm/mod.rs:6496)
tries the ARM64 maybe_run_live_baseline_jit_entry FIRST per entry (the same path the unit tests use), and
only falls to the generated executor when emit_baseline_function DECLINES (it bails on the FIRST unsupported
opcode, and Octane functions are large). Disabling the generated executor (--disable-baseline-generated-
executor) -> declined functions INTERPRET instead of running the slow shim.

(2) **With the executor OFF, the native JIT WINS on numeric benches but REGRESSES on property/call-heavy:**
navier 4.27->4.77 (+12%), crypto 2.70->2.90 (+7.6%); BUT richards 0.96->~0.31 (-68%), delta-blue 0.67->0.30
(-55%). Both validate ok -- purely perf. CAUSE (confirmed: richards baseline_generated_executions=0 yet still
0.28, far below the 0.96 a decline-to-interpreter would give, so its functions tier up NATIVE but run slow):
**Increment-1 FAR-CALLS every property load (get_by_id/get_by_val), closure read, and per-call callee
resolve.** Numeric benches (navier/crypto) have few of these -> win; property/call-heavy benches (richards/
delta-blue, and most of Octane) far-call on every field access + method call -> the far-calls dominate ->
~2-3x slower than the interpreter's inline dispatch.

THE R-GATE (measurement-confirmed, NOT more breadth): the INLINE versions.
- **Increment 2 = inline machine-code property load** (replace the get_by_id/put_by_id/get_by_val/closure
  far-calls with an inline structure-guarded load), GATED on **gc-r4 Batch 5** (the object-storage model:
  real inline slots on the cell + a machine-ADDRESSABLE butterfly pointer; today butterfly is a slab INDEX).
- **CallLinkInfo monomorphic cache** = a direct bl + callee-identity guard, skipping the per-call resolve
  far-call (needs the R4 visitWeak weak-processing, U7).
Then property/call-heavy benches win -> the JIT is a net speedup across -> flip the default (held) -> R moves.

LEVER A (disable the generated executor) is a correct hygiene win (it's the confirmed generated-* re-interp
divergence; STEP 5 deletes it) but NOT a net R-win alone (property/call-heavy regress). The breadth (Inc 1)
was necessary (functions tier up) but the FAR-CALLS make it perf-negative for property/call code until the
inline versions land. Batch 5 -> Increment 2 + the call cache is the evidence-backed path to R.

## 2026-06-30 LATE-3 — Increment 2 (inline load) WINS on numeric; call-heavy gated on the CALL path

Batch 5 (inline slots + machine-addressable butterfly) + Increment 2 (inline machine-code property load,
verbatim loadProperty, native==interp==oracle, no far-call on HIT) all LANDED + miri-clean (2827 green).
Re-measure (--baseline --disable-baseline-generated-executor --disable-generated-direct-call-generated-entry):
- **navier-stokes 4.29 -> 5.69 (+33% WIN)** -- the inline load is on its hot path (call-light arith/closure).
- richards 1.01 -> 0.36, delta-blue 0.49 -> 0.28 -- STILL ~3x regressions, UNMOVED by the inline load.

CAUSE (--tiering-summary, richards/delta-blue): they are CALL-dominated. ~5M generated_direct_call_transactions,
~100% NestedInterpreterFallback, native_entry_miss=HostBlockedX86_64, **property_load_observations=0**. Their
hot CALLER functions do NOT tier up to the ARM64 emit_baseline_function path (an opcode gap declines them) ->
they fall to the x86-64/generated-direct-call ROUTE ARBITER -> every call re-enters the interpreter (the route
arbiter's per-call overhead makes BaselineAllowed-mode SLOWER than InterpreterOnly), and the property loads run
in the interpreted callees where the native inline code never executes (hence property_load_observations=0).

SO THE R-PICTURE SPLITS BY BENCH SHAPE (measured, not assumed):
- **Numeric/arith-heavy (navier/crypto): WIN now** -- Increment 2 + the native arith path. The inline property
  load is the right, faithful fix and it works where property loads run native.
- **Call/property-heavy (richards/delta-blue, ~half of Octane): gated on the CALL PATH** -- their hot functions
  must tier up to ARM64-native (close the remaining emit_baseline_function opcode gap: the likely set from the
  scoping is the Load* constants [LoadUndefined/Null/Bool/String], LogicalNot, Equal/NotEqual, NegateNumber,
  NewObject/NewArray, ModNumber/ToNumber) so their CALLS go native via broad engagement (emit_op_call_dynamic),
  BYPASSING the generated-direct-call route arbiter; and/or delete that route arbiter (the confirmed divergence,
  STEP 1) so residual interpreted-mode calls stop paying its per-call overhead. NOT property ICs.

NEXT R-LEVER (evidence-derived): a diagnostic to dump the EXACT ARM64 emit_baseline_function declines for
richards/delta-blue's hot functions -> a targeted breadth batch of those opcodes -> they fully tier up + native-
call -> re-measure (do they flip from ~0.3x toward >1.0x?). Plus STEP 1 (route-arbiter deletion) for the residual.
