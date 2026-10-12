//! Persistence regressions using independent decoded audio and real filesystem operations.
//! The tests never use an installed DAW, audio device, network, or ambient user files.

use serde_json::json;
use soundcraft_audio_io::{AudioBuffer, FileFormat};
use soundcraft_engine::{Engine, io};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("soundcraft-persistence-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn path(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn text(path: &Path) -> &str {
    path.to_str().expect("test temp path must be UTF-8")
}

fn add_audio(engine: &mut Engine, name: &str, amplitude: f32) -> soundcraft_model::SourceId {
    io::add_source(engine.session_mut(), name, AudioBuffer { sample_rate: 48_000, channels: vec![vec![amplitude; 64]] }, None, FileFormat::Wav)
}

#[test]
fn duplicate_source_names_are_saved_as_distinct_wav_files_and_reopen_without_data_loss() {
    let dir = Scratch::new();
    let path = dir.path("session.scraft");
    let mut engine = Engine::default();
    let first = add_audio(&mut engine, "Voice", 0.25);
    let second = add_audio(&mut engine, "Voice", -0.5);

    assert_ne!(first, second);
    assert_eq!(io::save_session(&mut engine, text(&path)).unwrap(), 2);
    let sources = &engine.session().sources;
    assert_eq!(sources.len(), 2);
    assert_ne!(sources[0].path, sources[1].path, "same-named recordings must not share one WAV");
    for src in sources {
        assert!(!src.unsaved);
        assert!(dir.path(&src.path).is_file(), "missing saved source {}", src.path);
    }

    let mut restored = Engine::default();
    assert!(io::open_session(&mut restored, text(&path)).unwrap().is_empty());
    assert_eq!(restored.session().pool.get(first).unwrap().buffer.channels[0], vec![0.25; 64]);
    assert_eq!(restored.session().pool.get(second).unwrap().buffer.channels[0], vec![-0.5; 64]);

    let first_path = sources[0].path.clone();
    assert_eq!(io::save_session(&mut engine, text(&path)).unwrap(), 0);
    assert_eq!(engine.session().sources[0].path, first_path);
}

#[test]
fn first_save_never_overwrites_an_existing_media_file_with_the_same_name() {
    let dir = Scratch::new();
    let audio_dir = dir.path("Audio Files");
    std::fs::create_dir_all(&audio_dir).unwrap();
    let existing = audio_dir.join("Voice.wav");
    let sentinel = b"unrelated user-owned audio: do not overwrite";
    std::fs::write(&existing, sentinel).unwrap();

    let mut engine = Engine::default();
    let id = add_audio(&mut engine, "Voice", 0.75);
    let path = dir.path("save.scraft");
    assert_eq!(io::save_session(&mut engine, text(&path)).unwrap(), 1);
    assert_eq!(std::fs::read(&existing).unwrap(), sentinel);
    assert_ne!(engine.session().sources[0].path, "Audio Files/Voice.wav");

    let mut restored = Engine::default();
    assert!(io::open_session(&mut restored, text(&path)).unwrap().is_empty());
    assert_eq!(restored.session().pool.get(id).unwrap().buffer.channels[0], vec![0.75; 64]);
}

#[test]
fn save_session_copy_preserves_active_path_revision_dirty_state_and_document() {
    let dir = Scratch::new();
    let original = dir.path("original.scraft");
    let copy = dir.path("copy.scraft");
    let mut engine = Engine::default();
    engine.execute("track.new", &json!({"name": "Reference"})).unwrap();
    engine.execute("session.save", &json!({"path": text(&original)})).unwrap();
    engine.execute("track.new", &json!({"name": "New unsaved track"})).unwrap();

    let before = engine.session().to_json().unwrap();
    let revision = engine.revision;
    let active_path = engine.path.clone();
    assert!(engine.is_dirty());

    engine.execute("session.save_copy", &json!({"path": text(&copy)})).unwrap();
    assert_eq!(engine.path, active_path);
    assert_eq!(engine.revision, revision);
    assert!(engine.is_dirty(), "saving a copy must not mark unsaved edits clean");
    assert_eq!(engine.session().to_json().unwrap(), before);
    assert_eq!(engine.session().name, "original");

    let mut restored = Engine::default();
    assert!(io::open_session(&mut restored, text(&copy)).unwrap().is_empty());
    assert!(restored.session().track_by_name("New unsaved track").is_some());
    assert_eq!(restored.session().name, "original");
}

#[test]
fn failed_save_does_not_change_the_open_session_or_its_dirty_state() {
    let dir = Scratch::new();
    let target = dir.path("already-a-directory.scraft");
    std::fs::create_dir_all(&target).unwrap();
    let mut engine = Engine::default();
    engine.execute("track.new", &json!({"name": "Unsaved"})).unwrap();

    let revision = engine.revision;
    let document = engine.session().to_json().unwrap();
    assert!(engine.execute("session.save", &json!({"path": text(&target)})).is_err());
    assert!(target.is_dir());
    assert_eq!(engine.revision, revision);
    assert_eq!(engine.session().to_json().unwrap(), document);
    assert!(engine.is_dirty());
    assert!(engine.path.is_none());
}

#[test]
fn autosave_copy_keeps_unsaved_live_audio_and_restores_its_samples() {
    let dir = Scratch::new();
    let path = dir.path("recovery.scraft");
    let mut engine = Engine::default();
    let source_id = add_audio(&mut engine, "Live", -0.375);
    let revision = engine.revision;
    assert!(engine.session().sources[0].unsaved);

    assert_eq!(io::save_copy(&engine, text(&path)).unwrap(), 1);
    assert_eq!(io::save_copy(&engine, text(&path)).unwrap(), 0);
    assert!(engine.session().sources[0].unsaved, "saving recovery media must not clear live source flags");
    assert!(engine.path.is_none());
    assert_eq!(engine.revision, revision);

    let mut restored = Engine::default();
    assert!(io::open_session(&mut restored, text(&path)).unwrap().is_empty());
    assert_eq!(restored.session().pool.get(source_id).unwrap().buffer.channels[0], vec![-0.375; 64]);
}

#[test]
fn failed_open_of_corrupted_session_keeps_pending_edits_and_active_file() {
    let dir = Scratch::new();
    let original = dir.path("original.scraft");
    let corrupted = dir.path("corrupted.scraft");
    let mut engine = Engine::default();
    engine.execute("track.new", &json!({"name": "Saved"})).unwrap();
    io::save_session(&mut engine, text(&original)).unwrap();
    engine.execute("track.new", &json!({"name": "Pending"})).unwrap();
    let active_path = engine.path.clone();
    let revision = engine.revision;
    let before = engine.session().to_json().unwrap();

    std::fs::write(&corrupted, b"{\"invalid\":").unwrap();
    assert!(io::open_session(&mut engine, text(&corrupted)).is_err());
    assert_eq!(engine.path, active_path);
    assert_eq!(engine.revision, revision);
    assert_eq!(engine.session().to_json().unwrap(), before);
    assert!(engine.is_dirty());
    assert!(engine.session().track_by_name("Pending").is_some());
}
