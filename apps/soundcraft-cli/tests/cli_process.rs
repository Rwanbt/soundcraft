//! Exercise the installed CLI binary through OS processes, not private command helpers.
//! Test data is generated in per-test temp directories; no DAW, device or network required.
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
    Command::new(env!("CARGO_BIN_EXE_soundcraft-cli"))
        .env("SOUNDCRAFT_NO_PREFS", "1")
        .args(args)
        .output()
        .unwrap()
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
