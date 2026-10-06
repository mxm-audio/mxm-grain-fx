# AGENTS.md — plugins/mxm-grain-fx/

Parent: [`../AGENTS.md`](../AGENTS.md) · The measurements, history and reasoning behind each rule:
[`NOTES.md`](NOTES.md)

# Purpose

The `mxm-grain-fx` CLAP effect: plugin shell, parameters and identity, control map, telemetry and
three-card MXM editor around `crates/mxm-grain-fx-dsp`.

Product plan: `plans/plan-mxm-grain-fx.md` in the private archive. UI brief:
[`../../docs/briefs/mxm-grain-fx.md`](../../docs/briefs/mxm-grain-fx.md).

This is an original design from granular technique rather than a copy of a box. Evidence is in
`research:effects/granular-processing.md` and `research:effects/mutable-clouds.md`.

# Ownership

- `src/lib.rs` — CLAP export, layouts, process/activity contract and DSP adaptation.
- `src/params.rs` — seventeen parameter ids, their defaults and their value text.
- `src/preset.rs` — the `Instrument` impl; the factory bank is deliberately empty (below).
- `src/editor.rs`, `src/editor/` — app bar, paging, bindings, the three cards and the two
  displays in `editor/visuals.rs`.
- `src/telemetry.rs` — lock-free peak, sounding-grain count and the onset ring the timeline draws.
- `control-map.json` — one role, deliberately (below).
- `README.md` — product documentation; the licence is the repository's root `LICENSE` (`../../LICENSE`).

# Local Contracts

## Identity is permanent; the parameter ids are not, yet

- Product name: `mxm-grain-fx`. CLAP id: `dk.mxm.mxm-grain-fx`. The owner's ruling, 2026-09-09; the
  `-fx` keeps it apart from the granular *instrument* that follows.
- `plugin_name!` in `src/lib.rs` is the one crate-local name literal; the display name and CLAP id
  derive from it, and `bundler.toml` is the one external duplicate with a test pinning agreement.

**The seventeen parameter IDs are provisional until the listening gate.** The unreleased shell exists
because interaction cannot be judged from a rendered file. Once the gate fixes the set, replace this
paragraph with the ordinary permanent-ID contract.

A rename must update three hand-written ID lists; `every_parameter_is_bound_exactly_once` makes an
incomplete change fail in tests.

**Two tempo syncs**, each the collection's quarter note beside its knob: `ratesync` on
`params::RATE_SYNC` and `readdelaysync` on `params::READ_DELAY_SYNC`. `process` resolves both once a
block, and both are drawn once per grain at its onset, so a division needs no smoothing
([NOTES.md § Two tempo syncs](NOTES.md#two-tempo-syncs)).

## Audio layouts and Off

Mono input to stereo output, and stereo to stereo. Mono input feeds both wet channels; stereo input
keeps L/R through the dry path. No MIDI input and no developer channel.

**Mix at exactly zero is Off**: the wet fades to *exact* zero — snapped, so the output is dry to the
bit in both layouts — then the buffer and grain pool are emptied once and parked. Re-engaging starts
from empty and fades in; a fade interrupted by the other edge restarts rather than resuming, so
automation crossing zero repeatedly under a live feedback tail cannot expose held state.

## Naming and defaults

- **Scatter, Spray and Random are excluded** as control words. **A variation control sits beside the
  property it varies**; Onset timing is the exception and sits beside Grain rate. This file owns the
  naming rule; `docs/briefs/mxm-grain-fx.md` is the gating layout record
  ([NOTES.md § Vocabulary](NOTES.md#vocabulary-follows-the-property-being-varied)).
- **Density is grains per second**; Length and Density are independent, and overlap is
  `length × density` (`a_longer_grain_at_the_same_density_overlaps_more`).
- **Delay variation starts at 30 %** and **Stereo width at 60 %**: declared deviations from *every
  amount starts at zero*, each asserted above zero with its reason in `every_amount_starts_at_zero`.
  All five variation depths still ship, for the gate
  ([NOTES.md § Density](NOTES.md#density-is-a-rate-and-neither-delay-variation-nor-stereo-width-starts-at-zero)).
- **Pitch variation is skewed** (`FloatRange::Skewed`, `skew_factor(-1.8)`) so a few cents of detune
  are reachable; it reads cents below a semitone and semitones to one decimal above, the unit chosen
  from the *rounded* figure. `a_detune_is_reachable_on_the_pitch_spray` pins both halves. A curve
  change moves saved patches: free now, not after the gate. Delay variation's cramped low end is
  left for the gate ([NOTES.md § Pitch variation](NOTES.md#pitch-variation-is-skewed-because-a-detune-is-a-few-cents)).

## Activity and tail

- Nonzero input, a sustaining texture, or an empty parked engine report `ProcessStatus::Normal`.
- A decaying tail reports `Tail(n)`, recomputed every block and never latched.
- **Sustaining is measured on the signal**, the same way at every sample rate: a host may truncate a
  tail and must not truncate what somebody is holding. The contract and its regression test live in
  `mxm-grain-fx-dsp` ([NOTES.md § Sustain](NOTES.md#sustain-at-every-sample-rate)).
- Reset clears all DSP state. Input scanning flushes non-finite and subnormal values before they
  reach recursion. Empty means no callback work beyond that scan and the mono-output copy.

## The displays and the cards

- Two displays: **the buffer timeline** on Playback (time left to right, buffer age top to bottom)
  and **the grain envelope** on Grain, which reads the DSP's own `window::window`.
- **The marks are real onsets**, carried from the DSP's onset log by a lock-free ring, never a
  pattern computed from the controls.
- Displays are not controls: **neither calls `navigation::at`**. The timeline's axis assumes 48 kHz,
  because the editor is never told the host's rate; that costs the caption, not the marks.
- **Still deferred:** a live cloud/scatter display; the listening gate must establish a need first
  ([NOTES.md § The two displays](NOTES.md#the-two-displays-and-why-they-draw-real-data)).
- **Every card is a `mxm_ui::tree`**: `sections::card` describes each body once, **floors are
  computed**, and each card is as wide as its floor. `every_card_passes_the_tree_checks_in_every_state`
  holds it ([NOTES.md § Cards as layout trees](NOTES.md#cards-as-layout-trees)).

## Two things are deliberately absent until the gate

- **No factory presets.** Init works, generated from the parameter defaults; a bank would preserve
  values against laws that are still provisional.
- **One control-map role**, in the current `instruments` array schema: Mix fills the existing
  `fx.delay`. The seven performed roles this product wants are **not** authored until the gate
  fixes their ranking ([NOTES.md § Two things](NOTES.md#two-things-are-deliberately-absent-until-the-gate)).

## Parameter text

**Parameter text is idempotent through the host's normalized conversion**, held by
`text_round_trips_are_idempotent`. Three rules keep it so
([NOTES.md § Validator quirks](NOTES.md#validator-quirks-this-plugin-met)):

- Every `value_to_string` has a matching `string_to_value`.
- `with_unit` appends to the formatter's output, so a custom parser **strips its own unit**.
- **A reading that changes unit, precision or words chooses the branch from the rounding its finer
  branch prints**, never from the raw value.

# Work Guidance

- Measure a closed-loop change; compiling a granular feedback effect proves nothing about character
  or stability. Three design errors here survived every test until they were measured — see the DSP
  crate's Work Guidance.
- The gate can still move the parameter set. Until it passes, treat `src/params.rs` as a draft and
  keep the provisional note above true.

# Verification

```bash
cargo test -p mxm-grain-fx
cargo clippy -p mxm-grain-fx --all-targets
# Every page, light and dark, for review -> target/layout-tree/mxm-grain-fx/<MXM_PICTURES tag>/
MXM_PICTURES=after cargo test -p mxm-grain-fx --lib tree_pictures -- --ignored
cargo xtask bundle mxm-grain-fx --release
clap-validator validate "target/bundled/mxm-grain-fx.clap"
```

- Rename-sensitive guards: `every_parameter_is_bound_exactly_once`,
  `every_binding_carries_a_description`, `every_card_has_a_body`,
  `the_opening_size_is_the_budget_hugged`, `the_minimum_holds_the_widest_card` and
  `both_themes_paint_every_card_and_control_inside_the_reference_window`. What each holds, and the
  rest of the coverage: [NOTES.md § What the tests cover](NOTES.md#what-the-tests-cover).
- `text_round_trips_are_idempotent` formats with `include_unit: true`, walks `param_map` and probes
  each formatter's branch points; keep it that way.

Run `clap-validator` in debug and release. The debug run also checks that window tables and onset
telemetry are built off the audio thread rather than allocated in `process`.

**Not verified**: the design system's §15 QA gate by eye, a real DAW, and the owner's listening gate,
which is what the shell was built for. Fidelity is UNVERIFIED by the root's default (the monorepo
root's, before the split; the owner's working preference: never imply verification that did not
happen).

# Child DOX Index

No child `AGENTS.md` files.
