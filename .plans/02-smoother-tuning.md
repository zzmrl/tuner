# Smoother Tuning — Calibration Notes

## Source data

- Bass guitar, open low-E string, ~5 seconds (recorded 2026-05-13).
- ~230 frames at hop=1024 samples / sample_rate=48 kHz (≈21 ms/frame).
- Bass was ~+5 cents sharp of E1 (last tuned the night before).

## Observations

### Stable detection is excellent

Across the run, voiced frames clustered tightly:
- Hz: 41.30 – 41.35 (range ≈ 1 cent)
- cents: +4.1 to +5.8 (jitter ≈ ±1 cent frame-to-frame)
- confidence: 1.00 sustained

→ The core MPM detector is precise enough that aggressive smoothing isn't needed.

### One real failure mode: decay-phase octave glitch

Between 3.755 s and 4.053 s (~300 ms, ~15 consecutive frames), detection
flipped to E2 (~82.5 Hz, exactly 2× the fundamental) with confidence dropping
from 1.00 → 0.90. This is the MPM canonical failure: as a note decays the
2nd harmonic dominates the NSDF and the algorithm latches onto half the true
period.

→ A 5-frame median **cannot** rescue this — the glitch is 3× wider than the
window. A wider median would either add too much latency or still be defeated
by longer glitches.

→ Confidence gating alone doesn't catch it: 0.90 is well above any practical
threshold.

→ **The right tool is octave-aware logic, not a longer median.**

## Resulting defaults

| Field | Old | New | Reason |
|---|---|---|---|
| `min_confidence` | 0.6 | 0.75 | Real voiced frames easily clear 0.9; tighter floor skips a noisy initial attack frame |
| `median_window` | 5 | 7 | Modest bump for ±1 c jitter; ~80 ms additional latency |
| `hold_frames` | 4 | 6 | ~128 ms note-hold across pick noise / brief silence |
| `octave_guard_ratio_tol` | — | 0.05 | New: accept ±5 % around 2× / ½× as an octave glitch |
| `octave_guard_conf_drop` | — | 0.05 | New: require confidence to drop ≥ 0.05 before snapping (prevents catching genuine octave jumps) |

## Octave-guard logic

1. Compare new Hz to the last *stable* Hz.
2. Compare new confidence to the last *baseline* confidence.
   - Baseline is preserved across snaps — otherwise a sustained glitch would
     drag the baseline down and stop triggering on subsequent frames.
3. If ratio ≈ 2.0 or 0.5 within tolerance **and** confidence dropped by
   ≥ `octave_guard_conf_drop`, snap to the established octave.
4. Otherwise accept the raw Hz unchanged.

## Regression tests

`tuner-engine::smoother::tests`:
- `octave_guard_handles_sustained_decay_glitch` — synthesises the bass low-E
  scenario (40 frames @ 41.3 Hz / 15 frames @ 82.6 Hz w/ conf 0.90 / recovery)
  and asserts output never deviates from 41.3 Hz.
- `octave_guard_respects_real_octave_jump` — high-confidence jump from 110 Hz
  → 220 Hz is accepted.

## Open questions / next data to collect

- **Guitar high-E** (E4 ≈ 330 Hz). Higher frequencies behave differently
  through the NSDF; expect *less* octave-glitch risk (more periods in the
  window) but more potential for the parabolic interpolation to misjudge.
  Currently un-tested due to bass-only setup. Could synthesise or borrow.
- **Attack transient** behaviour — the recording starts at conf 0.71 then
  climbs. The new 0.75 floor skips ~1 frame; in practice this is invisible
  to the user but worth re-checking on a sharper attack (slap bass / hard
  pick).
- **Slow decay tail** — does the guard handle even longer (>1 s) glitches?
  Test would require a longer decay recording.
