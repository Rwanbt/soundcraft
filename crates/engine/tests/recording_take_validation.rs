//! Recording-take validation at the engine mutation boundary (no sound device required).

use soundcraft_engine::{Engine, EngineError, io};
use soundcraft_model::{ChannelFormat, Session, TrackId, TrackKind};

fn armed_engine() -> (Engine, TrackId) {
    let mut session = Session::default();
    let track = session.add_track(TrackKind::Audio, ChannelFormat::Stereo, Some("Capture"));
    session.track_mut(track).unwrap().mixer.record_arm = true;
    (Engine::new(session), track)
}

fn expect_rejected_unchanged(channels: Vec<Vec<f32>>, rate: u32, expected: &str) {
    let (mut engine, _) = armed_engine();
    let before = engine.session().to_json().unwrap();
    let revision = engine.revision;
    let err = io::add_recording(&mut engine, 42, channels, rate).unwrap_err();
    assert!(matches!(&err, EngineError::BadParams(command, detail) if command == "record" && detail.contains(expected)), "{err}");
    assert_eq!(engine.session().to_json().unwrap(), before);
    assert_eq!(engine.revision, revision);
    assert!(engine.session().sources.is_empty());
    assert!(!engine.is_dirty());
    assert!(!engine.can_undo());
}

#[test]
fn asymmetric_recording_channels_are_rejected_before_saving_any_source() {
    expect_rejected_unchanged(vec![vec![0.1, 0.2], vec![0.3]], 48_000, "inconsistent frame counts");
    expect_rejected_unchanged(vec![vec![0.1], vec![0.2, 0.3]], 48_000, "inconsistent frame counts");
    expect_rejected_unchanged(vec![vec![], vec![0.2]], 48_000, "inconsistent frame counts");
}

#[test]
fn malformed_recording_rates_do_not_create_mislabeled_audio() {
    for rate in [0, 1, 7_999, 768_001, u32::MAX] {
        expect_rejected_unchanged(vec![vec![0.1, 0.2], vec![0.3, 0.4]], rate, "sample rate");
    }
}

#[test]
fn nonfinite_recorded_samples_cannot_poison_session_audio() {
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        expect_rejected_unchanged(vec![vec![0.1, 0.2], vec![0.3, invalid]], 48_000, "non-finite");
    }
}

#[test]
fn valid_stereo_take_uses_separate_channels_and_one_undo_step() {
    let (mut engine, track) = armed_engine();
    let clips = io::add_recording(&mut engine, 42, vec![vec![0.25; 16], vec![-0.5; 16]], 48_000).unwrap();
    assert_eq!(clips.len(), 1);
    let recorded = &engine.session().sources[0];
    assert_eq!(recorded.frames, 16);
    assert_eq!(recorded.channels, 2);
    assert_eq!(recorded.sample_rate, 48_000);
    assert!(recorded.unsaved);
    let audio = engine.session().pool.get(recorded.id).unwrap();
    assert_eq!(audio.buffer.channels, vec![vec![0.25; 16], vec![-0.5; 16]]);
    let clip = &engine.session().track(track).unwrap().clips()[0];
    assert_eq!(clip.id, clips[0]);
    assert_eq!(clip.start, 42);
    assert_eq!(clip.length, 16);
    assert!(engine.can_undo());
    assert!(engine.undo(), "recording should be a single undo step");
    assert!(engine.session().sources.is_empty());
    assert!(engine.session().track(track).unwrap().clips().is_empty());
}

#[test]
fn empty_recording_is_a_noop_without_an_undo_step() {
    let (mut engine, _) = armed_engine();
    let revision = engine.revision;
    assert!(io::add_recording(&mut engine, 0, vec![vec![], vec![]], 48_000).unwrap().is_empty());
    assert!(io::add_recording(&mut engine, 0, vec![], 48_000).unwrap().is_empty());
    assert_eq!(engine.revision, revision);
    assert!(!engine.is_dirty());
    assert!(!engine.can_undo());
}
