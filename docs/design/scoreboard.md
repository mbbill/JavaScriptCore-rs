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
