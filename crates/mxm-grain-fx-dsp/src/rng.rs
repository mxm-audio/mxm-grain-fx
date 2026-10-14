//! The grains' generator, seeded on reset.
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
//! `mxm-rng`'s xorshift64* satisfies all three: one multiply and three shift-xors, a 64-bit state
//! that is part of the reset, and no allocation or table. Its draws take the top 24 bits, exactly
//! f32's mantissa, so every value is representable and the distribution has no gaps or repeats.

use mxm_rng::Xorshift64Star;

/// The seed every reset returns to. Any odd, non-zero value does; this one is arbitrary.
const SEED: u64 = 0x2545_F491_4F6C_DD1D;

/// The generator at its seed: what construction starts from and every `reset()` returns to.
pub(crate) const fn seeded() -> Xorshift64Star {
    Xorshift64Star::from_state(SEED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_returns_the_same_sequence() {
        let mut rng = seeded();
        let first: Vec<f32> = (0..64).map(|_| rng.unit_f32()).collect();
        rng = seeded();
        let second: Vec<f32> = (0..64).map(|_| rng.unit_f32()).collect();
        assert_eq!(first, second);
    }
}
