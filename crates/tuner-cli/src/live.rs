//! Live mic capture via cpal: stream samples into a shared buffer, run the
//! detector on the main thread, print to stdout.

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tuner_core::{DetectorConfig, McLeodDetector};

use crate::print_frame;

pub fn run(cfg: DetectorConfig, hop: usize) -> Result<()> {
    let host = cpal::default_host();
    let device = host.default_input_device()
        .ok_or_else(|| anyhow!("no default input device"))?;
    let supported = device.default_input_config()
        .context("default input config")?;
    let sample_rate = supported.sample_rate().0 as f32;
    let channels = supported.channels() as usize;
    let sample_format = supported.sample_format();
    let stream_cfg: StreamConfig = supported.into();

    eprintln!(
        "tuner-cli live: {} — {:.1} kHz, {} ch, format {:?}, window={} hop={}",
        device.name().unwrap_or_else(|_| "?".into()),
        sample_rate / 1000.0, channels, sample_format, cfg.window_len, hop,
    );

    let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(cfg.window_len * 4)));
    let err_fn = |e| eprintln!("stream error: {e}");

    let stream = {
        let buf = buffer.clone();
        match sample_format {
            SampleFormat::F32 => device.build_input_stream(
                &stream_cfg,
                move |data: &[f32], _| push(&buf, data, channels),
                err_fn, None,
            ),
            SampleFormat::I16 => device.build_input_stream(
                &stream_cfg,
                move |data: &[i16], _| {
                    let f: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                    push(&buf, &f, channels);
                },
                err_fn, None,
            ),
            SampleFormat::U16 => device.build_input_stream(
                &stream_cfg,
                move |data: &[u16], _| {
                    let f: Vec<f32> = data.iter()
                        .map(|&s| (s as f32 - 32768.0) / 32768.0).collect();
                    push(&buf, &f, channels);
                },
                err_fn, None,
            ),
            other => return Err(anyhow!("unsupported sample format: {other:?}")),
        }.context("build_input_stream")?
    };
    stream.play().context("stream.play")?;

    // Main loop: drain the buffer in hop-sized chunks, run the detector, print.
    let mut det = McLeodDetector::new(cfg.clone());
    let mut window = vec![0.0_f32; cfg.window_len];
    let start = Instant::now();

    // Wait for the first full window's worth of samples to accumulate.
    eprintln!("listening… (Ctrl-C to stop)");

    loop {
        // Drain new samples into a local scratch.
        let new = {
            let mut guard = buffer.lock().unwrap();
            std::mem::take(&mut *guard)
        };
        if new.is_empty() {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        // Slide-window via the existing `window` buffer: shift left, append.
        // (Cheap for hop sizes we care about; if it gets hot, swap for a ring.)
        if new.len() >= window.len() {
            // Took more than one window — keep the most recent.
            let start_idx = new.len() - window.len();
            window.copy_from_slice(&new[start_idx..]);
        } else {
            let keep = window.len() - new.len();
            window.copy_within(new.len().., 0);
            window[keep..].copy_from_slice(&new);
        }
        let pitch = det.detect(&window, sample_rate);
        print_frame(start.elapsed().as_secs_f64(), pitch);

        // Sleep approximately one hop's worth so we don't busy-loop.
        std::thread::sleep(Duration::from_secs_f32(hop as f32 / sample_rate));
    }
}

/// Mono-downmix and append into the shared buffer.
fn push(buf: &Arc<Mutex<Vec<f32>>>, data: &[f32], channels: usize) {
    let mut guard = buf.lock().unwrap();
    if channels <= 1 {
        guard.extend_from_slice(data);
    } else {
        let inv = 1.0 / channels as f32;
        for frame in data.chunks_exact(channels) {
            guard.push(frame.iter().sum::<f32>() * inv);
        }
    }
}
