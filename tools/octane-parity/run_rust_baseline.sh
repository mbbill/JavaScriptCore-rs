#!/bin/sh
# run_rust_baseline.sh — the symmetric Rust half of the Octane R scoreboard: drive
# the release octane_probe per-bench (fresh Vm per bench), with a per-bench timeout,
# IDENTICAL methodology to run_cpp_baseline.sh (same benches, iters, worst-case) so
# the per-bench r_i = Rust_i / C++_i is apples-to-apples. Prints one line per bench:
# "<bench>: ... score=..." (probe's own line) | "<bench>: TIMEOUT".
#
# Usage: run_rust_baseline.sh [ITERATIONS] [WORST_CASE_COUNT] [TIMEOUT_SECONDS] [bench...]
# Defaults: ITERATIONS=2 WORST_CASE_COUNT=1 TIMEOUT=300, all 15 benches.

set -u

ROOT=/Users/bytedance/Dev/JavaScriptCore-rs
PROBE="$ROOT/target/release/examples/octane_probe"
JETSTREAM_ROOT=/Users/bytedance/Dev/WebKit/PerformanceTests/JetStream3

ITERS="${1:-2}"
WC="${2:-1}"
TIMEOUT_S="${3:-300}"
shift 2>/dev/null || true
shift 2>/dev/null || true
shift 2>/dev/null || true

if [ "$#" -gt 0 ]; then
    BENCHES="$*"
else
    BENCHES="Box2D octane-code-load crypto delta-blue earley-boyer gbemu mandreel navier-stokes pdfjs raytrace regexp richards splay typescript octane-zlib"
fi

run_one() {
    bench="$1"
    "$PROBE" --benchmark "$bench" --jetstream-root "$JETSTREAM_ROOT" \
        --iterations "$ITERS" --worst-case-count "$WC" &
    pid=$!
    ( sleep "$TIMEOUT_S"; kill -9 "$pid" 2>/dev/null ) &
    watch=$!
    wait "$pid" 2>/dev/null
    rc=$?
    kill "$watch" 2>/dev/null
    wait "$watch" 2>/dev/null
    if [ "$rc" -ge 128 ]; then echo "$bench: TIMEOUT (${TIMEOUT_S}s) or signal rc=$rc"; fi
    return 0
}

echo "# Rust octane_probe Octane baseline  iters=$ITERS worstCase=$WC timeout=${TIMEOUT_S}s"
echo "# probe=$PROBE"
for bench in $BENCHES; do
    run_one "$bench"
done
