//! McLeod Pitch Method — peak picking on the NSDF with parabolic interpolation.
//!
//! Reference: Philip McLeod & Geoff Wyvill, "A Smarter Way to Find Pitch" (2005).

use crate::note::Pitch;
use crate::nsdf::NsdfWorkspace;

#[derive(Debug, Clone, Copy)]
pub struct DetectorConfig {
    /// Analysis window size in samples.
    pub window_len: usize,
    /// Reference frequency for A4 (default 440 Hz).
    pub a4_hz: f32,
    /// Fraction of the global NSDF maximum a peak must exceed to be selected
    /// (McLeod's `k`, typically 0.8–0.95).
    pub peak_threshold: f32,
    /// Minimum NSDF value to consider the result voiced (below → return None).
    pub min_confidence: f32,
    /// Hz bounds outside which detections are rejected.
    pub min_hz: f32,
    pub max_hz: f32,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            window_len: 4096,
            a4_hz: 440.0,
            peak_threshold: 0.9,
            min_confidence: 0.5,
            // Covers 5-string bass low B (≈30.87 Hz) through high-fret guitar.
            min_hz: 27.0,
            max_hz: 2000.0,
        }
    }
}

pub struct McLeodDetector {
    cfg: DetectorConfig,
    ws: NsdfWorkspace,
    nsdf: Vec<f32>,
}

impl McLeodDetector {
    pub fn new(cfg: DetectorConfig) -> Self {
        let ws = NsdfWorkspace::new(cfg.window_len);
        let nsdf = vec![0.0; cfg.window_len];
        Self { cfg, ws, nsdf }
    }

    pub fn config(&self) -> &DetectorConfig {
        &self.cfg
    }

    /// Detect pitch from a window of exactly `cfg.window_len` samples.
    pub fn detect(&mut self, samples: &[f32], sample_rate: f32) -> Option<Pitch> {
        assert_eq!(samples.len(), self.cfg.window_len);

        self.ws.compute(samples, &mut self.nsdf);

        // Lag bounds derived from frequency bounds.
        let min_tau = (sample_rate / self.cfg.max_hz).floor() as usize;
        let max_tau = (sample_rate / self.cfg.min_hz).ceil() as usize;
        let max_tau = max_tau.min(self.nsdf.len() - 2);
        if min_tau < 2 || min_tau >= max_tau {
            return None;
        }

        // 1. Skip past the initial positive lobe (NSDF starts at 1.0 and descends).
        //    Find first zero-crossing from positive → negative.
        let mut tau = min_tau.max(1);
        while tau < max_tau && self.nsdf[tau] > 0.0 {
            tau += 1;
        }
        if tau >= max_tau {
            return None;
        }

        // 2. Collect key maxima: in each region between zero-crossings, take
        //    the highest local max.
        let mut peaks: Vec<(usize, f32)> = Vec::new();
        while tau < max_tau {
            // Advance to next positive region.
            while tau < max_tau && self.nsdf[tau] <= 0.0 {
                tau += 1;
            }
            if tau >= max_tau {
                break;
            }
            // Track the max within this positive region.
            let mut local_tau = tau;
            let mut local_val = self.nsdf[tau];
            while tau < max_tau && self.nsdf[tau] > 0.0 {
                if self.nsdf[tau] > local_val {
                    local_val = self.nsdf[tau];
                    local_tau = tau;
                }
                tau += 1;
            }
            peaks.push((local_tau, local_val));
        }
        if peaks.is_empty() {
            return None;
        }

        // 3. Pick the *first* peak that exceeds `peak_threshold * global_max`.
        let global_max = peaks.iter().map(|p| p.1).fold(0.0_f32, f32::max);
        let threshold = self.cfg.peak_threshold * global_max;
        let (chosen_tau, _) = peaks
            .iter()
            .copied()
            .find(|&(_, v)| v >= threshold)
            .unwrap_or_else(|| {
                // Fallback: highest peak overall.
                peaks
                    .iter()
                    .copied()
                    .fold(peaks[0], |acc, p| if p.1 > acc.1 { p } else { acc })
            });

        // 4. Parabolic interpolation around the chosen lag for sub-sample accuracy.
        let (refined_tau, refined_val) = parabolic_interp(&self.nsdf, chosen_tau);
        if refined_val < self.cfg.min_confidence {
            return None;
        }
        if refined_tau <= 0.0 {
            return None;
        }

        let hz = sample_rate / refined_tau;
        if hz < self.cfg.min_hz || hz > self.cfg.max_hz {
            return None;
        }

        Some(Pitch::from_hz(
            hz,
            refined_val.clamp(0.0, 1.0),
            self.cfg.a4_hz,
        ))
    }
}

/// Fit a parabola through (tau-1, tau, tau+1) and return the interpolated
/// (lag, value) at the vertex.
fn parabolic_interp(nsdf: &[f32], tau: usize) -> (f32, f32) {
    if tau == 0 || tau + 1 >= nsdf.len() {
        return (tau as f32, nsdf[tau]);
    }
    let y0 = nsdf[tau - 1];
    let y1 = nsdf[tau];
    let y2 = nsdf[tau + 1];
    let denom = y0 - 2.0 * y1 + y2;
    if denom.abs() < 1e-9 {
        return (tau as f32, y1);
    }
    let delta = 0.5 * (y0 - y2) / denom;
    let refined_tau = tau as f32 + delta;
    let refined_val = y1 - 0.25 * (y0 - y2) * delta;
    (refined_tau, refined_val)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::TAU;

    fn synth_sine(hz: f32, sr: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (TAU * hz * i as f32 / sr).sin()).collect()
    }

    fn synth_sawtooth(hz: f32, sr: f32, n: usize) -> Vec<f32> {
        // Crude band-limited-ish saw via additive synthesis (5 harmonics).
        (0..n)
            .map(|i| {
                let t = i as f32 / sr;
                let mut s = 0.0;
                for k in 1..=5 {
                    s += (TAU * hz * k as f32 * t).sin() / k as f32;
                }
                s * 0.4
            })
            .collect()
    }

    fn assert_close(detected: f32, expected: f32, cents_tol: f32) {
        let cents = 1200.0 * (detected / expected).log2();
        assert!(
            cents.abs() < cents_tol,
            "detected={} expected={} ({:.2} cents off, tol={})",
            detected,
            expected,
            cents,
            cents_tol
        );
    }

    #[test]
    fn detects_a4_sine() {
        let sr = 48000.0;
        let mut det = McLeodDetector::new(DetectorConfig::default());
        let sig = synth_sine(440.0, sr, 4096);
        let p = det.detect(&sig, sr).expect("voiced");
        assert_close(p.hz, 440.0, 1.0);
    }

    #[test]
    fn detects_high_e_guitar() {
        let sr = 48000.0;
        let mut det = McLeodDetector::new(DetectorConfig::default());
        let sig = synth_sawtooth(329.628, sr, 4096);
        let p = det.detect(&sig, sr).expect("voiced");
        assert_close(p.hz, 329.628, 5.0);
    }

    #[test]
    fn detects_low_b_bass() {
        // 5-string bass low B — the stress test.
        let sr = 48000.0;
        let cfg = DetectorConfig {
            window_len: 8192,
            ..Default::default()
        };
        let mut det = McLeodDetector::new(cfg);
        let sig = synth_sawtooth(30.868, sr, 8192);
        let p = det.detect(&sig, sr).expect("voiced");
        assert_close(p.hz, 30.868, 10.0);
    }

    #[test]
    fn silence_returns_none() {
        let mut det = McLeodDetector::new(DetectorConfig::default());
        let sig = vec![0.0_f32; 4096];
        assert!(det.detect(&sig, 48000.0).is_none());
    }
}
