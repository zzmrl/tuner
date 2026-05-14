//! Frequency ↔ musical-note conversion.

use core::fmt;

/// The twelve pitch classes in 12-tone equal temperament.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteName {
    C,
    Cs,
    D,
    Ds,
    E,
    F,
    Fs,
    G,
    Gs,
    A,
    As,
    B,
}

impl NoteName {
    pub fn as_str(self) -> &'static str {
        match self {
            NoteName::C => "C",
            NoteName::Cs => "C#",
            NoteName::D => "D",
            NoteName::Ds => "D#",
            NoteName::E => "E",
            NoteName::F => "F",
            NoteName::Fs => "F#",
            NoteName::G => "G",
            NoteName::Gs => "G#",
            NoteName::A => "A",
            NoteName::As => "A#",
            NoteName::B => "B",
        }
    }

    fn from_pc(pc: i32) -> Self {
        match pc.rem_euclid(12) {
            0 => NoteName::C,
            1 => NoteName::Cs,
            2 => NoteName::D,
            3 => NoteName::Ds,
            4 => NoteName::E,
            5 => NoteName::F,
            6 => NoteName::Fs,
            7 => NoteName::G,
            8 => NoteName::Gs,
            9 => NoteName::A,
            10 => NoteName::As,
            11 => NoteName::B,
            _ => unreachable!(),
        }
    }
}

impl fmt::Display for NoteName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A specific note in scientific-pitch notation: pitch class + octave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    pub name: NoteName,
    pub octave: i32,
    /// MIDI note number (A4 = 69).
    pub midi: i32,
}

impl Note {
    /// Nearest equal-tempered note to `hz`, given a reference A4 frequency
    /// (typically 440.0).
    pub fn nearest(hz: f32, a4_hz: f32) -> Self {
        let midi_f = 69.0 + 12.0 * (hz / a4_hz).log2();
        let midi = midi_f.round() as i32;
        let pc = midi.rem_euclid(12);
        // MIDI 0 = C-1, so octave = midi/12 - 1.
        let octave = midi.div_euclid(12) - 1;
        Note {
            name: NoteName::from_pc(pc),
            octave,
            midi,
        }
    }

    /// Exact frequency of this note for the given A4 reference.
    pub fn frequency(&self, a4_hz: f32) -> f32 {
        a4_hz * ((self.midi - 69) as f32 / 12.0).exp2()
    }
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.name, self.octave)
    }
}

/// A detected pitch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pitch {
    /// Detected fundamental, in Hz.
    pub hz: f32,
    /// Nearest equal-tempered note.
    pub note: Note,
    /// Deviation from `note` in cents, range roughly [-50, +50].
    pub cents: f32,
    /// Confidence in [0, 1] — directly the NSDF peak value.
    pub confidence: f32,
}

impl Pitch {
    pub fn from_hz(hz: f32, confidence: f32, a4_hz: f32) -> Self {
        let note = Note::nearest(hz, a4_hz);
        let cents = 1200.0 * (hz / note.frequency(a4_hz)).log2();
        Pitch {
            hz,
            note,
            cents,
            confidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_is_a4() {
        let p = Pitch::from_hz(440.0, 1.0, 440.0);
        assert_eq!(p.note.name, NoteName::A);
        assert_eq!(p.note.octave, 4);
        assert_eq!(p.note.midi, 69);
        assert!(p.cents.abs() < 0.01);
    }

    #[test]
    fn low_b_bass() {
        // 5-string bass open low B ≈ 30.868 Hz, MIDI 23, B0.
        let p = Pitch::from_hz(30.868, 1.0, 440.0);
        assert_eq!(p.note.name, NoteName::B);
        assert_eq!(p.note.octave, 0);
        assert_eq!(p.note.midi, 23);
        assert!(p.cents.abs() < 1.0, "cents={}", p.cents);
    }

    #[test]
    fn high_e_guitar() {
        // High E on guitar (1st string) ≈ 329.628 Hz, MIDI 64, E4.
        let p = Pitch::from_hz(329.628, 1.0, 440.0);
        assert_eq!(p.note.name, NoteName::E);
        assert_eq!(p.note.octave, 4);
    }

    #[test]
    fn cents_sign() {
        // 10 cents sharp of A4.
        let hz = 440.0 * (10.0_f32 / 1200.0).exp2();
        let p = Pitch::from_hz(hz, 1.0, 440.0);
        assert!((p.cents - 10.0).abs() < 0.1);
    }
}
