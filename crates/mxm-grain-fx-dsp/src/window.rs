//! The grain envelope, as one continuous control from boxcar to Hann.
//!
//! `plans/plan-mxm-grain-fx.md` §5: with a periodic scheduler the grain envelope *is* the waveform
//! of an amplitude modulator whose period is the grain, so the window shape decides how many
//! sidebands there are and how far they reach (`research:effects/granular-processing.md` §5, from
//! Truax 1988). That makes shape a **performed control**, not an engineering choice, and this
//! module exists so one parameter can travel the whole useful range cheaply.
//!
//! The morph is one triangular ramp shaped two ways, the method
//! `research:effects/granular-processing.md` §12 records and `research:effects/mutable-clouds.md`
//! §3 shows in a shipped product. Below the midpoint the ramp is multiplied by a slope and clamped,
//! which is a trapezoid running from a near-boxcar to a triangle; above it the ramp is crossfaded
//! toward `sin²(g·π/2)`, which — substituting the triangle for `g` — **is exactly a Hann window**,
//! not an approximation to one. That `sin²` is [tabled](SMOOTH_POINTS), because it was the one
//! transcendental left on this crate's per-grain path and a third of what a grain cost.
//!
//! **The one thing deliberately not reproduced.** The published implementation the research read
//! has a silent point: at exactly the midpoint its two branches set both the slope and the
//! smoothing to zero, so every grain scheduled at that value renders at zero gain
//! (`research:effects/mutable-clouds.md` §3). That is a latent bug, not a wart of any hardware, and
//! the collection's rule is that a defect is not reproduced. The slope law here is written so the
//! midpoint is an exact triangle from either side.

/// Points in the raised-cosine table the smooth half of the morph is built from.
///
/// The one transcendental left on this crate's per-grain path was the `.sin()` this replaces:
/// `grain_cost` measured the Hann end of the morph at 795.3 ns/frame against the ramp end's
/// 558.7 at 63 live grains, so 3.76 of 12.26 ns — 31% of a grain — was one sine. The sibling
/// `mxm-creative-sampler` took the same saving with a shared 1024-point table
/// (`crates/mxm-creative-sampler-dsp/src/lib.rs`), and
/// [`docs/oscillators/10-granular.md`](../../../docs/oscillators/10-granular.md) §10.8 lists it as
/// one of the four cost decisions.
///
/// **2048 rather than the sampler's 1024, because this table is held to a tighter number.** The
/// sampler bounds its table against the 16-bit converter step; the test below keeps this one
/// inside the 1e-6 that `the_smooth_end_is_hann_to_the_bit_of_a_float` already demanded of the
/// computed form, and linear interpolation of `sin²(πx/2)` errs by at most `|f''|h²/8` —
/// `(π²/2)/(8·2048²)` = 1.5e-7 here, against 5.9e-7 at 1024. The table is 8 KB and every live
/// grain walks it in step, so it stays resident.
const SMOOTH_POINTS: usize = 2048;

/// `sin²(π·x/2)` for `x` in `0..=1`, with a duplicate final row so interpolating `i`→`i + 1` never
/// needs a bound.
///
/// Built once, off the audio thread: [`crate::GrainEngine::new`] forces it, so no callback can be
/// the first caller. It is `LazyLock` rather than a `const` because `sin` is not a const fn, and
/// this crate has no dependencies to generate one with.
static SMOOTH: std::sync::LazyLock<[f32; SMOOTH_POINTS + 1]> = std::sync::LazyLock::new(|| {
    let mut table = [0.0f32; SMOOTH_POINTS + 1];
    for (point, slot) in table.iter_mut().enumerate() {
        let x = point as f64 / SMOOTH_POINTS as f64;
        let s = (x * core::f64::consts::FRAC_PI_2).sin();
        *slot = (s * s) as f32;
    }
    table
});

/// Build the table now, so the first grain does not.
pub(crate) fn prepare() {
    std::sync::LazyLock::force(&SMOOTH);
}

/// `sin²(π·x/2)`, tabled. `x` is clamped into `0..=1`.
///
/// Substituting the triangular ramp for `x` gives `sin²(π·phase)`, which **is** the Hann window —
/// so this one table is the whole smooth half of the morph.
///
/// The clamp is redundant — the one caller derives `x` from an already-clamped phase — and it was
/// measured out and put back: dropping it moved nothing outside the run-to-run spread, and a
/// bounded lookup that reads correctly on its own is worth more than an operation the machine did
/// not charge for. Both table sizes were measured too; 1024 was not faster than 2048, so the size
/// is decided by the error bound above and nothing else.
#[inline]
fn smooth(x: f32) -> f32 {
    let scaled = x.clamp(0.0, 1.0) * SMOOTH_POINTS as f32;
    let point = (scaled as usize).min(SMOOTH_POINTS - 1);
    let blend = scaled - point as f32;
    let low = SMOOTH[point];
    low + (SMOOTH[point + 1] - low) * blend
}

/// How many points the mean and mean-square are integrated over.
///
/// Chosen. The window is smooth and these two numbers only set a gain, so a coarse sum is honest;
/// 128 puts the error on both well below a thousandth, and they are computed at control rate.
const MOMENT_POINTS: usize = 128;

/// The window at `phase` (0 at the grain's start, 1 at its end) for a given `shape`.
///
/// `shape` 0 is a near-boxcar, 0.5 an exact triangle, 1 an exact Hann.
#[inline]
pub fn window(shape: f32, phase: f32) -> f32 {
    let phase = phase.clamp(0.0, 1.0);
    // A triangle peaking at 1 in the middle of the grain. Everything below shapes this ramp.
    let ramp = if phase < 0.5 {
        phase * 2.0
    } else {
        2.0 - phase * 2.0
    };
    let shape = shape.clamp(0.0, 1.0);
    if shape < 0.5 {
        // Trapezoid. The slope law is chosen so that `shape` -> 0.5 gives a slope of exactly 1,
        // which is the triangle the upper branch starts from: the two halves meet with no step and
        // no silent point. At `shape` 0 the slope is 50, an attack of one hundredth of the grain —
        // audibly a boxcar, and deliberately clicky.
        let slope = 0.5 / (shape * 0.98 + 0.01);
        (ramp * slope).min(1.0)
    } else {
        // Triangle -> Hann. `sin²(ramp·π/2)` with `ramp = 2·phase` on the first half is
        // `sin²(π·phase)`, which is the Hann window by definition.
        let blend = (shape - 0.5) * 2.0;
        let hann = smooth(ramp);
        ramp + blend * (hann - ramp)
    }
}

/// The window's mean and mean-square, which are the two normalisation laws' whole content.
///
/// `plans/plan-mxm-grain-fx.md` §5: overlapping grains sum **coherently** under a periodic
/// scheduler, where the compensation is the reciprocal of the window's mean, and **incoherently**
/// under a stochastic one, where power adds and the compensation goes as one over the square root
/// of the count times the mean-square. Both numbers therefore have to be known for whatever shape
/// the control is sitting at, and integrating them is cheaper and less error-prone than carrying a
/// closed form through the crossfade in the upper branch.
///
/// Returned as `(mean, mean_square)`.
pub fn moments(shape: f32) -> (f32, f32) {
    let mut sum = 0.0f32;
    let mut sum_sq = 0.0f32;
    for i in 0..MOMENT_POINTS {
        // Midpoint rule: samples the window's interior rather than its two zero endpoints.
        let phase = (i as f32 + 0.5) / MOMENT_POINTS as f32;
        let w = window(shape, phase);
        sum += w;
        sum_sq += w * w;
    }
    let n = MOMENT_POINTS as f32;
    (sum / n, sum_sq / n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32, what: &str) {
        assert!((a - b).abs() <= tol, "{what}: {a} vs {b}");
    }

    #[test]
    fn every_shape_starts_and_ends_at_zero() {
        for i in 0..=20 {
            let shape = i as f32 / 20.0;
            assert_eq!(window(shape, 0.0), 0.0, "shape {shape} start");
            assert_eq!(window(shape, 1.0), 0.0, "shape {shape} end");
        }
    }

    #[test]
    fn every_shape_reaches_unity_in_the_middle() {
        for i in 0..=20 {
            let shape = i as f32 / 20.0;
            close(window(shape, 0.5), 1.0, 1e-6, "peak");
        }
    }

    #[test]
    fn the_midpoint_is_an_exact_triangle_from_either_side() {
        // The defect this crate refuses to inherit: a shape that renders silent because both
        // branches zeroed their own coefficient. Approach the midpoint from below and above and
        // require the triangle at, and either side of, it.
        for phase in [0.1f32, 0.25, 0.4, 0.5, 0.6, 0.75, 0.9] {
            let triangle = if phase < 0.5 {
                phase * 2.0
            } else {
                2.0 - phase * 2.0
            };
            close(window(0.5, phase), triangle, 1e-6, "at the midpoint");
            close(window(0.4999, phase), triangle, 1e-3, "just below");
            close(window(0.5001, phase), triangle, 1e-3, "just above");
        }
    }

    #[test]
    fn no_shape_is_silent() {
        // The general form of the test above: sweep the control finely and require every setting to
        // put real gain in the middle of the grain.
        for i in 0..=2000 {
            let shape = i as f32 / 2000.0;
            let (mean, _) = moments(shape);
            assert!(mean > 0.4, "shape {shape} has mean {mean}");
        }
    }

    /// The table against the sine it replaced, over the whole upper half of the morph.
    ///
    /// The sampler holds its window table to the 16-bit converter's own step. This one is held
    /// tighter, because `the_smooth_end_is_hann_to_the_bit_of_a_float` already demanded 1e-6 of the
    /// computed form and a saving may not quietly spend a tolerance that was being met.
    #[test]
    fn the_tabled_morph_matches_the_sine_it_replaced() {
        fn computed(shape: f32, phase: f32) -> f32 {
            let phase = phase.clamp(0.0, 1.0);
            let ramp = if phase < 0.5 {
                phase * 2.0
            } else {
                2.0 - phase * 2.0
            };
            let blend = (shape - 0.5) * 2.0;
            let s = (ramp * core::f32::consts::FRAC_PI_2).sin();
            ramp + blend * (s * s - ramp)
        }
        let mut worst = 0.0f32;
        for i in 0..=64 {
            let shape = 0.5 + (i as f32 / 64.0) * 0.5;
            // Deliberately off the table's own rows: an odd denominator lands between them as
            // often as on them, which is where a tabled curve is at its worst.
            for j in 0..=1999 {
                let phase = j as f32 / 1999.0;
                worst = worst.max((window(shape, phase) - computed(shape, phase)).abs());
            }
        }
        assert!(worst <= 1.0e-6, "the window table deviates by {worst}");
        // The landmarks stay exact rather than nearly: a grain must start and end at zero, and
        // reach unity, whatever the table does between.
        assert_eq!(window(1.0, 0.0), 0.0);
        assert_eq!(window(1.0, 1.0), 0.0);
        assert_eq!(window(1.0, 0.5), 1.0);
        // Out-of-domain arguments are clamped rather than indexed with. A NaN is *not* claimed
        // here: `window` never handled one and the engine's `clamp01` is what keeps one away.
        assert_eq!(window(2.0, -1.0), 0.0);
        assert_eq!(window(-1.0, 2.0), 0.0);
    }

    #[test]
    fn the_smooth_end_is_hann_to_the_bit_of_a_float() {
        for i in 0..=100 {
            let phase = i as f32 / 100.0;
            let expected = {
                let s = (core::f32::consts::PI * phase).sin();
                s * s
            };
            close(window(1.0, phase), expected, 1e-6, "hann");
        }
    }

    #[test]
    fn moments_match_the_shapes_they_describe() {
        // Boxcar: an attack of a hundredth each side, so mean 1 - 1/100 and mean-square close to 1.
        let (m1, m2) = moments(0.0);
        close(m1, 0.99, 0.01, "boxcar mean");
        close(m2, 0.987, 0.01, "boxcar mean-square");

        // Triangle: mean 1/2, mean-square 1/3, both exact.
        let (m1, m2) = moments(0.5);
        close(m1, 0.5, 0.005, "triangle mean");
        close(m2, 1.0 / 3.0, 0.005, "triangle mean-square");

        // Hann: mean 1/2, mean-square 3/8, both exact.
        let (m1, m2) = moments(1.0);
        close(m1, 0.5, 0.005, "hann mean");
        close(m2, 0.375, 0.005, "hann mean-square");
    }

    /// Energy in the far sidelobes of the window's own spectrum.
    ///
    /// This is the quantity §5 is about: under a periodic scheduler the window is the modulator's
    /// waveform, so how far its sidebands reach is how much of the spectrum the AM smears into.
    fn far_sidelobe_energy(shape: f32) -> f32 {
        const N: usize = 256;
        let w: Vec<f32> = (0..N)
            .map(|i| window(shape, (i as f32 + 0.5) / N as f32))
            .collect();
        // Bins well away from the main lobe, which for every one of these windows is a handful wide.
        (8..64)
            .map(|k| {
                let (mut re, mut im) = (0.0f64, 0.0f64);
                for (i, v) in w.iter().enumerate() {
                    let angle = -2.0 * core::f64::consts::PI * (k as f64) * (i as f64) / (N as f64);
                    re += f64::from(*v) * angle.cos();
                    im += f64::from(*v) * angle.sin();
                }
                ((re * re + im * im) / (N as f64 * N as f64)) as f32
            })
            .sum()
    }

    #[test]
    fn smoothing_the_window_pulls_its_sidebands_in() {
        // The control's justification: shape decides how far the AM's sidebands reach, so the
        // far-band energy has to collapse across the travel. Measured over the whole sweep at 256
        // points, bins 8..64:
        //
        //   boxcar 2.5e-3 · quarter 3.7e-5 · triangle 1.3e-5 · three-quarters 3.2e-6 · Hann 3.5e-30
        //
        // Two things that measurement settles, and which a guess got wrong first time:
        //
        // - **The upper half is strictly monotone**, triangle to Hann, and the Hann end is machine
        //   zero — a Hann window's spectrum really is three bins wide, which is independent
        //   confirmation that the morph's smooth end is exactly Hann rather than nearly.
        // - **The lower half is not.** A trapezoid's sidelobe nulls move as its ramp widens, so the
        //   far-band sum wiggles by a factor of two on its way down (0.10 -> 0.15 rises, so does
        //   0.35 -> 0.45). The trend is four orders of magnitude downward and the local direction is
        //   not the claim. Asserting monotonicity there would be asserting an artefact.
        let boxcar = far_sidelobe_energy(0.0);
        let triangle = far_sidelobe_energy(0.5);
        let hann = far_sidelobe_energy(1.0);
        assert!(
            triangle < boxcar * 0.02,
            "triangle {triangle} vs boxcar {boxcar}"
        );
        assert!(
            hann < triangle * 1.0e-6,
            "hann {hann} vs triangle {triangle}"
        );

        // The Hann end is the global minimum. The boxcar end is *not* the global maximum — a
        // near-boxcar with a slightly wider ramp can put more energy in this band than the narrowest
        // one, for the same reason the lower half wiggles — so that is not claimed.
        for i in 0..=40 {
            let shape = i as f32 / 40.0;
            let energy = far_sidelobe_energy(shape);
            assert!(energy >= hann, "shape {shape} beat the Hann: {energy}");
        }

        // The trend across the travel, which is the claim the wiggle does not disturb.
        let low: f32 = (0..=8).map(|i| far_sidelobe_energy(i as f32 / 40.0)).sum();
        let high: f32 = (32..=40)
            .map(|i| far_sidelobe_energy(i as f32 / 40.0))
            .sum();
        assert!(low > high * 100.0, "no trend: {low} against {high}");

        // And the half that carries the claim is monotone.
        let mut previous = f32::MAX;
        for i in 20..=40 {
            let shape = i as f32 / 40.0;
            let energy = far_sidelobe_energy(shape);
            assert!(
                energy <= previous * 1.001,
                "shape {shape} widened its sidebands: {energy} after {previous}"
            );
            previous = energy;
        }
    }

    #[test]
    fn the_morph_is_continuous_in_shape() {
        // A performed control may not jump, and the defect this crate refuses to inherit was
        // exactly a discontinuity — one shape value rendering silent between two that do not. Sweep
        // finely and bound the step.
        let steps = 4000;
        for i in 0..steps {
            let a = i as f32 / steps as f32;
            let b = (i + 1) as f32 / steps as f32;
            for j in 0..=16 {
                let phase = j as f32 / 16.0;
                let delta = (window(b, phase) - window(a, phase)).abs();
                assert!(
                    delta < 0.02,
                    "jump of {delta} between {a} and {b} at {phase}"
                );
            }
        }
    }

    #[test]
    fn the_window_is_symmetric() {
        // Truax's palindrome property: a symmetric envelope is what makes granular texture sound
        // the same played backwards, and asymmetry here would be an undeclared design choice.
        for i in 0..=20 {
            let shape = i as f32 / 20.0;
            for j in 0..=50 {
                let phase = j as f32 / 100.0;
                close(
                    window(shape, phase),
                    window(shape, 1.0 - phase),
                    1e-6,
                    "symmetry",
                );
            }
        }
    }
}
