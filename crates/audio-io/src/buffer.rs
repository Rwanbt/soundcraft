//! Planar `f32` audio buffer.

/// Planar f32 audio.
///
/// Every channel is expected to hold the same number of frames; [`AudioBuffer::frames`] reports the
/// shortest channel so a ragged buffer never causes out-of-bounds access.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioBuffer {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// One `Vec` of samples per channel.
    pub channels: Vec<Vec<f32>>,
}

impl AudioBuffer {
    /// A silent buffer with `channels` channels of `frames` frames each.
    pub fn new(sample_rate: u32, channels: usize, frames: usize) -> Self {
        Self { sample_rate, channels: vec![vec![0.0; frames]; channels] }
    }

    /// Number of frames (the length of the shortest channel; 0 when there are no channels).
    pub fn frames(&self) -> usize {
        self.channels.iter().map(Vec::len).min().unwrap_or(0)
    }

    /// Number of channels.
    pub fn num_channels(&self) -> usize {
        self.channels.len()
    }

    /// Duration in seconds (0 when the sample rate is 0).
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.frames() as f64 / f64::from(self.sample_rate)
    }

    /// Interleaved copy of the samples (frame-major: `L0 R0 L1 R1 ...`).
    pub fn interleaved(&self) -> Vec<f32> {
        let frames = self.frames();
        let mut out = Vec::with_capacity(frames.saturating_mul(self.channels.len()));
        for f in 0..frames {
            for ch in &self.channels {
                out.push(ch.get(f).copied().unwrap_or(0.0));
            }
        }
        out
    }

    /// Build a planar buffer from interleaved samples. A trailing partial frame is dropped;
    /// `channels == 0` yields an empty buffer.
    pub fn from_interleaved(sr: u32, channels: usize, data: &[f32]) -> Self {
        if channels == 0 {
            return Self { sample_rate: sr, channels: Vec::new() };
        }
        let frames = data.len() / channels;
        let mut out: Vec<Vec<f32>> = (0..channels).map(|_| Vec::with_capacity(frames)).collect();
        for frame in data.chunks_exact(channels) {
            for (ch, &s) in out.iter_mut().zip(frame) {
                ch.push(s);
            }
        }
        Self { sample_rate: sr, channels: out }
    }

    /// Absolute peak over all channels (NaN samples are ignored).
    pub fn peak(&self) -> f32 {
        self.channels.iter().flat_map(|c| c.iter()).fold(0.0f32, |acc, &s| acc.max(s.abs()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_interleaved_roundtrip_for_small_channel_counts_and_frame_sizes() {
        for channels in 0..=8usize {
            for frames in 0..=129usize {
                let original: Vec<Vec<f32>> = (0..channels).map(|ch| (0..frames).map(|frame| (ch * 1000 + frame) as f32 / 10.0).collect()).collect();
                let a = AudioBuffer { sample_rate: 96_000, channels: original };
                let interleaved = a.interleaved();
                let b = AudioBuffer::from_interleaved(96_000, channels, &interleaved);
                assert_eq!(b, a, "channels={channels}, frames={frames}");
                assert_eq!(b.num_channels(), channels);
                assert_eq!(b.frames(), if channels == 0 { 0 } else { frames });
                assert_eq!(interleaved.len(), channels * frames);
            }
        }
    }

    #[test]
    fn incomplete_frames_are_dropped_without_shifting_channels() {
        for channels in 1..=8usize {
            for trailing in 0..channels {
                let complete = 3 * channels;
                let samples: Vec<f32> = (0..complete + trailing).map(|x| x as f32).collect();
                let b = AudioBuffer::from_interleaved(48_000, channels, &samples);
                assert_eq!(b.frames(), 3);
                assert_eq!(b.interleaved(), samples[..complete]);
                for (ch, src) in b.channels.iter().enumerate() {
                    assert_eq!(src, &(0..3).map(|f| (f * channels + ch) as f32).collect::<Vec<_>>());
                }
            }
        }
        let zero = AudioBuffer::from_interleaved(48_000, 0, &[1.0, 2.0]);
        assert_eq!(zero.frames(), 0);
        assert_eq!(zero.num_channels(), 0);
    }

    #[test]
    fn ragged_audio_is_bounded_by_shortest_channel() {
        let b = AudioBuffer { sample_rate: 44_100, channels: vec![vec![1.0, 2.0, 99.0], vec![3.0, 4.0], vec![5.0, 6.0, 7.0, 8.0]] };
        assert_eq!(b.frames(), 2);
        assert_eq!(b.interleaved(), vec![1.0, 3.0, 5.0, 2.0, 4.0, 6.0]);
        let mut c = b.clone();
        c.channels[1].clear();
        assert_eq!(c.frames(), 0);
        assert!(c.interleaved().is_empty());
    }

    #[test]
    fn zero_sample_rate_and_silence_are_finite() {
        assert_eq!(AudioBuffer::new(0, 2, 5).duration_secs(), 0.0);
        let b = AudioBuffer::new(48_000, 2, 4_800);
        assert_eq!(b.num_channels(), 2);
        assert_eq!(b.frames(), 4_800);
        assert_eq!(b.duration_secs(), 0.1);
        assert_eq!(b.peak(), 0.0);
        assert!(b.interleaved().iter().all(|x| *x == 0.0));
        assert_eq!(AudioBuffer::default().duration_secs(), 0.0);
        assert_eq!(AudioBuffer::default().peak(), 0.0);
    }

    #[test]
    fn peaks_ignore_nan_and_preserve_infinite_overload_indicators() {
        let b = AudioBuffer { sample_rate: 48_000, channels: vec![vec![f32::NAN, -0.25, 0.8], vec![0.2, -1.2, f32::NAN]] };
        assert_eq!(b.peak(), 1.2);
        let overload = AudioBuffer { sample_rate: 48_000, channels: vec![vec![f32::INFINITY]] };
        assert_eq!(overload.peak(), f32::INFINITY);
    }
}
