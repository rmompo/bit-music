# Stereo audio

Status: **deferred by decision (2026-09-28), not implemented.** The engine
is mono end to end, on purpose (see below); adding real stereo touches
5-6 crates and carries real regression risk to a mature, well-tested
mixing/playback core. This document is the analysis that led to
deferring it, kept so the decision doesn't have to be re-researched if
it comes back.

## Why it came up

The ask: make the `sax` demo sample stereo, and have the oscilloscopes
(the live trace behind a track's header, and behind a sample/pattern row
in the properties panel) show both channels for it instead of one line.

## Today: mono, everywhere, on purpose

`bm-dsp::AudioBuffer` (`libs/bm-dsp/src/lib.rs`) is a flat mono buffer:

```rust
pub struct AudioBuffer {
    pub data: Vec<f32>,
    pub sample_rate: u32,
}
```

No channel count anywhere in it. Every primitive built on it
(`resample`, `mix_into`, `normalize`, `peak`) assumes one channel; the one
function that even knows multi-channel audio exists is `downmix`, which
*collapses* it:

```rust
/// Averages interleaved channels down to mono.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32>
```

`bm-wav::load_wav`/`load_wav_bytes` call `downmix` right after decoding —
the crate's own doc comment says why: *"multi-channel files are downmixed
on load because the bit-music mixing engine works in mono."*
`bm_wav::write_wav` is hard-coded to `channels: 1` in its `hound::WavSpec`;
there is no stereo write path at all, which is also why
`tools/gen-demo-samples` (`sax()` and friends) only ever produces mono
`Vec<f32>` buffers.

`bm-render` (`render_voice`, `render_pattern`, `render_tracks`,
`mix_tracks`) takes it from there: every voice, every track's
`TrackBuffer.audio`, and the final master mix are all plain mono
`Vec<f32>`/`AudioBuffer`.

`bm-playback::Engine` mixes in real time from those same mono
`TrackBuffer`s. Its audio callback (`Core::fill`) computes **one** mixed
`f32` value per frame (summing every unmuted track plus any active
preview) and then writes that *same* value into every output channel:

```rust
let value = (value * volume).clamp(-1.0, 1.0);
for sample in frame.iter_mut() {
    *sample = T::from_sample(value);
}
```

Note the output *device* is not the limitation — `Engine::new` already
opens the stream with the device's real channel count
(`supported.channels()`), so on a stereo device the signal already goes
out over two channels. It is just centered mono duplicated onto both,
not two independent channels.

The oscilloscope data (`Engine::track_scope`/`preview_scope`, via
`scope_window`) reads directly from each track's own mono
`TrackBuffer.audio` — not from the final device-mixed signal — around the
current playback position. `gui-player`'s `Transport::track_scopes`/
`preview_scope_*` expose that as one `Vec<f32>` per track/sample, and
`widgets::paint_scope` draws it as a single line.

## What it would take

### Option A — stereo only for the preview/scope; mixing stays mono

- `bm-wav`: stop downmixing on load (or add a variant that keeps the
  channels, e.g. returning per-channel data alongside the mono mix), and
  add a stereo `write_wav` (or a variant) so `gen-demo-samples` can
  actually produce a stereo `sax.wav` — `sax()` would need a second
  (right) channel, e.g. a slightly detuned/phase-shifted copy for a
  natural stereo width, not just a duplicate.
- The render/mix path (`bm-render`, `bm-playback`) keeps downmixing to
  mono right before building each `TrackBuffer`, so actual playback and
  `bm export --wav` are unaffected. Something needs to hold onto the
  stereo source alongside its mono downmix for the scope to read from —
  most likely `bm-session::Session` keeping the original per-channel
  sample data next to the decoded mono one, or `bm-playback` accepting
  an optional second channel per track for its scope buffer only.
- `gui-player`: `Transport::preview_scope_sample`/`track_scopes` return
  either one trace or two; `widgets::paint_scope` gets a variant (or a
  sibling function) that overlays two traces instead of one;
  `arrangement.rs`/`panels.rs` call whichever fits.
- Touches: `bm-wav`, `gen-demo-samples`, `bm-session` and/or
  `bm-playback`, `gui-player`. `bm-dsp` and the actual mixing math in
  `bm-render`/`bm-playback::Core::fill` stay untouched.

### Option B — real stereo throughout

- `AudioBuffer` redesigned to carry a channel count and either
  interleaved or per-channel (planar) data.
- Ripples through `bm-dsp` (`resample`, `mix_into`, `normalize`,
  `downmix` all need multi-channel forms), `bm-render` (every render
  function produces N-channel buffers), `bm-playback` (`Core::fill` mixes
  per output channel instead of one shared value — this is where the
  device's real channel count, already available today, would finally be
  used for something), `bm-wav` (load *and* write keep channel data),
  `bm-session`, and the GUI's scopes.
- A decision is needed either way for `bm export --wav` and the GUI's
  Export WAV: stay mono, or export stereo too (and if the source is
  mono, what "stereo" even means there).
- Every existing test across these crates that builds an `AudioBuffer` or
  checks mixing/rendering output assumes the mono shape; most would need
  rewriting, not just extending.
- Multi-hour, cross-cutting change to a mature, well-tested engine.

## Decision

Deferred. If this comes back, **Option A is the pragmatic starting
point**: it delivers exactly what was asked (a stereo `sax.wav`, its
oscilloscope showing both channels) without touching the mixing/playback
core that everything else depends on. Option B is only worth it if actual
stereo *panning/imaging* in playback and export is wanted for its own
sake, not just to make one oscilloscope show two lines.
