# Fork quality — remaining test and production risks

**Snapshot:** 2026-10-12; applies to `Rwanbt/soundcraft`, starting from `main`
`8b756f2851acfb1d9744237ad71b631f3efb31e8`. This is the **fork quality**
backlog. It complements (and does not replace) [product gaps](../gaps.md).

## Prioritization and missing deterministic evidence

| Priority | Functional contract / defect risk | Available evidence | Missing proof / next action | Gate |
| --- | --- | --- | --- | --- |
| P0 | Record without corruption, take loss or unbounded RAM use | Recorded buffer/transport tests; recorder-fix draft PRs | Long-running disk recording, cancellation, ENOSPC, recovery and actual duplex interface tests | BLOCKED by missing hardware/runtime work |
| P0 | Reopening session restores clips, media and plugin parameters | Session roundtrip tests and multiple draft fixes | Persistent crash/reopen tests with mixed audio, plugins, missing files and interrupted writes | PARTIAL |
| P0 | Reject oversized/malformed TCP control requests and keep request correlation | [PR #60](https://github.com/Rwanbt/soundcraft/pull/60), passing Rust/LLVM CI at `04efd175` | Idle-connection limits, thread exhaustion and end-to-end UI control on all platforms | PARTIAL |
| P1 | CLI conversions preserve sample rate, channel content, bit depth; invalid flags never silently change output | CLI process test baseline; [PR #61](https://github.com/Rwanbt/soundcraft/pull/61) | Verify latest PR HEAD on Linux, macOS, Windows and instrumented coverage; additional formats/atomic export errors | UNVERIFIED pending exact SHA |
| P1 | DSP remains numerically stable and finite-channel DC removal is unbiased | Existing DSP/property tests; [PR #62](https://github.com/Rwanbt/soundcraft/pull/62) | Verify latest PR HEAD; error-path and floating-point-domain matrix for all processors | UNVERIFIED pending exact SHA |
| P1 | Hardware playback uses selected interface/rate/buffer and handles disconnect/reconnect | Synthetic playback tests | Device matrix covering ALSA/PipeWire, CoreAudio, WASAPI/ASIO, drift and dropouts | BLOCKED by hardware and product gap |
| P1 | Desktop UI edit/undo/save/export workflows | Headless egui and control-path tests in draft branches | Native automated window E2E for major workflows and keyboard/focus accessibility | PARTIAL |
| P1 | Web application loads and reacts in real browser at multiple sizes | Chromium smoke workflow and WASM typecheck | Multi-browser input, responsive editing, persistence/export, browser error traces | PARTIAL |
| P1 | Audio/video/MIDI import reject corrupt and hostile files without crash or silent corruption | Fuzz-like deterministic regression suites, proptest; H.264 draft fixes | Coverage-guided fuzz corpora, decoders' size/time ceilings, format roundtrip oracle independent of decoder | PARTIAL |
| P2 | Third-party CLAP/VST3/AU plugin hosts survive failure, state restore and shutdown | Synthetic native plugin fixtures | Curated real-plugin matrix, failure isolation and code-signing/entitlement checks | PARTIAL |
| P2 | Packaging, installers and launch across supported targets | Packaging lint, FreeBSD/ARM build actions | Install/uninstall and launch verification, cross-platform runtime artifacts | PARTIAL |
| P2 | Branch, function and line coverage of all reachable business code | LLVM source coverage snapshots and 70% floor | Increase justified per-component floors and implement missed contract cases, not artificial exclusions | PARTIAL |

## Reproducibility requirements

1. Keep all fork-only PRs **draft** until applicable GitHub Actions are green on their *exact*
   HEAD SHA. A green workflow from an earlier commit or another stacked PR is not evidence for
   a different SHA.
2. Run Rustfmt, Clippy, workspace tests, architecture/asset checks, LLVM LCOV, and Windows/macOS
   tests. Run browser smoke for changes affecting the web app or core shared crates; retain
   native hardware/manual gates where automation is not available.
3. For every production bug fix keep a regression test that fails on the old behavior. Use
   synthetic fixtures, fixed seeds and independent expected outputs.
4. Preserve `main` and upstream, never force push, and do not merge without authorization.
5. Do not declare 100% coverage or `production-ready` from green test counts alone.

See [instrumented per-component snapshot](FEATURE-TEST-MATRIX.md) and the existing
[testing procedures](../testing-and-determinism.md). The roadmap itself labels the product
pre-alpha due to unimplemented hardware recording, MIDI I/O and AAF/OMF exchange.
