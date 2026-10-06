//! One grain, and the guard that decides where it is allowed to start.
//!
//! `plans/plan-mxm-grain-fx.md` §3 and §5. Two facts shape this module:
//!
//! - **The tuple is fixed at birth.** Start age, length, playback rate, direction, window shape,
//!   level and pan are drawn when the grain starts and are not touched again while it plays. Every
//!   granular processor the research read works this way and one of them says why — the expensive
//!   arithmetic happens once per grain rather than once per sample
//!   (`research:effects/granular-processing.md` §1). It is also what makes grain length a *timbre*
//!   control rather than a quality setting: a swept pitch produces a staircase whose step is the
//!   grain, which Red Panda documents as a feature and this crate keeps.
//! - **The read must never cross the record head.** Both ends of the buffer fail the same way — a
//!   splice of the newest sample onto the oldest, in the middle of a grain, where the window is at
//!   unity and cannot hide it. [`place`] is the guard, and it is the reason this crate can claim
//!   the invariant rather than hope for it.

use crate::capture::{Capture, MARGIN};

/// Which way a grain reads through the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    /// Toward newer samples — the ordinary case, and the one that can overtake the record head.
    Forward,
    /// Toward older samples — cannot overtake the head, but its read age grows by *both* travels,
    /// so it runs off the old end instead.
    Reverse,
}

impl Direction {
    /// `+1` forward, `-1` reverse. Age changes by `write_advance - sign * rate` each sample.
    #[inline]
    fn sign(self) -> f32 {
        match self {
            Direction::Forward => 1.0,
            Direction::Reverse => -1.0,
        }
    }
}

/// Where a grain may start, and how long it may be, for one direction and rate.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Placement {
    /// Grain length in samples, capped where the direction and rate would exhaust the buffer.
    pub(crate) length: f32,
    /// The youngest legal start age.
    pub(crate) lowest_age: f32,
    /// The oldest legal start age.
    pub(crate) highest_age: f32,
}

/// The guard.
///
/// Over a grain's life the age of the sample it reads changes by `write_advance - sign · rate` per
/// output sample, where `write_advance` is 1 while recording and 0 while the head is held. The
/// invariant is that the age stays in `[MARGIN, history - MARGIN]` throughout, and the worst case
/// differs by direction *and* by head state:
///
/// | | worst case for the young end | worst case for the old end |
/// |---|---|---|
/// | Forward | **held** — age falls at `rate`, with no head retreating ahead of it | recording and `rate < 1`, where age grows at `1 - rate` |
/// | Reverse | start, since age only grows | **recording** — age grows at `rate + 1` |
///
/// Placing every grain to satisfy both head states is also what makes freezing *mid-grain* safe:
/// under mixed dynamics the age at any moment is bracketed by the two pure cases, so a grain that
/// satisfies both cannot leave the range whenever the head stops or starts. That is §3's
/// "under an advancing head, under a frozen head, and across the transition between them", and it
/// is why this returns one placement rather than one per state.
///
/// Returns `None` when the buffer is too short to hold any grain of this direction and rate.
pub(crate) fn place(
    length: f32,
    rate: f32,
    direction: Direction,
    history: f32,
) -> Option<Placement> {
    if !length.is_finite() || !rate.is_finite() || !history.is_finite() {
        return None;
    }
    let rate = rate.max(1.0e-4);
    // Everything the two heads consume, per sample of grain, in each direction.
    let (young_travel, old_travel) = match direction {
        // Held: the read falls away from the head at `rate`. Recording with a slow rate: the head
        // runs away from the read at `1 - rate`.
        Direction::Forward => (rate, (1.0 - rate).max(0.0)),
        // The read walks back at `rate` while the head walks forward at 1.
        Direction::Reverse => (0.0, rate + 1.0),
    };
    let usable = history - 2.0 * MARGIN;
    if usable <= 0.0 {
        return None;
    }
    // A grain consumes `young_travel + old_travel` samples of buffer per sample of its own length,
    // so this is the longest it can be before the legal window closes.
    let consumption = young_travel + old_travel;
    let longest = if consumption > 0.0 {
        usable / consumption
    } else {
        usable
    };
    let length = length.clamp(1.0, longest.max(1.0));
    if longest < 1.0 {
        return None;
    }
    let lowest_age = MARGIN + young_travel * length;
    let highest_age = history - MARGIN - old_travel * length;
    if highest_age < lowest_age {
        return None;
    }
    Some(Placement {
        length,
        lowest_age,
        highest_age,
    })
}

/// One grain.
///
/// **The read age is recomputed from counters, never accumulated.** Adding `write_advance - travel`
/// to an age once per sample is the obvious way to write this and it drifts: over a grain of eight
/// thousand samples, f32 addition on a value in the thousands loses enough that the read walks past
/// the guard [`place`] proved it would stay inside. Two integer counters and one multiply give the
/// same trajectory with a single rounding, and the guard's sweep then measures what it claims to.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Grain {
    active: bool,
    /// The age this grain started at.
    age0: f32,
    /// `sign · rate` — what the read subtracts from the age each sample.
    travel: f32,
    /// Samples rendered so far.
    elapsed: u32,
    /// How many of those the record head advanced through. The difference between this and
    /// `elapsed` is exactly the time spent frozen, so a freeze landing mid-grain changes the
    /// trajectory from that sample on and nowhere else.
    recorded: u32,
    length: f32,
    /// `1 / length`, so the per-sample phase is a multiply. The tuple is fixed at birth, which is
    /// what makes every reciprocal here a birth cost rather than a per-sample one.
    inv_length: f32,
    shape: f32,
    /// The fold-pan matrix with the level draw already folded in: left is
    /// `l·left_from_left + r·left_from_right`, right the mirror. Resolving it at birth is the same
    /// move the sibling sampler made with Character — one DAC per voice rather than one per grain.
    left_from_left: f32,
    left_from_right: f32,
    right_from_right: f32,
    right_from_left: f32,
    /// Rising counter used only to pick a victim when the pool is full.
    started: u64,
}

impl Grain {
    pub(crate) const fn silent() -> Self {
        Self {
            active: false,
            age0: 0.0,
            travel: 0.0,
            elapsed: 0,
            recorded: 0,
            length: 1.0,
            inv_length: 1.0,
            shape: 0.5,
            left_from_left: 0.0,
            left_from_right: 0.0,
            right_from_right: 0.0,
            right_from_left: 0.0,
            started: 0,
        }
    }

    /// The age this grain reads on its next sample.
    #[inline]
    fn age(&self) -> f32 {
        self.age0 + self.recorded as f32 - self.travel * self.elapsed as f32
    }

    #[inline]
    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    #[inline]
    pub(crate) fn started_at(&self) -> u64 {
        self.started
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start(
        &mut self,
        age: f32,
        length: f32,
        rate: f32,
        direction: Direction,
        shape: f32,
        level: f32,
        pan: f32,
        started: u64,
    ) {
        self.active = true;
        self.age0 = age;
        self.travel = direction.sign() * rate;
        self.elapsed = 0;
        self.recorded = 0;
        self.length = length.max(1.0);
        self.inv_length = 1.0 / self.length;
        self.shape = shape;
        // The fold law. At centre this is the identity, so a stereo input keeps its image; panned
        // hard it sums the far channel into the near one rather than discarding it, so a grain
        // never loses half its energy to being placed. `research:effects/mutable-clouds.md` §3 is
        // the shape; the consequence — one channel carrying both — is what §6's gain budget has to
        // be evaluated against, and is exactly why zero randomisation depth is *not* the worst case.
        let pan = pan.clamp(0.0, 1.0);
        let (gain_l, gain_r) = if pan < 0.5 {
            (1.0, pan * 2.0)
        } else {
            ((1.0 - pan) * 2.0, 1.0)
        };
        self.left_from_left = gain_l * level;
        self.left_from_right = (1.0 - gain_r) * level;
        self.right_from_right = gain_r * level;
        self.right_from_left = (1.0 - gain_l) * level;
        self.started = started;
    }

    #[inline]
    pub(crate) fn silence(&mut self) {
        self.active = false;
    }

    /// Render one sample into `(left, right)` and advance.
    ///
    /// `head_advancing` is the record head's state *this sample*, so a freeze that lands in the
    /// middle of a grain changes its trajectory from here on — which is the case [`place`] brackets
    /// rather than forbids.
    #[inline]
    pub(crate) fn render(&mut self, capture: &Capture, head_advancing: bool) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }
        let phase = self.elapsed as f32 * self.inv_length;
        let w = crate::window::window(self.shape, phase);
        let (l, r) = capture.read(self.age());
        let out = (
            l * self.left_from_left + r * self.left_from_right,
            r * self.right_from_right + l * self.right_from_left,
        );
        self.elapsed += 1;
        if head_advancing {
            self.recorded += 1;
        }
        if self.elapsed as f32 >= self.length {
            self.active = false;
        }
        (out.0 * w, out.1 * w)
    }

    /// The age this grain is reading, for the invariant sweep to inspect.
    #[cfg(test)]
    pub(crate) fn read_age(&self) -> f32 {
        self.age()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk a placed grain's whole life under one head state and return the ages it visited.
    fn ages(
        placement: &Placement,
        start: f32,
        rate: f32,
        direction: Direction,
        recording: bool,
    ) -> Vec<f32> {
        let mut grain = Grain::silent();
        grain.start(start, placement.length, rate, direction, 0.5, 1.0, 0.5, 0);
        let capture = Capture::new(1 << 16, 8);
        let mut seen = vec![grain.read_age()];
        while grain.is_active() {
            let _ = grain.render(&capture, recording);
            if grain.is_active() {
                seen.push(grain.read_age());
            }
        }
        seen
    }

    #[test]
    fn the_guard_holds_for_every_direction_rate_and_head_state() {
        // §3's sweep. Every position, length, ratio and direction in the domain, under an advancing
        // head and a held one, asserting the read age never leaves the history.
        let history = 8000.0f32;
        let mut checked = 0u32;
        for &direction in &[Direction::Forward, Direction::Reverse] {
            for rate_step in 0..=24 {
                // Two octaves either side, which is the widest the product can ask for.
                let rate = 2.0f32.powf((rate_step as f32 / 24.0) * 4.0 - 2.0);
                for length_step in 0..=12 {
                    let asked = 8.0 * 2.0f32.powi(length_step);
                    let Some(p) = place(asked, rate, direction, history) else {
                        continue;
                    };
                    for position_step in 0..=10 {
                        let t = position_step as f32 / 10.0;
                        let start = p.lowest_age + t * (p.highest_age - p.lowest_age);
                        for &recording in &[true, false] {
                            for age in ages(&p, start, rate, direction, recording) {
                                assert!(
                                    age >= MARGIN - 1e-3,
                                    "young end crossed: dir {direction:?} rate {rate} len {} pos {t} recording {recording} age {age}",
                                    p.length
                                );
                                assert!(
                                    age <= history - MARGIN + 1e-3,
                                    "old end crossed: dir {direction:?} rate {rate} len {} pos {t} recording {recording} age {age}",
                                    p.length
                                );
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 100_000, "the sweep should be broad: {checked}");
    }

    #[test]
    fn freezing_mid_grain_stays_inside_the_history() {
        // The transition case: a grain placed under both head states, then frozen at every phase of
        // its life. Bracketing is what makes this safe, so it is proved rather than assumed.
        let history = 8000.0f32;
        for &direction in &[Direction::Forward, Direction::Reverse] {
            for rate_step in 0..=12 {
                let rate = 2.0f32.powf((rate_step as f32 / 12.0) * 4.0 - 2.0);
                let Some(p) = place(2000.0, rate, direction, history) else {
                    continue;
                };
                for position_step in 0..=6 {
                    let t = position_step as f32 / 6.0;
                    let start = p.lowest_age + t * (p.highest_age - p.lowest_age);
                    let total = p.length.ceil() as usize;
                    for freeze_at in (0..total).step_by((total / 16).max(1)) {
                        let mut grain = Grain::silent();
                        grain.start(start, p.length, rate, direction, 0.5, 1.0, 0.5, 0);
                        let capture = Capture::new(1 << 16, 8);
                        let mut n = 0usize;
                        while grain.is_active() {
                            let recording = n < freeze_at;
                            let _ = grain.render(&capture, recording);
                            n += 1;
                            if grain.is_active() {
                                let age = grain.read_age();
                                assert!(
                                    age >= MARGIN - 1e-3 && age <= history - MARGIN + 1e-3,
                                    "dir {direction:?} rate {rate} froze at {freeze_at}: age {age}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_short_history_refuses_rather_than_placing_badly() {
        assert!(place(1000.0, 1.0, Direction::Forward, 3.0).is_none());
        assert!(place(1000.0, 4.0, Direction::Reverse, 8.0).is_none());
    }

    #[test]
    fn length_is_capped_when_the_rate_would_exhaust_the_buffer() {
        let history = 4000.0f32;
        // Two octaves up: the read consumes four samples of buffer per sample of grain, so the
        // grain cannot be longer than about a quarter of the history.
        let p = place(f32::MAX / 4.0, 4.0, Direction::Forward, history).expect("placeable");
        assert!(p.length <= history / 4.0 + 1.0, "length {}", p.length);
        assert!(
            p.length > 100.0,
            "the cap should not collapse the grain: {}",
            p.length
        );
        // Reverse at the same rate consumes five, because the head advances against it.
        let r = place(f32::MAX / 4.0, 4.0, Direction::Reverse, history).expect("placeable");
        assert!(
            r.length < p.length,
            "reverse consumes more: {} vs {}",
            r.length,
            p.length
        );
    }

    #[test]
    fn the_forward_low_bound_is_the_frozen_case() {
        // §3's claim that a start legal under a moving head sits on the seam under a still one, so
        // the guard has to take the frozen case. Two octaves up, where the difference is largest.
        let p = place(500.0, 4.0, Direction::Forward, 8000.0).expect("placeable");
        // Held, the read falls at 4 samples per sample: 500 * 4 = 2000 of travel plus the margin.
        assert!(
            (p.lowest_age - (MARGIN + 2000.0)).abs() < 1.0,
            "{}",
            p.lowest_age
        );
    }

    #[test]
    fn a_grain_is_silent_once_its_phase_runs_out() {
        let capture = Capture::new(1024, 8);
        let mut grain = Grain::silent();
        grain.start(10.0, 32.0, 1.0, Direction::Forward, 0.5, 1.0, 0.5, 0);
        for _ in 0..33 {
            let _ = grain.render(&capture, true);
        }
        assert!(!grain.is_active());
        assert_eq!(grain.render(&capture, true), (0.0, 0.0));
    }

    #[test]
    fn the_fold_law_is_the_identity_at_centre_and_sums_at_the_edge() {
        let mut capture = Capture::new(1024, 8);
        for _ in 0..64 {
            capture.advance(1.0, 0.5);
        }
        // Centre: L stays L, R stays R.
        let mut centre = Grain::silent();
        centre.start(4.0, 64.0, 1.0, Direction::Forward, 0.0, 1.0, 0.5, 0);
        for _ in 0..31 {
            let _ = centre.render(&capture, false);
        }
        let (l, r) = centre.render(&capture, false);
        assert!((l - 1.0).abs() < 0.05, "left {l}");
        assert!((r - 0.5).abs() < 0.05, "right {r}");

        // Hard left: both channels arrive on the left and the right is silent.
        let mut hard = Grain::silent();
        hard.start(4.0, 64.0, 1.0, Direction::Forward, 0.0, 1.0, 0.0, 0);
        for _ in 0..31 {
            let _ = hard.render(&capture, false);
        }
        let (l, r) = hard.render(&capture, false);
        assert!((l - 1.5).abs() < 0.05, "folded left {l}");
        assert!(r.abs() < 0.05, "right should be empty: {r}");
    }
}
