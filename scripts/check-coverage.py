#!/usr/bin/env python3
"""Strict LCOV audit: make coverage holes visible and stop critical-module regressions.

Usage: python3 scripts/check-coverage.py coverage.lcov
Run after cargo llvm-cov --workspace --locked --lcov --output-path coverage.lcov.
This is a *measured minimum* gate, not a claim of 100% coverage or correctness.
"""

from __future__ import annotations

import argparse
from pathlib import Path

# Floors are intentionally <= the verified combined baseline measured on 2026-10-11.
# Tighten them after new tests pass; do not lower them to make a failure disappear.
FLOORS = {
    "apps/soundcraft-cli/src/main.rs": 45.0,
    "apps/soundcraft/src/control_server.rs": 65.0,
    "crates/engine/src/io.rs": 70.0,
    "crates/mix/src/lib.rs": 85.0,
    "crates/playback/src/lib.rs": 60.0,
    "crates/playback/src/record.rs": 80.0,
    "crates/ui-egui/src/control.rs": 12.0,
    "crates/ui-egui/src/edit_window.rs": 50.0,
    "crates/ui-egui/src/mix_window.rs": 40.0,
}
WORKSPACE_FLOOR = 75.0


def read_lcov(path: Path) -> dict[str, tuple[int, int]]:
    if not path.is_file() or path.stat().st_size == 0:
        raise ValueError(f"missing or empty LCOV evidence: {path}")
    entries: dict[str, tuple[int, int]] = {}
    source: str | None = None
    found = hit = None
    for line_number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if raw.startswith("SF:"):
            if source is not None:
                raise ValueError(f"record not terminated before line {line_number}")
            source, found, hit = raw[3:].replace("\\", "/"), None, None
        elif raw.startswith("LF:") or raw.startswith("LH:"):
            if source is None:
                raise ValueError(f"coverage count outside source record at line {line_number}")
            value = int(raw[3:])
            if raw.startswith("LF:"):
                if found is not None:
                    raise ValueError(f"duplicate LF count in {source}")
                found = value
            else:
                if hit is not None:
                    raise ValueError(f"duplicate LH count in {source}")
                hit = value
        elif raw == "end_of_record":
            if source is None or found is None or hit is None:
                raise ValueError(f"incomplete LCOV entry at line {line_number}")
            if found < 0 or hit < 0 or hit > found:
                raise ValueError(f"impossible coverage count in {source}: {hit}/{found}")
            # All targets are relative to workspace root; cargo-llvm-cov may
            # serialize them with an absolute checkout prefix.
            matches = [key for key in FLOORS if source == key or source.endswith("/" + key)]
            key = matches[0] if matches else source
            if key in entries:
                raise ValueError(f"duplicate source record {key}; refuse ambiguous counts")
            entries[key] = (found, hit)
            source, found, hit = None, None, None
    if source is not None:
        raise ValueError("truncated LCOV data without end_of_record")
    if not entries:
        raise ValueError("LCOV evidence contains no source records")
    return entries


def audit(path: Path) -> int:
    entries = read_lcov(path)
    lines = sum(found for found, _ in entries.values())
    covered = sum(hit for _, hit in entries.values())
    if lines == 0:
        raise ValueError("LCOV evidence contains zero instrumented lines")
    actual = covered * 100 / lines
    failures: list[str] = []
    print(f"Workspace: {covered}/{lines} lines = {actual:.2f}% (floor {WORKSPACE_FLOOR:.2f}%, goal 100%)")
    print(f"Uncovered instrumented lines: {lines - covered}")
    if actual < WORKSPACE_FLOOR:
        failures.append(f"workspace: {actual:.2f}% below {WORKSPACE_FLOOR:.2f}%")
    for name, floor in sorted(FLOORS.items()):
        if name not in entries:
            failures.append(f"required module is missing from coverage: {name}")
            continue
        count, hit = entries[name]
        if count == 0:
            failures.append(f"no instrumented lines for required module: {name}")
            continue
        percent = hit * 100 / count
        print(f"{name}: {hit}/{count} = {percent:.2f}% (floor {floor:.2f}%, uncovered {count - hit})")
        if percent < floor:
            failures.append(f"{name}: {percent:.2f}% below {floor:.2f}%")
    if failures:
        print("FAIL: " + "; ".join(failures))
        return 1
    print("PASS: minimum evidence floors met; the 100% goal is NOT yet met.")
    return 0


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("lcov", type=Path)
    args = parser.parse_args()
    try:
        raise SystemExit(audit(args.lcov))
    except (ValueError, OSError) as error:
        parser.exit(2, f"Invalid coverage evidence: {error}\n")
