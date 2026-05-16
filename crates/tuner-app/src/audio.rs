//! cpal input -> tuner-engine. Lifted from tuner-cli/live.rs and trimmed.

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use tuner_core::DetectorConfig;
use tuner_engine::{EngineConfig, SmootherConfig, TunerEngine, TunerHandle};

pub struct AudioSession {
    _stream: cpal::Stream,
    pub handle: TunerHandle,
    pub sample_rate: f32,
    pub device_name: String,
}

pub fn start(detector: DetectorConfig, hop: usize) -> Result<AudioSession> {
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

    let device_name = device
        .description()
        .as_ref()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "default input".to_string());

    let (engine, handle) = TunerEngine::new(EngineConfig {
        detector,
        sample_rate,
        hop,
        smoother: SmootherConfig::default(),
    });

    let stream = build_stream(&device, &stream_cfg, sample_format, channels, engine)?;
    stream.play().context("stream.play")?;

    Ok(AudioSession {
        _stream: stream,
        handle,
        sample_rate,
        device_name,
    })
}

fn build_stream(
    device: &cpal::Device,
    stream_cfg: &StreamConfig,
    sample_format: SampleFormat,
    channels: usize,
    mut engine: TunerEngine,
) -> Result<cpal::Stream> {
    let mut scratch: Vec<f32> = Vec::with_capacity(2048);
    let err_fn = |e| eprintln!("stream error: {e}");

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            stream_cfg,
            move |data: &[f32], _| feed(&mut engine, data, channels, &mut scratch, |s| s),
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            stream_cfg,
            move |data: &[i16], _| {
                feed(&mut engine, data, channels, &mut scratch, |s| {
                    s as f32 / i16::MAX as f32
                })
            },
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            stream_cfg,
            move |data: &[u16], _| {
                feed(&mut engine, data, channels, &mut scratch, |s| {
                    (s as f32 - 32768.0) / 32768.0
                })
            },
            err_fn,
            None,
        ),
        other => return Err(anyhow!("unsupported sample format: {other:?}")),
    }
    .context("build_input_stream")?;
    Ok(stream)
}

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
