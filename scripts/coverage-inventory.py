#!/usr/bin/env python3
"""Publish exact coverage gaps for EVERY instrumented SoundCraft source file.

This is an inventory, not a fake assertion that everything is tested. The separate
check-coverage.py gate rejects missing or corrupt evidence and enforces minimum floors.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import runpy

SOURCE = runpy.run_path(str(Path(__file__).with_name("check-coverage.py")))
read_lcov = SOURCE["read_lcov"]


def relative_path(source: str) -> str:
    name = source.replace("\\", "/")
    for prefix in ("crates/", "apps/", "xtask/"):
        if name.startswith(prefix):
            return name
        i = name.find("/" + prefix)
        if i >= 0:
            return name[i + 1 :]
    return name


def component(path: str) -> str:
    parts = path.split("/")
    return "/".join(parts[:2]) if parts[0] in ("crates", "apps") and len(parts) > 1 else parts[0]


def generate(coverage_path: Path) -> str:
    raw = read_lcov(coverage_path)
    entries: dict[str, tuple[int, int]] = {}
    for source, counts in raw.items():
        name = relative_path(source)
        if name in entries:
            raise ValueError(f"source recorded twice after path normalization: {name}")
        entries[name] = counts
    groups: dict[str, tuple[int, int]] = {}
    total_lines = total_hit = 0
    for name, (lines, hit) in entries.items():
        total_lines += lines
        total_hit += hit
        group = component(name)
        old_lines, old_hit = groups.get(group, (0, 0))
        groups[group] = (old_lines + lines, old_hit + hit)
    if total_lines <= 0:
        raise ValueError("no instrumented source lines")
    pct = total_hit / total_lines * 100
    out = [
        "# SoundCraft: complete LLVM line-coverage inventory",
        "",
        "Generated from the current CI commit's instrumented LCOV evidence.",
        "This is **not** proof of behavioral correctness, memory safety, hardware compatibility or 100% execution paths.",
        "",
        f"**Workspace: {total_hit:,}/{total_lines:,} lines ({pct:.2f}%), {total_lines-total_hit:,} lines not executed.**",
        f"**Source files measured: {len(entries)}; components measured: {len(groups)}.**",
        "",
        "## Components — largest uncovered counts first",
        "",
        "| Component | Covered / total lines | Line coverage | Uncovered lines |",
        "|---|---:|---:|---:|",
    ]
    for name, (lines, hit) in sorted(groups.items(), key=lambda e: (e[1][0] - e[1][1], e[0]), reverse=True):
        score = (hit / lines * 100) if lines else 0.0
        out.append(f"| `{name}` | {hit:,} / {lines:,} | {score:.2f}% | {lines-hit:,} |")
    out += [
        "",
        "## Every instrumented source file — largest uncovered counts first",
        "",
        "| File | Covered / total lines | Coverage | Uncovered lines |",
        "|---|---:|---:|---:|",
    ]
    for name, (lines, hit) in sorted(entries.items(), key=lambda e: (e[1][0] - e[1][1], e[0]), reverse=True):
        score = (hit / lines * 100) if lines else 0.0
        out.append(f"| `{name}` | {hit:,} / {lines:,} | {score:.2f}% | {lines-hit:,} |")
    out += [
        "",
        "## Interpretation",
        "",
        "- This report includes source code compiled and instrumented by Linux `cargo llvm-cov --workspace --locked`; missing uncompiled platform-specific code requires its own OS-native coverage evidence.",
        "- A 0% entry for a small wrapper can mean the harness never starts that process; OS/browser smoke tests may separately exercise it without contributing to these counters.",
        "- Prioritize critical runtime paths and failing behaviors over pursuing line coverage with assertion-free test calls.",
        "- A file with 100% of its lines run may still contain incorrect branch behavior, races, missed deadlines, invalid audio or unsafe plugin interaction.",
        "",
    ]
    return "\n".join(out)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("lcov", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    report = generate(args.lcov)
    args.output.write_text(report, encoding="utf-8")
    print("\n".join(report.splitlines()[:6]))
    print(f"Detailed per-component and per-file inventory written to {args.output}")


if __name__ == "__main__":
    main()
