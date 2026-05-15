//! Validation CLI for `tuner-core`.
//!
//! Two subcommands:
//!   - `file <path.wav>`  — analyse a WAV file frame-by-frame.
//!   - `live`             — capture from the default input device and print detections in real time.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tuner_core::{DetectorConfig, Pitch};

mod live;
mod wav;

#[derive(Parser)]
#[command(name = "tuner-cli", about = "Validation CLI for tuner-core")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,

    /// Analysis window size, in samples.
    #[arg(long, default_value_t = 4096, global = true)]
    window: usize,

    /// Hop size between successive frames, in samples.
    #[arg(long, default_value_t = 1024, global = true)]
    hop: usize,

    /// Reference frequency for A4.
    #[arg(long, default_value_t = 440.0, global = true)]
    a4: f32,

    /// Minimum confidence below which frames are reported as `--`.
    #[arg(long, default_value_t = 0.5, global = true)]
    min_confidence: f32,
}

#[derive(Subcommand)]
enum Cmd {
    /// Analyse a WAV file.
    File { path: PathBuf },
    /// Capture from the default input device.
    Live,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = DetectorConfig {
        window_len: cli.window,
        a4_hz: cli.a4,
        min_confidence: cli.min_confidence,
        ..Default::default()
    };
    match cli.cmd {
        Cmd::File { path } => wav::run(&path, cfg, cli.hop),
        Cmd::Live => live::run(cfg, cli.hop),
    }
}

/// Shared output formatter so file and live modes look identical.
pub(crate) fn print_frame(t_seconds: f64, pitch: Option<Pitch>) {
    match pitch {
        Some(p) => println!(
            "{:>8.3}s  {:<3}{:<1}  {:+6.1}c   {:7.2} Hz   conf {:.2}",
            t_seconds, p.note.name, p.note.octave, p.cents, p.hz, p.confidence
        ),
        None => println!("{:>8.3}s  --", t_seconds),
    }
}
