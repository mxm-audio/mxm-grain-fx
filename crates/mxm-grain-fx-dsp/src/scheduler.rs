//! When the next grain starts — the design, as `plans/plan-mxm-grain-fx.md` §4 puts it.
//!
//! Three families are named in the research and all three ship
//! (`research:effects/granular-processing.md` §4):
//!
//! - **Synchronous** — periodic onsets, which put an amplitude modulator at the grain rate under
//!   everything. That AM is the comb-like, pitched character, and it is the technique's own sound
//!   rather than a defect to be smoothed away.
//! - **Semi-synchronous** — the same clock with each onset jittered inside its slot. Still exactly
//!   one onset per slot.
//! - **Asynchronous** — a per-sample draw, which is what "cloud" means. Its slots may hold two
//!   onsets or none, and that is precisely what jitter can never produce however wide it is opened.
//!
//! **One control reaches all three, in two segments.** Below [`HANDOVER_AT`] the onset is drawn
//! inside a widening fraction of its slot. Above it the periodic process is *handed over* to the
//! stochastic one: each slot's onset is suppressed with probability `q`, while a per-sample draw
//! fires at `q / slot`. The two rates sum to `1 / slot` at every setting, so the mean density does
//! not move across the hand-over — only the statistics do — and at the top the phasor never fires
//! at all. That is a genuine path into the asynchronous process rather than a wide jitter wearing
//! its name.
//!
//! `q` is also what drives the normalisation crossfade in [`crate::GrainEngine`]: coherent overlap
//! is compensated by the reciprocal of the window's mean, incoherent by one over the square root of
//! the count, and the same travel that replaces one process with the other moves the law. Regime
//! and law change together, so there is no undefined region between them.

use crate::rng::Rng;

/// Where jitter ends and the hand-over begins, as a fraction of the control's travel.
///
/// Chosen, and a keyboard measurement per the plan: the first half opens the slot, the second hands
/// the process over. Half and half is the starting point the listening gate can move.
pub(crate) const HANDOVER_AT: f32 = 0.5;

/// How far into a slot a fully jittered onset may fall.
///
/// Less than one so a jittered onset cannot land exactly on the next slot's boundary and read as a
/// double-fire; the last sliver is the hand-over's job, not jitter's.
const MAX_JITTER: f32 = 0.98;

#[derive(Debug, Clone)]
pub(crate) struct Scheduler {
    /// How far into the current slot we are, in samples.
    slot_position: f32,
    /// The slot this cycle was started with. Held so a moving density control cannot shorten a slot
    /// out from under an onset that has already been scheduled inside it.
    slot_length: f32,
    /// Where in this slot the periodic onset fires.
    offset: f32,
    /// This slot's onset was handed to the stochastic process.
    suppressed: bool,
    /// The periodic onset for this slot has not fired yet.
    armed: bool,
}

/// How the control divides into a jitter depth and a hand-over fraction.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Scatter {
    /// How much of a slot a jittered onset may wander over, `0..=MAX_JITTER`.
    pub(crate) jitter: f32,
    /// How much of the density has been handed to the per-sample draw, `0..=1`.
    pub(crate) handover: f32,
}

impl Scatter {
    pub(crate) fn from_control(scatter: f32) -> Self {
        let s = scatter.clamp(0.0, 1.0);
        if s <= HANDOVER_AT {
            Self {
                jitter: (s / HANDOVER_AT) * MAX_JITTER,
                handover: 0.0,
            }
        } else {
            Self {
                jitter: MAX_JITTER,
                handover: (s - HANDOVER_AT) / (1.0 - HANDOVER_AT),
            }
        }
    }
}

impl Scheduler {
    pub(crate) const fn new() -> Self {
        Self {
            slot_position: 0.0,
            slot_length: 0.0,
            offset: 0.0,
            suppressed: false,
            armed: false,
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::new();
    }

    /// Advance one sample and say how many grains start on it.
    ///
    /// Both processes can fire on the same sample while the hand-over is part way across; that is
    /// not a defect to be guarded against but the Poisson statistics the asynchronous branch is
    /// there to provide.
    #[inline]
    pub(crate) fn tick(&mut self, slot: f32, scatter: Scatter, rng: &mut Rng) -> u32 {
        let slot = slot.max(1.0);
        if self.slot_position >= self.slot_length {
            // A new slot. Carry the overshoot so onsets do not drift against the sample clock, and
            // adopt the current control value here rather than mid-slot.
            self.slot_position -= self.slot_length;
            self.slot_length = slot;
            self.offset = rng.unit() * scatter.jitter * slot;
            self.suppressed = rng.unit() < scatter.handover;
            self.armed = true;
        }

        let mut starts = 0;
        if self.armed && !self.suppressed && self.slot_position >= self.offset {
            self.armed = false;
            starts += 1;
        }
        if scatter.handover > 0.0 && rng.unit() < scatter.handover / slot {
            starts += 1;
        }
        self.slot_position += 1.0;
        starts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the scheduler for `samples` and return the onset times.
    fn onsets(slot: f32, scatter: f32, samples: usize) -> Vec<usize> {
        let mut s = Scheduler::new();
        let mut rng = Rng::new();
        let sc = Scatter::from_control(scatter);
        let mut out = Vec::new();
        for n in 0..samples {
            for _ in 0..s.tick(slot, sc, &mut rng) {
                out.push(n);
            }
        }
        out
    }

    #[test]
    fn the_periodic_branch_is_exactly_periodic() {
        let times = onsets(100.0, 0.0, 10_000);
        assert!(times.len() > 90, "{} onsets", times.len());
        for pair in times.windows(2) {
            assert_eq!(pair[1] - pair[0], 100, "not periodic: {pair:?}");
        }
    }

    #[test]
    fn a_fractional_slot_does_not_drift() {
        // The overshoot has to be carried or the onset rate is quietly wrong at every slot that is
        // not a whole number of samples.
        let slot = 100.7f32;
        let times = onsets(slot, 0.0, 100_000);
        let n = times.len() as f32;
        let span = (times[times.len() - 1] - times[0]) as f32;
        let measured = span / (n - 1.0);
        assert!((measured - slot).abs() < 0.05, "measured slot {measured}");
    }

    #[test]
    fn jitter_keeps_one_onset_per_slot() {
        // The whole reason a widened slot is not the stochastic branch: however far it is opened,
        // every slot still holds exactly one onset.
        let slot = 64.0f32;
        let times = onsets(slot, HANDOVER_AT, 64_000);
        let expected: i32 = 64_000 / 64;
        assert!(
            (times.len() as i32 - expected).abs() <= 2,
            "{} onsets against {expected}",
            times.len()
        );
        let mut per_slot = vec![0u32; 1000];
        for t in &times {
            per_slot[t / 64] += 1;
        }
        for (slot_index, count) in per_slot.iter().enumerate().take(990).skip(1) {
            assert_eq!(*count, 1, "slot {slot_index} held {count} onsets");
        }
    }

    #[test]
    fn jitter_actually_moves_the_onset() {
        let slot = 64.0f32;
        let times = onsets(slot, HANDOVER_AT, 64_000);
        let offsets: Vec<usize> = times.iter().map(|t| t % 64).collect();
        let lowest = *offsets.iter().min().unwrap();
        let highest = *offsets.iter().max().unwrap();
        assert!(
            lowest < 4,
            "jitter should reach the start of the slot: {lowest}"
        );
        assert!(
            highest > 55,
            "jitter should reach the end of the slot: {highest}"
        );
    }

    #[test]
    fn the_top_of_the_control_leaves_slots_empty_and_doubled() {
        // The property that separates the asynchronous process from any amount of jitter.
        let slot = 64.0f32;
        let times = onsets(slot, 1.0, 640_000);
        let mut per_slot = vec![0u32; 10_000];
        for t in &times {
            per_slot[t / 64] += 1;
        }
        let empty = per_slot.iter().filter(|c| **c == 0).count();
        let doubled = per_slot.iter().filter(|c| **c >= 2).count();
        assert!(empty > 2_000, "a Poisson process leaves gaps: {empty}");
        assert!(doubled > 2_000, "and doubles up: {doubled}");
    }

    #[test]
    fn the_mean_rate_holds_across_the_whole_control() {
        // The hand-over may change the statistics; it may not change the density. If it did, the
        // control would be a density control in disguise and the normalisation crossfade would be
        // compensating the wrong thing.
        let slot = 50.0f32;
        let samples = 500_000;
        let expected = samples as f32 / slot;
        for step in 0..=20 {
            let scatter = step as f32 / 20.0;
            let n = onsets(slot, scatter, samples).len() as f32;
            let error = (n - expected).abs() / expected;
            assert!(
                error < 0.05,
                "scatter {scatter}: {n} onsets against {expected}"
            );
        }
    }

    #[test]
    fn the_phasor_is_silent_at_the_top() {
        // At full hand-over every slot is suppressed, so every onset came from the per-sample draw.
        let mut s = Scheduler::new();
        let mut rng = Rng::new();
        let sc = Scatter::from_control(1.0);
        let mut suppressed_slots = 0;
        let mut slots = 0;
        for _ in 0..100_000 {
            let before = s.slot_position >= s.slot_length;
            let _ = s.tick(64.0, sc, &mut rng);
            if before {
                slots += 1;
                if s.suppressed {
                    suppressed_slots += 1;
                }
            }
        }
        assert!(slots > 1_000, "{slots} slots");
        assert_eq!(suppressed_slots, slots, "every slot should be handed over");
    }

    #[test]
    fn reset_repeats_the_sequence() {
        let a = onsets(37.0, 0.8, 20_000);
        let b = onsets(37.0, 0.8, 20_000);
        assert_eq!(a, b);
    }

    #[test]
    fn the_scatter_split_is_continuous() {
        let below = Scatter::from_control(HANDOVER_AT - 1.0e-4);
        let at = Scatter::from_control(HANDOVER_AT);
        let above = Scatter::from_control(HANDOVER_AT + 1.0e-4);
        assert!((below.jitter - at.jitter).abs() < 1.0e-3);
        assert_eq!(at.handover, 0.0);
        assert!(above.handover < 1.0e-3);
        assert_eq!(Scatter::from_control(1.0).handover, 1.0);
    }
}
