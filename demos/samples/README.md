# Demo samples

These five samples (`kick`, `snare`, `hihat`, `epiano`, `sax`) are **synthesized
from code**, not recorded or downloaded from anywhere. They are produced by
[`tools/gen-demo-samples`](../../../tools/gen-demo-samples/), and are covered
by the same license as the rest of the project.

To regenerate them (the output is fully deterministic, so the files come out
identical), from the repository root:

```bash
cargo run -p gen-demo-samples
```

| File | Sound | Length | Root note |
|---|---|---|---|
| `kick.wav` | sine sweep 150 → 45 Hz with a short click | 0.50 s | none (percussion, default C4) |
| `snare.wav` | high-passed noise burst over two short tones | 0.25 s | none (percussion, default C4) |
| `hihat.wav` | very short high-passed noise plus metallic partials | 0.12 s | none (percussion, default C4) |
| `epiano.wav` | two-operator FM electric piano | 2.50 s | C3 |
| `sax.wav` | shaped harmonics with vibrato and breath noise | 1.20 s | C3 |

The pitched samples are synthesized at the note declared in `song1.bm1`
(`rootNote`/`rootOctave`), and a test in the generator checks it.
