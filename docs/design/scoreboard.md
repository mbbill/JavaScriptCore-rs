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
