//! The capture buffer, addressed by **age** rather than by position.
//!
//! `plans/plan-mxm-grain-fx.md` §3. A granular *effect* reads a buffer its own record head is
//! writing into, which is the part published descriptions of the technique omit and the part every
//! correctness problem in this crate comes from.
//!
//! Two decisions are worth stating because they are why the rest is simple:
//!
//! - **Reads are by age**, in samples behind the record head, never by absolute index. The wrap is
//!   then arithmetic nobody can get wrong, and the invariant a grain has to satisfy —
//!   `MARGIN <= age <= history` — is expressed in the same units the guard in [`crate::grain`]
//!   reasons about. `mxm-bucket-delay` shipped a tap that landed on the slot just written because
//!   it worked in absolute positions; this addressing makes that bug unrepresentable.
//! - **The record head has two states**, advancing and held, and the hold lives here rather than in
//!   the freeze feature that uses it. §3 is explicit that the invariant must hold under both and
//!   across the transition, so the hold is a G1 primitive and freeze (G2) is a product state built
//!   on top of it.

/// Samples of guard at each end of the legal read range.
///
/// **Two is exactly what the four-point reader needs, and that is why it was two.** A read at age
/// `a` takes taps at `a - 1 .. a + 2`; the guard keeps `a` inside `[MARGIN, history - MARGIN]`, so
/// the young tap never reaches age 0 — the slot the record head is about to overwrite — and the old
/// tap never reaches past the oldest written sample. The margin was set here before the reader
/// needed it, which is why raising the reader did not reopen [`crate::grain::place`] or its sweep.
pub(crate) const MARGIN: f32 = 2.0;

/// How long the record head keeps writing a side buffer after it is held.
///
/// Chosen. Freeze that merely stops the head leaves the buffer's newest and oldest samples adjacent
/// with nothing between them, and the step is audible on release; this is the material the release
/// crossfades over. Long enough to fade without being long enough to matter.
pub(crate) const RELEASE_FADE_S: f32 = 0.010;

#[derive(Debug, Clone)]
pub(crate) struct Capture {
    left: Vec<f32>,
    right: Vec<f32>,
    /// Where the next sample is written.
    write: usize,
    /// How much of the buffer holds real audio, capped at its capacity.
    written: usize,
    /// While true the record head stands still and grains read a frozen history.
    held: bool,
    /// What arrived while the head was held, kept only so release can crossfade rather than step.
    side_l: Vec<f32>,
    side_r: Vec<f32>,
    side_filled: usize,
    /// How many samples of the release crossfade are still to be written.
    release_left: usize,
}

impl Capture {
    pub(crate) fn new(capacity: usize, release_fade: usize) -> Self {
        let capacity = capacity.max(16);
        let release_fade = release_fade.max(1);
        Self {
            left: vec![0.0; capacity],
            right: vec![0.0; capacity],
            write: 0,
            written: 0,
            held: false,
            side_l: vec![0.0; release_fade],
            side_r: vec![0.0; release_fade],
            side_filled: 0,
            release_left: 0,
        }
    }

    /// How far back a read may legally reach, in samples.
    #[inline]
    pub(crate) fn history(&self) -> f32 {
        self.written as f32
    }

    #[inline]
    pub(crate) fn is_held(&self) -> bool {
        self.held
    }

    /// Stop or resume the record head.
    ///
    /// Resuming crossfades over what arrived while it was held, so the seam between the frozen
    /// history and the newly arriving audio is a fade rather than a step.
    pub(crate) fn set_held(&mut self, held: bool) {
        if held == self.held {
            return;
        }
        if held {
            self.side_filled = 0;
            self.release_left = 0;
        } else {
            // Arm the crossfade. It is applied to the samples about to be *written*, not to slots
            // already in the buffer: the seam is a discontinuity in the buffer's own timeline —
            // its newest sample is from the moment of the freeze and the next one would be from the
            // moment of release — and it is closed by writing a fade rather than by patching
            // history. The side buffer is the right thing to fade from because it holds the audio
            // that *would* have been written had the head never stopped.
            self.release_left = self.side_filled;
        }
        self.held = held;
    }

    /// Advance one sample.
    ///
    /// While held the sample goes to the side buffer instead and the head does not move, so a grain
    /// in flight keeps reading exactly the history it was placed against.
    #[inline]
    pub(crate) fn advance(&mut self, l: f32, r: f32) {
        if self.held {
            if self.side_filled < self.side_l.len() {
                self.side_l[self.side_filled] = flush(l);
                self.side_r[self.side_filled] = flush(r);
                self.side_filled += 1;
            }
            return;
        }
        let (l, r) = if self.release_left > 0 {
            let n = self.side_filled;
            let i = n - self.release_left;
            // 0 at the seam, 1 by the end of the fade.
            let t = (i as f32 + 0.5) / n as f32;
            self.release_left -= 1;
            (
                self.side_l[i] + t * (l - self.side_l[i]),
                self.side_r[i] + t * (r - self.side_r[i]),
            )
        } else {
            (l, r)
        };
        let len = self.left.len();
        self.left[self.write] = flush(l);
        self.right[self.write] = flush(r);
        self.write += 1;
        if self.write == len {
            self.write = 0;
        }
        if self.written < len {
            self.written += 1;
        }
    }

    /// Read both channels at `age` samples behind the record head, four-point cubic.
    ///
    /// **Catmull-Rom, and the 30 dB is why** (Catmull and Rom, 1974; the same interpolating spline
    /// `docs/oscillators/15-granular-in-the-wild.md` §15.9 calls a four-point cubic). That section
    /// measures a grain train reading a full-band recording and puts linear interpolation 29.7 dB
    /// behind cubic at unity rate and 29.5 dB behind at half rate — and the two identical above
    /// unity, where the source's own harmonics fold and no interpolator repairs it. This crate
    /// transposes two octaves either way, reads backwards, and keeps reading while the head is
    /// held, so its read is moving at almost every setting the panel offers and the below-unity
    /// half of that table is the half it lives in.
    ///
    /// **The four taps are held inside the written history, tap by tap, rather than by clamping the
    /// age.** Clamping the age would move a read; clamping a tap repeats the sample at the edge,
    /// which is what a one-sided read has to do and what keeps a direct read at age 1 exact. A tap
    /// at age 0 would be the slot the record head is about to overwrite and a tap past `history`
    /// would wrap onto the newest audio — the same splice [`crate::grain::place`] exists to prevent,
    /// so neither is reachable from here either.
    ///
    /// Callers are expected to have placed the read inside the legal range; the clamps here are the
    /// last line of defence against a non-finite control reaching the index, not the guard. The
    /// guard is [`crate::grain::place`], and its sweep is what proves grains stay inside.
    #[inline]
    pub(crate) fn read(&self, age: f32) -> (f32, f32) {
        let len = self.left.len();
        let history = self.written;
        if history < 2 {
            return (0.0, 0.0);
        }
        let highest = (history - 1) as f32;
        let age = if age.is_finite() {
            age.clamp(1.0, highest)
        } else {
            1.0
        };
        let whole = age as usize;
        let t = age - whole as f32;
        // `write` is one past the newest sample, so age 1 is the sample just written and age `a`
        // sits `a` slots before the write point. One conditional subtract replaces the remainder
        // this used to take twice: `write < len` and `whole <= len - 1`, so the sum is inside two
        // laps by construction and a division is never needed.
        let raw = self.write + len - whole;
        let i1 = if raw >= len { raw - len } else { raw };
        // Age decreasing is index increasing, and age increasing is index decreasing.
        let i0 = if whole > 1 { step_newer(i1, len) } else { i1 };
        let i2 = step_older(i1, len);
        let i3 = if whole + 2 <= history {
            step_older(i2, len)
        } else {
            i2
        };
        (
            cubic(
                self.left[i0],
                self.left[i1],
                self.left[i2],
                self.left[i3],
                t,
            ),
            cubic(
                self.right[i0],
                self.right[i1],
                self.right[i2],
                self.right[i3],
                t,
            ),
        )
    }

    /// Discard every sample. Constant time: history is invalidated, not zeroed, because `reset` and
    /// Off can both be reached from the audio thread and a high-rate buffer is tens of thousands of
    /// samples long.
    pub(crate) fn clear(&mut self) {
        self.write = 0;
        self.written = 0;
        self.side_filled = 0;
        self.release_left = 0;
        self.held = false;
    }

    /// Resize for a new sample rate. Allocates, so it is never called from `process`.
    pub(crate) fn resize(&mut self, capacity: usize, release_fade: usize) {
        let capacity = capacity.max(16);
        let release_fade = release_fade.max(1);
        self.left = vec![0.0; capacity];
        self.right = vec![0.0; capacity];
        self.side_l = vec![0.0; release_fade];
        self.side_r = vec![0.0; release_fade];
        self.clear();
    }
}

/// One slot newer — the neighbour at one sample less age.
#[inline]
fn step_newer(index: usize, len: usize) -> usize {
    let next = index + 1;
    if next == len { 0 } else { next }
}

/// One slot older — the neighbour at one sample more age.
#[inline]
fn step_older(index: usize, len: usize) -> usize {
    if index == 0 { len - 1 } else { index - 1 }
}

/// Catmull-Rom between `p1` and `p2`, with `p0` and `p3` as the outer taps.
///
/// Returns `p1` exactly at `t = 0` and `p2` exactly at `t = 1`, and reproduces a straight line
/// through all four taps exactly — which is what keeps the buffer's own ramp tests meaning what
/// they meant under the linear reader.
#[inline]
fn cubic(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let c1 = 0.5 * (p2 - p0);
    let c2 = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
    let c3 = 0.5 * (p3 - p0) + 1.5 * (p1 - p2);
    ((c3 * t + c2) * t + c1) * t + p1
}

/// Kill denormals and anything non-finite before it reaches recursive state.
#[inline]
pub(crate) fn flush(value: f32) -> f32 {
    if !value.is_finite() || value.abs() < f32::MIN_POSITIVE {
        0.0
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(capacity: usize, n: usize) -> Capture {
        let mut c = Capture::new(capacity, 8);
        for i in 0..n {
            let v = (i + 1) as f32;
            c.advance(v, -v);
        }
        c
    }

    #[test]
    fn age_one_is_the_sample_just_written() {
        let c = filled(64, 32);
        let (l, r) = c.read(1.0);
        assert_eq!(l, 32.0);
        assert_eq!(r, -32.0);
    }

    #[test]
    fn age_counts_backwards_through_the_wrap() {
        // More samples than the buffer holds, so the read has to cross the wrap point.
        let c = filled(16, 40);
        for age in 1..=15 {
            let (l, _) = c.read(age as f32);
            assert_eq!(l, (41 - age) as f32, "age {age}");
        }
    }

    #[test]
    fn the_reader_reproduces_a_ramp_between_neighbours() {
        // `filled` writes a ramp, and Catmull-Rom reproduces a straight line through its four taps
        // exactly — so a fractional read on it has a closed-form answer rather than a tolerance
        // chosen to fit. Age `a` holds `33 - a`.
        let c = filled(64, 32);
        for step in 0..=8 {
            let age = 3.0 + step as f32 / 8.0;
            let (l, r) = c.read(age);
            assert!((l - (33.0 - age)).abs() < 1e-4, "age {age}: {l}");
            assert!((r + (33.0 - age)).abs() < 1e-4, "age {age}: {r}");
        }
    }

    #[test]
    fn the_reader_returns_its_taps_exactly_at_whole_ages() {
        // A four-point spline that did not pass through its own knots would move every read by a
        // fraction of a sample, which is a fractional delay nobody asked for.
        let c = filled(64, 32);
        for age in 1..=31 {
            let (l, _) = c.read(age as f32);
            assert_eq!(l, (33 - age) as f32, "age {age}");
        }
    }

    #[test]
    fn the_outer_taps_never_leave_the_written_history() {
        // The splice the guard exists to prevent, approached from inside this function instead: a
        // tap at age 0 is the slot about to be overwritten and a tap past `history` wraps onto the
        // newest audio. Fill a buffer that has wrapped many times, so both would be loud, and sweep
        // every readable age including the two ends.
        let c = filled(16, 4000);
        for step in 0..=600 {
            let age = 1.0 + (step as f32 / 600.0) * 14.0;
            let (l, r) = c.read(age);
            // Every legal tap is within one of the ramp's own neighbours, so the interpolant cannot
            // leave the hull by more than the spline's overshoot. A wrapped tap would be thousands
            // away.
            let expected = 4001.0 - age;
            assert!(
                (l - expected).abs() < 1.0,
                "age {age} read {l}, expected about {expected}"
            );
            assert!((r + expected).abs() < 1.0, "age {age} read {r}");
        }
    }

    /// The linear reader this one replaced, kept as the reference the improvement is measured
    /// against — the same way `mxm-creative-sampler-dsp` keeps the kernel its table replaced.
    fn read_linear(c: &Capture, age: f32) -> f32 {
        let len = c.left.len();
        let history = c.written;
        let age = age.clamp(1.0, (history - 1) as f32);
        let newer_age = age as usize;
        let fraction = age - newer_age as f32;
        let older_age = (newer_age + 1).min(history - 1);
        let newer = (c.write + len - newer_age) % len;
        let older = (c.write + len - older_age) % len;
        c.left[newer] + fraction * (c.left[older] - c.left[newer])
    }

    /// How far the reader's error sits below the signal, in dB, for a sine read back at `rate`.
    ///
    /// **Measured against the analytic signal, not against a spectrum.** §15.9's table is a grain
    /// train's spurious energy; this is the one interpolation underneath it, and the honest way to
    /// price a reader on its own is to ask what it got wrong. Record `sin(2π·f·t)`, walk the read
    /// age at `rate` so every fractional offset is visited, and compare each read against the value
    /// the tone actually had at that instant. The result is the total error — the interpolator's
    /// droop and its distortion together — relative to the signal.
    fn error_db(cycles_per_sample: f64, rate: f32, cubic_reader: bool) -> f32 {
        const N: usize = 4096;
        // Record enough that the whole walk stays well inside the history at every rate, so this
        // measures the interpolator rather than the clamp at either end.
        let travel = (f64::from(rate) * N as f64) as usize;
        let recorded = travel + 2 * N;
        let mut c = Capture::new(1 << 17, 8);
        let tone = |t: f64| (core::f64::consts::TAU * cycles_per_sample * t).sin();
        for i in 0..recorded {
            c.advance(tone(i as f64) as f32, 0.0);
        }
        let (mut error_sq, mut signal_sq) = (0.0f64, 0.0f64);
        for n in 0..N {
            // Start well inside the history and walk toward the record head. An offset that is not
            // a round fraction, so the walk does not land repeatedly on whole samples.
            let age = recorded as f32 - N as f32 + 0.37 - rate * n as f32;
            let read = if cubic_reader {
                c.read(age).0
            } else {
                read_linear(&c, age)
            };
            // Age `a` is the sample written `a` steps ago, so its instant is `recorded - a`.
            let want = tone(recorded as f64 - f64::from(age));
            error_sq += (f64::from(read) - want) * (f64::from(read) - want);
            signal_sq += want * want;
        }
        (10.0 * (error_sq / signal_sq).log10()) as f32
    }

    #[test]
    fn the_cubic_reader_beats_the_linear_one_it_replaced() {
        // Measured here, over tone and rate, as error below signal in dB:
        //
        //   tone (x sample rate) |  0.25x  |   1x    |   4x    | linear at 1x
        //   1/64  (750 Hz)       |  -98.9  | -100.2  | -100.2  |  -59.0
        //   1/16  (3 kHz)        |  -61.9  |  -62.1  |  -62.1  |  -34.9
        //   1/8   (6 kHz)        |  -42.0  |  -40.9  |  -32.2  |  -23.0
        //   1/4   (12 kHz)       |  -21.1  |  -19.2  |  -15.4  |  -11.3
        //
        // **The advantage is frequency, not rate**, which is the honest reading and not the one
        // §15.9's table suggests on its own: linear interpolation's error here is mostly its droop,
        // a lowpass that reaches -3.9 dB at Nyquist, and a four-point spline flattens that. 40 dB
        // at 750 Hz, 27 dB at 3 kHz, and the two converge toward Nyquist where neither is good and
        // the source's own folding (§15.9, and no interpolator repairs it) has taken over anyway.
        let mut worst_gain = f32::MAX;
        for &rate in &[0.25f32, 0.5, 1.0, 2.0, 4.0] {
            let linear = error_db(1.0 / 16.0, rate, false);
            let cubic = error_db(1.0 / 16.0, rate, true);
            assert!(
                cubic <= linear - 15.0,
                "rate {rate}: cubic {cubic:.1} dB against linear {linear:.1} dB"
            );
            worst_gain = worst_gain.min(linear - cubic);
        }
        assert!(
            worst_gain >= 15.0,
            "only {worst_gain:.1} dB at the worst rate"
        );
    }

    #[test]
    fn an_empty_buffer_reads_silence() {
        let c = Capture::new(64, 8);
        assert_eq!(c.read(1.0), (0.0, 0.0));
        assert_eq!(c.history(), 0.0);
    }

    #[test]
    fn a_held_head_freezes_the_history() {
        let mut c = filled(64, 32);
        let before = c.read(1.0);
        c.set_held(true);
        for _ in 0..100 {
            c.advance(999.0, 999.0);
        }
        assert_eq!(c.read(1.0), before, "a held head must not move");
        assert_eq!(c.history(), 32.0);
    }

    #[test]
    fn release_crossfades_rather_than_stepping() {
        // The seam is a discontinuity in the buffer's own timeline: its newest sample is from the
        // moment of the freeze, and the next one written would be from the moment of release. The
        // side buffer is what *would* have been written had the head never stopped, so in a real
        // signal it is continuous with the last written sample â€” which is what the fade needs, and
        // what this test sets up. The live input then jumps, and the fade is what closes it.
        let mut c = Capture::new(256, 8);
        for _ in 0..64 {
            c.advance(1.0, 1.0);
        }
        c.set_held(true);
        for _ in 0..8 {
            c.advance(1.0, 1.0); // the continuation the head would have recorded
        }
        c.set_held(false);
        for _ in 0..8 {
            c.advance(-1.0, -1.0); // the input has moved on while we were frozen
        }
        // Walk the eight samples written since release, oldest first. Without the fade the first of
        // them would step the full 2.0 from the held material to the live input.
        let mut previous = 1.0f32;
        for age in (1..=8).rev() {
            let (v, _) = c.read(age as f32);
            assert!(
                v <= previous + 1.0e-6,
                "not monotone at age {age}: {v} after {previous}"
            );
            assert!(
                (v - previous).abs() < 0.5,
                "stepped at age {age}: {previous} -> {v}"
            );
            assert!((-1.0..=1.0).contains(&v), "out of range: {v}");
            previous = v;
        }
        let (newest, _) = c.read(1.0);
        assert!(
            newest < -0.5,
            "the fade should reach the live input: {newest}"
        );
    }

    #[test]
    fn a_release_with_nothing_held_writes_the_input_untouched() {
        let mut c = Capture::new(256, 8);
        c.set_held(true);
        c.set_held(false);
        c.advance(0.75, -0.75);
        c.advance(0.25, -0.25);
        c.advance(0.5, -0.5);
        // Reads run from age 1 to age `history - 1`: interpolating at an age needs its older
        // neighbour, so the oldest sample is a bound rather than a readable position.
        assert_eq!(c.read(1.0), (0.5, -0.5));
        assert_eq!(c.read(2.0), (0.25, -0.25));
    }

    #[test]
    fn clear_invalidates_history() {
        let mut c = filled(64, 32);
        c.clear();
        assert_eq!(c.history(), 0.0);
        assert_eq!(c.read(1.0), (0.0, 0.0));
    }

    #[test]
    fn denormals_and_non_finites_never_enter_the_buffer() {
        let mut c = Capture::new(64, 8);
        c.advance(f32::NAN, f32::INFINITY);
        c.advance(1.0e-40, -1.0e-40);
        for age in 1..=2 {
            let (l, r) = c.read(age as f32);
            assert!(l.is_finite() && r.is_finite());
            assert_eq!(l, 0.0);
            assert_eq!(r, 0.0);
        }
    }
}
