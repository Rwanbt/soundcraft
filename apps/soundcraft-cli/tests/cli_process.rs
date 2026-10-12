//! Exercise the installed CLI binary through OS processes, not private command helpers.
//! Test data is generated in per-test temp directories; no DAW, device or network required.
use soundcraft_audio_io::{AudioBuffer, BitDepth, EncodeOptions, FileFormat, SampleFormat, decode, encode, probe};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("soundcraft-cli-e2e-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_soundcraft-cli")).env("SOUNDCRAFT_NO_PREFS", "1").args(args).output().unwrap()
}

fn utf8(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn path_text(path: &Path) -> &str {
    path.to_str().expect("temporary test paths must be UTF-8")
}

#[test]
fn cli_session_save_and_open_round_trip() {
    let dir = Scratch::new();
    let session = dir.path("qa-session.scraft");
    let output = cli(&["run", "--cmd", r#"track.new={"name":"QA Voice"}"#, "--save", path_text(&session)]);
    assert!(output.status.success(), "save: {}", utf8(&output.stderr));
    assert!(session.is_file(), "CLI did not write the session file");

    let info = cli(&["info", path_text(&session)]);
    assert!(info.status.success(), "open: {}", utf8(&info.stderr));
    assert!(utf8(&info.stdout).contains("QA Voice"), "track name lost after save/reopen");

    let reopened = cli(&["run", "--in", path_text(&session), "--inspect"]);
    assert!(reopened.status.success(), "inspect: {}", utf8(&reopened.stderr));
    assert!(utf8(&reopened.stdout).contains("QA Voice"));
}

#[test]
fn cli_short_demo_bounce_produces_valid_wav_header() {
    let dir = Scratch::new();
    let wav = dir.path("short-bounce.wav");
    let out = cli(&["run", "--demo", "--bounce", path_text(&wav), "--start", "0", "--end", "0.05"]);
    assert!(out.status.success(), "bounce: {}", utf8(&out.stderr));
    let bytes = std::fs::read(&wav).unwrap();
    assert!(bytes.len() > 44, "bounce must contain samples");
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
}

#[test]
fn cli_rejects_bad_json_without_writing_a_session() {
    let dir = Scratch::new();
    let session = dir.path("should-not-exist.scraft");
    let out = cli(&["run", "--cmd", "track.new={bad json", "--save", path_text(&session)]);
    assert!(!out.status.success());
    assert!(utf8(&out.stderr).contains("bad JSON"));
    assert!(!session.exists(), "failed command must not save a partial session");
}

#[test]
fn cli_script_failure_reports_the_line_and_skips_final_save() {
    let dir = Scratch::new();
    let script = dir.path("commands.txt");
    let session = dir.path("should-not-exist.scraft");
    std::fs::write(&script, "track.new {\"name\":\"Voice\"}\ntrack.new {invalid}\n").unwrap();
    let out = cli(&["script", path_text(&script), "--save", path_text(&session)]);
    assert!(!out.status.success());
    assert!(utf8(&out.stderr).contains(":2: bad JSON"), "{}", utf8(&out.stderr));
    assert!(!session.exists(), "failed script must not commit the final output");
}

fn fixture_wav(dir: &Scratch) -> PathBuf {
    let path = dir.path("stereo-input.wav");
    let input = AudioBuffer { sample_rate: 48_000, channels: vec![vec![0.25; 4800], vec![-0.125; 4800]] };
    let bytes = encode(&input, &EncodeOptions { format: FileFormat::Wav, bit_depth: BitDepth::Int16, dither: false, bwf: None }).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn cli_info_reads_real_audio_header_and_convert_resamples_two_channels() {
    let dir = Scratch::new();
    let input = fixture_wav(&dir);
    let info = cli(&["info", path_text(&input)]);
    assert!(info.status.success(), "info: {}", utf8(&info.stderr));
    let metadata: serde_json::Value = serde_json::from_slice(&info.stdout).unwrap();
    assert_eq!(metadata["sample_rate"], 48_000);
    assert_eq!(metadata["channels"], 2);
    assert_eq!(metadata["frames"], 4800);
    assert_eq!(metadata["sample_format"], "Int16");

    let output = dir.path("converted.aiff");
    let result = cli(&["convert", path_text(&input), path_text(&output), "--rate", "24000", "--bit-depth", "16"]);
    assert!(result.status.success(), "conversion: {}", utf8(&result.stderr));
    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(&bytes[0..4], b"FORM");
    let info = probe(&bytes, None).unwrap();
    assert_eq!(info.format, FileFormat::Aiff);
    assert_eq!(info.sample_format, SampleFormat::Int16);
    assert_eq!((info.sample_rate, info.channels, info.frames), (24_000, 2, 2400));
    let (_, samples) = decode(&bytes, None).unwrap();
    assert_eq!(samples.frames(), 2400);
    assert!((samples.channels[0][1200] - 0.25).abs() < 0.005);
    assert!((samples.channels[1][1200] + 0.125).abs() < 0.005);
}

#[test]
fn cli_convert_normalizes_real_audio_and_writes_float32_wav() {
    let dir = Scratch::new();
    let input = fixture_wav(&dir);
    let output = dir.path("normalized.wav");
    let result = cli(&["convert", path_text(&input), path_text(&output), "--normalize", "--bit-depth", "32f"]);
    assert!(result.status.success(), "normalize: {}", utf8(&result.stderr));
    let bytes = std::fs::read(&output).unwrap();
    let info = probe(&bytes, None).unwrap();
    assert_eq!(info.sample_format, SampleFormat::Float32);
    assert_eq!((info.sample_rate, info.channels, info.frames), (48_000, 2, 4800));
    let (_, samples) = decode(&bytes, None).unwrap();
    assert!((0.985..=0.995).contains(&samples.peak()), "expected -0.1 dBFS, got {}", samples.peak());
    assert!((samples.channels[1][2400] + samples.channels[0][2400] / 2.0).abs() < 0.001);
}

#[test]
fn cli_rejects_invalid_conversion_flags_without_creating_files() {
    let dir = Scratch::new();
    let input = fixture_wav(&dir);
    let cases: &[(&[&str], &str)] = &[
        (&["--rate", "0"], "--rate"),
        (&["--rate", "768001"], "--rate"),
        (&["--rate", "nonsense"], "--rate"),
        (&["--rate", "4294967296"], "--rate"),
        (&["--rate"], "--rate"),
        (&["--bit-depth", "32bit"], "--bit-depth"),
        (&["--bit-depth"], "--bit-depth"),
    ];
    for (i, &(flags, expected_error)) in cases.iter().enumerate() {
        let output = dir.path(&format!("invalid-{i}.wav"));
        let mut arguments = vec!["convert", path_text(&input), path_text(&output)];
        arguments.extend_from_slice(flags);
        let result = cli(&arguments);
        assert!(!result.status.success(), "invalid options {flags:?} were accepted");
        assert!(utf8(&result.stderr).contains(expected_error), "{}", utf8(&result.stderr));
        assert!(!output.exists(), "invalid options {flags:?} created {}", output.display());
    }
}

#[test]
fn cli_convert_rejects_bad_input_and_unsupported_output_without_files() {
    let dir = Scratch::new();
    let input = fixture_wav(&dir);
    let unsupported = dir.path("unsupported.mp3");
    let result = cli(&["convert", path_text(&input), path_text(&unsupported)]);
    assert!(!result.status.success());
    assert!(utf8(&result.stderr).contains("cannot write"), "{}", utf8(&result.stderr));
    assert!(!unsupported.exists());

    let corrupt = dir.path("corrupt.wav");
    std::fs::write(&corrupt, b"RIFF\x00\x00").unwrap();
    let output = dir.path("corrupt-converted.wav");
    let result = cli(&["convert", path_text(&corrupt), path_text(&output)]);
    assert!(!result.status.success(), "malformed input must not convert");
    assert!(!output.exists(), "invalid input created an output file");
}
