# Tuner

A cross-platform guitar/bass tuner written in Rust. Designed from the start to
run both as a standalone desktop app and as a DAW plugin (CLAP + VST3) from a
single codebase.

Status: **early development.** Core DSP works; standalone UI and plugin frontend
are not yet built.

## Architecture

Three layers, each in its own crate:

```
crates/
├── tuner-core/      # Pure DSP. No I/O. McLeod Pitch Method (MPM) on FFT-based NSDF.
├── tuner-engine/    # Host-agnostic glue: ring buffer + smoothing + lock-free output.
└── tuner-cli/       # Validation CLI: analyse WAV files or live mic input.
```

Planned but not yet present:

```
├── tuner-app/       # Standalone desktop app (Slint UI + cpal).
└── tuner-plugin/    # CLAP + VST3 via nih-plug, reusing tuner-engine.
```

See [`.plans/01-architecture.md`](.plans/01-architecture.md) for the full design
and [`.plans/02-smoother-tuning.md`](.plans/02-smoother-tuning.md) for notes on
how the smoother defaults were calibrated from a real bass recording.

## Quick start

```bash
# Run all tests.
cargo test

# Analyse a WAV file (mono or stereo; auto-downmixed):
cargo run --release -p tuner-cli -- file path/to/recording.wav

# Live from the default input device:
cargo run --release -p tuner-cli -- live
```

Output format:

```
   0.405s  E1    +6.5c     41.36 Hz   conf 1.00
   0.427s  E1    +5.5c     41.34 Hz   conf 1.00
```

`time  note  cents-from-target  detected-Hz  confidence`

### CLI flags

| Flag | Default | Notes |
|---|---|---|
| `--window` | 4096 | Analysis window size in samples. Larger = more accurate at low frequencies, more latency. |
| `--hop` | 1024 | Samples between successive analysis frames. |
| `--a4` | 440.0 | Reference frequency for A4 (Hz). |
| `--min-confidence` | 0.5 | Frames below this NSDF peak value are dropped. |

## Crate API sketch

### `tuner-core`

Pure DSP. Feed samples, get a `Pitch` back:

```rust
use tuner_core::{McLeodDetector, DetectorConfig};

let mut det = McLeodDetector::new(DetectorConfig::default());
if let Some(p) = det.detect(&samples, 48_000.0) {
    println!("{}{} {:+.1}c ({:.2} Hz, conf {:.2})",
        p.note.name, p.note.octave, p.cents, p.hz, p.confidence);
}
```

### `tuner-engine`

Host-agnostic. Audio thread feeds samples; UI thread reads latest pitch
without locks (via `triple_buffer`).

```rust
use tuner_engine::{TunerEngine, EngineConfig};

let (mut engine, mut handle) = TunerEngine::new(EngineConfig::new(48_000.0));

// audio thread (cpal callback, plugin process(), …):
engine.process(&samples);

// UI thread:
if let Some(p) = handle.latest() {
    draw_needle(p);
}
```

The engine applies a four-stage smoothing pipeline to detector output:

1. **Confidence gate** — drop frames below `min_confidence`.
2. **Octave guard** — snap near-exact 2× / ½× ratios with falling confidence
   back to the established octave. Catches MPM's classic decay-phase failure
   where the 2nd harmonic dominates the NSDF.
3. **Median filter on Hz** — kill remaining single-frame outliers.
4. **Voiced-hold** — hold the previous note through brief low-confidence
   gaps so the UI doesn't flicker.

Tuning presets (`GUITAR_STANDARD`, `GUITAR_DROP_D`, `BASS_4_STANDARD`,
`BASS_5_STANDARD`) are exposed as data; the engine itself always reports the
nearest equal-tempered note.

## How the detector works

`tuner-core` uses the **McLeod Pitch Method** (Philip McLeod & Geoff Wyvill,
"A Smarter Way to Find Pitch", 2005). It:

1. Computes the **Normalized Square Difference Function (NSDF)** of the input
   window, using FFT-based autocorrelation in O(N log N).
2. Walks the NSDF to find key maxima (one per positive region).
3. Picks the first peak whose value exceeds `peak_threshold × global_max` —
   this prefers the fundamental over louder harmonics.
4. Applies **parabolic interpolation** around the chosen lag for sub-sample
   accuracy.
5. Returns Hz, the nearest 12-TET note, and cents deviation.

Chosen over YIN for better behaviour on bass-range frequencies; chosen over
plain FFT because FFT bin resolution is too coarse for tuning without
interpolation. The NSDF peak value doubles as a free confidence metric.

## Verified accuracy

Unit tests synthesise problem cases and assert tolerance in cents:

| Case | Window | Tolerance | Result |
|---|---|---|---|
| A4 sine (440 Hz) | 4096 | 1.0 c | ✓ |
| Guitar high E sawtooth (329.6 Hz) | 4096 | 5.0 c | ✓ |
| 5-string bass low B sawtooth (30.9 Hz) | 8192 | 10.0 c | ✓ |
| Silence → `None` | 4096 | — | ✓ |

Real-signal validation (bass low-E, sustained pluck) is documented in
[`.plans/02-smoother-tuning.md`](.plans/02-smoother-tuning.md), including the
sustained-decay octave glitch that drove the octave-guard design.

## Build

Requires Rust 1.85 (workspace pins `edition = "2024"`).

```bash
cargo build --release
cargo test
```

## Roadmap

1. [x] `tuner-core` — MPM detector with sub-cent accuracy on synthesised signals
2. [x] `tuner-cli` — file + live modes
3. [x] `tuner-engine` — ring buffer + smoothing + lock-free publish
4. [ ] `tuner-app` — Slint standalone UI (pending Slint licence review)
5. [ ] `tuner-plugin` — CLAP + VST3 via nih-plug
6. [ ] Mobile targets (Android/iOS)

## License

(pending on Slint adoption)
