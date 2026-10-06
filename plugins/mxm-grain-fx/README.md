# mxm-grain-fx

A granular processor on a **live buffer**. The input is recorded continuously; short windowed reads
of that recording — grains — are scattered in time and summed back over the dry signal. Each grain
has its own length, position, pitch, direction, window and place in the stereo field, drawn when it
starts and fixed for its life.

Part of the MXM collection ([github.com/mxm-audio](https://github.com/mxm-audio)); this repository's
[README](../../README.md). GPL-3.0-or-later — see the repository's [`LICENSE`](../../LICENSE) at its
root. CLAP only.

## What it does

Three questions, in order, and the panel is organised around them: **what one grain is**, **when
grains start**, and **how much each one differs from the last**. Every variation control sits beside
the property it varies.

- **Grain** — *Grain size* spans the three regions the technique is named for: a sub-period
  broadband buzz at 2 ms, the fusion region where grains become a texture, and separate audible
  events past 100 ms. *Grain rate* is a rate, so size and rate are independent and the overlap falls
  out of them. *Onset timing* travels all three onset families — a strict clock, a widening jitter
  inside each slot, and a per-sample draw that leaves slots empty and doubles up in others.
  *Grain shape* morphs from a clicking near-rectangle through a triangle to a Hann, drawn as the
  envelope itself because it is a tone control, not a quality setting.
- **Playback** — a timeline of the buffer with the grains that actually read it: time across, buffer
  age down, so onset scatter and read scatter are visibly two different things. *Read delay* sets how
  far back grains read; *Transpose* fixes a grain's pitch for its whole life, so a swept setting steps
  rather than glides; *Reverse chance* is the probability a grain reads backwards; *Stereo width*
  throws each grain across the field. Beside each, what varies it.
- **Loop & output** — *Feedback* returns the granulated output into the buffer, so each pass is
  granulated again rather than merely repeated. *Freeze* stops the recording while the
  grains keep reading and the dry still passes. *Mix* at zero is off, and the input passes untouched.

**With every variation shut the effect is a plain delay**, however randomly the onsets are placed —
overlapping grains read identical material and the overlap-add reconstructs the input. That is why
Delay variation and Stereo width do not start at zero.

## Status

**Pre-gate.** The DSP and the shell are built and green; the owner's listening gate has not yet
passed, so there are no factory presets and the parameter set is provisional. See
[`AGENTS.md`](AGENTS.md).

## Building

```bash
cargo xtask bundle mxm-grain-fx --release
clap-validator validate "target/bundled/mxm-grain-fx.clap"
```
