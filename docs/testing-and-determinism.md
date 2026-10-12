# Test matrix and deterministic validation

This document distinguishes tested behaviour from aspirational parity. A green unit-test
suite is not evidence that an audio workstation is production-ready or works with every
driver or third-party plugin.

For the fork's package-level **measured coverage matrix**, CI SHA attribution and explicit
remaining-risk gates, see [quality evidence](quality/FEATURE-TEST-MATRIX.md) and
[remaining risks](quality/REMAINING-RISKS.md). These are dated snapshots, not proof of
production readiness or a substitute for the live Actions status.

## Local checks

From the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo xtask assets
cargo xtask layers
rustup target add wasm32-unknown-unknown
cargo xtask wasm
```

The `cargo xtask ci` alias also runs formatting, Clippy, tests, attribution,
architecture and WASM checks. The dedicated CI workflow uses `--locked` for Rust
test/lint commands and runs native Windows/macOS suites in addition to Linux.

## Deterministic tests

- Construct audio, MIDI and session data in code. Avoid system audio devices,
  external media, clocks, nondeterministic thread ordering and network resources.
- Repeat the same offline render at different buffer sizes. Allow documented,
  small floating-point tolerances where bit-exact output is not a contractual
  guarantee.
- For block-boundary MIDI regressions, use tick/sample positions known to cross
  both exact and non-exact buffer boundaries.
- Compare serialized session semantics and the post-reload audio result. Audio
  sample pools are intentionally not serialized with the JSON session; use
  source-free MIDI fixtures or explicitly reattach source buffers when needed.
- CLI end-to-end tests invoke the actual binary in a fresh temporary directory,
  verify process exit status, stderr/stdout and the resulting WAV/session bytes.
- Property tests should retain minimal reproductions in their committed
  `*.proptest-regressions` files. A failure must not be hidden by skipping it.

## Evidence gates

| Gate | Automated evidence | Not established by this gate |
| --- | --- | --- |
| Rust correctness | fmt, Clippy, unit/integration results | Entire GUI behaviour |
| Sound processing | Synthetic signal, MIDI and offline-bounce comparisons | All audio-driver configurations |
| Plugin integration | CLAP/VST3 fixtures, state and lifecycle | Every commercial binary plugin |
| Platform portability | Native Linux, Windows x64 and macOS suites; WASM type checking | Native Windows ARM64, FreeBSD GUI, browsers |
| UI commands | Headless egui keyboard, docking and interactions | Real browser or desktop window E2E |
| Coverage | LLVM LCOV from instrumented Rust workspace tests | Behavioural completeness |

## Coverage policy

The [first successful LLVM coverage run](https://github.com/Rwanbt/soundcraft/actions/runs/38152950247)
on 2026-10-11 establishes a **measured Linux workspace baseline**:

| Scope | Covered source lines |
| --- | ---: |
| Whole workspace | **74.66%** (34,478 / 46,181) |
| `crates/mix/src/lib.rs` | 85.60% |
| `crates/engine/src/io.rs` | 73.39% |
| `crates/playback/src/lib.rs` | 54.56% |
| `crates/playback/src/record.rs` | 57.31% |
| `crates/vst3-host/src/ffi.rs` | 82.36% |
| `apps/soundcraft-cli/src/main.rs` | 13.04% |
| `apps/soundcraft/src/control_server.rs` | 0.00% |

The separate real-process CLI test PR has **not yet been merged into this
coverage branch**; the coverage numbers are a baseline, not a claim about
the combined changes. A conservative **70% workspace line-coverage floor**
is enforced from the LCOV record counts to prevent major regressions while
allowing independent new functionality. Do not treat this floor as a
claim of adequate coverage for recording or hardware paths.

Prioritize tests for the low-coverage playback, recording, executable CLI
and control-server code. Adopt higher per-module floors only after
explicitly testing their missed critical paths. Coverage is supporting
evidence, not a proxy for correct output.

## Manual / hardware gates still required

Test device selection, duplex recording, low-latency streaming, buffer-size
changes, unplug/replug, MIDI input/output, Windows ASIO/WASAPI, CoreAudio,
Linux ALSA/PipeWire, actual CLAP/VST3/AU binaries, graphics driver fallback,
and end-to-end web interaction with WebGPU/WebGL2. These cannot be established
by headless Rust tests alone.

If a workflow fails on one platform, preserve its logs and add a targeted
regression instead of removing the platform or disabling the test.
