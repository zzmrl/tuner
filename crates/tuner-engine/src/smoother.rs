//! Pitch smoothing.
//!
//! The detector emits per-frame results that can be noisy on real signals:
//! attack transients, octave errors, brief signal dropouts. The smoother
//! cleans this up with three small, independent mechanisms:
//!
//! 1. **Confidence gate** — drop frames below `min_confidence`.
//! 2. **Median filter on Hz** — kill single-frame outliers (e.g. octave jumps).
//! 3. **Voiced-hold** — once we've locked on, briefly hold the previous note
//!    through low-confidence gaps so the UI doesn't flicker to `--`.
//!
//! Tunings here are educated defaults. They are expected to be revised once
//! we observe live signals from real instruments.

use tuner_core::Pitch;

#[derive(Debug, Clone, Copy)]
pub struct SmootherConfig {
    /// Frames discarded if confidence is below this threshold.
    pub min_confidence: f32,
    /// Median-filter window in frames (must be odd; clamped to ≥ 1).
    pub median_window: usize,
    /// Number of consecutive low-confidence frames after which the engine
    /// gives up and emits `None`.
    pub hold_frames: usize,
}

impl Default for SmootherConfig {
    fn default() -> Self {
        Self {
            min_confidence: 0.6,
            median_window: 5,
            hold_frames: 4,
        }
    }
}

pub struct Smoother {
    cfg: SmootherConfig,
    a4_hz: f32,
    history: Vec<f32>,        // recent Hz values from voiced frames
    last_voiced: Option<Pitch>,
    silence_streak: usize,
}

impl Smoother {
    pub fn new(cfg: SmootherConfig) -> Self {
        Self::with_a4(cfg, 440.0)
    }

    pub fn with_a4(cfg: SmootherConfig, a4_hz: f32) -> Self {
        let window = cfg.median_window.max(1) | 1; // force odd
        Self {
            cfg: SmootherConfig { median_window: window, ..cfg },
            a4_hz,
            history: Vec::with_capacity(window),
            last_voiced: None,
            silence_streak: 0,
        }
    }

    pub fn reset(&mut self) {
        self.history.clear();
        self.last_voiced = None;
        self.silence_streak = 0;
    }

    /// Feed one raw detection. Returns the smoothed result for this frame.
    pub fn feed(&mut self, raw: Option<Pitch>) -> Option<Pitch> {
        match raw {
            Some(p) if p.confidence >= self.cfg.min_confidence => {
                self.silence_streak = 0;
                self.push_history(p.hz);
                let smoothed = Pitch::from_hz(self.median_hz(), p.confidence, self.a4_hz);
                self.last_voiced = Some(smoothed);
                Some(smoothed)
            }
            _ => {
                self.silence_streak += 1;
                if self.silence_streak <= self.cfg.hold_frames {
                    self.last_voiced
                } else {
                    self.history.clear();
                    self.last_voiced = None;
                    None
                }
            }
        }
    }

    fn push_history(&mut self, hz: f32) {
        if self.history.len() == self.cfg.median_window {
            self.history.remove(0);
        }
        self.history.push(hz);
    }

    fn median_hz(&self) -> f32 {
        let mut sorted: Vec<f32> = self.history.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        sorted[sorted.len() / 2]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pitch(hz: f32, conf: f32) -> Pitch {
        Pitch::from_hz(hz, conf, 440.0)
    }

    #[test]
    fn low_confidence_passes_through_during_hold() {
        let cfg = SmootherConfig { hold_frames: 2, ..Default::default() };
        let mut s = Smoother::new(cfg);
        for _ in 0..3 {
            s.feed(Some(pitch(440.0, 0.9)));
        }
        let held = s.feed(Some(pitch(440.0, 0.1)));
        assert!(held.is_some(), "should hold previous voiced result");
        s.feed(None);
        s.feed(None);
        assert!(s.feed(None).is_none(), "should give up after hold_frames");
    }

    #[test]
    fn median_rejects_single_octave_outlier() {
        let mut s = Smoother::new(SmootherConfig::default());
        s.feed(Some(pitch(440.0, 0.9)));
        s.feed(Some(pitch(440.0, 0.9)));
        s.feed(Some(pitch(880.0, 0.9))); // octave-up glitch
        s.feed(Some(pitch(440.0, 0.9)));
        let out = s.feed(Some(pitch(440.0, 0.9))).unwrap();
        assert!((out.hz - 440.0).abs() < 0.1, "got {}", out.hz);
    }
}
