#!/bin/sh
# run_cpp_baseline.sh — drive octane_driver.js under the local C++ jsc to produce
# the Octane C++ baseline, one fresh jsc process per benchmark (matches the Rust
# harness's fresh-Vm-per-bench), with a per-bench timeout. Prints one line per
# bench: "<bench>: score=..." | "<bench>: throw=..." | "<bench>: TIMEOUT".
#
# Usage:
#   run_cpp_baseline.sh [ITERATIONS] [WORST_CASE_COUNT] [TIMEOUT_SECONDS] [bench...]
# Defaults: ITERATIONS=2 WORST_CASE_COUNT=1 TIMEOUT=900, all 15 benches.
# (2/1 matches the reduced-iteration methodology used for the slow Rust
#  interpreter via octane_probe --iterations 2 --worst-case-count 1, applied
#  IDENTICALLY to both engines for an apples-to-apples r_i.)

set -u

WEBKIT_BUILD=/Users/bytedance/Dev/WebKit/WebKitBuild/Release
JSC="$WEBKIT_BUILD/jsc"
JETSTREAM_ROOT=/Users/bytedance/Dev/WebKit/PerformanceTests/JetStream3
HERE="$(cd "$(dirname "$0")" && pwd)"
DRIVER="$HERE/octane_driver.js"

ITERS="${1:-2}"
WC="${2:-1}"
TIMEOUT_S="${3:-900}"
shift 2>/dev/null || true
shift 2>/dev/null || true
shift 2>/dev/null || true

if [ "$#" -gt 0 ]; then
    BENCHES="$*"
else
    BENCHES="Box2D octane-code-load crypto delta-blue earley-boyer gbemu mandreel navier-stokes pdfjs raytrace regexp richards splay typescript octane-zlib"
fi

# locate a timeout implementation; fall back to a portable watchdog.
TIMEOUT_BIN=""
if command -v timeout >/dev/null 2>&1; then
    TIMEOUT_BIN="timeout"
elif command -v gtimeout >/dev/null 2>&1; then
    TIMEOUT_BIN="gtimeout"
fi

run_one() {
    bench="$1"
    if [ -n "$TIMEOUT_BIN" ]; then
        DYLD_FRAMEWORK_PATH="$WEBKIT_BUILD" DYLD_LIBRARY_PATH="$WEBKIT_BUILD" \
            "$TIMEOUT_BIN" "$TIMEOUT_S" "$JSC" "$DRIVER" -- "$bench" "$JETSTREAM_ROOT" "$ITERS" "$WC"
        rc=$?
        if [ "$rc" = "124" ]; then echo "$bench: TIMEOUT (${TIMEOUT_S}s)"; fi
        return 0
    fi
    # portable watchdog: run jsc in background, kill if it overruns.
    DYLD_FRAMEWORK_PATH="$WEBKIT_BUILD" DYLD_LIBRARY_PATH="$WEBKIT_BUILD" \
        "$JSC" "$DRIVER" -- "$bench" "$JETSTREAM_ROOT" "$ITERS" "$WC" &
    jsc_pid=$!
    ( sleep "$TIMEOUT_S"; kill -9 "$jsc_pid" 2>/dev/null ) &
    watch_pid=$!
    wait "$jsc_pid" 2>/dev/null
    rc=$?
    kill "$watch_pid" 2>/dev/null
    wait "$watch_pid" 2>/dev/null
    if [ "$rc" -ge 128 ]; then echo "$bench: TIMEOUT (${TIMEOUT_S}s) or signal rc=$rc"; fi
    return 0
}

echo "# C++ jsc Octane baseline  iters=$ITERS worstCase=$WC timeout=${TIMEOUT_S}s"
echo "# jsc=$JSC"
for bench in $BENCHES; do
    run_one "$bench"
done
