# Test matrix and deterministic validation

This document distinguishes tested behaviour from aspirational parity. A green unit-test
suite is not evidence that an audio workstation is production-ready or works with every
driver or third-party plugin.

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

Use the `Rust coverage evidence` workflow for an LCOV artefact and a measurable
baseline; do not claim a percentage from counting test files. Adopt separate
per-crate coverage floors only after reviewing uncovered critical paths,
especially `mix`, `engine`, `playback`, `audio-io` and plugin hosts.
Coverage is supporting evidence, not a proxy for correct output.

## Manual / hardware gates still required

Test device selection, duplex recording, low-latency streaming, buffer-size
changes, unplug/replug, MIDI input/output, Windows ASIO/WASAPI, CoreAudio,
Linux ALSA/PipeWire, actual CLAP/VST3/AU binaries, graphics driver fallback,
and end-to-end web interaction with WebGPU/WebGL2. These cannot be established
by headless Rust tests alone.

If a workflow fails on one platform, preserve its logs and add a targeted
regression instead of removing the platform or disabling the test.
