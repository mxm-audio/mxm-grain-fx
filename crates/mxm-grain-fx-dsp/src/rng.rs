//! A small deterministic generator, seeded on reset.
//!
//! The scheduler and every per-grain randomisation depth draw from this one generator, and
//! `plans/plan-mxm-grain-fx.md` §4 makes three demands of it that shape the type rather than just
//! its use:
//!
//! 1. **Seeded on `reset()`** — the same input and controls after a reset produce the same samples.
//!    Shimmer's shifter already works this way; this crate adopts the rule.
//! 2. **Advanced per sample, never per block** — if a draw happened once per callback the sequence
//!    would depend on where the host cut the buffer, and the output would stop being invariant
//!    under block partitioning.
//! 3. Cheap enough to call several times per sample without thinking about it.
//!
//! xorshift64* satisfies all three: one multiply and three shift-xors, a 64-bit state that is part
//! of the reset, and no allocation or table.

/// The seed every reset returns to. Any odd, non-zero value does; this one is arbitrary.
const SEED: u64 = 0x2545_F491_4F6C_DD1D;

#[derive(Debug, Clone)]
pub(crate) struct Rng {
    state: u64,
}

impl Rng {
    pub(crate) const fn new() -> Self {
        Self { state: SEED }
    }

    /// Return to the seed. Part of `reset()`, not a convenience.
    pub(crate) fn reset(&mut self) {
        self.state = SEED;
    }

    #[inline]
    fn next_u64(&mut self) -> u64 {
        // xorshift64*, Vigna 2016.
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, 1)`.
    #[inline]
    pub(crate) fn unit(&mut self) -> f32 {
        // Take the top 24 bits, which is exactly f32's mantissa, so every value is representable
        // and the distribution has no gaps or repeats.
        ((self.next_u64() >> 40) as f32) * (1.0 / 16_777_216.0)
    }

    /// Uniform in `[-1, 1)`.
    #[inline]
    pub(crate) fn bipolar(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_returns_the_same_sequence() {
        let mut rng = Rng::new();
        let first: Vec<f32> = (0..64).map(|_| rng.unit()).collect();
        rng.reset();
        let second: Vec<f32> = (0..64).map(|_| rng.unit()).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn unit_stays_in_range_and_moves() {
        let mut rng = Rng::new();
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        for _ in 0..100_000 {
            let v = rng.unit();
            assert!((0.0..1.0).contains(&v), "{v} out of range");
            min = min.min(v);
            max = max.max(v);
        }
        // Not a statistical test, just proof it is not stuck in a corner.
        assert!(min < 0.01, "min {min}");
        assert!(max > 0.99, "max {max}");
    }

    #[test]
    fn bipolar_is_centred() {
        let mut rng = Rng::new();
        let n = 200_000;
        let mean: f64 = (0..n).map(|_| f64::from(rng.bipolar())).sum::<f64>() / f64::from(n);
        assert!(mean.abs() < 0.01, "mean {mean}");
    }
}
