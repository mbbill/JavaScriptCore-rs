// octane_driver.js — C++ jsc replica of the Rust Octane harness.
//
// PURPOSE: produce a per-bench Octane "DefaultBenchmark" score under the local
// C++ JavaScriptCore `jsc` shell using the EXACT methodology of the Rust harness
// in src/shell/octane.rs, so the numbers are apples-to-apples comparable.
//
// What this replicates from src/shell/octane.rs:
//   * Load order (prepare_octane_benchmark, octane.rs:1715-1820):
//       prelude  ->  [deterministic-random]  ->  benchmark file(s)  ->  runner
//   * Prelude (generate_octane_prelude_source, octane.rs:2706):
//       isInBrowser=false; self=this; performance fallback.
//   * Deterministic Math.random (generate_octane_deterministic_random_source,
//       octane.rs:2717) — byte-for-byte identical seeded PRNG.
//   * Runner loop (generate_octane_runner_source, octane.rs:2742):
//       new Benchmark(iterations); per iter optionally prepareForNextIteration();
//       reset random seed (deterministic benches); time runIteration() with
//       performance.now(); elapsed = max(1, end-start); then validate().
//   * Score (score_results / octane_default_to_score, octane.rs:127-174,2924):
//       first   = 5000/max(1, times[0])
//       average = 5000/max(1, mean(times[1..]))
//       worst   = 5000/max(1, mean(sortDesc(times[1..])[0:worstCaseCount]))
//       score   = geomean(first, worst, average)
//
// DELIBERATE METHODOLOGY PINS (commented divergences from a naive jsc run):
//   1. performance.now is bound to preciseTime()*1000. The Rust harness's
//      performance.now (interpreter/mod.rs:21009) is a FULL-RESOLUTION monotonic
//      millisecond timer (Instant elapsed * 1000). jsc's NATIVE performance.now
//      is resolution-clamped (~0.16ms grid, Spectre mitigation), which would
//      quantize fast iterations and diverge from the Rust timer. preciseTime() is
//      jsc's full-resolution clock, so preciseTime()*1000 matches the Rust timer
//      semantics (full-res ms; only differences are used). This is bound BEFORE
//      the prelude so the prelude's `typeof performance === "undefined"` fallback
//      is a no-op, mirroring the Rust VM injecting its own performance host global.
//   2. Each benchmark runs in a FRESH jsc process (one process per bench, driven
//      by run_cpp_baseline.sh). The Rust harness builds a fresh Vm per bench
//      (execute_prepared_octane_benchmark -> Vm::new). Required for correctness:
//      two benches both declare top-level `class Benchmark`, which would collide
//      in one global lexical environment.
//   3. The prelude/random/runner are applied as driver logic (not as three
//      separate appended source "programs"). Benchmark files are still loaded as
//      their own global programs via load(), exactly as octane.rs loads each
//      benchmark file as its own source. Observable work/timing/scoring is
//      identical; only the source-record boundary differs.

"use strict";

// ---- plan table: mirrors OCTANE_DRIVER_PLANS (octane.rs:2781-2906) ----
// det = deterministic_random; iterations/worstCase = plan-level overrides
// (None in Rust -> falls back to OCTANE_DEFAULT_ITERATION_COUNT=120 /
//  OCTANE_DEFAULT_WORST_CASE_COUNT=4). CLI args override both.
var PLANS = {
    "Box2D":            { files: ["box2d.js"],                                          det: true  },
    "octane-code-load": { files: ["code-first-load.js"],                                det: true  },
    "crypto":           { files: ["crypto.js"],                                         det: true  },
    "delta-blue":       { files: ["deltablue.js"],                                      det: true  },
    "earley-boyer":     { files: ["earley-boyer.js"],                                   det: true  },
    "gbemu":            { files: ["gbemu-part1.js", "gbemu-part2.js"],                  det: true  },
    "mandreel":         { files: ["mandreel.js"],                                       det: true, iterations: 80 },
    "navier-stokes":    { files: ["navier-stokes.js"],                                  det: true  },
    "pdfjs":            { files: ["pdfjs.js"],                                          det: true  },
    "raytrace":         { files: ["raytrace.js"],                                       det: false },
    "regexp":           { files: ["regexp.js"],                                         det: true  },
    "richards":         { files: ["richards.js"],                                       det: true  },
    "splay":            { files: ["splay.js"],                                          det: true  },
    "typescript":       { files: ["typescript-compiler.js","typescript-input.js","typescript.js"], det: true, iterations: 15, worstCase: 2 },
    "octane-zlib":      { files: ["zlib-data.js", "zlib.js"],                           det: true, iterations: 15, worstCase: 2 },
};

var OCTANE_DEFAULT_ITERATION_COUNT = 120;   // octane.rs:35
var OCTANE_DEFAULT_WORST_CASE_COUNT = 4;    // octane.rs:36

// ---- scoring math: mirrors octane.rs exactly ----
function octaneDefaultToScore(timeMs) {        // octane.rs:2924
    return 5000.0 / (timeMs < 1.0 ? 1.0 : timeMs);
}
function arithmeticMean(values) {              // octane.rs:2928
    var s = 0.0;
    for (var i = 0; i < values.length; i++) s += values[i];
    return s / values.length;
}
function geometricMean(values) {               // octane.rs:2932
    var p = 1.0;
    for (var i = 0; i < values.length; i++) p *= values[i];
    return Math.pow(p, 1.0 / values.length);
}
function scoreResults(times, worstCaseCount) { // octane.rs:127-174
    var first = times[0];
    var remaining = times.slice(1);
    var firstIteration = octaneDefaultToScore(first);
    var average = octaneDefaultToScore(arithmeticMean(remaining));
    var slowest = remaining.slice().sort(function (a, b) { return b - a; });
    var worstCase = octaneDefaultToScore(arithmeticMean(slowest.slice(0, worstCaseCount)));
    return geometricMean([firstIteration, worstCase, average]);
}

// ---- deterministic Math.random: byte-for-byte from octane.rs:2717 ----
function installDeterministicRandom() {
    (function () {
        var initialSeed = 49734321;
        var seed = initialSeed;

        Math.random = function () {
            seed = ((seed + 0x7ed55d16) + (seed << 12)) & 0xffffffff;
            seed = ((seed ^ 0xc761c23c) ^ (seed >>> 19)) & 0xffffffff;
            seed = ((seed + 0x165667b1) + (seed << 5)) & 0xffffffff;
            seed = ((seed + 0xd3a2646c) ^ (seed << 9)) & 0xffffffff;
            seed = ((seed + 0xfd7046c5) + (seed << 3)) & 0xffffffff;
            seed = ((seed ^ 0xb55a4f09) ^ (seed >>> 16)) & 0xffffffff;
            return (seed >>> 0) / 0x100000000;
        };

        Math.random.__resetSeed = function () {
            seed = initialSeed;
        };
    })();
}

function main() {
    var benchName = arguments[0];
    var jetstreamRoot = arguments[1];
    var iterArg = arguments[2];
    var wcArg = arguments[3];

    var plan = PLANS[benchName];
    if (!plan) { print(benchName + ": error=unknown-benchmark"); return; }

    var iterations = (iterArg !== undefined && iterArg !== "")
        ? (iterArg | 0)
        : (plan.iterations !== undefined ? plan.iterations : OCTANE_DEFAULT_ITERATION_COUNT);
    var worstCaseCount = (wcArg !== undefined && wcArg !== "")
        ? (wcArg | 0)
        : (plan.worstCase !== undefined ? plan.worstCase : OCTANE_DEFAULT_WORST_CASE_COUNT);

    if (iterations <= worstCaseCount) {  // octane.rs:114 (IterationsMustExceedWorstCase)
        print(benchName + ": error=iterations-must-exceed-worst-case it=" + iterations + " wc=" + worstCaseCount);
        return;
    }

    // ---- methodology pin #1: full-resolution monotonic-ms performance.now ----
    var performance = { now: function () { return preciseTime() * 1000.0; } };

    // ---- prelude effects (octane.rs:2706) ----
    var isInBrowser = false;
    var self = this;
    // performance already defined (pin #1) -> prelude fallback is a no-op.

    // host globals the runner/benchmarks may touch (octane.rs runner + host set).
    var __octaneValidationFailed = false;
    var top = {
        currentResolve: function () {},
        currentReject: function () {},
    };
    var alert = function (msg) { __octaneValidationFailed = true; };

    try {
        // ---- deterministic random (octane.rs:1774) ----
        if (plan.det) installDeterministicRandom();

        // ---- benchmark files, in plan order (octane.rs:1804) ----
        for (var fi = 0; fi < plan.files.length; fi++) {
            load(jetstreamRoot + "/Octane/" + plan.files[fi]);
        }

        // ---- runner loop (octane.rs:2742-2776) ----
        var benchmark = new Benchmark(iterations);
        var results = [];
        for (var i = 0; i < iterations; i++) {
            if (typeof benchmark.prepareForNextIteration === "function")
                benchmark.prepareForNextIteration();
            if (plan.det) Math.random.__resetSeed();
            var start = performance.now();
            benchmark.runIteration();
            var end = performance.now();
            var elapsed = Math.max(1, end - start);
            results.push(elapsed);
        }
        if (typeof benchmark.validate === "function") {
            var v = benchmark.validate();
            if (v === false) alert("Octane validation failed");
        }

        var score = scoreResults(results, worstCaseCount);

        var note = __octaneValidationFailed ? " validation=failed" : "";
        print(benchName + ": score=" + score
              + " iters=" + iterations + " wc=" + worstCaseCount
              + " times=[" + results.map(function (t) { return t.toFixed(3); }).join(",") + "]"
              + note);
    } catch (e) {
        var msg = (e && e.message) ? e.message : String(e);
        print(benchName + ": throw=" + msg.replace(/\n/g, " "));
    }
}

main.apply(this, arguments);
