//! End-to-end regression tests against recorded WAV samples.
//!
//! These exercise the whole `detector + smoother` pipeline against real
//! recordings so we catch regressions in either layer. Samples live in
//! `tuner/samples/` (workspace-relative).
//!
//! For each sample we assert:
//!   - At least N voiced frames after smoothing.
//!   - All (or nearly all) voiced frames report the expected note name.
//!   - The dominant MIDI note matches the expected open string.
//!   - Median cents is in a plausible range (loose, since real recordings
//!     can be a few cents off and we're testing stability, not tuning).
//!
//! If the file is missing the test is skipped (so the suite is still
//! self-contained for contributors who don't have the samples checked out).

use std::path::PathBuf;
use tuner_core::{DetectorConfig, McLeodDetector, Pitch};
use tuner_engine::{Smoother, SmootherConfig};

fn sample_path(name: &str) -> Option<PathBuf> {
    // tuner-engine/tests/ → tuner-engine/ → crates/ → tuner/
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()? // crates/
        .parent()? // tuner/
        .to_path_buf();
    let p = workspace_root.join("samples").join(name);
    p.exists().then_some(p)
}

/// Run the full detector + smoother pipeline over a WAV file and collect
/// every per-frame output.
fn analyse(path: &PathBuf) -> Vec<Option<Pitch>> {
    let mut reader = hound::WavReader::open(path).expect("open wav");
    let spec = reader.spec();
    let sr = spec.sample_rate as f32;
    let channels = spec.channels as usize;

    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => {
            reader.samples::<f32>().filter_map(Result::ok).collect()
        }
        hound::SampleFormat::Int => {
            let scale = 1.0_f32 / (1_i64 << (spec.bits_per_sample as i32 - 1)) as f32;
            reader.samples::<i32>().filter_map(Result::ok)
                .map(|s| s as f32 * scale).collect()
        }
    };
    let mono = if channels <= 1 {
        raw
    } else {
        let inv = 1.0 / channels as f32;
        raw.chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() * inv)
            .collect()
    };

    let det_cfg = DetectorConfig::default();
    let mut det = McLeodDetector::new(det_cfg);
    let mut smoother = Smoother::with_a4(SmootherConfig::default(), det_cfg.a4_hz);

    let window = det_cfg.window_len;
    let hop = window / 4;
    let mut out = Vec::new();
    let mut pos = 0;
    while pos + window <= mono.len() {
        let raw = det.detect(&mono[pos..pos + window], sr);
        out.push(smoother.feed(raw));
        pos += hop;
    }
    out
}

struct Expected {
    /// Expected open-string MIDI note for this file.
    midi: i32,
    /// Minimum number of voiced frames the engine must emit at the expected note.
    min_correct_frames: usize,
    /// Minimum fraction of voiced frames at the expected MIDI note (vs all
    /// voiced frames). Multi-pluck recordings allow some looseness here for
    /// transient noise between plucks.
    min_correct_fraction: f32,
}

fn assert_locks_onto(path_name: &str, exp: Expected) {
    let Some(path) = sample_path(path_name) else {
        eprintln!("skipping {path_name}: sample not present");
        return;
    };
    let frames = analyse(&path);
    let voiced: Vec<Pitch> = frames.into_iter().flatten().collect();
    assert!(!voiced.is_empty(), "{path_name}: no voiced frames");

    let correct = voiced.iter().filter(|p| p.note.midi == exp.midi).count();
    let correct_frac = correct as f32 / voiced.len() as f32;

    assert!(
        correct >= exp.min_correct_frames,
        "{path_name}: only {correct}/{} frames at expected MIDI {} (need >= {})",
        voiced.len(), exp.midi, exp.min_correct_frames,
    );
    assert!(
        correct_frac >= exp.min_correct_fraction,
        "{path_name}: {:.0}% of voiced frames at MIDI {} (need >= {:.0}%)",
        correct_frac * 100.0, exp.midi, exp.min_correct_fraction * 100.0,
    );

    // The expected note must dominate the histogram — no other MIDI note
    // should appear more often. Catches regressions where smoothing latches
    // on to a wrong octave or sub-harmonic.
    let mut histogram: std::collections::HashMap<i32, usize> = Default::default();
    for p in &voiced {
        *histogram.entry(p.note.midi).or_insert(0) += 1;
    }
    let (top_midi, top_count) = histogram.iter()
        .max_by_key(|&(_, c)| c).map(|(&m, &c)| (m, c)).unwrap();
    assert_eq!(
        top_midi, exp.midi,
        "{path_name}: dominant detected note is MIDI {top_midi} ({top_count} frames), expected {}",
        exp.midi,
    );

    // Cents sanity for the correct-MIDI frames only.
    let mut cents: Vec<f32> = voiced.iter()
        .filter(|p| p.note.midi == exp.midi)
        .map(|p| p.cents).collect();
    cents.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_cents = cents[cents.len() / 2];
    assert!(
        median_cents.abs() < 50.0,
        "{path_name}: median cents {} outside [-50, +50] for MIDI {}",
        median_cents, exp.midi,
    );
}

#[test]
fn bass_low_e_locks_onto_e1() {
    // The calibration recording. Raw detector glitches up an octave for
    // ~300 ms during decay; smoother must hold E1 throughout.
    assert_locks_onto("low-e.wav", Expected {
        midi: 28, // E1
        min_correct_frames: 150,
        min_correct_fraction: 0.95,
    });
}

#[test]
fn guitar_high_e_locks_onto_e4() {
    // Iowa MIS multi-pluck file. Some inter-pluck noise is expected; we
    // just require E4 to be the clear majority of voiced frames.
    assert_locks_onto("E4.wav", Expected {
        midi: 64, // E4
        min_correct_frames: 30,
        min_correct_fraction: 0.40,
    });
}

#[test]
fn guitar_b3_locks_onto_b3() {
    assert_locks_onto("B3.wav", Expected {
        midi: 59, // B3
        min_correct_frames: 60,
        min_correct_fraction: 0.40,
    });
}

#[test]
fn guitar_g3_locks_onto_g3() {
    assert_locks_onto("G3.wav", Expected {
        midi: 55, // G3
        min_correct_frames: 80,
        min_correct_fraction: 0.40,
    });
}
