//! Normalized Square Difference Function (NSDF) — the core of MPM.
//!
//! NSDF(τ) = 2·r(τ) / m(τ)
//!
//! where
//!   r(τ) = Σ_{i=0}^{N−τ−1} x[i] · x[i+τ]   (autocorrelation)
//!   m(τ) = Σ_{i=0}^{N−τ−1} (x[i]² + x[i+τ]²)   (squared-sum normaliser)
//!
//! Autocorrelation is computed via FFT in O(N log N).

use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

pub struct NsdfWorkspace {
    n: usize,
    fft_len: usize,
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
    time_buf: Vec<f32>,
    freq_buf: Vec<Complex32>,
}

impl NsdfWorkspace {
    pub fn new(window_len: usize) -> Self {
        // Pad to 2N to avoid circular-correlation wrap-around.
        let fft_len = (2 * window_len).next_power_of_two();
        let mut planner = RealFftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(fft_len);
        let inverse = planner.plan_fft_inverse(fft_len);
        Self {
            n: window_len,
            fft_len,
            time_buf: vec![0.0; fft_len],
            freq_buf: vec![Complex32::new(0.0, 0.0); fft_len / 2 + 1],
            forward,
            inverse,
        }
    }

    /// Compute NSDF into `out`. `samples.len()` must equal `window_len()`.
    /// `out.len()` should be at least `window_len()`; only lags `[0, N)` are populated.
    pub fn compute(&mut self, samples: &[f32], out: &mut [f32]) {
        assert_eq!(samples.len(), self.n);
        assert!(out.len() >= self.n);

        // 1. Zero-padded forward FFT.
        self.time_buf[..self.n].copy_from_slice(samples);
        for v in &mut self.time_buf[self.n..] {
            *v = 0.0;
        }
        self.forward
            .process(&mut self.time_buf, &mut self.freq_buf)
            .unwrap();

        // 2. Power spectrum.
        for c in &mut self.freq_buf {
            let re = c.re;
            let im = c.im;
            c.re = re * re + im * im;
            c.im = 0.0;
        }

        // 3. Inverse FFT → autocorrelation r(τ) (unnormalized).
        self.inverse
            .process(&mut self.freq_buf, &mut self.time_buf)
            .unwrap();
        // realfft's inverse leaves an implicit scale of `fft_len`; we divide
        // numerator and denominator by the same factor in NSDF, so we can skip it.

        // 4. Compute m(τ) incrementally and form NSDF.
        // m(0) = 2 · Σ x[i]²  =  2 · r(0)  (since r(0) here is Σ x[i]²·fft_len).
        // Easier: compute m directly.
        // m(τ) = Σ_{i=0}^{N−τ−1} (x[i]² + x[i+τ]²)
        //
        // Recurrence:
        //   m(0) = 2 · Σ_{i=0}^{N−1} x[i]²
        //   m(τ+1) = m(τ) − x[τ]² − x[N−1−τ]²
        let mut m_tau: f64 = 0.0;
        for &v in &samples[..self.n] {
            m_tau += (v as f64) * (v as f64);
        }
        m_tau *= 2.0;

        let scale = self.fft_len as f64; // realfft inverse missing 1/N
        for tau in 0..self.n {
            let r_tau = self.time_buf[tau] as f64 / scale;
            let nsdf = if m_tau > 1e-12 {
                (2.0 * r_tau) / m_tau
            } else {
                0.0
            };
            out[tau] = nsdf as f32;

            // Update m for the next lag.
            let lo = samples[tau] as f64;
            let hi = samples[self.n - 1 - tau] as f64;
            m_tau -= lo * lo + hi * hi;
            if m_tau < 0.0 {
                m_tau = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::TAU;

    /// NSDF(0) must equal ~1.0 for any non-silent signal.
    #[test]
    fn nsdf_at_zero_is_one() {
        let n = 1024;
        let mut ws = NsdfWorkspace::new(n);
        let sig: Vec<f32> = (0..n)
            .map(|i| (TAU * 110.0 * i as f32 / 48000.0).sin())
            .collect();
        let mut out = vec![0.0; n];
        ws.compute(&sig, &mut out);
        assert!((out[0] - 1.0).abs() < 0.01, "NSDF(0) = {}", out[0]);
    }

    /// For a pure sine, NSDF should have a strong peak near the period.
    #[test]
    fn sine_has_peak_at_period() {
        let sr = 48000.0;
        let f0 = 220.0;
        let n = 4096;
        let mut ws = NsdfWorkspace::new(n);
        let sig: Vec<f32> = (0..n).map(|i| (TAU * f0 * i as f32 / sr).sin()).collect();
        let mut out = vec![0.0; n];
        ws.compute(&sig, &mut out);

        let expected_tau = (sr / f0) as usize;
        // Search window around the expected lag.
        let lo = expected_tau.saturating_sub(5);
        let hi = (expected_tau + 5).min(n - 1);
        let (peak_tau, &peak_val) = out[lo..=hi]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        assert!(peak_val > 0.9, "peak={} at offset {}", peak_val, peak_tau);
    }
}
