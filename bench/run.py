#!/usr/bin/env python3
"""The comparison: Rust, JavaScript and Go, doing the same work.

Three things are measured, and each one is measured against a check that
the three implementations really are doing the same work:

  * the SQL builder — the same query turned into the same statement;
  * the in-memory engine — the same ten thousand documents, filtered,
    sorted, grouped and updated;
  * the command line — how long it takes to start and print a line, and
    how much memory the process held while it did.

The check comes first. If the three did not agree on the answer, a
timing comparison would be comparing different work, and the number
would be worse than useless.

    ./run.py                 the whole thing
    ./run.py --quick         fewer iterations, for a fast look
    ./run.py --rounds 5      more rounds, for a steadier reading
    ./run.py --out table.md  write the tables to a file as well

Every timing is best-of-N: the machine is shared, and the slowest
reading says more about the neighbours than about the code.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BENCH = ROOT / "bench"
GO_DIR = BENCH / "go"

# Where the toolchains live when they are not on the path.
CANDIDATE_CARGO = [os.environ.get("CARGO_HOME", ""), str(Path.home() / "rust/prefix/bin")]
CANDIDATE_GO = [os.environ.get("GOROOT", ""), "/tmp/goinstall/go", "/usr/local/go"]


def find_tool(name: str, candidates: list[str] | None = None) -> str | None:
    """The tool, on the path or in one of the places it usually is."""
    found = shutil.which(name)
    if found:
        return found
    for candidate in candidates or []:
        if not candidate:
            continue
        path = Path(candidate) / name
        if path.exists():
            return str(path)
    return None


def run(cmd: list[str], cwd: Path | None = None, env: dict | None = None) -> str:
    """Run a command and return its output, or fail loudly."""
    merged = dict(os.environ)
    merged.update(env or {})
    result = subprocess.run(
        cmd, cwd=cwd, env=merged, capture_output=True, text=True, timeout=1800
    )
    if result.returncode != 0:
        sys.stderr.write(result.stdout)
        sys.stderr.write(result.stderr)
        raise SystemExit(f"failed: {' '.join(cmd)}")
    return result.stdout


def check(cmd: list[str], cwd: Path | None = None, env: dict | None = None) -> tuple[bool, str]:
    """Run a command and report whether it agreed with what it was given."""
    result = subprocess.run(
        cmd, cwd=cwd, env={**os.environ, **(env or {})}, capture_output=True, text=True
    )
    return result.returncode == 0, (result.stdout + result.stderr)


def parse_ops(output: str) -> dict[str, float]:
    """Read `label ns/op` lines into a dictionary."""
    ops: dict[str, float] = {}
    for line in output.splitlines():
        match = re.match(r"^\s*(\S+)\s+([0-9.]+)\s*ns/op", line)
        if match:
            ops[match.group(1)] = float(match.group(2))
    return ops


def parse_rss(stderr: str) -> int:
    """Read `rss_kb=` out of the measurement helper's output."""
    match = re.search(r"rss_kb=(\d+)", stderr)
    return int(match.group(1)) if match else 0


def measure_rss(cmd: list[str], cwd: Path | None = None) -> int:
    """The peak resident set size of a command, in KiB."""
    result = subprocess.run(
        [sys.executable, str(BENCH / "rss.py"), *cmd],
        cwd=cwd,
        capture_output=True,
        text=True,
    )
    return parse_rss(result.stderr)


def table(headers: list[str], rows: list[list[str]]) -> str:
    """A markdown table."""
    lines = ["| " + " | ".join(headers) + " |"]
    lines.append("| " + " | ".join("---" for _ in headers) + " |")
    for row in rows:
        lines.append("| " + " | ".join(row) + " |")
    return "\n".join(lines)


def ratio(rust_ns: float, other_ns: float) -> str:
    """How many times faster one is than the other, in words."""
    if other_ns <= 0 or rust_ns <= 0:
        return "—"
    factor = other_ns / rust_ns
    return f"{factor:.1f}× faster"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--quick", action="store_true", help="fewer iterations")
    parser.add_argument("--rounds", type=int, default=3, help="best-of-N rounds")
    parser.add_argument("--out", type=Path, help="also write the tables here")
    args = parser.parse_args()

    cargo = find_tool("cargo", CANDIDATE_CARGO)
    node = find_tool("node")
    go = find_tool("go", CANDIDATE_GO)
    if not cargo:
        raise SystemExit("cargo is not on the path and was not found in the usual places")
    if not node:
        raise SystemExit("node is not on the path")
    if not go:
        raise SystemExit(
            "go is not on the path and was not found in the usual places.\n"
            "The Go baseline needs a Go toolchain; the other two do not."
        )

    rounds = str(args.rounds)
    n = "4000" if args.quick else "20000"
    out: list[str] = []

    def emit(text: str) -> None:
        print(text)
        out.append(text)

    emit("# Rust, JavaScript and Go, doing the same work\n")
    emit(
        f"Rust {run([cargo, '--version']).strip()} · "
        f"Node {run([node, '--version']).strip()} · "
        f"{run([go, 'version']).strip()}\n"
    )

    # ------------------------------------------------------------ build --
    emit("## Building\n")
    run([cargo, "build", "--release", "--bin", "bench", "--bin", "mem-bench"], cwd=ROOT / "rust")
    run([cargo, "build", "--release", "--bin", "nanobase"], cwd=ROOT / "nanobase")
    go_env = {"GOTOOLCHAIN": "local", "GOCACHE": os.environ.get("GOCACHE", "/tmp/gocache")}
    run([go, "build", "-o", str(BENCH / "bin/sqlbench"), "./sqlbench"], cwd=GO_DIR, env=go_env)
    run([go, "build", "-o", str(BENCH / "bin/membench"), "./membench"], cwd=GO_DIR, env=go_env)
    run([go, "build", "-o", str(BENCH / "bin/cli"), "./cli"], cwd=GO_DIR, env=go_env)
    emit("Built the Rust binaries, the Go baselines and the command line.\n")

    # ----------------------------------------------------------- parity --
    emit("## The check: do the three agree?\n")
    rust_sql = run([str(ROOT / "rust/target/release/bench")], cwd=ROOT / "rust", env={"PRINT": "1", "N": "1", "ROUNDS": "1"})
    js_sql = run([node, str(ROOT / "rust/bench/node-bench.mjs")], env={"PRINT": "1", "N": "1", "ROUNDS": "1"})
    go_sql = run([str(BENCH / "bin/sqlbench")], env={"PRINT": "1", "N": "1", "ROUNDS": "1"})

    def statements(text: str) -> list[str]:
        return [line for line in text.splitlines() if line.startswith(("SELECT", "UPDATE", "INSERT"))]

    rust_lines, js_lines, go_lines = statements(rust_sql), statements(js_sql), statements(go_sql)
    sql_same = rust_lines[:3] == js_lines[:3] == go_lines[:3]
    emit(
        "**The SQL builder** — the same query in all three, compared "
        "statement by statement:\n"
    )
    if sql_same:
        emit("```\n" + rust_lines[0] + "\n```\n")
        emit("Identical in Rust, JavaScript and Go. ✓\n")
    else:
        emit("```\nRust: " + (rust_lines[0] if rust_lines else "?") + "\n")
        emit("JS:   " + (js_lines[0] if js_lines else "?") + "\n")
        emit("Go:   " + (go_lines[0] if go_lines else "?") + "\n```\n")
        emit("**The three do not agree.** The timings below would be meaningless.\n")

    rust_check = run(
        [str(ROOT / "rust/target/release/mem-bench")], cwd=ROOT / "rust", env={"CHECK": "1", "ROUNDS": "1"}
    )
    js_check = run([node, str(ROOT / "rust/bench/node-mem-bench.mjs")], env={"CHECK": "1", "ROUNDS": "1"})
    go_check = run([str(BENCH / "bin/membench")], env={"CHECK": "1", "ROUNDS": "1"})

    def check_line(text: str) -> str:
        for line in text.splitlines():
            if line.startswith("CHECK "):
                return line
        return "CHECK (missing)"

    rust_check_line = check_line(rust_check)
    js_check_line = check_line(js_check)
    go_check_line = check_line(go_check)
    engine_same = rust_check_line == js_check_line == go_check_line
    emit("**The in-memory engine** — ten thousand documents, filtered, grouped and updated:\n")
    emit("```\n" + rust_check_line + "\n```\n")
    if engine_same:
        emit("Identical in Rust, JavaScript and Go. ✓\n")
    else:
        emit(f"Rust: {rust_check_line}\nJS:   {js_check_line}\nGo:   {go_check_line}\n")
        emit("**The three do not agree.** The timings below would be meaningless.\n")

    # ---------------------------------------------------------- builder --
    emit("## The SQL builder\n")
    emit(f"{n} iterations, best of {rounds}.\n")
    rust_ops = parse_ops(
        run([str(ROOT / "rust/target/release/bench")], cwd=ROOT / "rust", env={"N": n, "ROUNDS": rounds})
    )
    js_ops = parse_ops(run([node, str(ROOT / "rust/bench/node-bench.mjs")], env={"N": n, "ROUNDS": rounds}))
    go_ops = parse_ops(run([str(BENCH / "bin/sqlbench")], env={"N": n, "ROUNDS": rounds}))
    rows = []
    for label in ["find", "update", "aggregate", "insert50"]:
        if label not in rust_ops:
            continue
        rows.append(
            [
                label,
                f"{rust_ops[label]:.0f} ns",
                f"{js_ops.get(label, 0):.0f} ns",
                f"{go_ops.get(label, 0):.0f} ns",
                ratio(rust_ops[label], js_ops.get(label, 0)),
                ratio(rust_ops[label], go_ops.get(label, 0)),
            ]
        )
    emit(
        table(
            ["case", "Rust", "JavaScript", "Go", "Rust vs JS", "Rust vs Go"],
            rows,
        )
        + "\n"
    )

    # ------------------------------------------------------------ engine --
    emit("## The in-memory engine\n")
    emit(f"Ten thousand documents. Best of {rounds}.\n")
    rust_mem = parse_ops(
        run([str(ROOT / "rust/target/release/mem-bench")], cwd=ROOT / "rust", env={"ROUNDS": rounds})
    )
    js_mem = parse_ops(run([node, str(ROOT / "rust/bench/node-mem-bench.mjs")], env={"ROUNDS": rounds}))
    go_mem = parse_ops(run([str(BENCH / "bin/membench")], env={"ROUNDS": rounds}))
    rows = []
    for label in ["find", "find+sort", "aggregate", "updateMany", "insert1"]:
        if label not in rust_mem:
            continue
        rows.append(
            [
                label,
                f"{rust_mem[label]:,.0f} ns",
                f"{js_mem.get(label, 0):,.0f} ns",
                f"{go_mem.get(label, 0):,.0f} ns",
                ratio(rust_mem[label], js_mem.get(label, 0)),
                ratio(rust_mem[label], go_mem.get(label, 0)),
            ]
        )
    emit(
        table(
            ["case", "Rust", "JavaScript", "Go", "Rust vs JS", "Rust vs Go"],
            rows,
        )
        + "\n"
    )

    # ------------------------------------------------------------ memory --
    emit("## Memory\n")
    emit("The peak resident set size — the most the process held at once.\n")
    rows = []
    for label, cmd, cwd in [
        ("SQL builder (release binary)", [str(ROOT / "rust/target/release/bench")], ROOT / "rust"),
        ("in-memory engine (release binary)", [str(ROOT / "rust/target/release/mem-bench")], ROOT / "rust"),
        ("SQL builder (Node)", [node, str(ROOT / "rust/bench/node-bench.mjs")], None),
        ("in-memory engine (Node)", [node, str(ROOT / "rust/bench/node-mem-bench.mjs")], None),
        ("SQL builder (Go)", [str(BENCH / "bin/sqlbench")], None),
        ("in-memory engine (Go)", [str(BENCH / "bin/membench")], None),
    ]:
        env = {"N": n, "ROUNDS": "1"}
        result = subprocess.run(
            [sys.executable, str(BENCH / "rss.py"), *cmd],
            cwd=cwd,
            capture_output=True,
            text=True,
            env={**os.environ, **env},
        )
        kb = parse_rss(result.stderr)
        rows.append([label, f"{kb:,} KiB", f"{kb / 1024:.1f} MiB"])
    emit(table(["what", "peak RSS", ""], rows) + "\n")

    # -------------------------------------------------------------- cli --
    emit("## The command line\n")
    emit(
        "The floor under every other command: how long it takes to start, "
        "read its arguments and print a line.\n"
    )
    nanobase = str(ROOT / "nanobase/target/release/nanobase")
    cli_cases = [
        ("nanobase --version", [nanobase, "--version"], None),
        ("Go equivalent", [str(BENCH / "bin/cli"), "--version"], None),
        ("node --version", [node, "--version"], None),
    ]
    rows = []
    for label, cmd, cwd in cli_cases:
        times = []
        for _ in range(30):
            start = time.perf_counter()
            subprocess.run(cmd, cwd=cwd, capture_output=True)
            times.append((time.perf_counter() - start) * 1000)
        kb = measure_rss(cmd, cwd)
        rows.append(
            [
                label,
                f"{min(times):.1f} ms",
                f"{statistics.median(times):.1f} ms",
                f"{kb:,} KiB",
                f"{kb / 1024:.1f} MiB",
            ]
        )
    emit(table(["command", "best", "median", "peak RSS", ""], rows) + "\n")

    text = "\n".join(out) + "\n"
    if args.out:
        args.out.write_text(text)
        print(f"\nWritten to {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
