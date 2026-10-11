//! Deterministic reference tests: no external devices, files or real-time clocks.
use soundcraft_audio_io::AudioBuffer;
use soundcraft_midi::{Note, Sequence};
use soundcraft_mix::render_range;
use soundcraft_model::{ChannelFormat, Clip, Insert, Session, SourceAudio, SourceId, TrackKind};
use soundcraft_time::Range;
use std::sync::Arc;

fn assert_audio_close(reference: &[Vec<f32>], actual: &[Vec<f32>], tolerance: f32) {
    assert_eq!(reference.len(), actual.len(), "channel count changed");
    for (channel, (a, b)) in reference.iter().zip(actual).enumerate() {
        assert_eq!(a.len(), b.len(), "frame count changed in channel {channel}");
        let mut worst = 0.0f32;
        for (index, (&expected, &got)) in a.iter().zip(b).enumerate() {
            assert!(got.is_finite(), "non-finite channel {channel} sample {index}");
            worst = worst.max((expected - got).abs());
        }
        assert!(worst <= tolerance, "channel {channel} differs by {worst}, tolerance {tolerance}");
    }
}

fn audio_session() -> Session {
    let mut session = Session::default();
    let track = session.add_track(TrackKind::Audio, ChannelFormat::Mono, None);
    let frames = 8_192;
    let samples: Vec<f32> = (0..frames).map(|i| (i as f32 * 0.03125).sin() * 0.4).collect();
    session.pool.insert(SourceId(9_000), Arc::new(SourceAudio::new(AudioBuffer { sample_rate: 48_000, channels: vec![samples] })));
    let clip_id = session.new_clip_id();
    let t = session.track_mut(track).unwrap();
    t.playlist_mut().unwrap().clips.push(Clip::audio(clip_id, "generated", SourceId(9_000), 0, 0, frames as i64));
    t.mixer.volume_db = -6.0;
    session
}

fn midi_session() -> Session {
    let mut session = Session::default();
    let track = session.add_track(TrackKind::Instrument, ChannelFormat::Stereo, None);
    let id = session.new_clip_id();
    let sequence = Sequence {
        notes: vec![Note { pitch: 60, velocity: 100, release_velocity: 64, channel: 0, start: 30, length: 120 }],
        ctrls: Vec::new(),
    };
    let t = session.track_mut(track).unwrap();
    t.playlist_mut().unwrap().clips.push(Clip::midi(id, "generated-midi", 0, 8_192, sequence));
    let mut instrument = Insert::new("subtractive_synth");
    instrument.params.insert("level".into(), -9.0);
    instrument.params.insert("cutoff".into(), 4_000.0);
    t.instrument = Some(instrument);
    session
}

#[test]
fn offline_audio_output_is_independent_of_block_partition() {
    let session = audio_session();
    let range = Range::new(0, 8_192);
    let reference = render_range(&session, range, 512);
    let peak = reference.iter().flatten().fold(0.0f32, |p, x| p.max(x.abs()));
    assert!(peak > 0.01, "fixture must generate audio");
    for block in [16, 64, 125, 2_048] {
        let rendered = render_range(&session, range, block);
        assert_audio_close(&reference, &rendered, 1e-5);
    }
}

#[test]
fn midi_note_boundaries_produce_consistent_output_across_blocks() {
    // Start tick 30 corresponds to sample 750 at 120 BPM / 48 kHz,
    // so block 125 lands exactly on a note boundary but 512 does not.
    let session = midi_session();
    let range = Range::new(0, 8_192);
    let reference = render_range(&session, range, 512);
    let peak = reference.iter().flatten().fold(0.0f32, |p, x| p.max(x.abs()));
    assert!(peak > 0.001, "synth must emit nonzero samples");
    for block in [125, 250, 2_048] {
        let rendered = render_range(&session, range, block);
        assert_audio_close(&reference, &rendered, 1e-4);
    }
}

#[test]
fn persisted_midi_instrument_renders_identically_after_reload() {
    let original = midi_session();
    let json = original.to_json().unwrap();
    let reloaded = Session::from_json(&json).unwrap();
    let second_json = reloaded.to_json().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        serde_json::from_str::<serde_json::Value>(&second_json).unwrap(),
        "session must round-trip without losing semantic state"
    );
    let range = Range::new(0, 8_192);
    let before = render_range(&original, range, 512);
    let after = render_range(&reloaded, range, 512);
    assert_audio_close(&before, &after, 0.0);
    let repeated = render_range(&reloaded, range, 512);
    assert_audio_close(&after, &repeated, 0.0);
}
