# bit-music composition format

This document is the **shared contract** between every component of the
project: `player` reads and plays it, `gui-editor` writes it, `gui-player`
reads it. Any change here affects all of them.

## File and extension

The content is JSON, but the file extension is **`.bm1`** (e.g.
`song1.bm1`), not `.json` — it unambiguously identifies the format.

## Overall structure

```json
{
  "metadata": { "...": "..." },
  "samples": [ { "...": "..." } ],
  "patterns": [ { "...": "..." } ],
  "arrangement": { "tracks": [ { "...": "..." } ] }
}
```

## `metadata`

| Field | Type | Required | Description |
|---|---|---|---|
| `version` | string | yes | format version |
| `title` | string | yes | composition title |
| `bpm` | integer | yes | beats per minute |
| `stepsPerBeat` | integer > 0 | no (default `4`) | how many `pattern.steps` fit in one beat. See [Timing: from step to real time](#timing-from-step-to-real-time) |
| `others` | array of `{key, value}` | no (default `[]`) | generic fields without enough identity to deserve their own dedicated field |

`others` is also used to configure default values recognized by the format
(see [Sample defaults](#sample-defaults-in-metadataothers)).

## `samples[]`

An audio sample together with the note/octave it was originally recorded
at (**root note / root key**, standard sampler terminology: the reference
note with no transposition, needed for the engine to compute the
pitch-shift when playing it at any other note).

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | unique identifier, referenced from `patterns[].sample` |
| `file` | string | yes | path to the audio file (`.wav`), absolute or relative (see [Sample path resolution](#sample-path-resolution)) |
| `rootNote` | string | no | note the sample is recorded at. Format: `<letter A-G><optional accidental # or b>`, e.g. `"C"`, `"C#"`, `"Eb"`. If omitted, resolved from the default (see below) |
| `rootOctave` | integer 0-8 | no | octave it's recorded at. If omitted, resolved from the default |

### Sample path resolution

`file` can be:
- **absolute**: used as-is.
- **relative** (including the case of just a bare file name, no directory,
  e.g. `"kick.wav"`): resolved with **the directory the `.bm1` itself
  lives in** as the base, not the working directory the player is run
  from. This lets a composition carry its samples along (in the same
  folder, or a relative one) and move together with them without breaking
  references.

Relative paths are preferred for this reason — it's what makes a
composition portable.

### Sample defaults (in `metadata.others`)

If a sample doesn't specify `rootNote`/`rootOctave`, `metadata.others` is
checked for:

- `sampleDefaultNote` (e.g. `"C"`)
- `sampleDefaultOctave` (e.g. `"4"`)

If those aren't there either, the final default is **`C4`** (bit-music
convention: no transposition = C at the center octave).

## `patterns[]`

A reusable "chunk": a `sample` (referenced by id) played following a
sequence of notes or silences.

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | unique identifier, referenced from `arrangement.tracks[].sequence` |
| `sample` | string | yes | id of an element in `samples[]` |
| `steps` | array of (string \| `null`) | yes | note sequence. Length **must be a multiple of 4** |

Each element of `steps` is:
- `null` → silence at that step
- a **full note**: `<letter A-G><octave 0-8><optional accidental # or b>`, e.g. `"C4"`, `"C4#"`, `"D3b"`

Each `step` lasts a fixed unit of time (the pattern's "grid"). The exact
step↔real-time relationship is derived from `metadata.bpm` and
`metadata.stepsPerBeat` — see [Timing: from step to real time](#timing-from-step-to-real-time).

### Timing: from step to real time

```
secondsPerBeat = 60 / bpm
secondsPerStep = secondsPerBeat / stepsPerBeat
```

With the default `stepsPerBeat = 4`, each `step` is a **sixteenth note**
assuming 4/4 time — i.e. a pattern of length 4 lasts exactly 1 beat, one of
length 16 lasts a full 4/4 bar. This also matches the rule that
`steps.length` be a multiple of 4: by default, every pattern lasts a whole
number of beats.

Example with `bpm=120`, `stepsPerBeat=4` (default): secondsPerBeat = 0.5,
secondsPerStep = 0.125s (125ms).

If `stepsPerBeat` is changed (e.g. to 3 for triplets, or 8 for
thirty-second notes), each step's duration adjusts accordingly — the
multiple-of-4 rule on `steps.length` doesn't change, it stays a fixed rule
of the format independent of the `stepsPerBeat` value.

## `arrangement.tracks[]`

The tracks that play **in parallel** with each other, each chaining
`patterns` by id over time.

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | track identifier |
| `sequence` | array of (string \| `null`) | yes | patterns chained over time |

Each element of `sequence` is:
- `null` → gap/silence at that position (column)
- the `id` of an element in `patterns[]`

### Synchronization between tracks (grid/column model)

`arrangement` is thought of as a **grid**: each position `i` of `sequence`
is a "column" shared by every track.

1. **Duration of column `i`** = the longest `steps.length` among every
   pattern occupying position `i` in any track (`null` doesn't contribute
   duration).
2. If every track has `null` in column `i` at the same time (nobody
   defines its duration), the column lasts the minimum unit: **4 steps**.
3. If a track's `sequence` is shorter than the arrangement's total number
   of columns, it **loops** (repeats its own sequence from the start)
   until it covers them all.
4. A trailing `null` in a `sequence` is valid and meaningful: if that
   sequence is the longest one, it adds trailing silence to the
   composition; if the track loops, it becomes the gap between one
   repetition and the next. There is no special case that forbids or
   reorders `null` at any position.

## Full example

See [`player/demos/songs/song1.bm1`](../player/demos/songs/song1.bm1).

## TODO / open items

- Valid range for `steps.length` beyond "multiple of 4" (upper bound?).
