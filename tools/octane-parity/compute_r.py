#!/usr/bin/env python3
"""compute_r.py — reduce official Octane parity runs into per-bench r_i and R.

Usage:
  tools/octane-parity/compute_r.py CPP_OUTPUT RUST_OUTPUT
  tools/octane-parity/compute_r.py --self-test

Inputs are captured stdout/stderr text from:
  tools/octane-parity/run_cpp_baseline.sh  > cpp.out  2>&1
  tools/octane-parity/run_rust_baseline.sh > rust.out 2>&1

This is a validator first and a reducer second. It implements the project's
scoreboard rule: local C++ `jsc` is the measuring instrument, and R is defined
only when both engines report all 15 Octane benchmarks as complete and valid.
The reducer therefore refuses to compute R when any benchmark is missing or
failed. In that case it prints `R UNDEFINED` and exits nonzero.

Rejected conditions include:
  * missing or duplicate bench result lines in either input;
  * C++ `validation=failed`, `throw=`, `TIMEOUT`, `failed`, or non-score lines;
  * Rust `failed`, `throw=`, `TIMEOUT`, `validation=failed`, or non-score lines;
  * non-finite, zero, or negative scores.

On success it prints each per-bench ratio:
  r_i = Rust_i / C++_i
and the suite ratio:
  R = geomean(Rust scores) / geomean(C++ scores)
which is equivalent to geomean(r_i) over the same 15 benchmarks.
"""

from __future__ import annotations

import argparse
import math
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Sequence

BENCHES: tuple[str, ...] = (
    "Box2D",
    "octane-code-load",
    "crypto",
    "delta-blue",
    "earley-boyer",
    "gbemu",
    "mandreel",
    "navier-stokes",
    "pdfjs",
    "raytrace",
    "regexp",
    "richards",
    "splay",
    "typescript",
    "octane-zlib",
)
BENCH_SET = set(BENCHES)

SCORE_RE = re.compile(r"(?:^|\s)score=([^\s]+)")
BAD_MARKERS_RE = re.compile(
    r"(?:^|\s)(?:TIMEOUT\b|throw=|failed\b|validation=failed\b|error=)",
    re.IGNORECASE,
)


@dataclass(frozen=True)
class ParseResult:
    scores: dict[str, float]
    errors: list[str]


def parse_score(raw: str) -> float | None:
    try:
        score = float(raw)
    except ValueError:
        return None
    if not math.isfinite(score) or score <= 0.0:
        return None
    return score


def parse_benchmark_output(engine: str, text: str) -> ParseResult:
    """Parse one run_{cpp,rust}_baseline output, collecting scores or errors."""
    scores: dict[str, float] = {}
    errors: list[str] = []
    seen_result_lines: dict[str, int] = {}

    for line_no, original_line in enumerate(text.splitlines(), start=1):
        line = original_line.strip()
        if not line or line.startswith("#") or ":" not in line:
            continue

        bench, rest = line.split(":", 1)
        bench = bench.strip()
        if bench not in BENCH_SET:
            continue

        rest = rest.strip()
        is_score_line = SCORE_RE.search(rest) is not None
        is_bad_line = BAD_MARKERS_RE.search(rest) is not None
        is_result_line = is_score_line or is_bad_line
        if not is_result_line:
            # Examples: Rust probe config lines (`bench: config mode=...`).
            continue

        if bench in seen_result_lines:
            previous = seen_result_lines[bench]
            errors.append(
                f"{engine} {bench}: duplicate result line at {line_no} "
                f"(previous {previous})"
            )
            continue
        seen_result_lines[bench] = line_no

        if is_bad_line:
            errors.append(f"{engine} {bench}: rejected line {line_no}: {line}")
            continue

        match = SCORE_RE.search(rest)
        if match is None:
            errors.append(f"{engine} {bench}: no score on line {line_no}: {line}")
            continue

        score = parse_score(match.group(1))
        if score is None:
            errors.append(
                f"{engine} {bench}: invalid score {match.group(1)!r} "
                f"on line {line_no}"
            )
            continue
        scores[bench] = score

    for bench in BENCHES:
        if bench not in scores and not any(
            error.startswith(f"{engine} {bench}:") for error in errors
        ):
            errors.append(f"{engine} {bench}: missing result")

    return ParseResult(scores=scores, errors=errors)


def geometric_mean(values: Iterable[float]) -> float:
    values = list(values)
    return math.exp(sum(math.log(value) for value in values) / len(values))


def compute_ratios(cpp_scores: dict[str, float], rust_scores: dict[str, float]) -> tuple[list[tuple[str, float, float, float]], float]:
    rows: list[tuple[str, float, float, float]] = []
    for bench in BENCHES:
        cpp = cpp_scores[bench]
        rust = rust_scores[bench]
        rows.append((bench, cpp, rust, rust / cpp))
    cpp_geo = geometric_mean(cpp_scores[bench] for bench in BENCHES)
    rust_geo = geometric_mean(rust_scores[bench] for bench in BENCHES)
    return rows, rust_geo / cpp_geo


def validate_and_render(cpp_text: str, rust_text: str) -> tuple[int, str]:
    cpp = parse_benchmark_output("C++", cpp_text)
    rust = parse_benchmark_output("Rust", rust_text)
    errors = cpp.errors + rust.errors

    if errors:
        lines = ["R UNDEFINED", "correctness gate failed:"]
        lines.extend(f"  - {error}" for error in errors)
        return 1, "\n".join(lines) + "\n"

    rows, ratio = compute_ratios(cpp.scores, rust.scores)
    lines = ["bench cpp_score rust_score r_i"]
    for bench, cpp_score, rust_score, bench_ratio in rows:
        lines.append(
            f"{bench} {cpp_score:.12g} {rust_score:.12g} {bench_ratio:.12g}"
        )
    lines.append(f"R {ratio:.12g}")
    return 0, "\n".join(lines) + "\n"


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        raise SystemExit(f"failed to read {path}: {error}") from error


def make_fixture_outputs(scale: float = 0.5) -> tuple[str, str]:
    cpp_lines = ["# C++ jsc Octane baseline  iters=2 worstCase=1 timeout=900s"]
    rust_lines = ["# Rust octane_probe Octane baseline  iters=2 worstCase=1 timeout=300s"]
    for index, bench in enumerate(BENCHES, start=1):
        cpp_score = 1000.0 + index
        rust_score = cpp_score * scale
        cpp_lines.append(
            f"{bench}: score={cpp_score} iters=2 wc=1 times=[1.000,1.000]"
        )
        rust_lines.append(
            f"{bench}: config mode=Interpreter iterations=2 worst_case_count=1"
        )
        rust_lines.append(
            f"{bench}: ok score={rust_score} first=1 worst=1 avg=1"
        )
    return "\n".join(cpp_lines) + "\n", "\n".join(rust_lines) + "\n"


def assert_self_test(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def run_self_tests() -> int:
    cpp_text, rust_text = make_fixture_outputs(scale=0.25)
    rc, rendered = validate_and_render(cpp_text, rust_text)
    assert_self_test(rc == 0, rendered)
    assert_self_test("R 0.25\n" in rendered, rendered)
    assert_self_test(rendered.count("\n") == len(BENCHES) + 2, rendered)

    cpp_bad = cpp_text.replace(
        "crypto: score=1003.0 iters=2 wc=1 times=[1.000,1.000]",
        "crypto: score=1003.0 iters=2 wc=1 times=[1.000,1.000] validation=failed",
    )
    rc, rendered = validate_and_render(cpp_bad, rust_text)
    assert_self_test(rc == 1, rendered)
    assert_self_test("R UNDEFINED" in rendered, rendered)
    assert_self_test("C++ crypto: rejected" in rendered, rendered)

    cpp_bad = cpp_text.replace(
        "Box2D: score=1001.0 iters=2 wc=1 times=[1.000,1.000]",
        "Box2D: throw=TypeError: boom",
    )
    rc, rendered = validate_and_render(cpp_bad, rust_text)
    assert_self_test(rc == 1, rendered)
    assert_self_test("C++ Box2D: rejected" in rendered, rendered)

    rust_bad = rust_text.replace(
        "mandreel: ok score=251.75 first=1 worst=1 avg=1",
        "mandreel: failed phase=Run order_index=None label=None detail=Exception",
    )
    rc, rendered = validate_and_render(cpp_text, rust_bad)
    assert_self_test(rc == 1, rendered)
    assert_self_test("Rust mandreel: rejected" in rendered, rendered)

    rust_bad = rust_text.replace(
        "splay: ok score=253.25 first=1 worst=1 avg=1",
        "splay: TIMEOUT (300s) or signal rc=137",
    )
    rc, rendered = validate_and_render(cpp_text, rust_bad)
    assert_self_test(rc == 1, rendered)
    assert_self_test("Rust splay: rejected" in rendered, rendered)

    rust_bad = "\n".join(
        line for line in rust_text.splitlines() if not line.startswith("pdfjs: ok ")
    )
    rc, rendered = validate_and_render(cpp_text, rust_bad)
    assert_self_test(rc == 1, rendered)
    assert_self_test("Rust pdfjs: missing result" in rendered, rendered)

    rust_bad = rust_text.replace(
        "raytrace: ok score=252.5 first=1 worst=1 avg=1",
        "raytrace: ok score=252.5 first=1 worst=1 avg=1\nraytrace: ok score=252.5 first=1 worst=1 avg=1",
    )
    rc, rendered = validate_and_render(cpp_text, rust_bad)
    assert_self_test(rc == 1, rendered)
    assert_self_test("Rust raytrace: duplicate result line" in rendered, rendered)

    cpp_bad = cpp_text.replace("regexp: score=1011.0", "regexp: score=nan")
    rc, rendered = validate_and_render(cpp_bad, rust_text)
    assert_self_test(rc == 1, rendered)
    assert_self_test("C++ regexp: invalid score" in rendered, rendered)

    print("compute_r.py self-test: ok")
    return 0


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate Octane baseline outputs and compute per-bench r_i plus R.",
    )
    parser.add_argument(
        "cpp_output",
        nargs="?",
        type=Path,
        help="captured output from run_cpp_baseline.sh",
    )
    parser.add_argument(
        "rust_output",
        nargs="?",
        type=Path,
        help="captured output from run_rust_baseline.sh",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run dependency-free fixture tests instead of reading files",
    )
    args = parser.parse_args(argv)
    if args.self_test:
        return args
    if args.cpp_output is None or args.rust_output is None:
        parser.error("CPP_OUTPUT and RUST_OUTPUT are required unless --self-test is used")
    return args


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    if args.self_test:
        return run_self_tests()
    cpp_text = read_text(args.cpp_output)
    rust_text = read_text(args.rust_output)
    rc, rendered = validate_and_render(cpp_text, rust_text)
    stream = sys.stdout if rc == 0 else sys.stderr
    stream.write(rendered)
    return rc


if __name__ == "__main__":
    raise SystemExit(main())
