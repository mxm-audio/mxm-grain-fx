# AGENTS.md — crates/mxm-grain-fx-dsp/

Parent: [`../../AGENTS.md`](../../AGENTS.md) · The measurements and reasoning behind each rule:
[`NOTES.md`](NOTES.md)

# Purpose

Framework-free, zero-dependency DSP for `mxm-grain-fx`: a granular processor on a **live capture
buffer** — a circular recording the input is still being written into, granulated by a scheduler
that reaches all three onset families, with a recycled feedback loop and a held-buffer freeze.

Technique evidence: `research:effects/granular-processing.md`. One implementation read to
build-from depth: `research:effects/mutable-clouds.md`. **This is an original design against those
references, not a copy of either**, and every constant here is chosen or measured for this
implementation. It was written clean-room with no source open, so it carries no third-party notice
([NOTES.md § Licensing](NOTES.md#licensing-and-why-this-crate-carries-no-third-party-notice)).
Product plan: `plans/plan-mxm-grain-fx.md` in the private archive.

# Ownership

- `src/lib.rs` — the engine: controls, the per-sample loop, normalisation, the recycled feedback
  path, freeze, the engagement edges and the activity/tail declarations.
- `src/capture.rs` — the circular buffer, addressed by age; the four-point reader; the record head's
  **hold**; the release crossfade.
- `src/grain.rs` — one grain, and `place`, the two-heads guard.
- `src/scheduler.rs` — the two onset branches and the two-segment scatter control between them.
- `src/window.rs` — the boxcar → triangle → Hann morph, its table and its moments.
- `src/rng.rs` — the one seeded generator every random draw comes from.
- The onset log lives in `src/lib.rs` beside the engine that writes it: `Onset`, `ONSET_LOG`,
  `clock()`, `onsets()` and `onsets_since()`.
- `examples/loop_probe.rs`, `examples/gate_measurements.rs` — the measurements in NOTES.md.
- `examples/grain_cost.rs` — what a grain costs here, and therefore whether `MAX_GRAINS` is a
  processor limit or a product decision.

No plugin-framework, parameter, preset, MIDI, editor or host type belongs here.

# Local Contracts

## The read never crosses the record head, in either direction or head state

For a grain's whole life the age of the sample it reads stays strictly inside the buffer's history
([NOTES.md § The read](NOTES.md#the-read-never-crosses-the-record-head-in-either-direction-or-head-state)).

- `grain::place` is the guard: a legal start window plus a length cap. It places against both worst
  cases — forward **frozen**, reverse **recording** — which is also what makes freezing mid-grain
  safe.
- **Both directions and both head states are proved at G1**, whatever the panel exposes: the sweep
  in `grain.rs` walks every direction, rate, length, position and head state, plus freezing at every
  phase of a grain.
- **The record head's hold is a primitive here**, not part of freeze. Freeze is a product state
  built on it.
- **Age is recomputed from counters, never accumulated**: two integer counters and one multiply.

## The reader

- Four-point Catmull-Rom. A read at age `a` takes taps at `a − 1 .. a + 2`; the guard holds `a`
  inside `[MARGIN, history − MARGIN]`, and taps are held tap by tap rather than by clamping the age.
- There is no mipmap yet; the aliasing above unity rate is the source's own folding, not the reader
  ([NOTES.md § The reader](NOTES.md#the-reader-is-four-point-and-the-margin-was-already-the-right-size)).

## Draws, position and the scheduler

- **Stereo width is one uniform draw**, because it is one quantity; there is no separate `spread`.
  Its worst case is proved by `the_fold_law_is_the_identity_at_centre_and_sums_at_the_edge`
  ([NOTES.md § Stereo width](NOTES.md#stereo-width-is-one-draw-because-it-is-one-quantity)).
- **Position is an absolute delay**: it addresses `position × BUFFER_S`, clamped into what the guard
  allows, never a fraction of `[lowest_age, highest_age]`
  ([NOTES.md § Position](NOTES.md#position-is-an-absolute-delay-not-a-fraction-of-the-history)).
- The scheduler has two branches, a phasor and a per-sample Bernoulli draw, and the scatter control
  moves between them in two segments: **Jitter** up to `HANDOVER_AT` (exactly one onset per slot),
  then **Hand-over** (each slot's onset suppressed with probability `q` while a per-sample draw fires
  at `q / slot`, so density does not move).
- Determinism: the generator is **seeded on `reset()`**, an onset lands on the **sample the draw
  chose**, and the draw is **per sample, never per block**
  ([NOTES.md § The scheduler](NOTES.md#the-scheduler-reaches-all-three-families-through-one-control)).

## Normalisation, window and level

- Two laws: coherent overlap by the reciprocal of the window's mean, incoherent by one over the
  square root of the count times the mean-square. The incoherent share is the hand-over **times** the
  open-ness of the position and rate depths; err toward the coherent law, which never
  under-compensates. The count is the **target** from the density control, never the pool's live
  occupancy ([NOTES.md § Two normalisation laws](NOTES.md#two-normalisation-laws-and-what-actually-selects-between-them)).
- The window is one triangular ramp shaped two ways: boxcar → triangle → Hann, the smooth end
  **exactly** Hann. The published implementation's silent midpoint is not reproduced;
  `no_shape_is_silent` keeps it that way
  ([NOTES.md § The window](NOTES.md#the-window-is-a-spectrum-control-and-the-midpoint-is-not-silent)).
- The level draw is decibels within `MAX_LEVEL_DROP_DB`. It cannot boost, depth zero is exactly
  unity (both pinned in `level_law`), the draw stays unconditional at zero depth, and
  `target_normalisation` ignores the level depth. The range is a listening question
  ([NOTES.md § The level draw](NOTES.md#the-level-draw-is-decibels-so-a-grain-is-quieter-rather-than-gone)).

## The onset log

- Every onset that reaches a grain is recorded (sample clock, starting age), at the end of `spawn`,
  so an onset the pool refused is not a mark. `onsets_since` is a **window on the recent past, not a
  queue**: reading does not consume. `ONSET_LOG` drops the oldest under pressure. A display draws
  these, never marks computed from the controls
  ([NOTES.md § The onset log](NOTES.md#the-onset-log-exists-so-a-display-cannot-invent-what-it-draws)).

## The loop, the tail and freeze

- Every path returning to the buffer is in one gain budget, a saturator sits in the loop, and a
  high-pass whose corner rises with the feedback amount removes sub-audio energy. The budget is
  proved at the density extreme **and** at the worst simultaneous in-domain draw of every
  randomisation family.
- `Tail(n)` is recomputed every block by a **rule, not a list**: every parameter that can move the
  latest audible read or the recycled gain, at its worst in-domain draw. It may overestimate and must
  not intentionally underestimate. **Sustaining is measured on the signal**, never read off the
  controls, by a level follower whose time is converted from the active rate
  (`the_sustain_measurement_does_not_depend_on_the_sample_rate`)
  ([NOTES.md § The loop](NOTES.md#the-loop-is-bounded-by-its-saturator-and-the-tail-is-a-rule)).
- Freeze: the record head stops, grains keep reading the held buffer, feedback is muted on a ramp,
  and **nothing is substituted for the muted loop**. Release crossfades over the side buffer. The
  input is never disconnected; only the wet field is held
  ([NOTES.md § Freeze](NOTES.md#freeze-holds-the-wet-field-the-dry-still-passes)).

## Mix at zero is Off, and both edges are decided

Reaching zero fades the wet to **exact** zero — snapped, so the output is dry to the bit — then
empties the buffer and pool once and parks. Re-engaging starts from empty and fades in. A fade
interrupted by the other edge restarts rather than resuming, so automation crossing zero repeatedly
under a live feedback tail cannot expose held state. Parking is audio-neutral by construction: it
only happens once the wet gain is exactly zero, which is what keeps it from making the render depend
on block size.

## The pool and the warts

- `MAX_GRAINS = 64` is a product decision, not a processor limit: anyone raising it is making a
  musical argument and should say so. Sub-sample onsets would reopen `place` and its sweep; they are
  a decision with a cost ([NOTES.md § The pool](NOTES.md#the-pool-is-a-product-decision-not-a-processor-limit)).
- Kept deliberately: periodic AM where windows do not tile, the boxcar clicks, feedback collapse,
  Length stopping at high upward shift, and the first `BUFFER_S` seconds repeating rather than
  delaying. Not reproduced, because they are defects: the silent window midpoint, and degrading
  grain quality once the pool is a quarter used
  ([NOTES.md § The warts](NOTES.md#the-warts-kept-deliberately)).

# Work Guidance

- **Measure a closed-loop change.** Compiling a granular feedback effect proves nothing about its
  character or its stability, and three separate design errors here were found by measurement after
  passing every test: the buffer-fill transposition, the over-corrected incoherent law, and the
  sideband probe pointed at the grain rate instead of the onset rate.
- Keep the boundedness oracle and listening apart. Tests can prove finite, bounded, silent,
  deterministic and partition-invariant; they cannot approve the cloud.
- **A fixed per-sample coefficient is a bug in this crate, not a shortcut.** The engine's own rule
  that every time is converted from the active rate had exactly one exception and it was wrong at
  both ends of the validator's sweep. `grep` for a bare float multiplying a difference before
  believing there are none left.
- **Measure both arms of a swap in one session, and quote the pair.** A cost figure here drifts a
  few percent between sessions, so subtracting this week's number from a figure in the DOX measures
  the week. Build both binaries, run them alternately, and report the difference.
- Do not extract a shared buffer, window or scheduler primitive. The granular **instrument** the
  owner intends next is the second honest implementation the root's crate rule waits for; it reads a
  stored sample and has none of this crate's two-heads problem, so what is genuinely shared will
  only be visible once it exists.

# Verification

```bash
cargo test -p mxm-grain-fx-dsp
cargo clippy -p mxm-grain-fx-dsp --all-targets
cargo run -p mxm-grain-fx-dsp --release --example loop_probe
cargo run -p mxm-grain-fx-dsp --release --example gate_measurements
cargo run -p mxm-grain-fx-dsp --release --example grain_cost
```

What the tests cover: [NOTES.md § What the tests cover](NOTES.md#what-the-tests-cover).

**Not verified here**: the plugin shell (G4), the editor (G5), the validator, the player and Bitwig
(G6). Fidelity is UNVERIFIED by the root's default — nothing was measured against any product, and
the listening gate (G3) is the owner's.

# Child DOX Index

None.
