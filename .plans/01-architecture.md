# Cross-Platform Guitar/Bass Tuner — Architecture Plan

## Goals

- Standalone desktop tuner (Linux/macOS/Windows), with mobile (Android/iOS) as a later target.
- Audio plugin form (CLAP + VST3) usable inside DAWs, sharing a single codebase with the standalone.
- Accurate across full guitar + bass range, including low-B (~30.87 Hz) and high-E (~1318 Hz).

## Non-Goals (for v1)

- Polyphonic detection.
- Microtonal / non-12-TET temperaments (data-driven later).
- AU plugin format (Logic users — add later if demand exists).
- Web/wasm build (possible, not in initial scope).

## Architecture: Three Layers

### 1. `tuner-core` — pure DSP, no I/O

- Input: `&[f32]` samples + sample rate.
- Output: `Pitch { hz: f32, note: Note, cents: f32, confidence: f32 }`.
- Algorithm: **McLeod Pitch Method (MPM)**, hand-rolled (~150 LoC).
  - Chosen over YIN for better low-frequency behavior on bass strings.
  - Chosen over FFT-only since FFT bin resolution is too coarse without quadratic interpolation, and MPM gives confidence for free via the NSDF peak.
- Window: 4096 samples, 75% overlap, Hann window.
- Dependencies kept minimal: `realfft`, possibly `num-complex`. No tokio, no cpal, no slint.
- Should be `no_std`-friendly (use `alloc`) so it can move to embedded/mobile cleanly.

### 2. `tuner-engine` — host-agnostic glue

- Owns the ring buffer; accepts samples from any audio thread via `engine.process(&[f32])`.
- Calls into `tuner-core` when enough samples are buffered.
- Smoothing pipeline:
  - Median filter on `cents` (window ~5 frames) to kill jitter.
  - Hysteresis on note transitions to avoid flicker at semitone boundaries.
  - Confidence gating: drop frames below a threshold.
- Lock-free UI handoff: `triple_buffer` or `arc-swap` so the audio thread never blocks and the UI thread always reads the latest `Pitch` without locks.
- Tuning presets as data: guitar standard, drop-D, DADGAD, 7-string, bass 4/5/6-string, custom. No `match` ladders in code.

### 3. Frontends — thin shells around `tuner-engine`

- **`tuner-cli`** — `cpal` input + terminal needle. First milestone. Validation tool.
- **`tuner-app`** — `cpal` input + Slint UI. Standalone desktop application.
- **`tuner-plugin`** — `nih-plug` wrapper. Same `tuner-engine` driven by the plugin host's audio callback. Compiles to both CLAP and VST3 from one source.

## Workspace Layout

```
audiotools/
├── Cargo.toml              # workspace
├── crates/
│   ├── tuner-core/         # DSP only, no I/O
│   ├── tuner-engine/       # ring buffer + smoothing + lock-free pub
│   ├── tuner-cli/          # cpal + terminal needle (validation tool)
│   ├── tuner-app/          # cpal + slint standalone
│   └── tuner-plugin/       # nih-plug → CLAP + VST3
```

## Key Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Pitch algorithm | MPM (hand-rolled) | Bass-string accuracy + confidence metric |
| FFT crate | `realfft` | Real-input optimization, no `num-complex` overhead in hot path |
| Plugin framework | `nih-plug` (CLAP + VST3) | One Rust codebase → both formats |
| UI ↔ audio comms | `triple_buffer` | Lock-free, audio thread never blocks |
| UI toolkit | Slint (license TBD) | Already familiar; egui/iced are fallbacks if licensing is wrong |
| Audio I/O (native) | `cpal` | Cross-platform standard |
| Plugin UI | Start with `nih-plug`'s built-in (egui/vizia); port to Slint only if it pays off | Slint-in-host requires `raw-window-handle` embedding work |

## Open Risks

1. **Slint licensing.** Three tracks: GPLv3 (viral — bad for closed plugin), royalty-free (likely OK for free desktop), commercial (paid). Must read [slint.dev/pricing](https://slint.dev/pricing) before committing UI code. Fallback: `egui` or `iced` (both permissive).
2. **Slint inside a plugin host window.** Requires embedding via `raw-window-handle` into the host-provided HWND/NSView. Not as smooth as `nih-plug`'s built-in UI options. Plan to ship the plugin with `nih-plug`'s native UI first.
3. **Low-frequency accuracy.** Bass low-B at 30.87 Hz means a 4096-sample window at 48 kHz captures only ~2.6 cycles. Validation against known recordings is mandatory before declaring core "done."

## Build Order

1. **`tuner-core` + `tuner-cli` reading WAV files.** Print `note cents hz confidence` per frame. De-risks pitch detection. Validate against known recordings — open low-B is the hard case.
2. **`tuner-cli` with live `cpal` input.** Terminal needle. Confirm end-to-end latency feels right (<50 ms).
3. **`tuner-engine` extraction.** Pull smoothing + lock-free handoff out of the CLI.
4. **`tuner-app` Slint UI.** Needle, cents readout, tuning selector. (Confirm Slint license first.)
5. **`tuner-plugin` via `nih-plug`.** Reuse `tuner-engine`. Start with `nih-plug`'s built-in UI framework. Port to Slint only if it clearly pays off.

## Confirmed Scope (from user)

- Desktop only initially; mobile is a later phase.
- Plugin formats: CLAP + VST3 via `nih-plug`. AU deferred.
- Slint chosen as UI provisionally; licensing posture to be reviewed before locking in.
