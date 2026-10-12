//! Corrupt session identifiers must be rejected before they can alias tracks or media.

use soundcraft_audio_io::FileFormat;
use soundcraft_model::{ChannelFormat, Clip, ClipId, MarkerKind, Session, SessionError, Source, SourceId, TrackKind};

#[test]
fn duplicate_track_ids_are_rejected_instead_of_editing_the_wrong_track() {
    let mut session = Session::default();
    session.add_track(TrackKind::Audio, ChannelFormat::Stereo, Some("One"));
    session.add_track(TrackKind::Audio, ChannelFormat::Mono, Some("Two"));

    let mut doc = serde_json::to_value(&session).unwrap();
    doc["tracks"][1]["id"] = doc["tracks"][0]["id"].clone();
    let result = Session::from_json(&doc.to_string());
    assert!(matches!(result, Err(SessionError::Invalid(ref message)) if message.contains("duplicate")), "{result:?}");
}

#[test]
fn source_id_colliding_with_track_id_is_rejected_before_media_pool_is_loaded() {
    let mut session = Session::default();
    let track = session.add_track(TrackKind::Audio, ChannelFormat::Mono, Some("Voice"));
    session.sources.push(Source {
        id: SourceId(track.0),
        name: "Unrelated source".into(),
        path: "Audio Files/Voice.wav".into(),
        channels: 1,
        frames: 2,
        sample_rate: 48_000,
        format: FileFormat::Wav,
        time_reference: 0,
        unsaved: false,
    });
    let result = Session::from_json(&session.to_json().unwrap());
    assert!(matches!(result, Err(SessionError::Invalid(ref message)) if message.contains("duplicate")), "{result:?}");
}

#[test]
fn zero_identity_is_reserved_and_cannot_be_imported() {
    let mut session = Session::default();
    session.add_track(TrackKind::Audio, ChannelFormat::Mono, Some("Voice"));
    let mut doc = serde_json::to_value(&session).unwrap();
    doc["tracks"][0]["id"] = serde_json::json!(0);
    let result = Session::from_json(&doc.to_string());
    assert!(matches!(result, Err(SessionError::Invalid(ref message)) if message.contains("zero")), "{result:?}");
}

#[test]
fn exhausted_allocator_is_rejected_before_a_new_clip_can_reuse_an_id() {
    let mut session = Session::default();
    let track = session.add_track(TrackKind::Audio, ChannelFormat::Mono, Some("Voice"));
    let clip = Clip::audio(ClipId(u64::MAX), "Exhausted", SourceId(100), 0, 0, 100);
    session.track_mut(track).unwrap().playlist_mut().unwrap().clips.push(clip);
    let result = Session::from_json(&session.to_json().unwrap());
    assert!(matches!(result, Err(SessionError::Invalid(ref message)) if message.contains("exhausted")), "{result:?}");
}

#[test]
fn exhausted_next_id_is_rejected_even_when_object_ids_are_small() {
    let mut session = Session::default();
    session.add_marker("Start", MarkerKind::Marker, 0, 0);
    session.next_id = u64::MAX;
    let result = Session::from_json(&session.to_json().unwrap());
    assert!(matches!(result, Err(SessionError::Invalid(ref message)) if message.contains("exhausted")), "{result:?}");
}

#[test]
fn stale_allocator_is_repaired_and_normal_sessions_still_round_trip() {
    let mut session = Session::default();
    let track = session.add_track(TrackKind::Audio, ChannelFormat::Stereo, Some("Valid"));
    session.add_marker("Start", MarkerKind::Marker, 0, 0);
    session.next_id = 1;
    let mut loaded = Session::from_json(&session.to_json().unwrap()).unwrap();
    assert_eq!(loaded.tracks.len(), 1);
    assert_eq!(loaded.tracks[0].id, track);
    let fresh = loaded.new_clip_id();
    assert!(fresh.0 > loaded.markers[0].id.0);
    assert_ne!(fresh.0, track.0);
}

#[test]
fn invalid_serialized_sample_rates_are_rejected_before_engine_use() {
    let session = Session::default();
    let mut doc = serde_json::to_value(&session).unwrap();
    for rate in [0u32, 1, 7_999, 768_001, u32::MAX] {
        doc["sample_rate"] = serde_json::json!(rate);
        let result = Session::from_json(&doc.to_string());
        assert!(matches!(result, Err(SessionError::Invalid(ref message)) if message.contains("sample rate")), "rate {rate}: {result:?}");
    }
}

#[test]
fn supported_sample_rate_boundaries_survive_json_load() {
    let mut doc = serde_json::to_value(Session::default()).unwrap();
    for rate in [8_000u32, 44_100, 48_000, 768_000] {
        doc["sample_rate"] = serde_json::json!(rate);
        let restored = Session::from_json(&doc.to_string()).unwrap();
        assert_eq!(restored.sample_rate.hz(), rate);
    }
}
