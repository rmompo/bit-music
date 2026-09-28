# Stereo audio

Status: **implemented (2026-09-28).** The engine carries real,
per-channel audio end to end — decoding, rendering, mixing, live
playback, export and the GUI's oscilloscopes. A composition that is
entirely mono still behaves exactly as before (mixes and plays at one
channel); a composition with a stereo sample mixes, plays, exports and
displays in real stereo. This document was originally the analysis that
led to *deferring* this (see git history for that version); it now
records what was actually built, as the reference for future changes.

## Why it came up

The ask: make the `sax` demo sample stereo, and have the oscilloscopes
(the live trace behind a track's header, and behind a sample/pattern row
in the list panel) show both channels for it instead of one line — and,
once that was underway, the ask widened to "real stereo throughout"
(playback, mixing and export too, not just the scope).

## Guiding principle: additive first, cut over last

Every crate below grew a `_multi` sibling of its existing mono
function/type, unit-tested against synthetic multi-channel data, without
changing any existing mono function's behavior. Only `bm-session` (the
facade every application goes through) actually switches the app over to
using them — a single, well-tested integration seam, instead of a risky
change spread thin across the whole call graph.

## What changed, crate by crate

**`bm-dsp`** (`libs/bm-dsp/src/lib.rs`) — `AudioBuffer` gained a private
`channels: u16` field (`AudioBuffer::new` keeps defaulting it to `1`, so
every existing mono call site is unaffected; `AudioBuffer::new_multi`
sets it explicitly), plus `channels()`/`frames()` accessors. Data stays a
flat, **interleaved** `Vec<f32>` (`[L, R, L, R, ...]`), matching `hound`'s
own WAV layout. New multi-channel primitives sit alongside the mono ones:
`resample_multi` (de-interleaves per channel, resamples each with the
existing `resample`, re-interleaves), `mix_into_multi` (frame-addressed
wrapper over `mix_into`), and `upmix` (the opposite of `downmix`:
duplicates/cycles channels to *widen* a buffer — a no-op if the source is
already at least as wide as the target).

**`bm-wav`** (`libs/bm-wav/src/lib.rs`) — `load_wav`/`load_wav_bytes`/
`write_wav` are unchanged (still downmix on load, still write mono).
`load_wav_multi`/`load_wav_bytes_multi` decode without downmixing;
`write_wav_multi(path, data, sample_rate, channels)` writes any channel
count.

**`gen-demo-samples`** (`tools/gen-demo-samples/src/main.rs`) — `sax.wav`
is now genuinely stereo: two independent phase accumulators, the right
channel detuned 2 cents sharp for a natural width (not a bare duplicate),
written with `write_wav_multi`. Every other demo sample stays mono.

**`bm-render`** (`libs/bm-render/src/lib.rs`) — `render_tracks`/
`render_pattern` are unchanged (mono, `channels: 1`).
`render_tracks_multi`/`render_pattern_multi` take an explicit
`channels: u16` and upmix any narrower voice to it via `dsp::upmix`
before mixing it in, so a mono kick and a stereo sax can share one
composition. `mix_tracks` needed **no changes at all** — summing an
interleaved buffer position by position is already channel-count
agnostic as long as every track shares one, which `render_tracks_multi`
guarantees by construction. `TrackBuffer` also carries
`native_channels`: the *actual* widest channel count among the samples
that specific track plays, kept alongside its (possibly wider,
homogenized-to-fit-the-mix) `audio` buffer — see "the homogenization
trap" below.

**`bm-playback`** (`libs/bm-playback/src/lib.rs`) — `Core` (the
device-independent, real-time-safe inner engine) stores tracks/previews
interleaved at a shared `mix_channels` (the widest among everything that
can sound), plus each one's own pre-upmix native channel count
(`track_channels`/`preview_channels`, parallel arrays). `Core::fill`
mixes per mix-channel into a fixed-size stack array (`MAX_MIX_CHANNELS`,
no allocation in the audio callback), then reconciles with the device's
real channel count when writing the output frame: pass-through, average
down (stereo mix -> mono device), or duplicate (mono mix -> N-channel
device, today's original behavior). `scope_channels` (a free function,
so it's testable without a real audio device) de-interleaves a buffer
into one `scope_window` trace per channel; `Core::track_scope_multi`/
`preview_scope_multi` call it and then truncate to that source's own
native channel count — *not* `mix_channels` — so a mono track/preview
shows one trace even while mixed alongside a stereo one.
`track_scope`/`preview_scope` (the original, single-`Vec<f32>` API) are
unchanged in signature, now just channel 0 of the `_multi` result.

**`bm-session`** (`libs/bm-session/src/lib.rs`) — **the cutover.**
`Session` gained a `channels: u16` field (the widest channel count among
its own samples; `1` for an all-mono composition). Samples load via
`load_wav_multi`/`load_wav_bytes_multi` instead of the downmixing
`load_wav`/`load_wav_bytes`; tracks render via `render_tracks_multi` at
that channel count. `master_duration_seconds` accounts for it (divides by
`channels`, not treating every sample as one frame). The two WAV-export
call sites (`bm export --wav` in `player/src/commands.rs`, the GUI's File
> Export > WAV in `gui-player/src/app.rs`) write with
`write_wav_multi(..., session.channels)` instead of the mono `write_wav`.

**`gui-player`** — `Transport`'s preview/track scope methods return
per-channel traces (`Vec<Vec<f32>>` for a sample/pattern preview,
`Vec<Vec<Vec<f32>>>` for every track) instead of a flat `Vec<f32>`; the
single-channel versions were removed once nothing used them anymore.
`widgets::paint_scope_multi` draws one trace per channel, stacked in
equal horizontal bands (a single channel still draws exactly as the
original `paint_scope`, across the whole rect). Wired into the track
header (`arrangement.rs`), and the sample/pattern list rows
(`panels.rs::list_row`). Pattern previews render via
`render_pattern_multi` at *that pattern's own sample's* channel count
(see below for why that matters), not a shared or hardcoded one.

## The homogenization trap (why two extra fixes were needed after the cutover)

The first working end-to-end version had two remaining bugs, both with
the same shape: a place where a buffer's own `channels()` no longer
reflected what that specific source actually was, because something else
upstream had already widened it to fit alongside a wider sibling.

1. **Pattern previews.** `gui-player` was still building every pattern
   preview through the *mono-only* `render_pattern` (hardcoded
   `channels: 1`). For a mono pattern this happened to still report the
   right count, but for the `sax` pattern it silently corrupted the
   audio: `dsp::upmix` can only *widen*, never narrow, so forcing
   `channels: 1` onto an already-stereo source left its `[L, R, L, R,
   ...]` data untouched while everything downstream treated it as mono
   (i.e. twice as many "frames" as it should have, L and R samples
   treated as separate consecutive mono frames). Fixed by rendering each
   pattern preview through `render_pattern_multi` at its own sample's
   real channel count.

2. **Track scopes.** `TrackBuffer.audio` is deliberately rendered at the
   *whole composition's* shared channel count (every track must share
   one so `mix_tracks` can sum them position by position) — so a mono
   track's own buffer reports `channels() == 2` too, as soon as anything
   else in the composition is stereo, exactly like the stereo one. By the
   time this buffer reaches `bm-playback`, that information is already
   gone; truncating to "the buffer's own channel count" there has
   nothing correct left to truncate to. Fixed by having `bm-render` keep
   each track's *actual* native width on the side
   (`TrackBuffer::native_channels`, computed from the samples that
   specific track plays, before the homogenizing upmix), and having
   `gui-player::Transport` truncate each track's scope to that instead of
   trusting the engine's (necessarily shared) channel count.

The lesson generalizes: whenever a buffer gets upmixed to share a width
with something else, whatever needs to tell the difference again later
(a scope, most likely) needs that original width recorded on the side —
the buffer's own `channels()` alone is no longer enough once it has been
homogenized into a mix.

## What deliberately stayed mono-shaped

Every mono-only function (`AudioBuffer::new`, `resample`, `mix_into`,
`load_wav`, `write_wav`, `render_tracks`, `render_pattern`) is still
there, unchanged, and still used by anything that has no reason to care
about channels (most tests, and any future tool that just wants "the
samples" without touching multi-channel machinery). Nothing forces a
caller to opt into the `_multi` API.
