//! Audio sources (files in the session's Audio Files folder) and their decoded data, and the
//! movies video clips reference.

use crate::SourceId;
use soundcraft_audio_io::{AudioBuffer, FileFormat, peaks::Peaks};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Metadata for an audio file used by the session (serialised).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Source {
    pub id: SourceId,
    pub name: String,
    /// Path relative to the session folder (or absolute for referenced media).
    pub path: String,
    pub channels: u16,
    pub frames: u64,
    pub sample_rate: u32,
    pub format: FileFormat,
    /// Original BWF time stamp (samples), if any.
    #[serde(default)]
    pub time_reference: u64,
    /// True when the file has not been written to disk yet (recorded/rendered this session).
    #[serde(default)]
    pub unsaved: bool,
}

/// A movie used by Video track clips (serialised). The pictures themselves are never stored in
/// the session: the UI opens the movie from `path` and decodes frames on demand.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VideoSource {
    pub id: SourceId,
    pub name: String,
    /// Absolute path of the movie (referenced, never copied into the session folder).
    pub path: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    /// Frames per second (0 = unknown).
    #[serde(default)]
    pub frame_rate: f64,
    /// Picture length in seconds.
    #[serde(default)]
    pub duration: f64,
    /// Codec id (`h264`, `prores`, `mjpeg`, `hevc`, …).
    #[serde(default)]
    pub codec: String,
}

impl VideoSource {
    /// Display aspect ratio (width / height), 16:9 when unknown.
    pub fn aspect(&self) -> f64 {
        if self.width == 0 || self.height == 0 { 16.0 / 9.0 } else { f64::from(self.width) / f64::from(self.height) }
    }
}

/// Decoded audio plus waveform overviews (not serialised).
#[derive(Debug)]
pub struct SourceAudio {
    pub buffer: AudioBuffer,
    pub peaks: Vec<Peaks>,
}

impl SourceAudio {
    pub fn new(buffer: AudioBuffer) -> Self {
        let peaks = buffer.channels.iter().map(|c| Peaks::build(c)).collect();
        SourceAudio { buffer, peaks }
    }
    pub fn frames(&self) -> usize {
        self.buffer.frames()
    }
    pub fn channel(&self, ch: usize) -> Option<&[f32]> {
        self.buffer.channels.get(ch).map(Vec::as_slice)
    }
}

/// Shared decoded audio, keyed by source id.
#[derive(Debug, Clone, Default)]
pub struct SourcePool {
    map: BTreeMap<SourceId, Arc<SourceAudio>>,
}

impl SourcePool {
    pub fn insert(&mut self, id: SourceId, audio: Arc<SourceAudio>) {
        self.map.insert(id, audio);
    }
    pub fn get(&self, id: SourceId) -> Option<&Arc<SourceAudio>> {
        self.map.get(&id)
    }
    pub fn remove(&mut self, id: SourceId) -> Option<Arc<SourceAudio>> {
        self.map.remove(&id)
    }
    pub fn contains(&self, id: SourceId) -> bool {
        self.map.contains_key(&id)
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    pub fn ids(&self) -> impl Iterator<Item = SourceId> + '_ {
        self.map.keys().copied()
    }
}

impl PartialEq for SourcePool {
    fn eq(&self, other: &Self) -> bool {
        self.map.len() == other.map.len() && self.map.iter().zip(other.map.iter()).all(|((a, x), (b, y))| a == b && Arc::ptr_eq(x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(samples: Vec<Vec<f32>>) -> Arc<SourceAudio> {
        Arc::new(SourceAudio::new(AudioBuffer { sample_rate: 48_000, channels: samples }))
    }

    #[test]
    fn source_pool_insert_replace_remove_and_sorted_ids_are_deterministic() {
        let mut pool = SourcePool::default();
        assert!(pool.is_empty());
        assert_eq!(pool.len(), 0);
        assert_eq!(pool.get(SourceId(11)), None);
        let first = audio(vec![vec![0.1, 0.2]]);
        let second = audio(vec![vec![0.3, 0.4]]);
        let replacement = audio(vec![vec![0.5, 0.6]]);
        pool.insert(SourceId(11), Arc::clone(&first));
        pool.insert(SourceId(2), Arc::clone(&second));
        assert_eq!(pool.ids().collect::<Vec<_>>(), vec![SourceId(2), SourceId(11)]);
        assert!(pool.contains(SourceId(11)));
        assert!(!pool.contains(SourceId(5)));
        assert_eq!(pool.len(), 2);
        pool.insert(SourceId(11), Arc::clone(&replacement));
        assert_eq!(pool.len(), 2, "replacing an ID must not add another source");
        assert!(Arc::ptr_eq(pool.get(SourceId(11)).unwrap(), &replacement));
        assert!(pool.remove(SourceId(999)).is_none());
        let removed = pool.remove(SourceId(11)).unwrap();
        assert!(Arc::ptr_eq(&removed, &replacement));
        assert_eq!(pool.ids().collect::<Vec<_>>(), vec![SourceId(2)]);
        assert!(pool.remove(SourceId(2)).is_some());
        assert!(pool.is_empty());
    }

    #[test]
    fn source_pool_clones_share_audio_and_equality_preserves_identity() {
        let shared = audio(vec![vec![1.0, -1.0]]);
        let mut a = SourcePool::default();
        a.insert(SourceId(7), Arc::clone(&shared));
        let mut b = a.clone();
        assert_eq!(a, b);
        assert!(Arc::ptr_eq(a.get(SourceId(7)).unwrap(), b.get(SourceId(7)).unwrap()));
        b.insert(SourceId(7), audio(vec![vec![1.0, -1.0]]));
        assert_ne!(a, b, "equal sample values must not hide a distinct audio cache allocation");
        b.remove(SourceId(7));
        assert_ne!(a, b, "different pool cardinalities are unequal");
        let mut c = SourcePool::default();
        c.insert(SourceId(8), shared);
        assert_ne!(a, c, "different keys with shared audio must not compare equal");
    }

    #[test]
    fn decoded_source_reports_shortest_channel_and_safe_channel_views() {
        let source = SourceAudio::new(AudioBuffer {
            sample_rate: 44_100,
            channels: vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0]],
        });
        assert_eq!(source.frames(), 2);
        assert_eq!(source.channel(0), Some(&[1.0, 2.0, 3.0][..]));
        assert_eq!(source.channel(1), Some(&[4.0, 5.0][..]));
        assert_eq!(source.channel(2), None);
        assert_eq!(source.peaks.len(), 2);
        assert_eq!(SourceAudio::new(AudioBuffer::default()).frames(), 0);
    }

    #[test]
    fn video_aspect_uses_default_for_missing_dimensions() {
        let mut video = VideoSource {
            id: SourceId(4), name: "Picture".into(), path: "video.mov".into(),
            width: 1920, height: 1080, frame_rate: 24.0, duration: 12.0, codec: "prores".into(),
        };
        assert_eq!(video.aspect(), 16.0 / 9.0);
        video.width = 0;
        assert_eq!(video.aspect(), 16.0 / 9.0);
        video.width = 1440;
        video.height = 0;
        assert_eq!(video.aspect(), 16.0 / 9.0);
        video.height = 1080;
        assert_eq!(video.aspect(), 4.0 / 3.0);
    }

    #[test]
    fn old_source_and_video_json_without_optional_fields_is_backward_compatible() {
        let audio = serde_json::json!({
            "id": 3, "name": "Voice", "path": "Audio Files/Voice.wav",
            "channels": 1, "frames": 400, "sample_rate": 48_000, "format": "Wav"
        });
        let src: Source = serde_json::from_value(audio).unwrap();
        assert_eq!(src.time_reference, 0);
        assert!(!src.unsaved);
        let movie = serde_json::json!({"id": 4, "name": "Clip", "path": "Clip.mov"});
        let parsed: VideoSource = serde_json::from_value(movie).unwrap();
        assert_eq!(parsed.width, 0);
        assert_eq!(parsed.height, 0);
        assert_eq!(parsed.frame_rate, 0.0);
        assert_eq!(parsed.duration, 0.0);
        assert_eq!(parsed.codec, "");
        assert_eq!(parsed.aspect(), 16.0 / 9.0);
    }
}
