# NOTES.md — crates/mxm-grain-fx-dsp/

The detail behind this folder's AGENTS.md: history, measurements, rationale and worked examples.
AGENTS.md is the contract; this file is the reference it links to.

## Licensing, and why this crate carries no third-party notice

Under the owner's ruling of 2026-09-09 (root `AGENTS.md`, *Research boundary*, *Published source
is a different question*), facts from a maker's permissively licensed source cross freely and only
**code** carries that licence's notice. This crate was written clean-room from the research prose
with no source open, so it carries none and was MIT like its siblings at the time. Product plan:
`plans/plan-mxm-grain-fx.md` in the private archive.

Since the 2026-10 split, the root `AGENTS.md` named here is the monorepo's, in the private archive;
its *Research boundary* (with *Published source is a different question*) is kept word for word in
mxm-kit's [`docs/collection-rules.md`](https://github.com/mxm-audio/mxm-kit/blob/main/docs/collection-rules.md#research-boundary),
and this repository's root `AGENTS.md`, *Research citations*, is its short form. The crate takes the
repository's licence, GPL-3.0-or-later.

## The read never crosses the record head, in either direction or head state

The contract this product has and no delay does. For a grain's whole life the age of the sample it
reads stays strictly inside the buffer's history. Both ends fail the same way — a splice of the
newest sample onto the oldest, in the middle of a grain, where the window is at unity and cannot
hide it.

`grain::place` is the guard. It takes the direction, the rate and the length, and returns a legal
start window plus a length cap where the direction and rate would otherwise exhaust the buffer. The
worst case differs by direction **and** by head state: forward is worst **frozen** (the read falls
at the full rate with no head retreating ahead of it), reverse is worst **recording** (the age grows
by both travels). Placing against both is also what makes freezing *mid-grain* safe, because under
mixed dynamics the age is bracketed by the two pure cases.

- **Both directions and both head states are proved at G1**, whatever the panel exposes, so no later
  decision can reopen the buffer. The sweep in `grain.rs` walks every direction, rate, length,
  position and head state, plus freezing at every phase of a grain.
- **The record head's hold is a primitive here**, not part of freeze. Freeze is a product state
  built on it.
- **Age is recomputed from counters, never accumulated.** Adding `write_advance - travel` per sample
  drifts: over an eight-thousand-sample grain, f32 addition on a value in the thousands loses enough
  to walk past the guard. Two integer counters and one multiply give the same trajectory with a
  single rounding. This was found by the sweep failing at 1.945 where 2.0 was proved.

## The reader is four-point, and the margin was already the right size

**Measured, and it is a quality change taken at a cost.** The buffer read was linear. mxm-kit's
`docs/oscillators/15-granular-in-the-wild.md` §15.9 prices that on a grain train — linear 29.7 dB
behind a four-point cubic at unity rate and 29.5 dB behind at half — and this engine's read is
*moving* at almost every setting the panel offers: any transposition, any reverse draw, and the
whole of freeze, where the head stops and the age falls at the full rate.

It is Catmull-Rom (Catmull and Rom, 1974), and its own error was measured here rather than inherited
from §15.9, because that section measures a windowed overlap-add of many grains and this is the one
interpolation underneath it. Error below signal, on a tone read back at each rate this crate can
ask for:

| tone | 0.25× | 1× | 4× | linear at 1× |
|---|---|---|---|---|
| 1/64 of the rate (750 Hz at 48 kHz) | −98.9 dB | −100.2 | −100.2 | −59.0 |
| 1/16 (3 kHz) | −61.9 | −62.1 | −62.1 | −34.9 |
| 1/8 (6 kHz) | −42.0 | −40.9 | −32.2 | −23.0 |
| 1/4 (12 kHz) | −21.1 | −19.2 | −15.4 | −11.3 |

**The advantage is frequency, not rate**, which §15.9's table does not say on its own: most of
linear interpolation's error here is its droop — a lowpass reaching −3.9 dB at Nyquist — and a
four-point spline flattens it. 40 dB at 750 Hz, 27 dB at 3 kHz, converging toward Nyquist where
neither reader is good and the source's own folding has taken over anyway. `loop_probe` shows the
consequence at the loop: with the droop gone, a recycling texture settles about 0.9 dB higher, and
no setting crossed between decaying and sustaining.

**The change cost nothing in the guard, because `MARGIN` was already 2.** A read at age `a` takes
taps at `a − 1 .. a + 2`, and the guard holds `a` inside `[MARGIN, history − MARGIN]`, so the young
tap cannot reach age 0 — the slot the record head is about to overwrite — and the old tap cannot
reach past the oldest written sample. Those are the same two splices `place` exists to prevent, and
neither is reachable from the reader either: the taps are held tap by tap rather than by clamping
the age, so a direct read at age 1 stays exact and only repeats the edge sample.

**The mipmap is still not here, and that is the remaining gap.** §15.9 measures a 22.7 dB floor at
2× that no interpolator repairs, because it is the source's own harmonics folding. Everything above
unity in the table is that floor, not this reader.

## Stereo width is one draw, because it is one quantity

**A misfiling, corrected 2026-09-11.** `Controls` carried a `spread` field whose law was
`spread * bipolar() * 0.5`, summed with `Randomisation::pan`'s identical law into a single pan
value. Two independent draws into one value is not two controls — and the panel could not explain
the difference between them because there was none to explain. The *depth* survived rather than the
misfiled field, because the depth is the member the plan's decision 9 ranges over and the gate can
still drop it.

Three consequences worth stating, because none of them is obvious from the diff:

- **The reach is unchanged and the distribution is not.** Two summed uniform draws are triangular:
  at full depth on both, an eighth of all grains hit a hard pan. One uniform draw reaches a hard pan
  at a vanishing rate. Uniform is what a control called *width* should mean, and the direction is
  safe — the fold returns *more* energy on one side, so fewer hard pans can only lower the peak.
- **It weakens `the_gain_budget_holds_at_the_worst_simultaneous_draw`, and that is recorded in the
  test.** The worst case is still proved, by
  `the_fold_law_is_the_identity_at_centre_and_sums_at_the_edge`, which calls `Grain::start` at a
  hard pan directly instead of waiting for the generator to draw one.
- **Every render moved**, because `spawn` now draws once where it drew twice and the generator's
  sequence shifted by one per grain. Nothing asserted broke; `gate_measurements`' stochastic rows
  moved by **at most 0.13 dB**, which is a different realisation of the same process rather than a
  changed law. The periodic table is bit-identical.

## Position is an absolute delay, not a fraction of the history

**Measured, and it changed the design.** Mapping the position control across `[lowest_age,
highest_age]` is the obvious thing and is wrong: `highest_age` grows while the buffer fills, so each
new grain starts further back than the last, the read point advances at `1 - position` samples per
sample, and **the whole input is transposed by that factor for the first `BUFFER_S` seconds** after
an insert or a reset. Measured at 440 Hz arriving as 352 Hz at position 0.2, then curing itself.

Position addresses `position × BUFFER_S` instead, clamped into what the guard allows. Before the
buffer holds that much the clamp repeats the oldest material, which is a smaller and more
explicable artefact and ends sooner.

## The scheduler reaches all three families through one control

Two branches are built — a phasor and a per-sample Bernoulli draw — and the scatter control moves
between them in **two segments**:

- **Jitter**, up to `HANDOVER_AT`: the onset is drawn inside a widening fraction of its slot. Still
  exactly one onset per slot, which is why this can never become the stochastic process however far
  it opens — a Bernoulli branch leaves slots empty and doubles up in others, and jitter cannot.
- **Hand-over**, above it: each slot's onset is suppressed with probability `q` while a per-sample
  draw fires at `q / slot`. The two rates sum to `1 / slot` throughout, so density does not move
  across the hand-over — only the statistics do — and at the top the phasor never fires.

Three conditions make a random element compatible with the collection's determinism, adopted from
mxm-shimmer's `crates/mxm-shimmer-dsp/AGENTS.md`: the generator is **seeded on `reset()`**, an onset
lands on the **sample the draw chose** rather than a block boundary, and the draw is **per sample,
never per block**. Only the third is new to this crate; shimmer already had the other two.

## Two normalisation laws, and what actually selects between them

Coherent overlap is compensated by the reciprocal of the window's mean; incoherent overlap by one
over the square root of the count times the mean-square. **Measured** (`gate_measurements`, 2):
using one law for both is wrong by up to 13 dB at density 32 — the wrong law is 17 dB out on the
decorrelated branch.

**A stochastic scheduler is not on its own enough to reach the incoherent law**, and the plan had it
crossfading on the hand-over alone. With every randomisation depth shut and the position control
still, every grain reads *the same age at the same rate*, so they sum coherently however their
onsets are scattered; the incoherent law then over-compensates by √N, measured as **+4.4 dBFS at
density 32 where it should have sat at −9**. What decorrelates grains is reading different material,
so the incoherent share is the hand-over **times** the open-ness of the position and rate depths.
Erring toward the coherent law is the safe direction: it never under-compensates, so the failure is
a texture too quiet rather than one that grows.

The count is the **target** from the density control, never the pool's live occupancy — a measured
count would depend on pool state and would not survive block partitioning or a reset.

## The window is a spectrum control, and the midpoint is not silent

Under a periodic scheduler the grain envelope is the waveform of an amplitude modulator at the onset
rate, so shape decides the sidebands. One triangular ramp shaped two ways covers boxcar → triangle →
Hann; the smooth end is **exactly** Hann, confirmed independently by its far-band energy measuring
machine zero.

**The published implementation's silent point is not reproduced.** At exactly the midpoint of its
morph both of its branches zero their own coefficient and every grain renders silent
(`research:effects/mutable-clouds.md` §3). That is a latent bug, not anybody's hardware, and the
collection does not reproduce defects. The slope law here gives an exact triangle at the midpoint
from either side, and `no_shape_is_silent` sweeps two thousand settings to keep it that way.

Measured (`gate_measurements`, 1): AM appears where the windows fail to **tile**, not everywhere.
Overlapping windows at a periodic rate sum to `density × mean(w)`, a constant wherever they tile, and
perfect tiling is perfect reconstruction with no modulation at all. A Hann and a triangle both reach
**exactly zero** sidebands at density 2; a near-boxcar keeps a residual because its one-hundredth
ramp does not quite tile. That is why the useful AM lives around one grain per grain-length.

## The level draw is decibels, so a grain is quieter rather than gone

It was `1 - depth * u`, a linear attenuation reaching **zero**: at any real depth some grains simply
did not sound, so the control put holes in a sparse texture instead of varying it, and its readout
could not name a unit. `MAX_LEVEL_DROP_DB` is the range and the draw is in decibels within it.

- **It cannot boost**, and depth zero is *exactly* unity, so a patch that does not ask for the
  control is untouched by it. Both are pinned in `level_law`.
- **The draw stays unconditional at zero depth.** Short-circuiting would make the generator's
  sequence depend on a control value, and two renders at different depths would stop being
  comparable.
- **It does not move the normalisation.** Over the draw's domain at full depth the old law has mean
  0.500 and RMS 0.577 against the new one's 0.542 and 0.582 — **+0.07 dB of ensemble power**, which
  is the whole argument for `target_normalisation` continuing to ignore the level depth.
- Twelve decibels is a starting point and a listening question. Changing it is one line and no
  identifier.

## The onset log exists so a display cannot invent what it draws

Every onset that reaches a grain is recorded — the engine's sample clock and the buffer age the
grain began reading — and `onsets_since` hands a host the ones it has not seen. It is a **window on
the recent past, not a queue**: reading does not consume, so an editor that misses a frame draws the
same marks one frame later rather than a hole.

**The alternative is what makes this worth its fields.** A display could compute marks from the
controls, and at the top of the scatter travel it would draw an even comb over a per-sample draw —
the point in the design where the sound is least like a comb. `mxm-chorus-06`'s Sweep records the
same rule for its own display: a picture with its own clock drifts from the sound, which is the lie
a display exists to prevent.

Recorded at the end of `spawn`, after the pool has given a slot, so **an onset the pool refused is
not a mark**. `ONSET_LOG` drops the oldest under pressure, which is the loss design system §13
licenses a display to take.

## The loop is bounded by its saturator, and the tail is a rule

Every path returning to the buffer is in one gain budget, a saturator sits in the loop, and a
high-pass whose corner rises with the feedback amount removes the sub-audio energy that actually
blows a granular loop up. The corner law is **a G2 measurement, not an inheritance** — shimmer
planned two tone filters inside its loop and measured both out.

The budget is proved at the density extreme **and at the worst simultaneous in-domain draw of every
randomisation family**, because zero depth does not dominate: the fold-pan law puts both channels'
energy on one side, above what centre returns.

`Tail(n)` is recomputed every block by a **rule, not a list**: every parameter that can move the
latest audible read or the recycled gain, each at its worst in-domain draw, so nothing a later
decision puts on the panel can fall outside the declaration. It may overestimate and must not
intentionally underestimate. **Sustaining is measured on the signal** over a long window and never
read off the controls — a large finite tail must not stand in for a texture somebody is holding.

**And the level follower that measurement reads is a time, which it was not.** It was a fixed 0.0005
per sample: 42 ms at 48 kHz, but 250 ms at 8 kHz and 2 s at the 1 kHz the validator sweeps down to,
where the follower's own lag was the whole comparison window and the measurement read the lag rather
than the field. `the_sustain_measurement_does_not_depend_on_the_sample_rate` fails on the old
coefficient with *a held texture was never reported as sustaining at 8000 Hz* — which is a host
being told it may truncate something somebody is holding, the exact outcome this rule exists to
prevent. Every other time in this crate was already converted from the active rate; this one is now
too, at the 42 ms it had at the rate it was written for.

## Freeze holds the wet field; the dry still passes

The record head stops and grains keep reading the held buffer. Feedback is muted on a ramp, because
a frozen buffer inside a feedback loop is an unconditional oscillator. **Nothing is substituted for
the muted loop** — a reverb is another processor, not a freeze setting. Release crossfades over the
side buffer, which holds what would have been written had the head never stopped.

The input is never disconnected. Shimmer's first freeze was rejected for exactly that
(`plugins/mxm-shimmer/AGENTS.md`); here only the wet field is held. (That file is in the mxm-shimmer
repository.)

## The pool is a product decision, not a processor limit

**Measured** (`grain_cost`, i9-13900K, on an idle machine — never one that is also building). A live
grain here costs **12.6 ns**, taken as the marginal against occupancy rather than read off a single
run: 63 live grains measure 842 ns per frame against 30 ns for an empty engine. **The whole 64-grain
pool is 4.0% of one core**, and 12.6 ns buys 1 650 grains at 48 kHz in all of it.

So `MAX_GRAINS = 64` stands on the scheduler and the character of the cloud, and on nothing to do
with cost — there is roughly 26× of headroom above it. Anyone raising it is making a musical
argument and should say so; the CPU will not be the objection.

**The window's `sin()` was tabled, and it bought less than the plan projected.** 2048 points of
`sin²(πx/2)`, linear-interpolated, replacing the one transcendental left on the per-grain path.
Measured across the swap in one session, on the same machine: the Hann end's extra over the ramp end
fell from **3.71 ns per grain to 1.97** — so the table saved 1.74 ns, not the 3.57 that pricing the
`sin()` alone suggested, because a lookup into 8 KB is not free while a 1.5 MB capture buffer is
already competing for the cache. mxm-kit's `docs/oscillators/10-granular.md` §10.8 lists the same
saving at 1.06× on the sibling sampler's cheaper kernel, and it is the smaller of the two numbers
that generalises.

**That saving was spent on the reader, and the engine came out 5% slower.** 12.0 ns per grain before
the pass, 12.6 after: the four-point read costs 2.64 ns per grain and the table returned 1.74 of it.
A 5% cost for 27 dB at 3 kHz is the right side of the trade on an engine using 4% of a core, and it
is recorded as a trade rather than as a win. **Both arms were measured in one session**, because the
crate's published 11.53 ns and this pass's 12.0 ns baseline are the same code a session apart — the
directory's own rule that a cost figure drifts a few percent between sessions, and that only a
within-session pair may be subtracted.

**Two further savings were measured and not taken.** Dropping the redundant clamp inside the window
lookup, and halving the table to 1024 points, both moved the frame time by less than the
run-to-run spread; the clamped 2048-point form is kept because it reads correctly on its own and
its error bound is what the window's 1e-6 test is met by. The list-compaction §10.6.1 prices at
2.8× sparse and 1.03× at a full pool is the next candidate and has the same shape: this pool is
walked in full every sample, which is real at low density and is not what 4% of a core is spent on.

**For scale, and it is the useful comparison**: a properly built stereo grain costs about 10 ns
(§10.6.1), a windowed *sine* 9.42 ns, and this crate's sibling `mxm-creative-sampler`'s Grain 93 ns
when first measured and **28.8 ns** since its kernel was tabled — the rest of its gap is sixteen
taps it keeps on purpose, measured at 23 dB of alias rejection against eight. This engine is within
1.3× of the technique's own floor and now reads with four taps rather than two.

**Fractional onsets, and why §10.3's 71 dB does not apply here.** That measurement is of a
*synchronous grain train used as an oscillator*, where each grain carries a synthesised carrier
whose phase is measured from its own onset, so rounding the onset decoheres the carriers and most of
the 71 dB is that cancellation. A grain here reads the buffer at an age measured from the moving
record head, so consecutive grains read material that advances exactly with their onsets and stay
coherent by construction — which is the same fact the 0.997 purity measurement behind Spray's
default records. What is left is the envelope train jittering by up to half a sample against a
fractional slot, on an AM this crate keeps as a wart and offers scatter to cure. Sub-sample onsets
would shift every read age by up to one grain-rate's worth and reopen `place` and its sweep, so they
are a decision with a cost rather than the free win §10.3 describes.

## The warts, kept deliberately

1. **Periodic amplitude modulation** at the onset rate, wherever the windows do not tile. The
   technique's own sound and the reason shape is a performed control; the cure is scatter, and the
   cure removes the character.
2. **The boxcar clicks.** Shape at zero is a one-hundredth attack, and it is the glitch setting.
3. **Feedback collapse.** With gaps between grains, recycling subdivides the material every pass
   until it is dust — measured at 0.53 s to −60 dB at density 0.5 and full scatter, against never
   at density 8. Red Panda documents the same behaviour on its own pedal. With recycle the only loop
   in v1 there is no other topology to move to, so it is character rather than a decision.
4. **Length stops responding at high upward shift**, because the placement cap binds. Two octaves up
   caps a grain at about a quarter of the buffer, and the alternative is a splice mid-grain.
5. **The first `BUFFER_S` seconds after an insert or reset repeat rather than delay**, while the
   buffer holds less than the position control asks for.

Not reproduced, because they are defects rather than warts: the silent window midpoint, and
degrading grain quality — and with it the window — once the pool is a quarter used.

## What the tests cover

Tests cover the placement sweep across every direction, rate, length, position and head state plus
freezing mid-grain; the buffer's four-point reader — its taps exact at whole ages, a ramp reproduced
between them, neither outer tap leaving the written history, and its error measured against the
linear reader it replaced across every rate; the window's landmarks, its exact Hann end, its
continuity, that no setting is silent, and its table held to the 1e-6 the computed form met; the
scheduler's periodicity, its non-drifting fractional slot, one-onset-per-slot jitter,
empty-and-doubled slots at the top, and a mean rate that holds across the whole control; and at
engine level silence-in-silence-out, block-partition invariance under a dense stochastic setting,
reset determinism, finiteness from 1 kHz to 768 kHz at control corners, the gain budget at the worst
simultaneous draw, Off dry to the bit, click-free engagement edges, freeze holding the field while
the dry passes, freeze muting the loop, the three activity states with a decaying counter-example,
the sustain decision holding at 8 kHz, 48 kHz and 192 kHz, both pool policies at an exhausted pool,
recovery from non-finite input, and the length control reaching all three regions. Plus the level
law's two properties — never above unity, exactly unity at zero depth — and the onset log: that a
regular clock records even intervals and a handed-over one does not, that the position draw spreads
the recorded ages and a shut one does not, that the ring wraps in order under more onsets than it
holds, and that `onsets_since` returns only what is new.
