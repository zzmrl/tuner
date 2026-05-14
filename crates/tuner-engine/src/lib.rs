//! Host-agnostic tuner engine.
//!
//! Splits the world cleanly between:
//!   - [`TunerEngine`] — owns the ring buffer + detector + smoother. Driven
//!     from whatever thread is supplying audio samples (cpal callback,
//!     plugin host callback, file reader, …). Single-producer.
//!   - [`TunerHandle`] — lock-free reader of the latest smoothed result.
//!     Polled from the UI thread (Slint, egui, terminal, …). Single-consumer.
//!
//! The audio side never blocks: pitch results are handed off through a
//! `triple_buffer`, which is wait-free for both sides.

mod ring;
mod smoother;
mod tuning;

pub use smoother::{Smoother, SmootherConfig};
pub use tuning::{Tuning, GUITAR_STANDARD, GUITAR_DROP_D, BASS_4_STANDARD, BASS_5_STANDARD};

use ring::RingBuffer;
use triple_buffer::triple_buffer;
use tuner_core::{DetectorConfig, McLeodDetector, Pitch};

#[derive(Debug, Clone, Copy)]
pub struct EngineConfig {
    /// Detector configuration (window size, A4 reference, etc.).
    pub detector: DetectorConfig,
    /// Sample rate of the incoming audio, in Hz.
    pub sample_rate: f32,
    /// Samples between successive analysis frames. Smaller = more frequent
    /// detections at the cost of CPU. Typical: window_len / 4.
    pub hop: usize,
    /// Smoothing parameters applied to detector output.
    pub smoother: SmootherConfig,
}

impl EngineConfig {
    pub fn new(sample_rate: f32) -> Self {
        let detector = DetectorConfig::default();
        let hop = detector.window_len / 4;
        Self {
            detector,
            sample_rate,
            hop,
            smoother: SmootherConfig::default(),
        }
    }
}

/// Audio-side half of the engine. Feed samples; smoothed pitch is published
/// to the paired [`TunerHandle`].
pub struct TunerEngine {
    detector: McLeodDetector,
    ring: RingBuffer,
    scratch: Vec<f32>,
    hop: usize,
    sample_rate: f32,
    smoother: Smoother,
    output: triple_buffer::Input<Option<Pitch>>,
    samples_since_last_frame: usize,
}

/// UI-side half. Lock-free `latest()` returns the most recent smoothed pitch.
pub struct TunerHandle {
    input: triple_buffer::Output<Option<Pitch>>,
}

impl TunerEngine {
    /// Create a paired (engine, handle). The engine goes to the audio thread,
    /// the handle to the UI thread.
    pub fn new(cfg: EngineConfig) -> (Self, TunerHandle) {
        let detector = McLeodDetector::new(cfg.detector);
        let ring_capacity = cfg.detector.window_len * 2;
        let ring = RingBuffer::new(ring_capacity);
        let scratch = vec![0.0_f32; cfg.detector.window_len];
        let smoother = Smoother::with_a4(cfg.smoother, cfg.detector.a4_hz);
        let (input, output) = triple_buffer(&None);
        let engine = Self {
            detector,
            ring,
            scratch,
            hop: cfg.hop,
            sample_rate: cfg.sample_rate,
            smoother,
            output: input,
            samples_since_last_frame: 0,
        };
        let handle = TunerHandle { input: output };
        (engine, handle)
    }

    /// Feed `samples` (mono, in `[-1, 1]`). Safe to call from a realtime
    /// audio thread: no allocations after construction, no locks.
    pub fn process(&mut self, samples: &[f32]) {
        self.ring.push(samples);
        self.samples_since_last_frame += samples.len();

        let window_len = self.scratch.len();
        if self.ring.len() < window_len { return; }

        while self.samples_since_last_frame >= self.hop {
            self.ring.read_latest(&mut self.scratch);
            let raw = self.detector.detect(&self.scratch, self.sample_rate);
            let smoothed = self.smoother.feed(raw);
            self.output.write(smoothed);
            self.samples_since_last_frame -= self.hop;
        }
    }

    /// Reset all smoothing state (use after long silence or device change).
    pub fn reset(&mut self) {
        self.ring.clear();
        self.smoother.reset();
        self.samples_since_last_frame = 0;
        self.output.write(None);
    }
}

impl TunerHandle {
    /// Returns the most recent smoothed pitch. Lock-free; never blocks.
    pub fn latest(&mut self) -> Option<Pitch> {
        *self.input.read()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::TAU;

    fn synth_sine(hz: f32, sr: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (TAU * hz * i as f32 / sr).sin()).collect()
    }

    #[test]
    fn engine_publishes_after_window() {
        let sr = 48000.0;
        let cfg = EngineConfig::new(sr);
        let (mut engine, mut handle) = TunerEngine::new(cfg);
        assert!(handle.latest().is_none(), "should start empty");

        // Feed one window's worth of A4.
        let sig = synth_sine(440.0, sr, cfg.detector.window_len);
        engine.process(&sig);

        let p = handle.latest().expect("should have a detection");
        assert!((p.hz - 440.0).abs() < 1.0, "hz={}", p.hz);
    }

    #[test]
    fn engine_handles_chunked_input() {
        let sr = 48000.0;
        let cfg = EngineConfig::new(sr);
        let (mut engine, mut handle) = TunerEngine::new(cfg);
        let sig = synth_sine(220.0, sr, cfg.detector.window_len * 2);
        // Drip-feed in small chunks the way cpal would.
        for chunk in sig.chunks(128) {
            engine.process(chunk);
        }
        let p = handle.latest().expect("should have a detection");
        assert!((p.hz - 220.0).abs() < 1.0, "hz={}", p.hz);
    }

    #[test]
    fn reset_clears_output() {
        let sr = 48000.0;
        let cfg = EngineConfig::new(sr);
        let (mut engine, mut handle) = TunerEngine::new(cfg);
        let sig = synth_sine(440.0, sr, cfg.detector.window_len);
        engine.process(&sig);
        assert!(handle.latest().is_some());
        engine.reset();
        assert!(handle.latest().is_none());
    }
}
