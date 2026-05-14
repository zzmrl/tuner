//! Minimal mono-sample ring buffer.
//!
//! Single-threaded — synchronisation across the audio/UI boundary is the
//! triple-buffer's job, not the ring's. The ring exists only so the engine
//! can accumulate enough samples for a full analysis window even when the
//! audio host hands us tiny callback chunks (cpal can deliver 64–256 samples
//! at a time; we need 4096).

pub(crate) struct RingBuffer {
    data: Vec<f32>,
    /// Write index, monotonically increasing modulo `data.len()`.
    write: usize,
    /// Total samples currently stored (clamped at capacity).
    filled: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            data: vec![0.0; capacity],
            write: 0,
            filled: 0,
        }
    }

    pub fn len(&self) -> usize { self.filled }

    pub fn clear(&mut self) {
        self.write = 0;
        self.filled = 0;
        for v in &mut self.data { *v = 0.0; }
    }

    /// Append samples. If more than `capacity` arrive, only the most-recent
    /// `capacity` are retained.
    pub fn push(&mut self, samples: &[f32]) {
        let cap = self.data.len();
        // Drop any prefix we'd overwrite anyway.
        let src = if samples.len() > cap {
            &samples[samples.len() - cap..]
        } else {
            samples
        };
        for &s in src {
            self.data[self.write] = s;
            self.write = (self.write + 1) % cap;
        }
        self.filled = (self.filled + src.len()).min(cap);
    }

    /// Copy the most-recent `out.len()` samples into `out`, oldest first.
    /// `out.len()` must be ≤ `len()`.
    pub fn read_latest(&self, out: &mut [f32]) {
        let n = out.len();
        let cap = self.data.len();
        assert!(n <= self.filled, "asked for {n}, only have {}", self.filled);
        // The latest sample sits at index (write + cap - 1) % cap; we want
        // n samples ending there.
        let start = (self.write + cap - n) % cap;
        if start + n <= cap {
            out.copy_from_slice(&self.data[start..start + n]);
        } else {
            let first = cap - start;
            out[..first].copy_from_slice(&self.data[start..]);
            out[first..].copy_from_slice(&self.data[..n - first]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_and_wraps() {
        let mut r = RingBuffer::new(4);
        r.push(&[1.0, 2.0, 3.0]);
        assert_eq!(r.len(), 3);
        let mut out = [0.0; 3];
        r.read_latest(&mut out);
        assert_eq!(out, [1.0, 2.0, 3.0]);

        r.push(&[4.0, 5.0]); // wraps; oldest sample 1.0 gets evicted
        assert_eq!(r.len(), 4);
        let mut out = [0.0; 4];
        r.read_latest(&mut out);
        assert_eq!(out, [2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn drops_excess_on_push() {
        let mut r = RingBuffer::new(3);
        r.push(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(r.len(), 3);
        let mut out = [0.0; 3];
        r.read_latest(&mut out);
        assert_eq!(out, [3.0, 4.0, 5.0]);
    }
}
