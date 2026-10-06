//! G2's measurements: what the recycled loop actually does.
//!
//! `plans/plan-mxm-grain-fx.md` §6 says the in-loop high-pass corner law is *a G2 measurement, not
//! an inheritance*, and §7 says *sustaining* is measured on the signal rather than read off the
//! controls. Both need numbers rather than reasoning, and this is where they come from.
//!
//! ```text
//! cargo run -p mxm-grain-fx-dsp --release --example loop_probe
//! ```

use mxm_grain_fx_dsp::{Activity, Controls, GrainEngine, Randomisation};

const RATE: f32 = 48_000.0;

fn base() -> Controls {
    Controls {
        mix: 1.0,
        length_s: 0.040,
        density_hz: 200.0,
        scatter: 0.3,
        position: 0.3,
        pitch_semitones: 0.0,
        reverse: 0.0,
        window_shape: 0.7,
        feedback: 0.0,
        freeze: false,
        randomisation: Randomisation::default(),
    }
}

/// Excite the engine, then let it run in silence and report how the field behaves.
struct Decay {
    /// Seconds until the field falls 60 dB below its peak, or `None` if it never does.
    to_minus_60: Option<f32>,
    /// Level after thirty seconds, relative to the peak.
    after_30s: f32,
    /// What the engine said it was doing at the end.
    activity: Activity,
}

fn measure(controls: &Controls, seconds: f32) -> Decay {
    let mut engine = GrainEngine::new(RATE);
    engine.set_controls(controls);

    // Two seconds of a broadband burst, so the buffer holds something with structure.
    let burst = (RATE * 2.0) as usize;
    let mut state = 9_871u32;
    let mut l = vec![0.0f32; burst];
    let mut r = vec![0.0f32; burst];
    for i in 0..burst {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (state >> 9) as f32 / 8_388_608.0 - 1.0;
        let tone = (i as f32 * 220.0 * core::f32::consts::TAU / RATE).sin();
        l[i] = 0.4 * tone + 0.2 * noise;
        r[i] = l[i];
    }
    let mut at = 0;
    while at < burst {
        let take = 128.min(burst - at);
        let _ = engine.process(&mut l[at..at + take], &mut r[at..at + take]);
        at += take;
    }

    // Then silence, watching the envelope.
    let block = 256usize;
    let total = (RATE * seconds) as usize;
    let mut peak = 0.0f32;
    let mut envelope = 0.0f32;
    let mut to_minus_60 = None;
    let mut activity = Activity::Parked;
    let mut done = 0usize;
    while done < total {
        let mut bl = vec![0.0f32; block];
        let mut br = vec![0.0f32; block];
        activity = engine.process(&mut bl, &mut br);
        for i in 0..block {
            let level = bl[i].abs().max(br[i].abs());
            envelope += 0.001 * (level - envelope);
            peak = peak.max(envelope);
            if to_minus_60.is_none() && peak > 0.0 && envelope < peak * 0.001 {
                to_minus_60 = Some((done + i) as f32 / RATE);
            }
        }
        done += block;
    }
    Decay {
        to_minus_60,
        after_30s: if peak > 0.0 { envelope / peak } else { 0.0 },
        activity,
    }
}

fn main() {
    println!("mxm-grain-fx — loop probe at {RATE} Hz\n");

    println!("Feedback against decay, at the base setting:");
    println!(
        "  {:>8}  {:>12}  {:>12}  activity",
        "feedback", "-60 dB", "after 30 s"
    );
    for step in 0..=10 {
        let feedback = step as f32 / 10.0;
        let mut c = base();
        c.feedback = feedback;
        let d = measure(&c, 30.0);
        let sixty = d
            .to_minus_60
            .map(|s| format!("{s:.2} s"))
            .unwrap_or_else(|| "never".into());
        println!(
            "  {:>8.1}  {:>12}  {:>12.5}  {:?}",
            feedback, sixty, d.after_30s, d.activity
        );
    }

    println!("\nDensity and scatter at full feedback — the collapse the plan calls character:");
    println!(
        "  {:>8}  {:>8}  {:>12}  {:>12}",
        "density", "scatter", "-60 dB", "after 30 s"
    );
    for &density in &[0.5f32, 2.0, 8.0, 24.0] {
        for &scatter in &[0.0f32, 0.5, 1.0] {
            let mut c = base();
            c.feedback = 1.0;
            c.density_hz = density / c.length_s;
            c.scatter = scatter;
            let d = measure(&c, 30.0);
            let sixty = d
                .to_minus_60
                .map(|s| format!("{s:.2} s"))
                .unwrap_or_else(|| "never".into());
            println!(
                "  {density:>8.1}  {scatter:>8.1}  {sixty:>12}  {:>12.5}",
                d.after_30s
            );
        }
    }

    println!("\nWindow shape at full feedback — a boxcar returns the most energy per grain:");
    println!("  {:>8}  {:>12}  {:>12}", "shape", "-60 dB", "after 30 s");
    for step in 0..=4 {
        let shape = step as f32 / 4.0;
        let mut c = base();
        c.feedback = 1.0;
        c.window_shape = shape;
        let d = measure(&c, 30.0);
        let sixty = d
            .to_minus_60
            .map(|s| format!("{s:.2} s"))
            .unwrap_or_else(|| "never".into());
        println!("  {shape:>8.2}  {sixty:>12}  {:>12.5}", d.after_30s);
    }
}
