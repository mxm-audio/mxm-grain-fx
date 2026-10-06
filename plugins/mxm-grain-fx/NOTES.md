# NOTES.md — plugins/mxm-grain-fx/

The detail behind this folder's AGENTS.md: history, measurements, rationale and worked examples.
AGENTS.md is the contract; this file is the reference it links to.

## Two tempo syncs

**Two tempo syncs** (2026-09-25, `plans/plan-tempo-sync-controls.md`), each the collection's quarter
note beside its knob: `ratesync` on `params::RATE_SYNC` (1/64 to a whole note, the top the fastest)
and `readdelaysync` on `params::READ_DELAY_SYNC` (1/64 to two bars, the top the longest). Read delay
is stored as a fraction of the four-second buffer, so its sync resolves in seconds over `0..BUFFER_S`
and returns to that fraction. `process` resolves both once a block; both are drawn once per grain at
its onset, so a division needs no smoothing. The timeline's read line and the knobs follow the
division when synced to a tempo (`Telemetry::tempo`).

## Vocabulary follows the property being varied

The words **Scatter, Spray and Random are excluded** because they do not distinguish onset timing
from grain properties. **A variation control sits beside the property it varies.** Grouping the
depths together made them look like a subsystem; the pairing is the only thing that tells a player
whether a control changes *when a grain starts* or *what a grain is*. Onset timing is the deliberate
exception — it sits beside Grain rate, because it varies the clock rather than a property of a
grain.

`docs/briefs/mxm-grain-fx.md` is the gating layout record; this section owns the naming rule.

## Density is a rate, and neither Delay variation nor Stereo width starts at zero

Density is a corrected rate law; Delay variation and Stereo width are declared deviations from the
shared *every amount starts at zero* rule.

- **Density was the number of grains sounding at once.** With overlap fixed, lengthening a grain
  could only slow the onset rate: the sounding count measured at **5 for every length from 10 ms to
  250 ms**, so a longer grain gave *fewer* grains rather than more overlap and the smooth thickening
  a granular processor exists for was unreachable. Reported as *"it just makes fewer grains"*.
  Density is now **grains per second**, Length and Density are independent, and overlap is
  `length × density`. `a_longer_grain_at_the_same_density_overlaps_more` pins it.
- **Delay variation starts at 30 %.** With it shut, every grain reads the same age at the same
  rate, so overlapping grains read *identical* material and the overlap-add reconstructs the input:
  measured purity **0.997** against a 440 Hz tone, which is a plain delay. `plugins/AGENTS.md` also
  rules that an effect starts engaged and demonstrates the effect it is named after, and where the
  two rules collide that one wins — a granular processor whose default is a delay demonstrates
  nothing. `overlapping_grains_decorrelate_when_the_spray_is_open` pins both directions.
- **Stereo width starts at 60 %, and this deviation was latent before it was declared.** It is the
  merged survivor of Spread and the pan depth (`crates/mxm-grain-fx-dsp/AGENTS.md`, *Stereo width is
  one draw*; the detail is in that crate's `NOTES.md`). Spread defaulted to 0.6 and had never been in
  the init-patch test — the half that *was* tested is the half that started at zero, so the non-zero
  one slipped through. Merged, it is a variation depth and therefore an amount, so it is declared:
  the plan's §7 rules this topology Stereo *because per-grain pan is the technique's stereo and it is
  meaningless in mono*, and a centred default would contradict the argument the stereo layout rests
  on.

Both are pinned by `every_amount_starts_at_zero`, which asserts each is above zero **and says why**
— a deviation that is only a default is an oversight; one with an assertion and a reason is a
decision. All five variation depths still ship, so the gate can audition the alternatives its
decision 9 chooses between.

## Pitch variation is skewed, because a detune is a few cents

The control must make a few cents of detune reachable without sacrificing its two-octave end. Two
independent contracts provide that:

- **The travel.** The depth reaches `MAX_PITCH_SEMITONES` either way — two octaves — and it was
  linear, against `crates/ui`'s single 200 px full-scale drag. That is a semitone every eight
  pixels, so everything from dead in tune to a quarter tone lived inside the first two, which is
  what the report measures. It is now `FloatRange::Skewed` at `skew_factor(-1.8)`: a quarter turn is
  19 cents, 40 % of the travel is the first whole semitone, half is about two, and the wide cloud
  still has the top half to itself.
- **The reading.** `variation_semitones` formatted whole semitones, so every setting in that same
  detuning half printed `± 0 st`. A curve alone would have moved the sound without ever telling the
  player where they were. It reads cents below a semitone now and semitones to one decimal above,
  with the unit chosen from the *rounded* figure so the round trip cannot oscillate at the boundary.

`a_detune_is_reachable_on_the_pitch_spray` pins both halves, and the parameter's own comment carries
the arithmetic. The precedent is `mxm-creative-sampler`'s grain spread, which met the same problem
first and records it as *"without the skew the musical part of the control is the first pixel"* —
that one is skewed harder at `-2.2` over half the range.

**This is a curve change on a normalised depth, so a saved patch moves.** It costs nothing here: the
factory bank is empty by the section below, and the ids are still provisional. It would not be free
after the gate.

**Delay variation is the same shape of problem, unfixed and deliberately so.** Its span is the whole
four-second buffer, so a subtle smear is as cramped as a detune was. Nobody has reported it, its
default already sits open at 30 %, and 20 ms against 100 ms of smear is a difference of degree where
ten cents against a semitone is a difference of kind. The gate decides. Size and Level variation are
even across their spans and need nothing.

## Sustain at every sample rate

**Sustaining is measured on the signal**, not read off the controls: a held texture must not be
advertised as a large finite tail, because a host may truncate a tail and must not truncate what
somebody is holding. **And it is measured the same way at every sample rate**, which it was not: the
DSP's level follower was a fixed per-sample coefficient, so at 8 kHz a held texture was reported as
a finite tail and a host could have cut it. Corrected in `mxm-grain-fx-dsp`, where the contract and
the regression test live.

## The two displays, and why they draw real data

Two displays expose behavior needed to perform the listening gate:

- **The buffer timeline**, on Playback. Time runs left to right so a mark's horizontal position is
  *when* a grain started; buffer age runs top to bottom so its vertical position is *how far back*
  it read. Onset timing scatters the marks sideways, Delay variation smears them vertically, and
  that orthogonality on screen is the distinction the old panel hid.
- **The grain envelope**, on Grain, which reads the DSP's own `window::window` so it cannot drift
  from the envelope grains are actually given.

**The marks are real onsets, not a pattern computed from the controls.** That is the whole reason
`crates/mxm-grain-fx-dsp` grew an onset log and this plugin a lock-free ring to carry it: at the top
of Onset timing the scheduler is a per-sample draw, and a control-derived picture would draw an even
comb exactly where the sound is least like one.

Two consequences of them being displays rather than controls: **neither calls `navigation::at`**, or
the keyboard cursor's `Coverage::Exactly` would report a parameter that is not one; and the
timeline's axis assumes 48 kHz, because the editor is never told the host's rate — what that costs
is the caption, not the marks, and `editor/visuals.rs` says so at the constant.

**Still deferred:** a live cloud/scatter display; the listening gate must establish a need before
another visualization is designed.

## Cards as layout trees

**Every card is a `mxm_ui::tree`** (`crates/ui/AGENTS.md`, *A card body as data*; in mxm-kit).
`sections::card` describes each body once — knob rows at `KNOB_COLUMN` (`KNOB_COLUMN_MIN`), the
envelope filling the rest of Grain shape's row, the timeline over Playback's knobs, Freeze's toggle
— and that description is measured for the card's floor and height and drawn leaf by leaf through
the bindings (`sections::paint`), through `paging::editor::show`. **Floors are computed**, each
tree's narrowest plus the card's chrome, with no usability minimum declared beside them, and each
card is as wide as its floor. The Grain card paints *Size*, *Rate* and *Shape* (a host reads *Grain
size* …). The displays state their own sizes in `editor/visuals.rs`: `visual::PLOT_HEIGHT` tall, at
least `timeline_min_width()` and `envelope_min_width()` wide, each filling what its row leaves.
`every_card_passes_the_tree_checks_in_every_state` runs the shared checks
(`mxm_plugin_test::tree_checks`) at Init, every control at its bottom, and every control at its top
with the onset log full.

## Two things are deliberately absent until the gate

- **No factory presets.** The `Instrument` impl ships and Init works — it is generated from the
  parameter defaults, so the browser, Save As and favourites are live. A bank is a set of parameter
  values, and the gate can still change *which parameters exist*; authoring one now would preserve
  values against laws that are still provisional.
- **One control-map role.** The file uses the current collection `instruments` array schema; the
  retired single `clap_id`/`roles` object can parse as an empty no-op and is not compatibility.
  Mix fills the existing `fx.delay` — the same quantity, following `mxm-folded-spring`'s and
  `mxm-bucket-delay`'s precedent. The seven performed roles this product wants are **not** authored:
  roles are permanent, the plan holds page 13 by a ranked *policy* rather than an inventory, and the
  ranking is fixed at the gate. Page 13 enters `docs/MXM_CONTROL_MAP.md` (in mxm-kit) in the same
  pass.

## Validator quirks this plugin met

**Parameter text is idempotent through the host's normalized conversion**: formatted with the unit,
parsed, normalized and formatted again, it is the same string. `text_round_trips_are_idempotent`
holds it for every parameter in `param_map`, at the twenty-step grid, `clap-validator` 0.4.1's own
grid, both sides of every formatter branch point (`branch_points`) and a hair either side of zero.
Three ways it has broken here:

- A `value_to_string` **without a matching `string_to_value`** breaks the round-trip. Freeze had a
  Held/Running formatter; a normalised 0.505 formatted as the true label, parsed back through the
  default reader as 0.0, and formatted as the false one. It is a plain `BoolParam` now, as
  `mxm-shimmer`'s Freeze is.
- `with_unit` appends to the formatter's output, so a custom parser has to **strip its own unit**.
  Density's did not and failed on `"0.05 grains"`.
- **A reading that changes unit, precision or words chooses the branch from the rounding its finer
  branch prints**, never from the raw value. Grain rate switched to whole numbers at a raw `10.0`,
  so 9.97 printed `10.0 grains/s`, parsed to ten and came back `10 grains/s`; Onset timing switched
  segment at a raw `0.5`, so 0.498 printed `Jittered 100 %`, parsed to one half and came back
  `Handed over 0 %`. Neither was on the validator's grid, which is why its runs were clean; the
  seconds, variation and shape readings already chose from the rounded figure.

## What the tests cover

Tests cover Off in both layouts, audible Init, tail/activity, non-finite input, the two Init
deviations, text round-trips, the declared parameter set, keyboard reachability, detune reach and
name/ID/bundle agreement. Rename-sensitive guards include:

- `every_parameter_is_bound_exactly_once` compares the bound ids against `param_map()`. The ids are
  written by hand in three files and no compiler relates them; this is what makes the next rename
  safe. Ported from `mxm-bucket-delay`.
- `every_binding_carries_a_description`, design system §7.1's sentence per control.
- `every_card_has_a_body`, because `card()` dispatches on a positional index.
- `the_opening_size_is_the_budget_hugged` holds `REFERENCE` to the quarter-4K budget hugged to the
  cards (`plans/plan-editor-standard.md` F1).
- `the_minimum_holds_the_widest_card` and
  `both_themes_paint_every_card_and_control_inside_the_reference_window` — ported from
  `mxm-shimmer`, the effect-tier precedent. **The last is the test that would have caught the
  unreadable panel**: it asserts the words the panel actually paints, and it is what the three-card
  window size was hugged against.

`text_round_trips_are_idempotent` formats with `include_unit: true` — `false` is *not* the path the
validator or the editor takes, so it could pass while `param-conversions` failed — and walks
`param_map` rather than a hand-written list, through the wrapper's step-count arithmetic. A grid
alone missed both branch defects above, so its probes include each formatter's branch points.
