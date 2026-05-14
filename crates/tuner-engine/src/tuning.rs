//! Instrument tuning presets.
//!
//! A `Tuning` is just a list of open-string MIDI numbers plus a display name.
//! The engine doesn't *use* tunings directly — it always reports the nearest
//! 12-TET note. Frontends consult tunings to decide which string the user is
//! likely trying to tune and how to highlight it.

use tuner_core::{Note, NoteName};

/// Open-string definition for an instrument.
#[derive(Debug, Clone, Copy)]
pub struct Tuning {
    pub name: &'static str,
    /// MIDI numbers of the open strings, low to high.
    pub strings: &'static [i32],
}

impl Tuning {
    /// Iterate the open strings as [`Note`]s.
    pub fn notes(&self) -> impl Iterator<Item = Note> + '_ {
        self.strings.iter().map(|&midi| {
            let pc = midi.rem_euclid(12);
            let octave = midi.div_euclid(12) - 1;
            Note {
                name: pc_to_name(pc),
                octave,
                midi,
            }
        })
    }

    /// Closest string (by MIDI distance) to `midi`. Returns `(index, distance)`.
    pub fn nearest_string(&self, midi: i32) -> Option<(usize, i32)> {
        self.strings.iter().enumerate()
            .map(|(i, &s)| (i, (s - midi).abs()))
            .min_by_key(|&(_, d)| d)
    }
}

fn pc_to_name(pc: i32) -> NoteName {
    match pc.rem_euclid(12) {
        0  => NoteName::C,  1  => NoteName::Cs,
        2  => NoteName::D,  3  => NoteName::Ds,
        4  => NoteName::E,
        5  => NoteName::F,  6  => NoteName::Fs,
        7  => NoteName::G,  8  => NoteName::Gs,
        9  => NoteName::A,  10 => NoteName::As,
        11 => NoteName::B,
        _  => unreachable!(),
    }
}

// MIDI cheat sheet: E2=40, A2=45, D3=50, G3=55, B3=59, E4=64
//                   B0=23, E1=28, A1=33, D2=38, G2=43
pub const GUITAR_STANDARD: Tuning = Tuning {
    name: "Guitar standard (EADGBE)",
    strings: &[40, 45, 50, 55, 59, 64],
};

pub const GUITAR_DROP_D: Tuning = Tuning {
    name: "Guitar drop-D (DADGBE)",
    strings: &[38, 45, 50, 55, 59, 64],
};

pub const BASS_4_STANDARD: Tuning = Tuning {
    name: "Bass 4-string (EADG)",
    strings: &[28, 33, 38, 43],
};

pub const BASS_5_STANDARD: Tuning = Tuning {
    name: "Bass 5-string (BEADG)",
    strings: &[23, 28, 33, 38, 43],
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guitar_standard_notes() {
        let notes: Vec<_> = GUITAR_STANDARD.notes().collect();
        assert_eq!(notes.len(), 6);
        assert_eq!(notes[0].midi, 40); // E2
        assert_eq!(notes[0].name, NoteName::E);
        assert_eq!(notes[0].octave, 2);
        assert_eq!(notes[5].midi, 64); // E4
        assert_eq!(notes[5].octave, 4);
    }

    #[test]
    fn nearest_string_picks_closest() {
        // A2 = 45 should map to the A string (index 1) on standard guitar.
        let (idx, dist) = GUITAR_STANDARD.nearest_string(45).unwrap();
        assert_eq!(idx, 1);
        assert_eq!(dist, 0);
        // Slightly sharp of A2 still maps to A.
        let (idx, _) = GUITAR_STANDARD.nearest_string(46).unwrap();
        assert_eq!(idx, 1);
    }

    #[test]
    fn bass_5_includes_low_b() {
        let notes: Vec<_> = BASS_5_STANDARD.notes().collect();
        assert_eq!(notes[0].name, NoteName::B);
        assert_eq!(notes[0].octave, 0);
        assert_eq!(notes[0].midi, 23);
    }
}
