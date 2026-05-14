//! Pure-DSP pitch detection.
//!
//! No audio I/O, no UI, no async. Feed it `&[f32]` samples and a sample rate,
//! get back a [`Pitch`] (or `None` if the signal is too weak / unvoiced).

mod note;
mod nsdf;
mod mpm;

pub use note::{Note, NoteName, Pitch};
pub use mpm::{McLeodDetector, DetectorConfig};
