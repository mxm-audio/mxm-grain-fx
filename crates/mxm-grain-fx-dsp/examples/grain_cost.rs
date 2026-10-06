//! What a grain costs in this engine.
//!
//! The other three examples measure what the engine *sounds* like — sidebands, loudness laws,
//! tails. None of them measures what it costs, and the pool ceiling ([`MAX_GRAINS`]) was chosen
//! without one. This closes that gap.
//!
//! Method, following `mxm-creative-sampler-dsp/examples/measure_readers.rs`: render a fixed number
//! of frames through the real public API with the pool driven to a known occupancy, time it with
//! `Instant`, and report against realtime. No dependency is added — this crate has none and keeps
//! none, so there is no criterion here and the figures are best-of-N rather than a distribution.
//!
//! **The marginal cost is the result.** A single occupancy mixes the engine's fixed per-sample work
//! — capture write, two high-passes, the scheduler, the smoothers — with the grains, and the fixed
//! part does not scale. Two occupancies separate them.
//!
//! Run with: `cargo run -p mxm-grain-fx-dsp --release --example grain_cost`

use mxm_grain_fx_dsp::{Controls, GrainEngine, MAX_GRAINS};
use std::hint::black_box;
use std::time::Instant;

const RATE: f32 = 48_000.0;
const BLOCK: usize = 128;
const FRAMES: usize = 240_000;
const RUNS: usize = 5;

/// One timed render. Returns nanoseconds per output frame and the live grain count observed at the
/// end, so the requested occupancy can be checked rather than assumed.
fn probe(controls: &Controls) -> (f64, usize) {
    let mut engine = GrainEngine::new(RATE);
    engine.set_controls(controls);
    let mut left = [0.0f32; BLOCK];
    let mut right = [0.0f32; BLOCK];
    // A deterministic broadband input. Silence would leave the grains reading zeros, which costs
    // the same but would let a future denormal-flush change hide here.
    let mut noise: u32 = 0x1234_5678;
    let mut fill = |left: &mut [f32; BLOCK], right: &mut [f32; BLOCK]| {
        for i in 0..BLOCK {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            let x = (noise >> 8) as f32 / 8_388_608.0 - 1.0;
            left[i] = x * 0.25;
            right[i] = -x * 0.25;
        }
    };
    // Fill the capture buffer and let the pool reach its steady occupancy before timing.
    for _ in 0..(BUFFER_WARMUP / BLOCK) {
        fill(&mut left, &mut right);
        engine.process(&mut left, &mut right);
    }
    let started = Instant::now();
    for _ in 0..(FRAMES / BLOCK) {
        fill(&mut left, &mut right);
        black_box(engine.process(&mut left, &mut right));
        black_box(left[0] + right[0]);
    }
    let elapsed = started.elapsed().as_secs_f64();
    let frames = (FRAMES / BLOCK) * BLOCK;
    (elapsed * 1.0e9 / frames as f64, engine.active_grains())
}

/// Frames rendered before timing starts: long enough to fill the four-second capture buffer and
/// settle every smoother.
const BUFFER_WARMUP: usize = 5 * 48_000;

fn best_of(controls: &Controls) -> (f64, usize) {
    let mut best = f64::INFINITY;
    let mut live = 0;
    for _ in 0..RUNS {
        let (ns, n) = probe(controls);
        if ns < best {
            best = ns;
        }
        live = n;
    }
    (best, live)
}

/// Controls that put `target` grains in the air: density is a rate, so occupancy is
/// `length_s * density_hz`.
fn at_occupancy(target: f64, shape: f32) -> Controls {
    let length_s = 0.25f32;
    Controls {
        mix: 1.0,
        length_s,
        density_hz: (target / f64::from(length_s)) as f32,
        scatter: 0.0,
        position: 0.5,
        window_shape: shape,
        feedback: 0.0,
        ..Controls::default()
    }
}

fn main() {
    println!("mxm-grain-fx cost probe\n");
    println!(
        "{FRAMES} frames at {RATE:.0} Hz in blocks of {BLOCK}, best of {RUNS}. One frame lasts\n\
         {:.0} ns, so `% core` is ns/frame against that. `per grain` is the marginal cost taken\n\
         from the row above it, which is what separates the grains from the engine's fixed work.\n",
        1.0e9 / f64::from(RATE)
    );
    let frame_ns = 1.0e9 / f64::from(RATE);

    println!(
        "{:>18}  {:>6}  {:>10}  {:>8}  {:>10}",
        "variant", "live", "ns/frame", "% core", "per grain"
    );

    let mut previous: Option<(f64, f64)> = None;
    for target in [0.0f64, 1.0, 8.0, 32.0, 64.0] {
        let controls = at_occupancy(target.max(0.001), 0.75);
        let controls = if target == 0.0 {
            Controls {
                density_hz: 0.5,
                length_s: 0.002,
                ..controls
            }
        } else {
            controls
        };
        let (ns, live) = best_of(&controls);
        let marginal = previous.map(|(pn, pl)| (ns - pn) / (live as f64 - pl).max(1.0));
        println!(
            "{:>18}  {:>6}  {:>10.1}  {:>7.1}%  {:>10}",
            format!("{target:.0} asked"),
            live,
            ns,
            ns / frame_ns * 100.0,
            marginal.map_or("-".to_string(), |m| format!("{m:.2} ns"))
        );
        previous = Some((ns, live as f64));
    }

    // The window is the one transcendental left on the per-grain path: `window()` takes a `.sin()`
    // whenever shape >= 0.5, and returns a scaled ramp below it. The two ends are not the same
    // envelope, so this is a price, not a substitution.
    println!(
        "\n{:>18}  {:>6}  {:>10}  {:>8}",
        "window", "live", "ns/frame", "% core"
    );
    for (label, shape) in [("ramp (no sin)", 0.25f32), ("hann (sin)", 1.0)] {
        let (ns, live) = best_of(&at_occupancy(64.0, shape));
        println!(
            "{label:>18}  {live:>6}  {ns:>10.1}  {:>7.1}%",
            ns / frame_ns * 100.0
        );
    }

    println!(
        "\nPool ceiling is MAX_GRAINS = {MAX_GRAINS}. Whether that is a CPU limit or a product\n\
         choice is what the marginal column answers: multiply it by the pool and compare against\n\
         {:.0} ns.",
        frame_ns
    );
}
