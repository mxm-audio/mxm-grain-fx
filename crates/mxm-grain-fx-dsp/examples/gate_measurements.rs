//! The three measurements `plans/plan-mxm-grain-fx.md` G3 asks for, before the listening gate.
//!
//! G3 is the owner's, and it is judged by ear. What can be measured is measured first, so the ear
//! is deciding about character rather than about whether the thing works:
//!
//! 1. **AM sidebands** at a fixed grain length as the window sweeps boxcar to Hann, against what §5
//!    predicts — the envelope is the modulator's waveform, so a sharper window puts more energy
//!    further out.
//! 2. **The loudness law across density on both scheduler branches.** Coherent and incoherent
//!    overlap do not sum the same way, and a processor using one law for both is wrong by up to the
//!    square root of the grain count on one of them. This prints what the shipped pair does and what
//!    each single law would have done.
//! 3. **A length sweep across all three regions of §5** — sub-period buzz, fusion, discrete events —
//!    so the short end is proved reached rather than assumed.
//!
//! ```text
//! cargo run -p mxm-grain-fx-dsp --release --example gate_measurements
//! ```

use mxm_grain_fx_dsp::{Controls, GrainEngine, MAX_LENGTH_S, MIN_LENGTH_S, Randomisation};

const RATE: f32 = 48_000.0;
const TONE_HZ: f32 = 440.0;

fn steady() -> Controls {
    Controls {
        mix: 1.0,
        length_s: 0.040,
        density_hz: 80.0,
        scatter: 0.0,
        position: 0.2,
        pitch_semitones: 0.0,
        reverse: 0.0,
        window_shape: 0.5,
        feedback: 0.0,
        freeze: false,
        randomisation: Randomisation::default(),
    }
}

/// Render a steady sine through the engine and return the settled output.
///
/// **The buffer has to be full before anything is measured.** `BUFFER_S` of audio has to arrive
/// before the position control can mean what it says, and analysing during the fill measures the
/// startup transient rather than the effect. Everything before the last `seconds` is discarded.
fn render_tone(controls: &Controls, seconds: f32) -> Vec<f32> {
    let mut engine = GrainEngine::new(RATE);
    engine.set_controls(controls);
    let settle = (RATE * (mxm_grain_fx_dsp::BUFFER_S + 1.0)) as usize;
    let n = (RATE * seconds) as usize + settle;
    let mut l: Vec<f32> = (0..n)
        .map(|i| (i as f32 * TONE_HZ * core::f32::consts::TAU / RATE).sin() * 0.5)
        .collect();
    let mut r = l.clone();
    let mut at = 0;
    while at < n {
        let take = 256.min(n - at);
        let _ = engine.process(&mut l[at..at + take], &mut r[at..at + take]);
        at += take;
    }
    l.split_off(settle)
}

/// Magnitude at one frequency, by Goertzel over the whole block.
fn magnitude_at(signal: &[f32], hz: f32) -> f32 {
    let w = core::f64::consts::TAU * f64::from(hz) / f64::from(RATE);
    let (c, s) = (w.cos(), w.sin());
    let coeff = 2.0 * c;
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in signal {
        let s0 = f64::from(v) + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let real = s1 - s2 * c;
    let imag = s2 * s;
    ((real * real + imag * imag).sqrt() / (signal.len() as f64 / 2.0)) as f32
}

fn rms(signal: &[f32]) -> f32 {
    if signal.is_empty() {
        return 0.0;
    }
    (signal
        .iter()
        .map(|v| f64::from(*v) * f64::from(*v))
        .sum::<f64>()
        / signal.len() as f64)
        .sqrt() as f32
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1.0e-12).log10()
}

/// How many sidebands stand above a floor relative to the carrier, and how far out the last one is.
fn sideband_profile(signal: &[f32], grain_hz: f32) -> (usize, f32, f32) {
    let carrier = magnitude_at(signal, TONE_HZ);
    let floor = carrier * 0.01; // -40 dB relative to the carrier
    let mut count = 0usize;
    let mut furthest = 0.0f32;
    let mut energy = 0.0f32;
    for k in 1..=24 {
        let offset = grain_hz * k as f32;
        for side in [TONE_HZ - offset, TONE_HZ + offset] {
            if side < 20.0 || side > RATE * 0.45 {
                continue;
            }
            let m = magnitude_at(signal, side);
            energy += m * m;
            if m > floor {
                count += 1;
                furthest = furthest.max(offset);
            }
        }
    }
    (count, furthest, energy.sqrt() / carrier.max(1.0e-9))
}

fn measurement_one() {
    println!("1. AM sidebands against density, at three window shapes");
    println!(
        "   Grain 40 ms against a {TONE_HZ:.0} Hz carrier. The modulator sits at the onset rate,
   which is density/length — 12.5 Hz at density 0.5, 150 Hz at density 6.
"
    );
    println!(
        "   Where the AM comes from, and why this sweeps density rather than shape: overlapping
   windows at a periodic rate sum to `density x mean(w)`, which is a *constant* wherever the
   windows tile. Perfect tiling is perfect reconstruction and there is no modulation at all.
   The AM Truax describes is what is left when the tiling fails — most of it around one grain
   per grain-length, where each grain is followed by the next rather than overlapped with it.
   The window shape decides how fast the tiling recovers as density rises.
"
    );
    for &shape in &[0.0f32, 0.5, 1.0] {
        let name = match shape {
            s if s < 0.25 => "boxcar",
            s if s < 0.75 => "triangle",
            _ => "hann",
        };
        println!("   window {name} ({shape:.2}):");
        println!(
            "   {:>8}  {:>16}  {:>12}",
            "density", "sideband/carrier", "carrier dB"
        );
        for &overlap in &[0.5f32, 0.75, 1.0, 1.5, 2.0, 3.0, 6.0] {
            let mut c = steady();
            c.window_shape = shape;
            c.density_hz = overlap / c.length_s;
            c.scatter = 0.0;
            let out = render_tone(&c, 2.0);
            // The modulator's period is the *inter-onset interval*, not the grain length: at
            // density 0.5 a 40 ms grain is followed by another 80 ms later.
            let (_, _, ratio) = sideband_profile(&out, c.density_hz);
            println!(
                "   {overlap:>8.2}  {ratio:>16.4}  {:>12.2}",
                db(magnitude_at(&out, TONE_HZ))
            );
        }
        println!();
    }
    println!(
        "   A boxcar tiles at density 1 and a Hann at density 2, so each goes quiet once it does.
   Below that the envelope is the modulator's waveform and the sidebands are its spectrum —
   which is why window shape is a performed control and not a quality setting.
"
    );
}

fn measurement_two() {
    println!("2. The loudness law across density, on both branches");
    println!(
        "   The shipped pair against what a single law would have given. `coherent-only` and\n   `incoherent-only` are what the output would measure if one law were used for both.\n"
    );
    for (name, scatter, spray) in [
        ("periodic, grains reading the same material", 0.0f32, 0.0f32),
        ("stochastic, grains reading the same material", 1.0, 0.0),
        (
            "stochastic, grains decorrelated by a position spray",
            1.0,
            1.0,
        ),
    ] {
        println!("   {name}:");
        println!(
            "   {:>8}  {:>12}  {:>16}  {:>18}",
            "density", "output dBFS", "coherent-only dB", "incoherent-only dB"
        );
        let mut first = None;
        for &density in &[0.5f32, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0] {
            // overlap, converted to a rate below
            let mut c = steady();
            c.density_hz = density / c.length_s;
            c.scatter = scatter;
            c.window_shape = 0.75;
            c.randomisation.position = spray;
            let out = render_tone(&c, 3.0);
            let level = db(rms(&out));
            if first.is_none() {
                first = Some(level);
            }
            // What the other law would have done to the same sum, as a level offset.
            let (mean, mean_sq) = mxm_grain_fx_dsp::window::moments(0.75);
            let coherent = 1.0 / (density * mean).max(1.0);
            let incoherent = 1.0 / (density * mean_sq).max(1.0).sqrt();
            let applied = coherent + scatter * spray * (incoherent - coherent);
            println!(
                "   {density:>8.1}  {level:>12.2}  {:>16.2}  {:>18.2}",
                level + db(coherent / applied),
                level + db(incoherent / applied)
            );
        }
        println!();
    }
    println!(
        "   The shipped column should stay flat across the sweep. Where the two hypothetical\n   columns diverge from it is the error a single law would have made.\n"
    );
}

fn measurement_three() {
    println!("3. The length sweep across the three regions of §5");
    println!(
        "   Measured at density 0.8 — with a gap between grains, so each grain is an event rather
   than one tile of a perfect reconstruction. At density 2 and above this sweep would be flat,
   because a tiling granulator at unity rate is an exact delay whatever the grain length is.
"
    );
    println!(
        "   {:>10}  {:>10}  {:>16}  {:>10}  region",
        "length ms", "onset Hz", "sideband/carrier", "flatness"
    );
    let lengths = [
        MIN_LENGTH_S,
        0.004,
        0.008,
        0.013,
        0.025,
        0.050,
        0.100,
        0.250,
        MAX_LENGTH_S,
    ];
    for length in lengths {
        let mut c = steady();
        c.length_s = length;
        c.density_hz = 0.8 / length;
        c.scatter = 0.0;
        c.window_shape = 0.5;
        let out = render_tone(&c, 2.0);
        let (_, _, ratio) = sideband_profile(&out, c.density_hz);

        // Spectral flatness over a log-spaced set of bins: geometric mean over arithmetic mean.
        let bins: Vec<f32> = (0..48)
            .map(|i| 60.0 * 1.12f32.powi(i))
            .filter(|f| *f < RATE * 0.45)
            .map(|f| magnitude_at(&out, f).max(1.0e-9))
            .collect();
        let log_mean =
            (bins.iter().map(|m| m.ln() as f64).sum::<f64>() / bins.len() as f64).exp() as f32;
        let mean = bins.iter().sum::<f32>() / bins.len() as f32;
        let flatness = log_mean / mean.max(1.0e-12);

        let region = if length * 1000.0 <= 13.0 {
            "sub-period"
        } else if length * 1000.0 <= 50.0 {
            "fusion"
        } else {
            "events"
        };
        println!(
            "   {:>10.1}  {:>10.0}  {ratio:>16.4}  {flatness:>10.4}  {region}",
            length * 1000.0,
            c.density_hz
        );
    }
    println!(
        "
   **The onset rate is the axis, and the sideband ratio deliberately is not.** It sits at
   about 0.82 for every length, because at a fixed density the modulator's *waveform* does not
   change with grain length — only its rate does. That is the measurement: the same modulation
   is placed at 400 Hz for a 2 ms grain and at 2 Hz for a 500 ms one, which crosses both of the
   psychoacoustic lines §5 names. Above the carrier the sidebands are inharmonic and the result
   is the broadband buzz the sub-period region is named for; a few hertz down at the other end it
   is heard as separate repeats rather than as timbre. A floor of thirty-odd milliseconds would
   start this table two thirds of the way down, which is why MIN_LENGTH_S is 2 ms.

   Spectral flatness is printed because it was expected to track this and it does not — it wanders
   between 0.02 and 0.16 with no trend, since a windowed sine is a windowed sine at any length.
   It is left in as the negative result rather than quietly dropped.
"
    );
}

fn main() {
    println!("mxm-grain-fx — G3 measurements at {RATE} Hz\n");
    println!("These settle what can be settled without ears. The listening gate is the owner's.\n");
    measurement_one();
    measurement_two();
    measurement_three();
}
