# Fork quality — instrumented coverage and verification matrix

**Snapshot date:** 2026-10-12. **Repository:** `Rwanbt/soundcraft`. **Reference branch:** `main` at `8b756f2851acfb1d9744237ad71b631f3efb31e8`.

## Reproducible evidence

This is a measured **PR snapshot**, not a claim of full coverage on `main`. Results are from
[`fix/control-server-request-size-limit` commit `04efd175dfab1fb4d70dc4fddeddfb605cd978ee`](https://github.com/Rwanbt/soundcraft/pull/60)
in [LLVM coverage run 38268289269](https://github.com/Rwanbt/soundcraft/actions/runs/38268289269).
The companion [Rust quality run 38268289274](https://github.com/Rwanbt/soundcraft/actions/runs/38268289274)
passed Linux fmt, Clippy, workspace tests, attribution and layers, native Windows/macOS tests, and WASM compilation.
Its Linux test log reported **598 passed, 0 failed, 1 ignored** across 58 suite results.
Coverage is instrumented **Rust source lines**; functions and branches are different denominators and
are not represented by the percentages below. The LCOV run measured **35,815 / 47,018 lines (76.17%)**.

The rows below sum the exact `lines` and `missed` counts from each file's LLVM report by
package directory. They are **not** a merger of incomparable languages, branches or runs.
All package-wide functional states are `PARTIAL`: execution of tests does not establish
every external contract, platform interaction or edge case.

| Component / crate | Covered / total lines | Lines % | Functional state | Primary evidence / residual |
| --- | ---: | ---: | --- | --- |
| `apps/soundcraft-web` | 0 / 3 | 0.00% | PARTIAL | Browser checks are separate; unit LCOV does not measure browser use |
| `apps/soundcraft-cli` | 151 / 322 | 46.89% | PARTIAL | CLI real-process suite; conversion and failures still under review in PR #61 |
| `apps/soundcraft` | 880 / 1,147 | 76.72% | PARTIAL | Control protocol, process startup and graphics/platform paths |
| `crates/au-host` | 165 / 185 | 89.19% | PARTIAL | AU runtime/hardware/macOS plugins not exhaustively certified |
| `crates/audio-io` | 1,297 / 1,411 | 91.92% | PARTIAL | Synthetic roundtrips, hostile-format inputs; missing codec/device cases |
| `crates/automation` | 200 / 286 | 69.93% | PARTIAL | MCP, remote errors/timeouts and disconnect sequences |
| `crates/clap-host` | 1,223 / 1,443 | 84.75% | PARTIAL | Fixture lifecycle; real third-party plugins remain an external gate |
| `crates/dsp` | 3,615 / 3,839 | 94.17% | PARTIAL | DSP invariants; nonfinite DC-centering correction in PR #62 |
| `crates/engine` | 9,488 / 10,613 | 89.40% | PARTIAL | Command/undo/state persistence; broader session corruption tests needed |
| `crates/midi` | 587 / 599 | 98.00% | PARTIAL | SMF parse/serialize and edit operations; no hardware MIDI integration |
| `crates/mix` | 1,555 / 1,798 | 86.48% | PARTIAL | Synthetic renders; missing real plugin parity/hardware measurements |
| `crates/model` | 1,257 / 1,433 | 87.72% | PARTIAL | Data invariants; schema evolution/migrations require review |
| `crates/playback` | 672 / 986 | 68.15% | PARTIAL | Audio device, recording and lifecycle testing have environment gaps |
| `crates/time` | 628 / 696 | 90.23% | PARTIAL | Math and boundary cases; all-rate and negative-time transitions |
| `crates/ui-egui` | 4,558 / 9,098 | 50.10% | PARTIAL | Interactive windows, control UI and native event loop |
| `crates/video` | 7,586 / 10,736 | 70.66% | PARTIAL | H.264/ProRes malformed input, reference frames and state transitions |
| `crates/vst3-host` | 1,797 / 2,080 | 86.39% | PARTIAL | Synthetic fixtures; native plugin-host integration |
| `xtask` | 156 / 343 | 45.48% | PARTIAL | Packaging, code generation, asset checks and CLI handlers |
| **Total** | **35,815 / 47,018** | **76.17%** | **PARTIAL** | **LCOV from this exact run** |

## Traceability policy

A feature row is `VERIFIED` only with a documented expected contract, regression oracle, actual
unit/integration/E2E results as applicable, platform/feature coverage, CI run URL and commit SHA.
New functionality added by a PR remains `UNVERIFIED` until checks for its exact HEAD complete.
Existing Windows/macOS job success does **not** establish tests on physical audio interfaces.
A 100% line figure would not imply 100% behavioral certainty.

See [existing test execution and deterministic policy](../testing-and-determinism.md)
and [residual risks](REMAINING-RISKS.md). Avoid treating this snapshot as `main` branch coverage.
