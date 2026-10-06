//! `mxm-grain-fx` DSP: a granular processor on a live capture buffer.
//!
//! Framework-free and zero-dependency. The product is `plans/plan-mxm-grain-fx.md`; the technique
//! and its literature are `research:effects/granular-processing.md`, and one implementation read to
//! build-from depth is `research:effects/mutable-clouds.md`. **This is an original design against
//! those references, not a copy of either**, and every constant here is chosen or measured for this
//! implementation — none is read out of a product.
//!
//! Under the owner's ruling of 2026-09-09 (root `AGENTS.md`, *Research boundary* — the monorepo's;
//! since the split mxm-kit's `docs/collection-rules.md`, and this repository's root *Research
//! citations*), facts from a maker's permissively licensed source cross into this repository freely
//! and only *code* carries that licence's notice. **No source was opened while this crate was
//! written** — it is clean-room from the research prose — so the crate carries no third-party
//! notice and is MIT like its siblings (it was then; since the split it is GPL-3.0-or-later, the
//! repository's licence).
//!
//! # The shape, in one paragraph
//!
//! Input is written into a circular buffer. A scheduler decides when grains start; each grain draws
//! a tuple at birth — where in the buffer it reads, how long it lasts, at what rate and direction,
//! which window shape, at what level, where in the stereo field — and never changes it again. The
//! grains are summed, normalised by a law that depends on which scheduler regime is running, and
//! crossfaded against the dry signal. A share of the result is fed back into the buffer, so each
//! pass is granulated again.
//!
//! # What is hard about it, and where that lives
//!
//! - **Two heads share one buffer**, and the read must never cross the write point in either
//!   direction or under either head state. [`grain::place`] is the guard and its sweep is the
//!   proof. This is the part published descriptions of the technique omit.
//! - **The scheduler is the design.** Three families reach the panel through one control, and the
//!   route into the stochastic one is a hand-over between processes rather than a widened jitter —
//!   [`scheduler`].
//! - **The window is a spectrum control**, because under a periodic scheduler the grain envelope is
//!   the waveform of an amplitude modulator at the grain rate — [`window`].
//! - **Two normalisation laws**, because coherent and incoherent overlap do not sum the same way;
//!   using one for both is wrong by up to the square root of the grain count.
//!
//! # The warts, kept deliberately
//!
//! Named here because the collection's rule is that a wart is reproduced on purpose and labelled:
//!
//! 1. **Periodic amplitude modulation.** With the scatter control down, onsets are periodic and the
//!    grain envelope modulates everything at the grain rate. It is the technique's own sound and the
//!    reason window shape is a performed control; the cure is scatter, and the cure removes the
//!    character.
//! 2. **The boxcar clicks.** Window shape at zero is a near-rectangular envelope with a one-hundredth
//!    attack, and it is meant to be the glitch setting.
//! 3. **Feedback collapse.** With gaps between grains, recycling subdivides the material every pass
//!    until it is dust. That is what a recycled granular loop does, and with recycle the only loop
//!    in v1 there is no other topology to move to.
//! 4. **Length stops responding at high upward shift**, because the placement cap binds
//!    ([`grain::place`]). A grain transposed two octaves up cannot be longer than about a quarter of
//!    the buffer, and the alternative to the cap is a splice in the middle of the grain.
//!
//! What is **not** a wart and is not reproduced: the published implementation's silent window shape
//! at exactly the midpoint of its morph, and its habit of dropping grain quality — and with it the
//! window — once the pool is a quarter used. Both are defects rather than anybody's hardware. See
//! [`window`] and *the pool* below.

#![forbid(unsafe_code)]

mod capture;
mod grain;
mod rng;
pub mod scheduler;
pub mod window;

use capture::{Capture, RELEASE_FADE_S, flush};
use grain::{Direction, Grain};
use rng::Rng;
use scheduler::{Scatter, Scheduler};

/// How many grains may sound at once.
///
/// Chosen. The stochastic branch's count fluctuates above its mean — that is what a Poisson process
/// does — so the pool has to stand well clear of [`MAX_DENSITY`] or the ceiling would be reached at
/// ordinary settings and the exhaustion policy would become audible where it should be a corner.
pub const MAX_GRAINS: usize = 64;

/// The fastest and slowest the scheduler will start grains, in grains per second.
///
/// **Density is a rate, not an overlap count**, and that is a correction the owner's ear made
/// (2026-09-09). It was the number of grains sounding at once, which made Length and Density
/// dependent in the one way they must not be: at a fixed overlap, lengthening a grain could only
/// slow the onset rate, so a longer grain gave *fewer* grains rather than more overlap and the
/// smooth thickening every granular processor is reached for could not be produced at all. As a
/// rate, the two are genuinely independent — overlap is `length x density` and falls out of them —
/// which is what the plan's size-first argument was for.
///
/// Chosen: half a grain per second is one every two seconds, a scatter delay; five hundred is far
/// into the buzzing region where the onset rate is itself a pitch.
pub const MIN_DENSITY_HZ: f32 = 0.5;
pub const MAX_DENSITY_HZ: f32 = 500.0;

/// The shortest grain, in seconds.
///
/// Chosen against the psychoacoustic figures in `research:effects/granular-processing.md` §2. At
/// two milliseconds a grain is shorter than one period of most pitched material, which is the
/// **sub-period** region where the result is broadband buzz rather than a transposed fragment. The
/// plan (§5) makes reaching this region a requirement rather than a nicety: it is one of the three
/// regions the length control has to travel, and a floor of thirty-odd milliseconds — where the
/// published implementation the research read sits — cannot produce it at all.
pub const MIN_LENGTH_S: f32 = 0.002;

/// The longest grain, in seconds.
///
/// Chosen. Half a second is well past the ~50 ms line between audio-rate fusion and separate
/// events, so the control's top end is unambiguously in the **events** region and the processor has
/// become a scatter delay, which is the third of §5's three regions.
pub const MAX_LENGTH_S: f32 = 0.500;

/// How much audio the capture buffer holds, in seconds.
///
/// Chosen. Long enough that the position control reaches material a listener remembers hearing,
/// and long enough that a grain transposed two octaves up still has room after the placement cap
/// takes its quarter.
pub const BUFFER_S: f32 = 4.0;

/// The widest transposition, in semitones either way.
pub const MAX_PITCH_SEMITONES: f32 = 24.0;

/// The deepest a level draw may attenuate one grain, in decibels.
///
/// Chosen. The law it replaces was `1 - depth * u`, a linear attenuation that reaches **silence**:
/// at full depth some grains simply did not sound, so the control put holes in a sparse texture
/// rather than varying it. In decibels every grain stays audible and the control means what its
/// readout says. Twelve is the starting point; it is a listening question and changing it costs one
/// line and no identifier.
///
/// **It does not move the normalisation.** Over the draw's own domain at full depth the old law has
/// mean 0.500 and RMS 0.577, the new one 0.542 and 0.582 — **+0.07 dB of ensemble power**, which is
/// why [`GrainEngine::target_normalisation`] can go on ignoring the level depth entirely.
pub const MAX_LEVEL_DROP_DB: f32 = 12.0;

/// How long the wet takes to fade in or out at an engagement edge, in seconds.
///
/// Chosen: long enough not to click, short enough that Off feels immediate.
const ENGAGE_FADE_S: f32 = 0.020;

/// How long feedback takes to mute when freeze engages, in seconds.
///
/// Chosen. A frozen buffer inside a feedback loop is an unconditional oscillator, so the loop has
/// to go — but cutting it would be a step in the middle of a tail, so it ramps.
const FREEZE_MUTE_S: f32 = 0.050;

/// The corner of the in-loop high-pass at no feedback and at full feedback, in hertz.
///
/// Chosen, and a **G2 measurement** rather than an inheritance. A granular feedback loop
/// accumulates sub-audio energy first, so the corner rises with the amount of feedback rather than
/// sitting still. `mxm-shimmer` planned two tone filters inside its loop and measured both out,
/// keeping only a DC blocker, because either cost more per lap than any sensible gain returned; a
/// rising sub-audio high-pass is much nearer that blocker than a tone control, and is still put to
/// the same test.
const LOOP_HP_MIN_HZ: f32 = 12.0;
const LOOP_HP_MAX_HZ: f32 = 140.0;

/// How many recent onsets the engine remembers for a display.
///
/// Chosen. A display may drop frames — design system §13 — and this is where that licence is spent:
/// at the density ceiling the ring holds about a second, and past that the oldest marks are lost
/// rather than the newest, because a display of a cloud is about what it is doing *now*.
pub const ONSET_LOG: usize = 512;

/// One grain onset, recorded because a display must not invent them.
///
/// **This exists so the editor's buffer timeline can draw onsets that actually fired.** The
/// alternative is a mark pattern computed from the controls, which draws an even comb wherever the
/// scheduler has handed over to its per-sample draw — exactly where the sound is a Poisson cloud.
/// `mxm-chorus-06`'s Sweep records the same rule for its own display: a picture with its own clock
/// drifts from the sound, which is the lie a display exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Onset {
    /// The engine's sample clock when this grain started. Compare against [`GrainEngine::clock`].
    pub at: u64,
    /// Where in the buffer it began reading, as a fraction of [`BUFFER_S`]. This is the axis the
    /// position draw spreads, and it is what makes *when a grain starts* and *where it reads*
    /// separable by eye.
    pub age_fraction: f32,
}

/// Below this the engine is treated as silent.
const QUIET: f32 = 1.0e-7;

/// How long a signal must fail to decay before it is called sustaining, in seconds.
///
/// Chosen. `plans/plan-mxm-grain-fx.md` §7: *sustaining* is measured on the signal over a long
/// window and never read off the controls, because a large finite tail must not stand in for a
/// texture somebody is holding.
const SUSTAIN_WINDOW_S: f32 = 2.0;

/// How much of its own level a signal must keep across that window to count as sustaining.
const SUSTAIN_RATIO: f32 = 0.9;

/// How long the wet's level follower takes to settle, in seconds.
///
/// Chosen, and **a time rather than a sample count, which is the correction**. This was a fixed
/// per-sample coefficient of 0.0005 — two thousand samples, which is 42 ms at 48 kHz and 2 s at the
/// 1 kHz the validator sweeps down to, where it would have been the whole of
/// [`SUSTAIN_WINDOW_S`] and the sustain comparison would have been reading its own lag. Every other
/// time in this crate is converted from the active rate and this one was not; 42 ms is what it was
/// at the rate it was written for, so the audible behaviour there is unchanged.
const ENVELOPE_S: f32 = 0.042;

/// Per-grain randomisation depths, each a draw inside its own control's domain.
///
/// `plans/plan-mxm-grain-fx.md` §5. Every product the research read randomises some members of the
/// grain tuple at each onset and none randomises all of them; which of these become *parameters* is
/// decision 9, taken at the listening gate. All five are carried here from G1 regardless, so the
/// gate auditions real alternatives rather than approving whichever subset happened to be built.
///
/// **Each depth draws inside the deterministic control's own domain** — position inside the window
/// the placement guard allows, rate inside the pitch range, pan inside the field, length inside the
/// length range, and level as an *attenuation from unity, never a boost*. That rule is not there to
/// make zero the worst case (it is not: a pan draw to a hard side folds both channels into one, and
/// returns more than centre does) but to make the worst case **enumerable**, so the gain budget can
/// be proved once against every inventory the gate might choose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Randomisation {
    /// How far a grain's start may wander inside the legal placement window, `0..=1`.
    pub position: f32,
    /// How far a grain's rate may wander inside the pitch range, `0..=1`.
    pub rate: f32,
    /// How far a grain's pan may wander from the centre, `0..=1` — **the whole of this engine's
    /// stereo width**, and the only control over it.
    ///
    /// There used to be a second knob for the same quantity. `Controls` carried a `spread` field
    /// whose law was `spread * bipolar() * 0.5`, summed with this one into a single pan value: a
    /// per-grain random draw filed outside the struct whose documented job is per-grain random
    /// draws. Two independent draws into one value is not two controls, and the panel could not
    /// explain the difference because there was none. Merged 2026-09-11; the depth survived rather
    /// than the misfiled field, because the depth is what the plan's decision 9 ranges over.
    pub pan: f32,
    /// How far a grain's length may wander inside the length range, `0..=1`.
    pub length: f32,
    /// How far a grain's level may fall below unity, `0..=1`.
    pub level: f32,
}

impl Default for Randomisation {
    fn default() -> Self {
        Self {
            position: 0.0,
            rate: 0.0,
            pan: 0.0,
            length: 0.0,
            level: 0.0,
        }
    }
}

impl Randomisation {
    /// Every depth at its maximum. The worst case the gain budget is proved against.
    pub const fn widest() -> Self {
        Self {
            position: 1.0,
            rate: 1.0,
            pan: 1.0,
            length: 1.0,
            level: 1.0,
        }
    }

    fn sanitised(self) -> Self {
        Self {
            position: clamp01(self.position),
            rate: clamp01(self.rate),
            pan: clamp01(self.pan),
            length: clamp01(self.length),
            level: clamp01(self.level),
        }
    }
}

/// Everything the engine is asked to do.
///
/// Nothing here is a parameter id or a range for the plugin shell — that is G4's, and
/// `plans/AGENTS.md` (in the private archive) keeps it out of the plan and out of this crate.
/// These are the quantities the DSP needs, in their own units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Controls {
    /// Dry/wet crossfade. **Exactly zero is Off**: the wet fades out, the engine empties once and
    /// parks, and re-engaging starts from empty.
    pub mix: f32,
    /// Grain length, in seconds. Clamped to [`MIN_LENGTH_S`]..=[`MAX_LENGTH_S`].
    pub length_s: f32,
    /// How many grains start per second. **A rate, not an overlap count** — see
    /// [`MAX_DENSITY_HZ`]. How many sound at once is `length_s * density_hz`, and the engine caps
    /// it at [`MAX_GRAINS`] rather than thrashing the pool.
    pub density_hz: f32,
    /// The scheduler control: periodic at zero, jittered slots through the first half, handed over
    /// to a per-sample draw across the second. See [`scheduler`].
    pub scatter: f32,
    /// Where in the buffer grains read, as a fraction of [`BUFFER_S`]: `0` at the record head and
    /// `1` a whole buffer behind it. It is an **absolute delay**, clamped into whatever the
    /// placement guard allows — not a fraction of the history that happens to have accumulated. See
    /// [`GrainEngine::spawn`]'s note on why: a position that tracked a growing history would
    /// transpose the input while the buffer filled.
    pub position: f32,
    /// Transposition in semitones, `-MAX_PITCH_SEMITONES..=MAX_PITCH_SEMITONES`.
    pub pitch_semitones: f32,
    /// The probability that a grain reads backwards, `0..=1`.
    pub reverse: f32,
    /// The window morph: `0` a near-boxcar, `0.5` a triangle, `1` a Hann. See [`window`].
    pub window_shape: f32,
    /// How much of the wet is written back into the buffer, `0..=1`.
    pub feedback: f32,
    /// Hold the record head. Grains keep reading; the dry path still passes input.
    pub freeze: bool,
    /// The five per-grain randomisation depths.
    pub randomisation: Randomisation,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            mix: 0.5,
            length_s: 0.080,
            density_hz: 50.0,
            scatter: 0.25,
            position: 0.25,
            pitch_semitones: 0.0,
            reverse: 0.0,
            window_shape: 0.75,
            feedback: 0.0,
            freeze: false,
            randomisation: Randomisation::default(),
        }
    }
}

/// What the engine is doing, for the shell to turn into a host process status.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Activity {
    /// Nothing in the buffer and nothing sounding. The shell reports `Normal` and the engine costs
    /// a dry copy.
    Parked,
    /// A bounded tail. The shell reports `Tail(n)`, recomputed every block and never latched.
    Decaying {
        /// Conservative — it may overestimate and must not intentionally underestimate.
        tail_seconds: f32,
    },
    /// A texture that is not decaying, measured on the signal rather than read off the controls.
    /// The shell reports `Normal`: a held texture must not be advertised as a large finite tail,
    /// because a host may truncate a tail and must not truncate the thing somebody is holding.
    Sustaining,
}

/// The pool's exhaustion policy.
///
/// Two are open and both live entirely in the allocator, which is what lets the choice wait for the
/// listening gate without touching the renderer. The third the research found in a shipped product
/// — degrading the quality, and with it the window, of grains allocated once the pool is a quarter
/// used — is **rejected**: it is a renderer variant, and it makes the texture's timbre depend on
/// the density knob in a way its own manual never mentions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PoolPolicy {
    /// Take the oldest sounding grain. The texture stays dense at the ceiling; what a player hears
    /// is grains cut short, which the window does not hide.
    #[default]
    StealOldest,
    /// Refuse the onset. The texture thins at the ceiling instead, and nothing is cut.
    Drop,
}

#[derive(Debug, Clone, Copy)]
struct Smoothed {
    value: f32,
    coefficient: f32,
}

impl Smoothed {
    const fn new(value: f32) -> Self {
        Self {
            value,
            coefficient: 0.0,
        }
    }

    fn set_time(&mut self, seconds: f32, sample_rate: f32) {
        let samples = (seconds * sample_rate).max(1.0);
        self.coefficient = 1.0 - (-1.0 / samples).exp();
    }

    #[inline]
    fn step(&mut self, target: f32) -> f32 {
        self.value += self.coefficient * (target - self.value);
        self.value
    }
}

/// A one-pole high-pass whose corner moves with the feedback amount.
#[derive(Debug, Clone, Copy)]
struct HighPass {
    last_in: f32,
    last_out: f32,
    alpha: f32,
}

impl HighPass {
    const fn new() -> Self {
        Self {
            last_in: 0.0,
            last_out: 0.0,
            alpha: 0.99,
        }
    }

    fn set_corner(&mut self, hz: f32, sample_rate: f32) {
        // Ordered below Nyquist even at the validator's lowest stress rate.
        let hz = hz.clamp(1.0, sample_rate * 0.45);
        let rc = 1.0 / (core::f32::consts::TAU * hz);
        let dt = 1.0 / sample_rate;
        self.alpha = rc / (rc + dt);
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.alpha * (self.last_out + x - self.last_in);
        self.last_in = x;
        self.last_out = flush(y);
        self.last_out
    }

    fn clear(&mut self) {
        self.last_in = 0.0;
        self.last_out = 0.0;
    }
}

/// Monotonic, odd, and bounded: exactly linear below unity and asymptotic to ±2 above it.
///
/// This is what bounds the recycled path. `plans/plan-mxm-grain-fx.md` §6 takes shimmer's argument
/// by citation — every path returning to the buffer is in one gain budget and a saturating element
/// sits in the loop — with the granular addition that the budget is evaluated at the density
/// extreme *and* at the worst simultaneous randomisation draw, because the fold-pan law means zero
/// depth does not dominate.
#[inline]
fn saturate(x: f32) -> f32 {
    let a = x.abs();
    if a <= 1.0 {
        x
    } else {
        x.signum() * (2.0 - 1.0 / a)
    }
}

/// `10^(-db/20)`, as a gain at or below unity. `db` is expected non-negative.
///
/// `exp2` rather than `powf`, which is what the pitch draw beside it already uses.
#[inline]
fn decibels_below_unity(db: f32) -> f32 {
    (-db * (1.0 / 6.020_6)).exp2()
}

#[inline]
fn clamp01(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The granular engine.
pub struct GrainEngine {
    sample_rate: f32,
    capture: Capture,
    grains: [Grain; MAX_GRAINS],
    scheduler: Scheduler,
    rng: Rng,
    policy: PoolPolicy,

    controls: Controls,
    /// Window moments, recomputed only when the shape control moves.
    shape_cached: f32,
    window_mean: f32,
    window_mean_square: f32,

    mix: Smoothed,
    feedback: Smoothed,
    normalisation: Smoothed,
    engage: Smoothed,
    freeze_mute: Smoothed,

    hp_l: HighPass,
    hp_r: HighPass,
    /// Last sample's wet, which is what feeds back.
    recycle_l: f32,
    recycle_r: f32,

    parked: bool,
    engaged_target: f32,
    started_counter: u64,

    /// A monotonic count of output samples, for placing onsets on a time axis.
    clock: u64,
    /// The most recent onsets, oldest overwritten. Fixed size; never allocates.
    onsets: [Onset; ONSET_LOG],
    onset_write: usize,
    onset_len: usize,

    /// Slow envelope of the wet, for the sustain measurement.
    envelope: Smoothed,
    envelope_at_window_start: f32,
    sustain_countdown: u32,
    sustain_window: u32,
    sustaining: bool,
    input_quiet_for: u32,
}

impl GrainEngine {
    pub fn new(sample_rate: f32) -> Self {
        // Force the window table off the audio thread. A callback must never be the first caller of
        // a `LazyLock`, and an engine has to exist before one can render.
        window::prepare();
        let sample_rate = sanitise_rate(sample_rate);
        let mut engine = Self {
            sample_rate,
            capture: Capture::new(
                (BUFFER_S * sample_rate) as usize,
                (RELEASE_FADE_S * sample_rate) as usize,
            ),
            grains: [Grain::silent(); MAX_GRAINS],
            scheduler: Scheduler::new(),
            rng: Rng::new(),
            policy: PoolPolicy::default(),
            controls: Controls::default(),
            shape_cached: f32::NAN,
            window_mean: 0.5,
            window_mean_square: 0.375,
            mix: Smoothed::new(0.0),
            feedback: Smoothed::new(0.0),
            normalisation: Smoothed::new(1.0),
            engage: Smoothed::new(0.0),
            freeze_mute: Smoothed::new(0.0),
            hp_l: HighPass::new(),
            hp_r: HighPass::new(),
            recycle_l: 0.0,
            recycle_r: 0.0,
            parked: true,
            engaged_target: 0.0,
            started_counter: 0,
            clock: 0,
            onsets: [Onset {
                at: 0,
                age_fraction: 0.0,
            }; ONSET_LOG],
            onset_write: 0,
            onset_len: 0,
            envelope: Smoothed::new(0.0),
            envelope_at_window_start: 0.0,
            sustain_countdown: 0,
            sustain_window: 0,
            sustaining: false,
            input_quiet_for: 0,
        };
        engine.configure_rate();
        engine
    }

    /// Reallocate for a new sample rate. Allocates, so never call it from `process`.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        let sample_rate = sanitise_rate(sample_rate);
        if (sample_rate - self.sample_rate).abs() < f32::EPSILON {
            return;
        }
        self.sample_rate = sample_rate;
        self.capture.resize(
            (BUFFER_S * sample_rate) as usize,
            (RELEASE_FADE_S * sample_rate) as usize,
        );
        self.configure_rate();
        self.reset();
    }

    fn configure_rate(&mut self) {
        let sr = self.sample_rate;
        // Every time here is a time, converted from the active rate. No fixed sample count may
        // replace one: the validator sweeps from a few kilohertz to a few hundred.
        self.mix.set_time(0.005, sr);
        self.feedback.set_time(0.010, sr);
        self.normalisation.set_time(0.010, sr);
        self.engage.set_time(ENGAGE_FADE_S, sr);
        self.freeze_mute.set_time(FREEZE_MUTE_S, sr);
        self.envelope.set_time(ENVELOPE_S, sr);
        self.sustain_window = (SUSTAIN_WINDOW_S * sr) as u32;
        self.hp_l.set_corner(LOOP_HP_MIN_HZ, sr);
        self.hp_r.set_corner(LOOP_HP_MIN_HZ, sr);
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Choose the pool's exhaustion policy. Decision 8, taken at the listening gate.
    pub fn set_pool_policy(&mut self, policy: PoolPolicy) {
        self.policy = policy;
    }

    pub fn pool_policy(&self) -> PoolPolicy {
        self.policy
    }

    /// Clear every trace of what has been played.
    ///
    /// The random generator returns to its seed, which is part of the contract rather than tidiness:
    /// the same input and controls after a reset must produce the same samples.
    pub fn reset(&mut self) {
        self.capture.clear();
        self.grains = [Grain::silent(); MAX_GRAINS];
        self.scheduler.reset();
        self.rng.reset();
        self.mix.value = 0.0;
        self.feedback.value = 0.0;
        self.normalisation.value = 1.0;
        self.engage.value = 0.0;
        self.freeze_mute.value = 0.0;
        self.hp_l.clear();
        self.hp_r.clear();
        self.recycle_l = 0.0;
        self.recycle_r = 0.0;
        self.parked = true;
        self.engaged_target = 0.0;
        self.started_counter = 0;
        self.clock = 0;
        self.onset_write = 0;
        self.onset_len = 0;
        self.envelope.value = 0.0;
        self.envelope_at_window_start = 0.0;
        self.sustain_countdown = 0;
        self.sustaining = false;
        self.input_quiet_for = 0;
    }

    pub fn set_controls(&mut self, controls: &Controls) {
        let c = Controls {
            mix: clamp01(controls.mix),
            length_s: finite_or(controls.length_s, MIN_LENGTH_S).clamp(MIN_LENGTH_S, MAX_LENGTH_S),
            density_hz: finite_or(controls.density_hz, 1.0).clamp(MIN_DENSITY_HZ, MAX_DENSITY_HZ),
            scatter: clamp01(controls.scatter),
            position: clamp01(controls.position),
            pitch_semitones: finite_or(controls.pitch_semitones, 0.0)
                .clamp(-MAX_PITCH_SEMITONES, MAX_PITCH_SEMITONES),
            reverse: clamp01(controls.reverse),
            window_shape: clamp01(controls.window_shape),
            feedback: clamp01(controls.feedback),
            freeze: controls.freeze,
            randomisation: controls.randomisation.sanitised(),
        };
        if c.window_shape != self.shape_cached {
            let (mean, mean_square) = window::moments(c.window_shape);
            self.window_mean = mean.max(1.0e-4);
            self.window_mean_square = mean_square.max(1.0e-6);
            self.shape_cached = c.window_shape;
        }
        self.controls = c;
    }

    pub fn controls(&self) -> &Controls {
        &self.controls
    }

    /// The inter-onset interval in samples, and how many grains that leaves sounding at once.
    ///
    /// The interval is simply `sample_rate / density`, because density is a rate. It is floored so
    /// the requested overlap cannot exceed the pool: past that point more density only steals
    /// grains from itself, and a control that does nothing is better than one that thrashes.
    fn slot_and_overlap(&self) -> (f32, f32) {
        let length = (self.controls.length_s * self.sample_rate).max(1.0);
        let interval = (self.sample_rate / self.controls.density_hz).max(1.0);
        let floor = length / MAX_GRAINS as f32;
        let slot = interval.max(floor);
        (slot, length / slot)
    }

    /// The normalisation this setting asks for, before smoothing.
    ///
    /// `plans/plan-mxm-grain-fx.md` §5. Overlapping grains sum **coherently** under a periodic
    /// scheduler — amplitude adds, so the compensation is the reciprocal of `density × mean(w)` —
    /// and **incoherently** under a stochastic one, where power adds and it goes as one over the
    /// square root of `density × mean(w²)`. Neither ever boosts: below one grain of overlap there is
    /// nothing to compensate for.
    ///
    /// The count used is the **target** from the density control rather than the pool's live
    /// occupancy: a measured count would depend on pool state and would therefore not be invariant
    /// under block partitioning or repeatable after a reset.
    ///
    /// **A stochastic scheduler is not on its own enough to reach the incoherent law**, and the plan
    /// had it crossfading on the hand-over alone. Measured (`examples/gate_measurements`, 2): with
    /// every randomisation depth at zero and the position control still, every grain reads *the same
    /// age at the same rate* — so they are reading identical material and sum coherently however
    /// their onsets are scattered. Applying the incoherent law there over-compensates by the square
    /// root of the count, which measured as +4.4 dBFS at density 32 against −9 dB where it should
    /// have sat: audibly, the cloud gets louder as it gets denser.
    ///
    /// What actually decorrelates grains is reading *different material* — a position or rate draw.
    /// So the incoherent share is the hand-over **times** how far those two depths are open, and
    /// with them shut the coherent law holds on both branches. Erring toward the coherent law is
    /// also the safe direction: it never under-compensates, so the failure is a texture that is too
    /// quiet rather than one that grows.
    fn target_normalisation(&self) -> f32 {
        // The count that matters is how many grains *sound at once*, which is what the overlap-add
        // actually sums — not the rate at which they start.
        let (_, overlap) = self.slot_and_overlap();
        let coherent = 1.0 / (overlap * self.window_mean).max(1.0);
        let incoherent = 1.0 / (overlap * self.window_mean_square).max(1.0).sqrt();
        let handover = Scatter::from_control(self.controls.scatter).handover;
        let r = self.controls.randomisation;
        let decorrelated = r.position.max(r.rate);
        coherent + handover * decorrelated * (incoherent - coherent)
    }

    /// A conservative tail, recomputed every block from the controls that can move the latest
    /// audible read or the recycled gain.
    ///
    /// `plans/plan-mxm-grain-fx.md` §7 states this as a **rule rather than a list**, and each
    /// randomisation depth is taken at its worst in-domain draw rather than its current value — so
    /// nothing the listening gate later puts on the panel can fall outside the declaration. It may
    /// overestimate; it must not intentionally underestimate.
    fn tail_seconds(&self) -> f32 {
        // The longest a grain already sounding can still run, at the worst length draw.
        let grain = MAX_LENGTH_S;
        // How far back a read can reach, which is one lap of the recycled path.
        let lap = BUFFER_S;
        let gain = self.feedback.value.max(self.controls.feedback);
        if gain <= 1.0e-3 {
            return grain + lap;
        }
        // Where the loop no longer decays the signal, the *signal* is what says so — the sustain
        // measurement below — and the tail declaration stops being the right answer. Declare a long
        // one rather than a wrong one until that measurement catches up.
        if gain >= 0.999 {
            return grain + lap * 200.0;
        }
        let laps = QUIET.ln() / gain.ln();
        grain + lap * laps.min(200.0)
    }

    /// Process one stereo block in place, and say what the engine is doing.
    ///
    /// The recursion is one sample deep and no state is derived from where a block begins, so
    /// output is bit-identical under any partition of the same stream — including a partition that
    /// changes between calls.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) -> Activity {
        let n = left.len().min(right.len());
        self.engaged_target = if self.controls.mix > 0.0 { 1.0 } else { 0.0 };

        // An engagement edge always starts from empty. Reaching zero fades the wet out and then
        // empties; re-engaging clears first and fades in, so a fade interrupted by the other edge
        // restarts rather than resuming and automation crossing zero cannot expose held state.
        if self.engaged_target > 0.0 && self.parked {
            self.hard_empty();
            self.parked = false;
        }

        if self.parked && self.engaged_target == 0.0 {
            // Doing nothing costs nothing: the dry is already in the buffers.
            return Activity::Parked;
        }

        let scatter = Scatter::from_control(self.controls.scatter);
        let (slot, _) = self.slot_and_overlap();
        let target_norm = self.target_normalisation();
        let hp_corner = LOOP_HP_MIN_HZ
            + (LOOP_HP_MAX_HZ - LOOP_HP_MIN_HZ) * self.controls.feedback * self.controls.feedback;
        self.hp_l.set_corner(hp_corner, self.sample_rate);
        self.hp_r.set_corner(hp_corner, self.sample_rate);
        self.capture.set_held(self.controls.freeze);
        let head_advancing = !self.capture.is_held();

        let mut input_peak = 0.0f32;

        for i in 0..n {
            let dry_l = flush(left[i]);
            let dry_r = flush(right[i]);
            input_peak = input_peak.max(dry_l.abs()).max(dry_r.abs());

            // Snap the last sliver of the fade to exact zero. That is what makes Off *dry to the
            // bit* rather than dry to within an epsilon, and it is also what makes the decision to
            // park audio-neutral: once this is zero the output is already exactly the dry signal,
            // so it cannot matter which block the engine notices it may stop in — and the render
            // stays invariant under block partitioning either way.
            let mut engage = self.engage.step(self.engaged_target);
            if self.engaged_target == 0.0 && engage < 1.0e-6 {
                engage = 0.0;
                self.engage.value = 0.0;
            }
            let feedback = self.feedback.step(self.controls.feedback);
            let freeze_mute = self
                .freeze_mute
                .step(if self.controls.freeze { 1.0 } else { 0.0 });

            // A frozen buffer inside a feedback loop is an unconditional oscillator, so the loop is
            // muted while held — on a ramp, because a step in the middle of a tail is a click.
            let recycle_gain = feedback * (1.0 - freeze_mute);
            let fb_l = self.hp_l.process(self.recycle_l) * recycle_gain;
            let fb_r = self.hp_r.process(self.recycle_r) * recycle_gain;
            self.capture
                .advance(saturate(dry_l + fb_l), saturate(dry_r + fb_r));

            let history = self.capture.history();
            for _ in 0..self.scheduler.tick(slot, scatter, &mut self.rng) {
                self.spawn(history);
            }

            let mut wet_l = 0.0f32;
            let mut wet_r = 0.0f32;
            for grain in &mut self.grains {
                let (l, r) = grain.render(&self.capture, head_advancing);
                wet_l += l;
                wet_r += r;
            }

            let norm = self.normalisation.step(target_norm);
            wet_l = flush(wet_l * norm);
            wet_r = flush(wet_r * norm);
            self.recycle_l = wet_l;
            self.recycle_r = wet_r;

            let wet_gain = self.mix.step(self.controls.mix) * engage;
            let out_l = dry_l + wet_gain * (wet_l - dry_l);
            let out_r = dry_r + wet_gain * (wet_r - dry_r);
            left[i] = flush(out_l);
            right[i] = flush(out_r);

            let level = wet_l.abs().max(wet_r.abs());
            // A slow envelope, so the sustain measurement reads the field rather than the waveform.
            // Its time constant is converted from the active rate like every other one here.
            self.envelope.step(level);
            self.clock = self.clock.wrapping_add(1);
        }

        self.observe(n, input_peak);

        if self.parked {
            Activity::Parked
        } else if self.sustaining {
            Activity::Sustaining
        } else {
            Activity::Decaying {
                tail_seconds: self.tail_seconds(),
            }
        }
    }

    /// Update the sustain measurement and decide whether the engine may park.
    fn observe(&mut self, samples: usize, input_peak: f32) {
        let n = samples as u32;
        if input_peak > QUIET {
            self.input_quiet_for = 0;
        } else {
            self.input_quiet_for = self.input_quiet_for.saturating_add(n);
        }

        // Sustaining is measured on the signal, never read off the controls: compare the field's
        // level now against a window ago, and only while nothing new is arriving.
        self.sustain_countdown = self.sustain_countdown.saturating_add(n);
        if self.sustain_countdown >= self.sustain_window {
            self.sustain_countdown = 0;
            let held = self.envelope.value > QUIET
                && self.envelope.value >= self.envelope_at_window_start * SUSTAIN_RATIO
                && self.input_quiet_for >= self.sustain_window;
            self.sustaining = held;
            self.envelope_at_window_start = self.envelope.value;
        }
        if self.envelope.value <= QUIET {
            self.sustaining = false;
        }

        // The far edge of Off: once the wet has faded to exact zero the output is already the dry
        // signal, so emptying and parking here changes nothing that can be heard or measured.
        if self.engaged_target == 0.0 && self.engage.value == 0.0 {
            self.hard_empty();
            self.parked = true;
        }
    }

    fn hard_empty(&mut self) {
        self.capture.clear();
        for grain in &mut self.grains {
            grain.silence();
        }
        self.scheduler.reset();
        self.hp_l.clear();
        self.hp_r.clear();
        self.recycle_l = 0.0;
        self.recycle_r = 0.0;
        self.envelope.value = 0.0;
        self.envelope_at_window_start = 0.0;
        self.sustaining = false;
        self.engage.value = 0.0;
    }

    /// Draw one grain's tuple and place it.
    fn spawn(&mut self, history: f32) {
        let c = self.controls;
        let r = c.randomisation;

        // Every draw is inside its own control's domain, which is what makes the worst case
        // enumerable rather than open-ended.
        let length_span = MAX_LENGTH_S - MIN_LENGTH_S;
        let length_s = (c.length_s + r.length * self.rng.bipolar() * length_span * 0.5)
            .clamp(MIN_LENGTH_S, MAX_LENGTH_S);
        let semitones = (c.pitch_semitones + r.rate * self.rng.bipolar() * MAX_PITCH_SEMITONES)
            .clamp(-MAX_PITCH_SEMITONES, MAX_PITCH_SEMITONES);
        let rate = (semitones / 12.0).exp2();
        let direction = if self.rng.unit() < c.reverse {
            Direction::Reverse
        } else {
            Direction::Forward
        };

        let asked = length_s * self.sample_rate;
        let Some(placement) = grain::place(asked, rate, direction, history) else {
            return;
        };

        // **Position is an absolute delay, not a fraction of the history.**
        //
        // Mapping the control across `[lowest_age, highest_age]` is the obvious thing and it is
        // wrong, because `highest_age` grows while the buffer fills. Each new grain would then start
        // a little further back than the last, so the read point would advance at `1 - position`
        // samples per sample and the whole input would be **transposed by that factor** for the
        // first `BUFFER_S` seconds after an insert or a reset — measured at 440 Hz arriving as
        // 352 Hz at position 0.2, an effect nobody asked for that then disappears on its own.
        //
        // An absolute target settles as soon as the buffer holds enough to honour it. Before that
        // the clamp holds the read at the oldest material there is, which repeats rather than
        // detunes — a much smaller and more explicable artefact, and one that ends sooner.
        let position = clamp01(c.position + r.position * self.rng.bipolar());
        let target_age = position * BUFFER_S * self.sample_rate;
        let age = target_age.clamp(placement.lowest_age, placement.highest_age);
        // Level is an attenuation from unity and never a boost, and it is drawn in **decibels** so
        // a grain comes out quieter rather than absent. The draw is unconditional even at zero
        // depth: short-circuiting would make the generator's sequence depend on a control value and
        // two renders at different depths would stop being comparable.
        let level = decibels_below_unity(MAX_LEVEL_DROP_DB * r.level * self.rng.unit());
        // One draw, because stereo width is one quantity. See `Randomisation::pan`.
        let pan = clamp01(0.5 + r.pan * self.rng.bipolar() * 0.5);

        let Some(slot) = self.take_slot() else {
            return;
        };
        self.started_counter = self.started_counter.wrapping_add(1);
        let counter = self.started_counter;
        // Recorded here rather than at the scheduler's tick, so what the display draws is a grain
        // that actually sounded: an onset the pool refused above is not a mark.
        self.onsets[self.onset_write] = Onset {
            at: self.clock,
            age_fraction: clamp01(age / (BUFFER_S * self.sample_rate)),
        };
        self.onset_write = (self.onset_write + 1) % ONSET_LOG;
        self.onset_len = (self.onset_len + 1).min(ONSET_LOG);
        self.grains[slot].start(
            age,
            placement.length,
            rate,
            direction,
            c.window_shape,
            level,
            pan,
            counter,
        );
    }

    /// Find a grain to use, applying the exhaustion policy when the pool is full.
    fn take_slot(&mut self) -> Option<usize> {
        let mut oldest = 0usize;
        let mut oldest_started = u64::MAX;
        for (index, grain) in self.grains.iter().enumerate() {
            if !grain.is_active() {
                return Some(index);
            }
            if grain.started_at() < oldest_started {
                oldest_started = grain.started_at();
                oldest = index;
            }
        }
        match self.policy {
            PoolPolicy::StealOldest => Some(oldest),
            PoolPolicy::Drop => None,
        }
    }

    /// The engine's sample clock: how many output samples it has rendered since the last reset.
    ///
    /// An [`Onset`]'s `at` is a value of this, so `clock() - onset.at` is how long ago a grain
    /// started, in samples, and dividing by the sample rate puts it on a time axis.
    pub fn clock(&self) -> u64 {
        self.clock
    }

    /// Copy only the onsets newer than `since`, oldest first, and say how many were written.
    ///
    /// This is what a host-side telemetry channel wants: a block produces a handful of onsets, and
    /// republishing the whole window every block would be four kilobytes of copy to carry two
    /// entries. The ring is chronological, so this walks back from the newest and stops, which
    /// makes the cost the number of *new* onsets rather than the size of the log.
    pub fn onsets_since(&self, since: u64, out: &mut [Onset]) -> usize {
        let mut found = 0usize;
        for step in 0..self.onset_len.min(out.len()) {
            let index = (self.onset_write + ONSET_LOG - 1 - step) % ONSET_LOG;
            let onset = self.onsets[index];
            if onset.at <= since {
                break;
            }
            found = step + 1;
        }
        // Walked newest-first; hand them back oldest-first, as `onsets` does.
        for (i, slot) in out[..found].iter_mut().enumerate() {
            let index = (self.onset_write + ONSET_LOG - found + i) % ONSET_LOG;
            *slot = self.onsets[index];
        }
        found
    }

    /// Copy the recorded onsets, oldest first, and say how many were written.
    ///
    /// Realtime-safe: a copy into a caller-owned slice, no allocation and no lock. Reading does not
    /// consume them — the ring is a window on the recent past, not a queue — so a display that
    /// misses a frame sees the same grains one frame later rather than a hole.
    pub fn onsets(&self, out: &mut [Onset]) -> usize {
        let take = self.onset_len.min(out.len());
        // `onset_write` is one past the newest, so the oldest of `take` sits `take` behind it.
        let start = (self.onset_write + ONSET_LOG - take) % ONSET_LOG;
        for (i, slot) in out[..take].iter_mut().enumerate() {
            *slot = self.onsets[(start + i) % ONSET_LOG];
        }
        take
    }

    /// How many grains are sounding. For tests and telemetry, not for the normalisation law.
    pub fn active_grains(&self) -> usize {
        self.grains.iter().filter(|g| g.is_active()).count()
    }

    /// Whether the engine has emptied and stopped.
    pub fn is_parked(&self) -> bool {
        self.parked
    }
}

fn sanitise_rate(sample_rate: f32) -> f32 {
    if sample_rate.is_finite() {
        sample_rate.clamp(1_000.0, 768_000.0)
    } else {
        48_000.0
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

#[cfg(test)]
mod level_law {
    use super::*;

    /// The level draw attenuates and never boosts, at every depth and every draw.
    #[test]
    fn a_level_draw_never_exceeds_unity_and_never_silences() {
        let floor = decibels_below_unity(MAX_LEVEL_DROP_DB);
        for depth_step in 0..=32 {
            let depth = depth_step as f32 / 32.0;
            for draw_step in 0..=32 {
                let draw = draw_step as f32 / 32.0;
                let level = decibels_below_unity(MAX_LEVEL_DROP_DB * depth * draw);
                assert!(level <= 1.0, "depth {depth} draw {draw} boosted to {level}");
                assert!(
                    level >= floor - 1.0e-6,
                    "depth {depth} draw {draw} fell past the declared floor: {level}"
                );
            }
        }
        // The law it replaced reached zero. This one does not: that is the whole point.
        assert!(floor > 0.24, "a grain at full depth went quiet: {floor}");
    }

    /// Zero depth is exactly unity, so a default patch is untouched by the law.
    #[test]
    fn zero_depth_is_exactly_unity() {
        for draw_step in 0..=32 {
            let draw = draw_step as f32 / 32.0;
            assert_eq!(decibels_below_unity(MAX_LEVEL_DROP_DB * 0.0 * draw), 1.0);
        }
    }

    /// The readout's own arithmetic: the constant is a decibel figure, so half depth is half of it.
    #[test]
    fn the_law_is_decibels_rather_than_a_gain_fraction() {
        let half = decibels_below_unity(MAX_LEVEL_DROP_DB * 0.5);
        let expected = 10.0f32.powf(-(MAX_LEVEL_DROP_DB * 0.5) / 20.0);
        assert!((half - expected).abs() < 1.0e-4, "{half} vs {expected}");
    }
}
