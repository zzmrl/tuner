//! WAV-file analysis: read samples, slide a window, print per-frame detections.

use anyhow::{Context, Result};
use std::path::Path;
use tuner_core::{DetectorConfig, McLeodDetector};

use crate::print_frame;

pub fn run(path: &Path, cfg: DetectorConfig, hop: usize) -> Result<()> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("failed to open WAV at {}", path.display()))?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate as f32;
    let channels = spec.channels as usize;

    // Read all samples as f32 in [-1, 1] and downmix to mono.
    let mono: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => {
            let raw: Vec<f32> = reader.samples::<f32>().filter_map(Result::ok).collect();
            downmix(&raw, channels)
        }
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample as i32;
            let scale = 1.0_f32 / (1_i64 << (bits - 1)) as f32;
            let raw: Vec<f32> = reader
                .samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| s as f32 * scale)
                .collect();
            downmix(&raw, channels)
        }
    };

    eprintln!(
        "tuner-cli: {} — {:.1} kHz, {} ch, {} samples ({:.2}s), window={} hop={}",
        path.display(),
        sample_rate / 1000.0,
        channels,
        mono.len(),
        mono.len() as f32 / sample_rate,
        cfg.window_len,
        hop,
    );

    if mono.len() < cfg.window_len {
        anyhow::bail!("input is shorter than analysis window");
    }

    let mut det = McLeodDetector::new(cfg.clone());
    let mut pos = 0;
    while pos + cfg.window_len <= mono.len() {
        let window = &mono[pos..pos + cfg.window_len];
        let t = pos as f64 / sample_rate as f64;
        let pitch = det.detect(window, sample_rate);
        print_frame(t, pitch);
        pos += hop;
    }
    Ok(())
}

fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    let frames = interleaved.len() / channels;
    let mut mono = Vec::with_capacity(frames);
    let inv = 1.0 / channels as f32;
    for frame in interleaved.chunks_exact(channels) {
        mono.push(frame.iter().sum::<f32>() * inv);
    }
    mono
}
