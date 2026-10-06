# mxm-grain-fx — UI design brief

Required by `MXM_DESIGN_SYSTEM.md` §14, and written before the editor it describes. This effect is
an original granular processor on a live capture buffer, not the interface or constants of any
product. Technique evidence is `research:effects/granular-processing.md`; one implementation read to
build-from depth is `research:effects/mutable-clouds.md`.

**Plugin:** `mxm-grain-fx`; CLAP id `dk.mxm.mxm-grain-fx`. Stereo wet topology, with mono-to-stereo
and stereo-to-stereo layouts. Product plan: `plans/plan-mxm-grain-fx.md`.

**This editor exists so the plan's listening gate can happen at all.** The plan originally put that
gate before the shell; the owner could not judge a granular processor without turning its knobs, and
the ordering was corrected (plan revision 10). Until the gate passes, **the parameter set is
provisional** — nothing is released, so ids cost nothing to change yet, and the plugin's AGENTS.md
says so.

**Revision 2, 2026-09-11 — the first panel was built and played, and it did not read.** The owner's
report: *Scatter* and *Spray* are synonyms in English for *when a grain starts* and *what a grain
is*; *Random* named a card whose siblings — Scatter, Spread, Reverse — are equally probabilistic;
four of the five variation depths sat on a card away from the values they vary; and several controls
showed a percentage where a real unit existed. **That is pre-gate feedback of exactly the kind this
editor was built to collect**, so this revision spends it: three cards instead of five, every
variation beside the property it varies, the words Scatter/Spray/Random gone from the product, and
§8's deferred display taken — because the gate cannot judge a texture through an interface that
hides the distinction it is judging. The parameter set stays provisional; the gate has not moved.

## 1. Primary sound-design task

**Decide what a grain is, then decide when the grains start, then decide how much each one differs
from the last.** Three questions, and the panel keeps the first two apart while binding the third to
whichever property it varies. Every product the research read collapses the first two into each other
somewhere, and the whole argument for this one is that it does not.

The third question is what revision 2 got wrong the first time. Collecting the variation depths on a
card of their own made them look like a subsystem; they are not one. **A variation control belongs
beside the value it varies**, because that pairing is the only thing that says which of the two it
is — and the distinction the player most needs is that scattering *onsets* is not the same as making
grains read *different material*. With every variation shut, however randomly the onsets are placed,
overlapping grains read identical material and the overlap-add reconstructs the input.

## 2. Controls reached for most

1. **Grain size** — from sub-period buzz through fusion to discrete events.
2. **Grain rate** — how many start per second, independently of size.
3. **Onset timing** — regular, jittered, or random. One control, two segments.
4. **Read delay** — how far back in the buffer the grains read.
5. **Mix** — dry/wet, and Off at zero.

**Freeze** is a performed toggle beside those, not hidden setup. It is the collection's on/off
(design system §7.2), in the parameter's own word, so a host's generic UI and the panel share one
vocabulary: *Freeze*, on or off. (It was *Buffer: Live / Frozen*, but the parameter never carried
those words, so a host read *Off* where the panel read *Live*; `plans/plan-editor-standard.md`
revision 23, *R2's call, the owner's to overrule*.)

## 3. Signal flow that must be visible

```text
input ──┬──────────────────────────────── dry ──────────────────┐
        │                                                       │
        ├─▶ [ capture buffer ]◀── freeze                        ├──▶ Mix
        │        │                                              │
        │        ▼   scheduler ──▶ grains ──▶ overlap-add ──────┘
        │        │                                    │
        └────────┴──── feedback ◀── saturate ◀── HP ◀─┘
```

Three facts the interface must carry: **the buffer is being written while it is read** (which is why
Read delay is a delay and why Frozen means the writing stops, not the sound); **feedback returns into
the buffer**, so each pass is granulated again rather than merely repeated; and **grain shape is a
tone control**, not a quality setting. The buffer timeline of §8 carries the first of the three
directly, which is most of why it is now on the panel.

## 4. Play view

There is no Play view and no view bar. This is an effect, following the four standalone effects
already in the collection. Space-derived paging may split the cards when they no longer fit, without
changing controller pages.

## 5. Advanced controls and disclosure

No control is hidden. The cards are the disclosure, and each variation sits beside what it varies:

| Card | Controls | Job |
|---|---|---|
| **Grain** | Grain size · Size variation; Grain rate · Onset timing; Grain shape (painted *Size*, *Rate*, *Shape*: the card says whose) | What one grain is, and when grains start |
| **Playback** | *buffer timeline*; Read delay · Delay variation; Transpose · Pitch variation; Reverse chance · Stereo width · Level variation | Where in the buffer each grain reads, and how far each one departs from the last |
| **Loop & output** | Feedback · Mix; Freeze | Recirculation, output balance, and the held buffer |

**All five per-grain variation depths are still reachable**, which is what decision 9 needs — the
choice of which become permanent is still made by ear rather than by argument, and whichever do not
survive come off the panel with their ids. What revision 2 removes is not a depth but a *duplicate*:
Spread and the pan depth were two knobs summing independent draws into one pan value, so they are one
control, **Stereo width**. Absorbing Spread into the depth rather than the reverse is deliberate — the
depth is the member of decision 9's inventory, and the gate can still drop it.

Onset timing is paired with Grain rate rather than with a variation, because it varies *the clock*
and not a property of a grain. It is drawn as a labelled travel, `Regular — Jittered — Random`, since
a percentage says nothing about which of the three onset families is running.

## 6. Categories, cards, and grouping

All three cards are in **Effects**, ordered by signal flow: Grain, Playback, Loop & output. Grain
stays whole while space permits, because Grain size and Grain rate are the product's primary pair and
separating them would hide the decision the whole design rests on. The shared paging renderer derives
one to three visual pages from available space.

**Playback is the dense card** — seven controls and a timeline — and its floor and ceiling are
measured, not inherited from the old uniform five. `mxm-bucket-delay`'s Taps card, a constellation
above six faders, is the existing proof that a card of this shape works.

## 7. Identity accent

The collection accent, unchanged, in both themes. Effects are distinguished in a chain by their names
and signal roles, not by a hue family of their own.

## 8. Live visualization

**Two, and revision 2 reverses revision 1 to get them.** Revision 1 deferred every display, and its
reasoning was sound: the obvious granular display gets designed against a guess about what the
texture looks like, and the gate is what says which controls a player actually watches. **What that
argument did not anticipate is a panel the gate cannot be performed on.** The owner played it and
could not tell scattering onsets from varying grains — which is the product's central distinction,
the one §1 says the whole design rests on. A display that makes it visible is not a guess about the
texture; it is the thing that lets the gate happen. So:

- **A capture-buffer timeline**, on Playback. Record head at the right; the read point placed by Read
  delay; a shaded band for Delay variation; onset marks showing regular, jittered or random spacing;
  mark width from Grain size. It carries §3's first fact — the buffer is written while it is read —
  and it shows onset timing and per-grain variation as two visibly different things.
- **The grain envelope**, on Grain, drawn as the morph itself rather than as a percentage.

Both are plugin-local, following `mxm-bucket-delay`'s constellation and `mxm-chorus-06`'s Sweep; both
take their geometry from `crates/ui`'s telemetry tokens rather than inventing numbers; and anything
live is driven from `Telemetry`, never from an editor-side clock. The app bar's level meter stays.

**What is still deferred**: any display of the grain cloud *itself* — a scatter of live grains — which
remains the guess revision 1 was right to refuse.

## 9. Control map

Page 13, Effects/Grain. Mix fills the existing `fx.delay` role — the same quantity, following the
spring and delay precedents rather than minting a duplicate. **The seven performed roles are fixed at
the gate**, not here: the plan's §7 holds the page by a ranked policy rather than an inventory,
precisely because the gate can move which controls are performed. The proposed ranking, in revision
2's names, is Grain rate, Onset timing, Grain size, Transpose, Feedback, Buffer, Grain shape; Read
delay, Reverse chance, Stereo width and the four remaining variation depths stay unmapped as stored
shape. Renaming did not reorder it — the same quantities are ranked in the same order.

## 10. What this brief does not settle

The parameter ranges, the init patch's values, and the factory bank. All three wait for the gate, and
the plan says so. **The display no longer waits** — see §8 for why revision 2 took it early, and what
part of it is still deferred.
