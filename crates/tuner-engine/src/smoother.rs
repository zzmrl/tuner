//! Pitch smoothing.
//!
//! The detector emits per-frame results that can be noisy on real signals:
//! attack transients, octave errors, brief signal dropouts. The smoother
//! cleans this up with four small, independent mechanisms applied in order:
//!
//! 1. **Confidence gate** — drop frames below `min_confidence`.
//! 2. **Octave guard** — snap near-exact 2× (or ½×) jumps with a confidence
//!    drop back to the established octave. Catches MPM's classic
//!    decay-phase failure where the 2nd harmonic dominates and the
//!    detector picks half the true period.
//! 3. **Median filter on Hz** — kill remaining single-frame outliers.
//! 4. **Voiced-hold** — once we've locked on, briefly hold the previous note
//!    through low-confidence gaps so the UI doesn't flicker to `--`.

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
    /// Octave guard: a new detection is snapped to the established octave
    /// when its Hz ratio to the last stable value is within this tolerance
    /// of 2.0 or 0.5 *and* its confidence dropped by at least
    /// `octave_guard_conf_drop`. Set to 0.0 to disable.
    pub octave_guard_ratio_tol: f32,
    /// Minimum confidence drop (vs. last stable frame) required to enable
    /// the octave guard. Prevents snapping when the user genuinely jumps
    /// octaves on the instrument.
    pub octave_guard_conf_drop: f32,
}

impl Default for SmootherConfig {
    fn default() -> Self {
        // Defaults tuned against a recorded bass low-E take (2026-05-13).
        // See .plans/02-smoother-tuning.md for the data they were derived from.
        Self {
            min_confidence: 0.75,
            median_window: 7,
            hold_frames: 6,
            octave_guard_ratio_tol: 0.05,
            octave_guard_conf_drop: 0.05,
        }
    }
}

pub struct Smoother {
    cfg: SmootherConfig,
    a4_hz: f32,
    history: Vec<f32>,          // recent (post-octave-guard) Hz values
    last_stable_hz: Option<f32>,
    last_stable_conf: f32,
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
            last_stable_hz: None,
            last_stable_conf: 0.0,
            last_voiced: None,
            silence_streak: 0,
        }
    }

    pub fn reset(&mut self) {
        self.history.clear();
        self.last_stable_hz = None;
        self.last_stable_conf = 0.0;
        self.last_voiced = None;
        self.silence_streak = 0;
    }

    /// Feed one raw detection. Returns the smoothed result for this frame.
    pub fn feed(&mut self, raw: Option<Pitch>) -> Option<Pitch> {
        match raw {
            Some(p) if p.confidence >= self.cfg.min_confidence => {
                self.silence_streak = 0;
                let (corrected_hz, snapped) = self.octave_correct(p.hz, p.confidence);
                self.push_history(corrected_hz);
                let smoothed = Pitch::from_hz(self.median_hz(), p.confidence, self.a4_hz);
                self.last_stable_hz = Some(corrected_hz);
                // Only update the confidence baseline when we trusted the raw
                // frame. Snapped frames preserve the prior high-confidence
                // reference so a sustained glitch keeps getting snapped.
                if !snapped {
                    self.last_stable_conf = p.confidence;
                }
                self.last_voiced = Some(smoothed);
                Some(smoothed)
            }
            _ => {
                self.silence_streak += 1;
                if self.silence_streak <= self.cfg.hold_frames {
                    self.last_voiced
                } else {
                    self.history.clear();
                    self.last_stable_hz = None;
                    self.last_stable_conf = 0.0;
                    self.last_voiced = None;
                    None
                }
            }
        }
    }

    /// If the new Hz is near-exactly an octave above (or below) the last
    /// stable Hz **and** confidence has dropped meaningfully, snap to the
    /// established octave. Returns `(corrected_hz, was_snapped)`.
    fn octave_correct(&self, hz: f32, confidence: f32) -> (f32, bool) {
        let last = match self.last_stable_hz { Some(h) => h, None => return (hz, false) };
        if self.cfg.octave_guard_ratio_tol <= 0.0 { return (hz, false); }
        if confidence > self.last_stable_conf - self.cfg.octave_guard_conf_drop {
            return (hz, false); // confidence didn't drop → trust the new detection
        }
        let tol = self.cfg.octave_guard_ratio_tol;
        let ratio = hz / last;
        if (ratio - 2.0).abs() < 2.0 * tol { return (hz / 2.0, true); } // octave-up glitch
        if (ratio - 0.5).abs() < 0.5 * tol { return (hz * 2.0, true); } // octave-down glitch
        (hz, false)
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
            s.feed(Some(pitch(440.0, 0.95)));
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
        s.feed(Some(pitch(440.0, 0.95)));
        s.feed(Some(pitch(440.0, 0.95)));
        s.feed(Some(pitch(880.0, 0.95))); // octave glitch but same confidence
        s.feed(Some(pitch(440.0, 0.95)));
        let out = s.feed(Some(pitch(440.0, 0.95))).unwrap();
        assert!((out.hz - 440.0).abs() < 0.1, "got {}", out.hz);
    }

    /// Models the bass low-E recording: 3 s of stable detection at 41.3 Hz
    /// with conf 1.00, then a ~0.3 s run of 82.5 Hz with conf 0.90 (the
    /// MPM-2nd-harmonic glitch), then back to 41.3 Hz.
    #[test]
    fn octave_guard_handles_sustained_decay_glitch() {
        let mut s = Smoother::new(SmootherConfig::default());

        // Stable run.
        for _ in 0..40 {
            let out = s.feed(Some(pitch(41.30, 1.00))).unwrap();
            assert!((out.hz - 41.30).abs() < 0.5, "stable: got {}", out.hz);
        }

        // Octave-up glitch (15 frames @ confidence 0.90).
        for _ in 0..15 {
            let out = s.feed(Some(pitch(82.60, 0.90))).unwrap();
            assert!((out.hz - 41.30).abs() < 1.0,
                "should stay near 41.3 during glitch, got {}", out.hz);
        }

        // Recovery.
        for _ in 0..5 {
            let out = s.feed(Some(pitch(41.30, 1.00))).unwrap();
            assert!((out.hz - 41.30).abs() < 0.5, "recovery: got {}", out.hz);
        }
    }

    /// A genuine octave jump (e.g. user frets the 12th fret) should be
    /// accepted, not snapped down. Confidence stays high.
    #[test]
    fn octave_guard_respects_real_octave_jump() {
        let mut s = Smoother::new(SmootherConfig::default());
        for _ in 0..10 {
            s.feed(Some(pitch(110.0, 1.00)));
        }
        // Genuine jump to A3: confidence stays at 1.0.
        for _ in 0..10 {
            s.feed(Some(pitch(220.0, 1.00)));
        }
        // Median window is 7, so by 10 frames in we've fully transitioned.
        let out = s.feed(Some(pitch(220.0, 1.00))).unwrap();
        assert!((out.hz - 220.0).abs() < 1.0,
            "should accept genuine octave jump, got {}", out.hz);
    }

    #[test]
    fn min_confidence_floor_rejects_attack_transient() {
        let cfg = SmootherConfig::default();
        let mut s = Smoother::new(cfg);
        // Below the 0.75 floor → dropped.
        assert!(s.feed(Some(pitch(440.0, 0.7))).is_none());
        // At/above → accepted.
        let out = s.feed(Some(pitch(440.0, 0.8))).unwrap();
        assert!((out.hz - 440.0).abs() < 0.1);
    }
}
