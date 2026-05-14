//! Live mic capture via cpal, driving `tuner-engine`.
//!
//! The engine is moved into the cpal callback (single audio-thread owner).
//! The handle stays on the main thread and is polled at the display rate.

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use std::time::{Duration, Instant};
use tuner_core::DetectorConfig;
use tuner_engine::{EngineConfig, SmootherConfig, TunerEngine};

use crate::print_frame;

/// How often the UI polls the handle. 30 Hz is the usual "feels live" threshold;
/// faster doesn't help when the analysis hop is what limits new data.
const DISPLAY_HZ: f32 = 30.0;

pub fn run(detector: DetectorConfig, hop: usize) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("no default input device"))?;
    let supported = device
        .default_input_config()
        .context("default input config")?;
    let sample_rate = supported.sample_rate() as f32;
    let channels = supported.channels() as usize;
    let sample_format = supported.sample_format();
    let stream_cfg: StreamConfig = supported.into();

    eprintln!(
        "tuner-cli live: {} — {:.1} kHz, {} ch, format {:?}, window={} hop={}",
        device
            .description()
            .as_ref()
            .map(|d| d.name())
            .unwrap_or("?"),
        sample_rate / 1000.0,
        channels,
        sample_format,
        detector.window_len,
        hop,
    );

    let engine_cfg = EngineConfig {
        detector,
        sample_rate,
        hop,
        smoother: SmootherConfig::default(),
    };
    let (engine, mut handle) = TunerEngine::new(engine_cfg);
    let err_fn = |e| eprintln!("stream error: {e}");

    let stream = build_stream(
        &device,
        &stream_cfg,
        sample_format,
        channels,
        engine,
        err_fn,
    )?;
    stream.play().context("stream.play")?;

    eprintln!("listening… (Ctrl-C to stop)");
    let start = Instant::now();
    let tick = Duration::from_secs_f32(1.0 / DISPLAY_HZ);
    loop {
        let pitch = handle.latest();
        print_frame(start.elapsed().as_secs_f64(), pitch);
        std::thread::sleep(tick);
    }
}

fn build_stream(
    device: &cpal::Device,
    stream_cfg: &StreamConfig,
    sample_format: SampleFormat,
    channels: usize,
    mut engine: TunerEngine,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream> {
    let mut scratch: Vec<f32> = Vec::with_capacity(2048);

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            stream_cfg,
            move |data: &[f32], _| {
                feed(&mut engine, data, channels, &mut scratch, |s| s);
            },
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            stream_cfg,
            move |data: &[i16], _| {
                feed(&mut engine, data, channels, &mut scratch, |s| {
                    s as f32 / i16::MAX as f32
                });
            },
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            stream_cfg,
            move |data: &[u16], _| {
                feed(&mut engine, data, channels, &mut scratch, |s| {
                    (s as f32 - 32768.0) / 32768.0
                });
            },
            err_fn,
            None,
        ),
        other => return Err(anyhow!("unsupported sample format: {other:?}")),
    }
    .context("build_input_stream")?;
    Ok(stream)
}

/// Convert + mono-downmix + push into the engine. No allocation per call after
/// the first one that grows `scratch`.
fn feed<S: Copy>(
    engine: &mut TunerEngine,
    data: &[S],
    channels: usize,
    scratch: &mut Vec<f32>,
    convert: impl Fn(S) -> f32,
) {
    let frames = data.len() / channels.max(1);
    scratch.clear();
    if frames > scratch.capacity() {
        scratch.reserve(frames - scratch.capacity());
    }
    if channels <= 1 {
        scratch.extend(data.iter().copied().map(&convert));
    } else {
        let inv = 1.0 / channels as f32;
        for frame in data.chunks_exact(channels) {
            let sum: f32 = frame.iter().copied().map(&convert).sum();
            scratch.push(sum * inv);
        }
    }
    engine.process(scratch);
}
