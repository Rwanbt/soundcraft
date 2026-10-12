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

## Verified incremental fork CI (2026-10-13)

These **independent draft PR checks** are not pooled into one coverage percentage or
assumed to exist on `main`. The Rust quality jobs include Linux formatting,
Clippy and workspace tests plus Windows/macOS matrix validation. Every success
listed here corresponds to the PR's actual HEAD SHA.

| Fork PR | Exact head (short) | Rust quality | LLVM coverage | Additional evidence | Scope |
| --- | --- | --- | --- | --- | --- |
| [#60](https://github.com/Rwanbt/soundcraft/pull/60) | `04efd175` | [success](https://github.com/Rwanbt/soundcraft/actions/runs/38268289274) | [success](https://github.com/Rwanbt/soundcraft/actions/runs/38268289269) | WASM compile | TCP request cap, malformed input and recovery |
| [#61](https://github.com/Rwanbt/soundcraft/pull/61) | `032c6aff` | [success](https://github.com/Rwanbt/soundcraft/actions/runs/38274289850) | [success](https://github.com/Rwanbt/soundcraft/actions/runs/38274290571) | Windows/macOS | Audio conversion and real-process CLI |
| [#62](https://github.com/Rwanbt/soundcraft/pull/62) | `0e55ba68` | [success](https://github.com/Rwanbt/soundcraft/actions/runs/38274218145) | [success](https://github.com/Rwanbt/soundcraft/actions/runs/38274218320) | [Chromium success](https://github.com/Rwanbt/soundcraft/actions/runs/38274218272) | Nonfinite DSP DC correction |
| [#64](https://github.com/Rwanbt/soundcraft/pull/64) | `12e41b97` | [success](https://github.com/Rwanbt/soundcraft/actions/runs/38278108745) | [success](https://github.com/Rwanbt/soundcraft/actions/runs/38278108912) | [Chromium success](https://github.com/Rwanbt/soundcraft/actions/runs/38278108762) | Session save and recovery collision tests |
| [#65](https://github.com/Rwanbt/soundcraft/pull/65) | `2e408332` | [success](https://github.com/Rwanbt/soundcraft/actions/runs/38278237685) | [success](https://github.com/Rwanbt/soundcraft/actions/runs/38278237746) | [Chromium success](https://github.com/Rwanbt/soundcraft/actions/runs/38278237672), [FreeBSD success](https://github.com/Rwanbt/soundcraft/actions/runs/38278237673) | Recording-frame integrity |
| [#66](https://github.com/Rwanbt/soundcraft/pull/66) | `96b7bff9` | [success](https://github.com/Rwanbt/soundcraft/actions/runs/38313570703) | [success](https://github.com/Rwanbt/soundcraft/actions/runs/38313570794) | [Chromium success](https://github.com/Rwanbt/soundcraft/actions/runs/38313570700) | Correlated TCP replies and at-most-once client dispatch |
| [#67](https://github.com/Rwanbt/soundcraft/pull/67) | `741740fc` | [success](https://github.com/Rwanbt/soundcraft/actions/runs/38313734760) | [success](https://github.com/Rwanbt/soundcraft/actions/runs/38313734754) | [Chromium success](https://github.com/Rwanbt/soundcraft/actions/runs/38313734751) | Bounded MCP JSON-RPC input and batch errors |

Additional LLVM snapshots (do not sum them): PR #66 at `96b7bff9` measured
**35,797 / 46,947 lines = 76.25%** globally, with
`crates/automation/src/lib.rs` **90 / 107 lines = 84.11%**.
PR #67 at `741740fc` measured **35,798 / 46,975 lines = 76.21%**
globally, with `crates/automation/src/mcp.rs`
**200 / 231 lines = 86.58%**.
These values are independently instrumented on distinct branches, not a
combined result or a comparable fixed-denominator global trend.

PR #63 is a documentation-only evidence update, without triggered Rust test workflows.
All listed PRs remain **DRAFT**, are based on the unmodified fork `main`
(except explicitly stacked later PRs), and have not been merged. Platform checks
do not establish physical audio device functionality. The baseline coverage
above remains the measured number for PR #60, not an aggregate of these PRs.

## Traceability policy

A feature row is `VERIFIED` only with a documented expected contract, regression oracle, actual
unit/integration/E2E results as applicable, platform/feature coverage, CI run URL and commit SHA.
New functionality added by a PR remains `UNVERIFIED` until checks for its exact HEAD complete.
Existing Windows/macOS job success does **not** establish tests on physical audio interfaces.
A 100% line figure would not imply 100% behavioral certainty.

See [existing test execution and deterministic policy](../testing-and-determinism.md)
and [residual risks](REMAINING-RISKS.md). Avoid treating this snapshot as `main` branch coverage.
