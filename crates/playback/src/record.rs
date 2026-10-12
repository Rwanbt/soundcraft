//! Audio input capture for recording.
//!
//! A [`Recorder`] opens the default input device and appends interleaved samples to a shared
//! buffer while armed. The callback only `try_lock`s; if the UI thread holds the lock the block is
//! kept in a small local backlog and appended next time, so nothing blocks the audio thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// Captured audio: planar f32 at `sample_rate`.
#[derive(Debug, Clone, Default)]
pub struct Take {
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

struct Shared {
    armed: AtomicBool,
    data: Mutex<Vec<f32>>,
}

pub struct Recorder {
    shared: Arc<Shared>,
    /// Live input for monitoring (always fed while the input stream runs).
    pub monitor: Arc<InputRing>,
    #[cfg(not(target_os = "freebsd"))]
    _stream: Option<cpal::Stream>,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: usize,
}

/// Cap: one hour of 8-channel 192 kHz audio; 64 Mi samples (256 MB of f32) on 32-bit targets
/// (wasm32, i686), where the 64-bit product doesn't fit in `usize`.
const MAX_SAMPLES: usize = if usize::BITS >= 64 { (3600u64 * 192_000 * 8) as usize } else { 64 << 20 };

impl Recorder {
    /// Open the default input device. Errors when there is none (the caller shows a message).
    #[cfg(not(target_os = "freebsd"))]
    pub fn open() -> Result<Recorder, String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or("no audio input device")?;
        #[allow(deprecated)]
        let name = device.name().unwrap_or_else(|_| "Input".into());
        let cfg = device.default_input_config().map_err(|e| e.to_string())?;
        if cfg.sample_format() != cpal::SampleFormat::F32 {
            return Err(format!("unsupported input sample format {:?}", cfg.sample_format()));
        }
        let config = cfg.config();
        let shared = Arc::new(Shared { armed: AtomicBool::new(false), data: Mutex::new(Vec::new()) });
        let sh = Arc::clone(&shared);
        let monitor = InputRing::new(usize::from(config.channels), config.sample_rate.0);
        let mon = Arc::clone(&monitor);
        let input_channels = usize::from(config.channels).max(1);
        let mut backlog: Vec<f32> = Vec::new();
        let stream = device
            .build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    crate::mark_audio_thread();
                    mon.push(data);
                    append_captured(&sh, &mut backlog, data, MAX_SAMPLES, 1 << 20, input_channels);
                },
                |e| {
                    // Possibly the audio thread (see `crate::mark_audio_thread`).
                    crate::mark_audio_thread();
                    log::warn!("input stream error: {e}");
                },
                None,
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Recorder {
            monitor,
            shared,
            _stream: Some(stream),
            device_name: name,
            sample_rate: config.sample_rate.0,
            channels: usize::from(config.channels).max(1),
        })
    }

    #[cfg(target_os = "freebsd")]
    pub fn open() -> Result<Recorder, String> {
        Err("recording is not supported on this platform yet".into())
    }

    /// Start capturing (clears any previous take).
    pub fn arm(&self) {
        self.shared.data.lock().unwrap_or_else(PoisonError::into_inner).clear();
        self.shared.armed.store(true, Ordering::Relaxed);
    }

    pub fn is_armed(&self) -> bool {
        self.shared.armed.load(Ordering::Relaxed)
    }

    /// Stop capturing and return the take.
    pub fn take(&self) -> Take {
        self.shared.armed.store(false, Ordering::Relaxed);
        let data = std::mem::take(&mut *self.shared.data.lock().unwrap_or_else(PoisonError::into_inner));
        deinterleave(&data, self.channels, self.sample_rate)
    }

    /// Samples captured so far (per channel).
    pub fn captured_frames(&self) -> usize {
        self.shared.data.try_lock().map_or(0, |d| d.len() / self.channels.max(1))
    }
}

/// Collect input without blocking on the UI-owned take buffer. Both the persisted take
/// and the temporary backlog have hard sample limits, including whole incoming blocks.
/// The oldest captured samples win when the one-hour cap is reached.
fn append_captured(shared: &Shared, backlog: &mut Vec<f32>, input: &[f32], limit: usize, backlog_limit: usize, channels: usize) {
    if !shared.armed.load(Ordering::Relaxed) {
        backlog.clear();
        return;
    }
    // A backlog capped mid-frame would shift every channel in subsequent captured audio.
    // Align both caps and incoming samples without blocking or allocating in the callback.
    let channels = channels.max(1);
    let whole = |len: usize| len - len % channels;
    let limit = whole(limit);
    let backlog_limit = whole(backlog_limit);
    let input = &input[..whole(input.len())];
    backlog.truncate(whole(backlog.len()));

    if let Ok(mut captured) = shared.data.try_lock() {
        let complete = whole(captured.len());
        captured.truncate(complete);
        let remaining = limit.saturating_sub(captured.len());
        let from_backlog = whole(backlog.len().min(remaining));
        captured.extend_from_slice(&backlog[..from_backlog]);
        let from_input = whole(input.len().min(remaining - from_backlog));
        captured.extend_from_slice(&input[..from_input]);
        backlog.clear();
    } else {
        let room = backlog_limit.saturating_sub(backlog.len());
        backlog.extend_from_slice(&input[..whole(input.len().min(room))]);
    }
}

pub fn deinterleave(data: &[f32], channels: usize, sample_rate: u32) -> Take {
    let ch = channels.max(1);
    let frames = data.len() / ch;
    let mut out = vec![Vec::with_capacity(frames); ch];
    for frame in data.chunks_exact(ch) {
        for (c, v) in frame.iter().enumerate() {
            if let Some(o) = out.get_mut(c) {
                o.push(*v);
            }
        }
    }
    Take { sample_rate, channels: out }
}

#[cfg(test)]
mod tests {
    fn simulated_recorder(channels: usize) -> super::Recorder {
        super::Recorder {
            shared: std::sync::Arc::new(super::Shared { armed: std::sync::atomic::AtomicBool::new(false), data: std::sync::Mutex::new(Vec::new()) }),
            monitor: super::InputRing::new(channels, 48_000),
            #[cfg(not(target_os = "freebsd"))]
            _stream: None,
            device_name: "Synthetic input".into(),
            sample_rate: 48_000,
            channels,
        }
    }

    #[test]
    fn recorder_arm_take_and_rearm_are_deterministic_without_a_device() {
        let recorder = simulated_recorder(2);
        assert!(!recorder.is_armed());
        recorder.shared.data.lock().unwrap().extend([0.1, 0.2, 0.3, 0.4]);
        assert_eq!(recorder.captured_frames(), 2);

        // Starting a new take discards any stale samples from the previous recording.
        recorder.arm();
        assert!(recorder.is_armed());
        assert_eq!(recorder.captured_frames(), 0);
        recorder.shared.data.lock().unwrap().extend([1.0, 2.0, 3.0, 4.0]);
        let take = recorder.take();
        assert_eq!(take.sample_rate, 48_000);
        assert_eq!(take.channels, vec![vec![1.0, 3.0], vec![2.0, 4.0]]);
        assert!(!recorder.is_armed());
        assert_eq!(recorder.captured_frames(), 0);
        assert!(recorder.take().channels.iter().all(Vec::is_empty));

        recorder.arm();
        recorder.shared.data.lock().unwrap().extend([5.0, 6.0]);
        assert_eq!(recorder.take().channels, vec![vec![5.0], vec![6.0]]);
    }

    #[test]
    fn recorder_frame_count_is_zero_when_the_shared_buffer_is_locked() {
        let recorder = simulated_recorder(2);
        recorder.shared.data.lock().unwrap().extend([0.1, 0.2, 0.3, 0.4]);
        let lock = recorder.shared.data.lock().unwrap();
        assert_eq!(recorder.captured_frames(), 0, "callback-facing frame count must never block");
        drop(lock);
        assert_eq!(recorder.captured_frames(), 2);
    }

    #[test]
    fn audio_callback_take_cap_is_hard_even_for_an_oversized_block() {
        let recorder = simulated_recorder(2);
        let mut backlog = Vec::new();
        recorder.arm();
        super::append_captured(&recorder.shared, &mut backlog, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 4, 8, recorder.channels);
        assert_eq!(recorder.captured_frames(), 2, "capture must not overshoot the cap");
        super::append_captured(&recorder.shared, &mut backlog, &[7.0, 8.0], 4, 8, recorder.channels);
        assert_eq!(recorder.take().channels, vec![vec![1.0, 3.0], vec![2.0, 4.0]]);
    }

    #[test]
    fn audio_callback_keeps_backlog_in_order_under_contention() {
        let recorder = simulated_recorder(2);
        let mut backlog = Vec::new();
        recorder.arm();
        let guard = recorder.shared.data.lock().unwrap();
        super::append_captured(&recorder.shared, &mut backlog, &[1.0, 2.0], 8, 4, recorder.channels);
        super::append_captured(&recorder.shared, &mut backlog, &[3.0, 4.0], 8, 4, recorder.channels);
        super::append_captured(&recorder.shared, &mut backlog, &[9.0, 10.0], 8, 4, recorder.channels);
        assert_eq!(backlog, vec![1.0, 2.0, 3.0, 4.0], "backlog cap must not be exceeded");
        drop(guard);

        super::append_captured(&recorder.shared, &mut backlog, &[5.0, 6.0], 8, 4, recorder.channels);
        assert!(backlog.is_empty());
        assert_eq!(recorder.take().channels, vec![vec![1.0, 3.0, 5.0], vec![2.0, 4.0, 6.0]]);
    }

    #[test]
    fn disarming_discards_pending_callback_backlog() {
        let recorder = simulated_recorder(1);
        recorder.arm();
        let mut backlog = vec![12.0, 13.0];
        let _ = recorder.take();
        super::append_captured(&recorder.shared, &mut backlog, &[14.0, 15.0], 8, 4, recorder.channels);
        assert!(backlog.is_empty());
        assert_eq!(recorder.captured_frames(), 0);
    }

    #[test]
    fn callback_contention_never_splits_three_channel_frames_at_backlog_limit() {
        let recorder = simulated_recorder(3);
        recorder.arm();
        let mut backlog = Vec::new();
        let guard = recorder.shared.data.lock().unwrap();
        // Five sample slots cannot hold a complete second three-channel frame.
        super::append_captured(&recorder.shared, &mut backlog, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 12, 5, recorder.channels);
        assert_eq!(backlog, vec![1.0, 2.0, 3.0]);
        drop(guard);

        super::append_captured(&recorder.shared, &mut backlog, &[7.0, 8.0, 9.0], 12, 5, recorder.channels);
        assert!(backlog.is_empty());
        assert_eq!(recorder.take().channels, vec![vec![1.0, 7.0], vec![2.0, 8.0], vec![3.0, 9.0]]);
    }

    #[test]
    fn callback_hard_cap_and_partial_input_preserve_complete_stereo_frames() {
        let recorder = simulated_recorder(2);
        recorder.arm();
        let mut backlog = Vec::new();
        // The configured five-sample cap must truncate to four samples (two frames).
        super::append_captured(&recorder.shared, &mut backlog, &[1.0, 2.0, 3.0, 4.0, 5.0], 5, 5, recorder.channels);
        assert_eq!(recorder.shared.data.lock().unwrap().len(), 4);
        super::append_captured(&recorder.shared, &mut backlog, &[6.0, 7.0], 5, 5, recorder.channels);
        assert_eq!(recorder.take().channels, vec![vec![1.0, 3.0], vec![2.0, 4.0]]);
    }

    #[test]
    fn deinterleave_discards_incomplete_frames_without_shifting_channels() {
        let take = super::deinterleave(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 3, 96_000);
        assert_eq!(take.sample_rate, 96_000);
        assert_eq!(take.channels, vec![vec![0.0, 3.0], vec![1.0, 4.0], vec![2.0, 5.0]]);
    }

    #[test]
    fn deinterleaves() {
        let t = super::deinterleave(&[1.0, 2.0, 3.0, 4.0, 5.0], 2, 48_000);
        assert_eq!(t.channels, vec![vec![1.0, 3.0], vec![2.0, 4.0]]);
        let t = super::deinterleave(&[], 0, 48_000);
        assert_eq!(t.channels.len(), 1);
    }
}

/// Ring state behind the lock: interleaved input frames at the input device's rate, plus the
/// fractional read position (in input frames) of the next output sample.
struct RingState {
    data: std::collections::VecDeque<f32>,
    phase: f64,
}

/// A small interleaved FIFO from the input callback to the output callback, for input
/// monitoring. Both sides only `try_lock`; it holds at most ~0.5 s and drops the oldest frames.
/// The input device may run at a different rate than the session, so the reader converts
/// rates with linear interpolation (cheap, allocation-free, fine for monitoring).
pub struct InputRing {
    state: Mutex<RingState>,
    pub channels: std::sync::atomic::AtomicUsize,
    /// Input device sample rate in Hz (0 when unknown, which reads as "same as the session").
    rate: u32,
    /// Most samples kept: half a second of input, or 24 000 frames when the rate is unknown.
    cap: usize,
}

impl InputRing {
    pub fn new(channels: usize, rate: u32) -> Arc<InputRing> {
        let ch = channels.max(1);
        let frames = if rate == 0 { 24_000 } else { rate as usize / 2 };
        Arc::new(InputRing {
            state: Mutex::new(RingState { data: std::collections::VecDeque::with_capacity(rate.max(48_000) as usize * ch), phase: 0.0 }),
            channels: std::sync::atomic::AtomicUsize::new(ch),
            rate,
            cap: frames * ch,
        })
    }

    pub fn push(&self, samples: &[f32]) {
        if let Ok(mut s) = self.state.try_lock() {
            // Never temporarily grow the FIFO to the input callback's size.
            // Its initial capacity already holds more than the configured half-second cap.
            let tail = if samples.len() > self.cap { &samples[samples.len() - self.cap..] } else { samples };
            let excess = s.data.len().saturating_add(tail.len()).saturating_sub(self.cap);
            s.data.drain(..excess);
            s.data.extend(tail.iter().copied());
        }
    }

    /// Fill `out` (planar, at `session_rate`) with up to `frames` frames, converting from the
    /// input rate. Missing frames are silence.
    pub fn pop_into(&self, out: &mut [Vec<f32>], frames: usize, session_rate: u32) {
        let ch = self.channels.load(Ordering::Relaxed).max(1);
        for c in out.iter_mut() {
            c.iter_mut().take(frames).for_each(|x| *x = 0.0);
        }
        let Ok(mut s) = self.state.try_lock() else { return };
        // Input frames consumed per output frame.
        let step = if self.rate == 0 || session_rate == 0 { 1.0 } else { f64::from(self.rate) / f64::from(session_rate) };
        for f in 0..frames {
            // Drop input frames we have moved past; keep the one at `phase` and the next one.
            while s.phase >= 1.0 && s.data.len() >= 2 * ch {
                s.data.drain(..ch);
                s.phase -= 1.0;
            }
            if s.phase >= 1.0 || s.data.len() < 2 * ch {
                break;
            }
            let fr = s.phase as f32;
            for c in 0..ch {
                let a = s.data.get(c).copied().unwrap_or(0.0);
                let b = s.data.get(ch + c).copied().unwrap_or(a);
                if let Some(x) = out.get_mut(c).and_then(|o| o.get_mut(f)) {
                    *x = a + (b - a) * fr;
                }
            }
            s.phase += step;
        }
    }
}

#[cfg(test)]
mod ring_tests {
    use super::InputRing;

    /// A 1 kHz sine recorded at 16 kHz must come out as a 1 kHz sine at 48 kHz, across calls.
    #[test]
    fn monitor_converts_input_rate_to_session_rate() {
        let ring = InputRing::new(1, 16_000);
        let input: Vec<f32> = (0..4_000).map(|i| (2.0 * std::f64::consts::PI * 1_000.0 * f64::from(i) / 16_000.0).sin() as f32).collect();
        ring.push(&input);
        let mut out = vec![vec![0.0f32; 48]];
        let mut k = 0usize;
        for _ in 0..10 {
            ring.pop_into(&mut out, 48, 48_000);
            for v in &out[0] {
                let want = (2.0 * std::f64::consts::PI * 1_000.0 * k as f64 / 48_000.0).sin() as f32;
                assert!((v - want).abs() < 0.05, "sample {k}: got {v}, want {want}");
                k += 1;
            }
        }
    }

    /// Input faster than the session (96 kHz into 48 kHz): every other frame is read, in order.
    #[test]
    fn monitor_decimates_when_input_is_faster() {
        let ring = InputRing::new(1, 96_000);
        let input: Vec<f32> = (0..1_000).map(|i| i as f32).collect();
        ring.push(&input);
        let mut out = vec![vec![0.0f32; 100]];
        ring.pop_into(&mut out, 100, 48_000);
        for (k, v) in out[0].iter().enumerate() {
            assert_eq!(*v, (2 * k) as f32, "output {k}");
        }
    }

    /// Same rate: samples pass through unchanged.
    #[test]
    fn monitor_passes_through_at_equal_rates() {
        let ring = InputRing::new(2, 48_000);
        let input: Vec<f32> = (0..200).map(|i| i as f32).collect();
        ring.push(&input);
        let mut out = vec![vec![0.0f32; 50], vec![0.0f32; 50]];
        ring.pop_into(&mut out, 50, 48_000);
        assert_eq!(out[0][0], 0.0);
        assert_eq!(out[1][0], 1.0);
        assert_eq!(out[0][49], 98.0);
    }

    #[test]
    fn monitor_oversized_callback_does_not_grow_the_ring_capacity() {
        let ring = InputRing::new(2, 100);
        let original_capacity = ring.state.lock().unwrap().data.capacity();
        let oversized: Vec<f32> = (0..100_000).map(|i| i as f32).collect();
        ring.push(&oversized);
        let state = ring.state.lock().unwrap();
        assert_eq!(state.data.len(), ring.cap);
        assert_eq!(state.data.capacity(), original_capacity, "audio callback must not allocate for an oversized input block");
        let expected = &oversized[oversized.len() - ring.cap..];
        assert!(state.data.iter().copied().eq(expected.iter().copied()), "retain the newest input frames in exact order");
    }

    #[test]
    fn monitor_discards_oldest_frames_at_half_second_capacity() {
        let ring = InputRing::new(1, 48_000);
        let input: Vec<f32> = (0..30_000).map(|i| i as f32).collect();
        ring.push(&input);
        assert_eq!(ring.state.lock().unwrap().data.len(), 24_000);
        let mut out = vec![vec![-1.0f32; 32]];
        ring.pop_into(&mut out, 32, 48_000);
        assert_eq!(out[0], (6_000..6_032).map(|i| i as f32).collect::<Vec<_>>());
    }

    #[test]
    fn monitor_try_lock_never_blocks_and_zeroes_previous_samples() {
        let ring = InputRing::new(2, 48_000);
        ring.push(&[0.1, 0.2, 0.3, 0.4]);
        let mut output = vec![vec![7.0f32; 8], vec![7.0f32; 8]];
        let lock = ring.state.lock().unwrap();
        // Audio callback must be able to return silence immediately if the UI owns
        // the monitoring state lock, rather than waiting on the lock.
        ring.pop_into(&mut output, 8, 48_000);
        assert!(output.iter().flatten().all(|v| *v == 0.0));
        ring.push(&[0.8, 0.9]);
        drop(lock);
        let mut out = vec![vec![0.0; 1], vec![0.0; 1]];
        ring.pop_into(&mut out, 1, 48_000);
        assert_eq!(out[0][0], 0.1);
        assert_eq!(out[1][0], 0.2);
    }

    #[test]
    fn monitor_unknown_rates_match_one_input_frame_per_output() {
        let ring = InputRing::new(1, 0);
        ring.push(&[0.2, 0.4, 0.6, 0.8]);
        let mut out = vec![vec![0.0; 3]];
        ring.pop_into(&mut out, 3, 0);
        assert_eq!(out[0], vec![0.2, 0.4, 0.6]);
    }

    /// Nothing buffered: silence, not stale data.
    #[test]
    fn monitor_underrun_is_silence() {
        let ring = InputRing::new(1, 16_000);
        let mut out = vec![vec![1.0f32; 16]];
        ring.pop_into(&mut out, 16, 48_000);
        assert!(out[0].iter().all(|v| *v == 0.0));
    }
}
