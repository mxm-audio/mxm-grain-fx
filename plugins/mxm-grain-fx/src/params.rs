//! Permanent parameter definitions for `mxm-grain-fx`.
//!
//! **Provisional until the listening gate.** `plans/plan-mxm-grain-fx.md` §9: the shell is built
//! before the gate so the owner can play with it, and the ids are fixed the day the gate passes.
//! Nothing is released, so changing one costs nothing yet — the same state the root `AGENTS.md`
//! records as the only moment the collection's one id change was free. Do not treat these as
//! permanent until the plugin's AGENTS.md stops saying they are not.

use mxm_grain_fx_dsp::{
    BUFFER_S, MAX_DENSITY_HZ, MAX_LENGTH_S, MAX_LEVEL_DROP_DB, MAX_PITCH_SEMITONES, MIN_DENSITY_HZ,
    MIN_LENGTH_S,
};
use mxm_preset::PresetIdentity;
use nice_plug::prelude::*;
use std::sync::{Arc, RwLock};

type ValueToString = Arc<dyn Fn(f32) -> String + Send + Sync>;
type StringToValue = Arc<dyn Fn(&str) -> Option<f32> + Send + Sync>;

fn percent_to_string() -> ValueToString {
    Arc::new(|value| format!("{:.0} %", value * 100.0))
}

fn string_to_percent() -> StringToValue {
    Arc::new(|text| {
        text.trim()
            .trim_end_matches('%')
            .trim()
            .parse::<f32>()
            .ok()
            .map(|value| value / 100.0)
    })
}

/// Milliseconds below a second, seconds above — chosen from the *rounded* display value so a host's
/// text round-trip is idempotent, the trap `mxm-shimmer` records for frequencies.
fn seconds_to_string() -> ValueToString {
    Arc::new(|seconds| {
        let ms = seconds * 1_000.0;
        if ms.round() >= 1_000.0 {
            format!("{:.2} s", seconds)
        } else {
            format!("{ms:.0} ms")
        }
    })
}

fn string_to_seconds() -> StringToValue {
    Arc::new(|text| {
        let lower = text.trim().to_ascii_lowercase();
        if let Some(number) = lower.strip_suffix("ms") {
            number.trim().parse::<f32>().ok().map(|v| v / 1_000.0)
        } else {
            lower.trim_end_matches('s').trim().parse::<f32>().ok()
        }
    })
}

/// Grains per second, named as such.
///
/// **The unit lives in the formatter, not in `with_unit`.** `with_unit` appends to whatever the
/// formatter returned and hands the parser the result unhelped, which is how this very control
/// failed `param-conversions` once on `"0.05 grains"`. Keeping the unit inside means one place
/// writes it and one place strips it, which is the shape `mxm-bucket-delay`'s bias trimmer uses.
///
/// **One decimal below ten, whole numbers from ten — chosen from the one-decimal reading**, the
/// `seconds_to_string` rule. At a raw `value < 10.0`, 9.97 printed `10.0 grains/s`, parsed to ten
/// and came back `10 grains/s`.
fn grains_to_string() -> ValueToString {
    Arc::new(|value| {
        if (value * 10.0).round() >= 100.0 {
            format!("{value:.0} grains/s")
        } else {
            format!("{value:.1} grains/s")
        }
    })
}

fn string_to_grains() -> StringToValue {
    Arc::new(|text| {
        text.trim()
            .trim_end_matches("grains/s")
            .trim_end_matches("/s")
            .trim_end_matches("grains")
            .trim()
            .parse::<f32>()
            .ok()
    })
}

/// A variation depth, as the span in seconds it actually reaches.
///
/// **No unit is invented here.** The depth is a fraction of a range the DSP publishes, so
/// `depth x range` is a real figure rather than a percentage of something unnamed - the principle
/// `mxm-folded-spring`'s brief states as *a spring's length is a length*. The sign marks a **span**,
/// which is what a variation is; design system 8.5's signed convention is for a bipolar *value*,
/// which this is not.
fn variation_seconds(range_s: f32) -> (ValueToString, StringToValue) {
    let v2s: ValueToString = Arc::new(move |value| {
        let ms = value * range_s * 1_000.0;
        // The unit is chosen from the *rounded* figure, so the round trip cannot oscillate across
        // the boundary - the trap `mxm-shimmer` records for frequencies.
        if ms.round() >= 1_000.0 {
            format!("\u{00b1} {:.2} s", ms / 1_000.0)
        } else {
            format!("\u{00b1} {ms:.0} ms")
        }
    });
    let s2v: StringToValue = Arc::new(move |text| {
        let lower = text
            .trim()
            .trim_start_matches('\u{00b1}')
            .trim()
            .to_ascii_lowercase();
        let seconds = if let Some(number) = lower.strip_suffix("ms") {
            number.trim().parse::<f32>().ok().map(|v| v / 1_000.0)
        } else {
            lower.trim_end_matches('s').trim().parse::<f32>().ok()
        };
        seconds.map(|s| s / range_s)
    });
    (v2s, s2v)
}

/// A variation depth, as the span in semitones it actually reaches.
///
/// **Cents below a semitone, and that is the half of this control the reading used to hide.** A
/// whole-semitone format prints `± 0 st` across everything from dead in tune to a quarter tone, so
/// the entire detuning half of the travel had one reading and a player could not tell a section of
/// strings apart from no variation at all. The unit is chosen from the *rounded* figure — the trap
/// `variation_seconds` records above — so the round trip cannot oscillate across the boundary.
fn variation_semitones(range_st: f32) -> (ValueToString, StringToValue) {
    let v2s: ValueToString = Arc::new(move |value| {
        let cents = (value * range_st * 100.0).round();
        if cents < 100.0 {
            format!("\u{00b1} {cents:.0} ct")
        } else {
            format!("\u{00b1} {:.1} st", cents / 100.0)
        }
    });
    let s2v: StringToValue = Arc::new(move |text| {
        let lower = text
            .trim()
            .trim_start_matches('\u{00b1}')
            .trim()
            .to_ascii_lowercase();
        // A bare number is semitones, the parameter's own unit. Cents are read back only when the
        // writer said `ct`, which is what stops `19` from naming two different spans.
        let semitones = if let Some(number) = lower.strip_suffix("ct") {
            number.trim().parse::<f32>().ok().map(|c| c / 100.0)
        } else {
            lower.trim_end_matches("st").trim().parse::<f32>().ok()
        };
        semitones.map(|st| st / range_st)
    });
    (v2s, s2v)
}

/// The level draw, as the attenuation range it spans.
///
/// A grain is drawn somewhere between untouched and this far down, so the reading is a range from
/// zero rather than a single figure. **ASCII `-`, deliberately**: a typographic minus in the
/// formatter against a plain hyphen in the parser is the same self-unreadable output that failed
/// the validator here before, wearing a different glyph.
fn level_variation() -> (ValueToString, StringToValue) {
    let v2s: ValueToString = Arc::new(|value| {
        let drop = value * MAX_LEVEL_DROP_DB;
        if (drop * 10.0).round() == 0.0 {
            "0 dB".to_string()
        } else {
            format!("0 to -{drop:.1} dB")
        }
    });
    let s2v: StringToValue = Arc::new(|text| {
        let trimmed = text.trim().trim_end_matches("dB").trim();
        if trimmed == "0" {
            return Some(0.0);
        }
        trimmed
            .rsplit('-')
            .next()?
            .trim()
            .parse::<f32>()
            .ok()
            .map(|db| db / MAX_LEVEL_DROP_DB)
    });
    (v2s, s2v)
}

/// The scheduler control, named for the onset family it is running.
///
/// **A percentage says nothing about which of the three families is live**, and the three are the
/// whole design: a strict clock, a widening jitter inside each slot, and a per-sample draw that
/// leaves slots empty and doubles up in others. The travel is continuous and two-segment - jitter
/// up to the hand-over, then the hand-over itself - so this names the segment and gives the
/// position inside it rather than pretending there are three discrete settings.
fn onset_timing() -> (ValueToString, StringToValue) {
    // **The segment is chosen from the rounded half-percent, not from the raw value.** At a raw
    // `value < 0.5`, 0.498 printed `Jittered 100 %`, parsed to exactly one half and came back
    // `Handed over 0 %`. One rounding decides the segment and the figure inside it, so the words
    // partition the travel.
    let v2s: ValueToString = Arc::new(|value| {
        let half_percents = (value * 200.0).round();
        if half_percents < 100.0 {
            if half_percents == 0.0 {
                "Regular".to_string()
            } else {
                format!("Jittered {half_percents:.0} %")
            }
        } else {
            let handed = half_percents - 100.0;
            if handed >= 100.0 {
                "Random".to_string()
            } else {
                format!("Handed over {handed:.0} %")
            }
        }
    });
    let s2v: StringToValue = Arc::new(|text| {
        let lower = text.trim().to_ascii_lowercase();
        if lower == "regular" {
            return Some(0.0);
        }
        if lower == "random" {
            return Some(1.0);
        }
        let number = |rest: &str| rest.trim().trim_end_matches('%').trim().parse::<f32>().ok();
        if let Some(rest) = lower.strip_prefix("jittered") {
            return number(rest).map(|n| n / 200.0);
        }
        if let Some(rest) = lower.strip_prefix("handed over") {
            return number(rest).map(|n| 0.5 + n / 200.0);
        }
        None
    });
    (v2s, s2v)
}

/// The grain envelope, named at its three landmarks.
///
/// The card draws the envelope itself; this is what a host's generic UI and the tooltip get, and
/// "75 %" told nobody that three-quarters along is most of the way to a Hann.
fn grain_shape() -> (ValueToString, StringToValue) {
    let v2s: ValueToString = Arc::new(|value| {
        let percent = (value * 100.0).round();
        if percent == 0.0 {
            "Hard".to_string()
        } else if percent == 50.0 {
            "Triangle".to_string()
        } else if percent == 100.0 {
            "Smooth".to_string()
        } else {
            format!("{percent:.0} %")
        }
    });
    let s2v: StringToValue = Arc::new(|text| {
        let lower = text.trim().to_ascii_lowercase();
        match lower.as_str() {
            "hard" => Some(0.0),
            "triangle" => Some(0.5),
            "smooth" => Some(1.0),
            other => other
                .trim_end_matches('%')
                .trim()
                .parse::<f32>()
                .ok()
                .map(|v| v / 100.0),
        }
    });
    (v2s, s2v)
}

/// **Grain rate's tempo sync** (`plans/plan-tempo-sync-controls.md`): 1/64 to a whole note, the
/// slice of the ladder a grain rate holds at 120 bpm, the top the fastest.
pub const RATE_SYNC: mxm_tempo::Ladder = mxm_tempo::Ladder::new(
    mxm_tempo::Span::new(mxm_tempo::Division::SixtyFourth, mxm_tempo::Division::Whole),
    mxm_tempo::Direction::Rate,
);

/// **Read delay's tempo sync**: 1/64 to two bars, the slice the four-second buffer holds at 120 bpm,
/// the top the longest. Read delay is stored as a fraction of the buffer, so it resolves in seconds
/// over `0..BUFFER_S` and returns to that fraction.
pub const READ_DELAY_SYNC: mxm_tempo::Ladder = mxm_tempo::Ladder::new(
    mxm_tempo::Span::new(
        mxm_tempo::Division::SixtyFourth,
        mxm_tempo::Division::TwoBars,
    ),
    mxm_tempo::Direction::Time,
);

#[derive(Params)]
pub struct MxmGrainFxParams {
    #[id = "mix"]
    pub mix: FloatParam,
    #[id = "size"]
    pub size: FloatParam,
    #[id = "rate"]
    pub rate: FloatParam,
    /// Grain rate's tempo sync: its position picks a division of the host's tempo.
    #[id = "ratesync"]
    pub rate_sync: BoolParam,
    #[id = "onsettiming"]
    pub onset_timing: FloatParam,
    #[id = "readdelay"]
    pub read_delay: FloatParam,
    /// Read delay's tempo sync.
    #[id = "readdelaysync"]
    pub read_delay_sync: BoolParam,
    #[id = "transpose"]
    pub transpose: FloatParam,
    #[id = "reversechance"]
    pub reverse_chance: FloatParam,
    #[id = "shape"]
    pub shape: FloatParam,
    #[id = "width"]
    pub width: FloatParam,
    #[id = "feedback"]
    pub feedback: FloatParam,
    #[id = "freeze"]
    pub freeze: BoolParam,
    #[id = "delayvar"]
    pub delay_variation: FloatParam,
    #[id = "pitchvar"]
    pub pitch_variation: FloatParam,
    #[id = "sizevar"]
    pub size_variation: FloatParam,
    #[id = "levelvar"]
    pub level_variation: FloatParam,

    #[persist = "preset"]
    pub preset: RwLock<PresetIdentity>,
}

impl Default for MxmGrainFxParams {
    fn default() -> Self {
        let percent = || (percent_to_string(), string_to_percent());

        let float = |name: &str, default: f32| {
            let (v2s, s2v) = percent();
            FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(v2s)
                .with_string_to_value(s2v)
        };

        Self {
            // **The init patch is an audible granular setting**, because `plugins/AGENTS.md` rules
            // that an effect starts engaged: inserted with no further gesture it should demonstrate
            // the effect it is named after. Mid-length grains, a modest overlap and a little scatter
            // is a wash rather than a special effect, which is the honest first impression.
            mix: float("Mix", 0.55),
            size: FloatParam::new(
                "Grain size",
                0.060,
                FloatRange::Skewed {
                    min: MIN_LENGTH_S,
                    max: MAX_LENGTH_S,
                    factor: FloatRange::skew_factor(-1.4),
                },
            )
            .with_smoother(SmoothingStyle::Linear(40.0))
            .with_value_to_string(seconds_to_string())
            .with_string_to_value(string_to_seconds()),
            // **A rate, in grains per second.** It was the number sounding at once, and the owner's
            // ear found what that costs: with overlap fixed, lengthening a grain could only slow the
            // onset rate, so a longer grain gave *fewer* grains instead of more overlap and the
            // thickening a granular processor exists for was unreachable. As a rate, Length and
            // Density are independent and the overlap is `length x density`, which is what the
            // plan's size-first argument was actually for.
            rate: FloatParam::new(
                "Grain rate",
                67.0,
                FloatRange::Skewed {
                    min: MIN_DENSITY_HZ,
                    max: MAX_DENSITY_HZ,
                    factor: FloatRange::skew_factor(-1.4),
                },
            )
            .with_smoother(SmoothingStyle::Linear(40.0))
            .with_value_to_string(grains_to_string())
            .with_string_to_value(string_to_grains()),
            rate_sync: BoolParam::new("Grain rate sync", false),
            onset_timing: {
                let (v2s, s2v) = onset_timing();
                FloatParam::new(
                    "Onset timing",
                    0.25,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(v2s)
                .with_string_to_value(s2v)
            },
            read_delay_sync: BoolParam::new("Read delay sync", false),
            // Shown as the delay it is, which is what §3 of the plan made it.
            read_delay: FloatParam::new(
                "Read delay",
                0.15,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_smoother(SmoothingStyle::Linear(60.0))
            .with_value_to_string(Arc::new(|v| {
                let ms = v * BUFFER_S * 1_000.0;
                if ms.round() >= 1_000.0 {
                    format!("{:.2} s", ms / 1_000.0)
                } else {
                    format!("{ms:.0} ms")
                }
            }))
            .with_string_to_value(Arc::new(|text| {
                let lower = text.trim().to_ascii_lowercase();
                let seconds = if let Some(n) = lower.strip_suffix("ms") {
                    n.trim().parse::<f32>().ok().map(|v| v / 1_000.0)
                } else {
                    lower.trim_end_matches('s').trim().parse::<f32>().ok()
                };
                seconds.map(|s| s / BUFFER_S)
            })),
            transpose: FloatParam::new(
                "Transpose",
                0.0,
                FloatRange::Linear {
                    min: -MAX_PITCH_SEMITONES,
                    max: MAX_PITCH_SEMITONES,
                },
            )
            .with_step_size(1.0)
            .with_smoother(SmoothingStyle::Linear(40.0))
            .with_unit(" st")
            .with_value_to_string(formatters::v2s_f32_rounded(0))
            // Given a parser of its own rather than left to nice-plug's default. The default only
            // strips the unit on the `None` arm; the moment a control carries a `value_to_string`
            // its parser is handed the unit and no help, which is the round-trip this plugin has
            // already failed once.
            .with_string_to_value(Arc::new(|text| {
                text.trim()
                    .trim_end_matches("st")
                    .trim()
                    .parse::<f32>()
                    .ok()
            })),
            reverse_chance: float("Reverse chance", 0.0),
            shape: {
                let (v2s, s2v) = grain_shape();
                FloatParam::new(
                    "Grain shape",
                    0.75,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(v2s)
                .with_string_to_value(s2v)
            },
            // **The second deviation from *every amount starts at zero*, and it is recorded
            // rather than inherited.** This control is the merged survivor of Spread and the pan
            // depth; the Spread half defaulted to 0.6 and was never in the init-patch test, because
            // the half that *was* tested is the one that started at zero. Merged, it is a variation
            // depth and therefore an amount, so the deviation is now explicit: the plan's §7 rules
            // this topology Stereo *because per-grain pan is the technique's stereo and it is
            // meaningless in mono*, and a centred default would contradict the argument the stereo
            // layout rests on. Pinned by `every_amount_starts_at_zero`.
            width: float("Stereo width", 0.6),
            feedback: float("Feedback", 0.0),
            // Plain, with no custom formatter. A `value_to_string` without a matching
            // `string_to_value` breaks the validator's round-trip: a normalised 0.505 formats as the
            // true label, parses back through the default reader as 0.0, and formats as the false
            // one. `mxm-shimmer`'s own Freeze is plain for the same reason.
            freeze: BoolParam::new("Freeze", false),
            // **Spray does not start at zero, and that is a deviation recorded on purpose.**
            // `plugins/AGENTS.md` says every *amount* starts at zero; it also says an effect starts
            // engaged and must demonstrate the effect it is named after, and for this effect the two
            // rules collide. With no spray every grain reads the same age at the same rate, so
            // overlapping grains read *identical* material and the overlap-add reconstructs the
            // input: measured purity 0.997 against a tone, which is to say a plain delay. The
            // effects rule wins, because a granular processor whose default is a delay demonstrates
            // nothing. The reason and the measurement are in this plugin's AGENTS.md.
            delay_variation: {
                let (v2s, s2v) = variation_seconds(BUFFER_S);
                FloatParam::new(
                    "Delay variation",
                    0.30,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(v2s)
                .with_string_to_value(s2v)
            },
            // **The one variation depth that is not linear, because pitch is not heard linearly.**
            // The span reaches two octaves either way, and across a linear travel that puts a
            // semitone every eight pixels: a section of players is a few *cents* apart, so the
            // whole detuning half of this control lived in the first pixel and the owner reported
            // it as such — *"just 1-2 pixels and it's a semitone"*. Skewed, a quarter turn is a
            // detune, half is a chorus and the top half is still the cloud. Same reasoning and the
            // same shape as `mxm-creative-sampler`'s grain spread, which met this first.
            //
            // The other four depths stay linear, and **Delay variation is the one to watch**: its
            // span is the whole four-second buffer, so a *subtle* smear is as cramped there as a
            // detune was here. It is left alone because nobody has reported it, its default already
            // sits open at 30 %, and 20 ms against 100 ms of smear is a difference of degree where
            // ten cents against a semitone is a difference of kind. Size and Level are even across
            // their spans and need nothing.
            pitch_variation: {
                let (v2s, s2v) = variation_semitones(MAX_PITCH_SEMITONES);
                FloatParam::new(
                    "Pitch variation",
                    0.0,
                    FloatRange::Skewed {
                        min: 0.0,
                        max: 1.0,
                        factor: FloatRange::skew_factor(-1.8),
                    },
                )
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(v2s)
                .with_string_to_value(s2v)
            },
            size_variation: {
                // Half the length range, because the draw is bipolar about the set size.
                let (v2s, s2v) = variation_seconds((MAX_LENGTH_S - MIN_LENGTH_S) * 0.5);
                FloatParam::new(
                    "Size variation",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(v2s)
                .with_string_to_value(s2v)
            },
            level_variation: {
                let (v2s, s2v) = level_variation();
                FloatParam::new(
                    "Level variation",
                    0.0,
                    FloatRange::Linear { min: 0.0, max: 1.0 },
                )
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(v2s)
                .with_string_to_value(s2v)
            },

            preset: RwLock::new(PresetIdentity::default()),
        }
    }
}

impl MxmGrainFxParams {
    /// Grain rate while its sync follows the host, or `None` for its free value: the modulated
    /// position picks a division on [`RATE_SYNC`]. Resolved once a buffer by the plugin.
    pub fn synced_rate(&self, tempo: Option<f64>) -> Option<f32> {
        let param = &self.rate;
        RATE_SYNC
            .resolve(
                self.rate_sync.value(),
                tempo,
                param.modulated_normalized_value(),
                f64::from(param.preview_plain(0.0)),
                f64::from(param.preview_plain(1.0)),
            )
            .map(|hz| hz as f32)
    }

    /// Read delay while its sync follows the host, as a fraction of the buffer, or `None` for its
    /// free value: the modulated position picks a division on [`READ_DELAY_SYNC`], in seconds over
    /// the buffer's length. Resolved once a block by the plugin.
    pub fn synced_read_delay(&self, tempo: Option<f64>) -> Option<f32> {
        let buffer = f64::from(BUFFER_S);
        READ_DELAY_SYNC
            .resolve(
                self.read_delay_sync.value(),
                tempo,
                self.read_delay.modulated_normalized_value(),
                0.0,
                buffer,
            )
            .map(|seconds| (seconds / buffer) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Grain rate's sync picks a division and is inert without a tempo**
    /// (`plans/plan-tempo-sync-controls.md`): off, or with no tempo, the knob's own hertz stand; on
    /// at 120 bpm the ends are the ladder's ends that the range can hold, the top the fastest.
    #[test]
    fn grain_rate_sync_picks_a_division_and_is_inert_without_a_tempo() {
        use nice_plug::params::InternalParamMut;
        fn set<P: InternalParamMut>(param: &P, normalized: f32) {
            unsafe {
                let _ = param._internal_set_normalized_value(normalized);
            }
        }
        let p = MxmGrainFxParams::default();
        set(&p.rate, 1.0);
        assert_eq!(p.synced_rate(Some(120.0)), None, "off is the free rate");
        set(&p.rate_sync, 1.0);
        assert_eq!(p.synced_rate(None), None, "no tempo is the free rate");

        let top = p.synced_rate(Some(120.0)).expect("synced at a tempo");
        set(&p.rate, 0.0);
        let bottom = p.synced_rate(Some(120.0)).expect("synced at a tempo");
        let (lo, hi) = (
            f64::from(p.rate.preview_plain(0.0)),
            f64::from(p.rate.preview_plain(1.0)),
        );
        assert!(
            top > bottom,
            "the top of a rate is the fastest: {bottom} to {top}"
        );
        let reach = RATE_SYNC.reachable(120.0, lo, hi).divisions();
        let fastest = reach[0].hz(120.0) as f32;
        let slowest = reach[reach.len() - 1].hz(120.0) as f32;
        assert!((top - fastest).abs() < 1e-4, "{top} against {fastest}");
        assert!(
            (bottom - slowest).abs() < 1e-4,
            "{bottom} against {slowest}"
        );
    }

    /// **Read delay's sync picks a division and is inert without a tempo**: off or tempo-less it is
    /// the knob; on at 120 bpm the ends are 1/64 and two bars, as fractions of the four-second buffer.
    #[test]
    fn read_delay_sync_picks_a_division_and_is_inert_without_a_tempo() {
        use mxm_tempo::Division;
        use nice_plug::params::InternalParamMut;
        fn set<P: InternalParamMut>(param: &P, normalized: f32) {
            unsafe {
                let _ = param._internal_set_normalized_value(normalized);
            }
        }
        let p = MxmGrainFxParams::default();
        assert_eq!(
            p.synced_read_delay(Some(120.0)),
            None,
            "off is the free delay"
        );
        set(&p.read_delay_sync, 1.0);
        assert_eq!(
            p.synced_read_delay(None),
            None,
            "no tempo is the free delay"
        );
        set(&p.read_delay, 0.0);
        let bottom = p.synced_read_delay(Some(120.0)).expect("synced");
        set(&p.read_delay, 1.0);
        let top = p.synced_read_delay(Some(120.0)).expect("synced");
        let fraction = |d: Division| (d.seconds(120.0) / f64::from(BUFFER_S)) as f32;
        assert!((bottom - fraction(Division::SixtyFourth)).abs() < 1e-6);
        assert!((top - fraction(Division::TwoBars)).abs() < 1e-6);
    }

    use nice_plug::params::Param;

    #[test]
    fn every_amount_starts_at_zero() {
        // `plugins/AGENTS.md`'s init-patch contract, pinned as the rule rather than as the taste:
        // a depth or an amount is zero, so the first control a person turns does the thing it is
        // named after and nothing else does anything.
        let p = MxmGrainFxParams::default();
        for (name, value) in [
            ("Feedback", p.feedback.default_plain_value()),
            ("Reverse chance", p.reverse_chance.default_plain_value()),
            ("Pitch variation", p.pitch_variation.default_plain_value()),
            ("Size variation", p.size_variation.default_plain_value()),
            ("Level variation", p.level_variation.default_plain_value()),
        ] {
            assert_eq!(value, 0.0, "{name} is an amount and must start at zero");
        }
        assert_eq!(
            p.transpose.default_plain_value(),
            0.0,
            "no transposition by default"
        );
        // **Two deviations, both pinned so each reads as a decision rather than an oversight.**
        assert!(
            p.delay_variation.default_plain_value() > 0.0,
            "Delay variation must start open: with it shut every grain reads the same age at the \
             same rate, the overlap-add reconstructs the input, and the effect is a plain delay"
        );
        assert!(
            p.width.default_plain_value() > 0.0,
            "Stereo width must start open: per-grain pan is this technique's stereo, and the plan \
             rules the topology Stereo for that reason"
        );
        assert!(!p.freeze.default_plain_value(), "freeze starts running");
    }

    #[test]
    fn the_effect_starts_engaged() {
        // The effects' rule, which is the opposite of the instruments': inserted with no gesture it
        // must demonstrate the effect it is named after.
        let p = MxmGrainFxParams::default();
        assert!(
            p.mix.default_plain_value() > 0.2,
            "an inserted effect must be audible"
        );
        // Overlap is length x density, and it has to be above one or the grains do not overlap.
        let overlap = p.size.default_plain_value() * p.rate.default_plain_value();
        assert!(overlap > 1.5, "grains must overlap by default: {overlap}");
    }

    #[test]
    fn a_detune_is_reachable_on_the_pitch_spray() {
        // **The owner's report of 2026-09-11, pinned as arithmetic.** The depth reaches two octaves
        // either way; across a linear travel and this collection's 200 px full-scale drag that is
        // one semitone every eight pixels, so a section of strings — a few cents apart — was
        // unplayable and unreadable at once. Both halves are asserted here, because either one
        // alone still leaves the control impossible to set by ear.
        let p = MxmGrainFxParams::default();
        let span =
            |normalised: f32| p.pitch_variation.preview_plain(normalised) * MAX_PITCH_SEMITONES;

        // A quarter turn is a detune, not a transposition.
        let quarter = span(0.25);
        assert!(
            (0.05..0.5).contains(&quarter),
            "a quarter turn must be a detune, in cents rather than semitones: {quarter} st"
        );
        // Half is a chorus, and the wide cloud still has the top half of the travel to itself.
        let half = span(0.5);
        assert!(
            (1.0..4.0).contains(&half),
            "half a turn must be a chorus rather than a chord: {half} st"
        );
        assert!(
            span(1.0) >= MAX_PITCH_SEMITONES - 0.001,
            "the skew must not cost the top of the range"
        );

        // And the reading has to move with it. A whole-semitone format made every one of these
        // read the same, which is the half of the defect a curve alone would not have fixed.
        let readings =
            [0.0, 0.1, 0.25, 0.4].map(|n| p.pitch_variation.normalized_value_to_string(n, true));
        for (index, reading) in readings.iter().enumerate().skip(1) {
            assert_ne!(
                reading, &readings[0],
                "the detuning part of the travel must read as something other than no variation \
                 (step {index}: {reading})"
            );
        }
    }

    /// Plain values either side of a formatter's branch point: the point, and fractions of the
    /// **finer** branch's printed step around it, both halves of its rounding included.
    fn around(point: f32, step: f32) -> impl Iterator<Item = f32> {
        [
            -1.0, -0.6, -0.5, -0.49, -0.4, -0.1, 0.0, 0.1, 0.4, 0.49, 0.5, 0.6, 1.0,
        ]
        .into_iter()
        .map(move |k| point + k * step)
    }

    /// Every place a formatter here changes its unit, its precision or its words, in the
    /// parameter's plain units, with the finer branch's printed step. A point past a range's end is
    /// clamped by the parameter and costs nothing, so it stays listed rather than being reasoned away.
    fn branch_points(id: &str) -> Vec<(f32, f32)> {
        let seconds = |range_s: f32| (1.0 / range_s, 0.001 / range_s);
        match id {
            // `seconds_to_string`: milliseconds below a second, seconds to two decimals from it.
            "size" => vec![(1.0, 0.001)],
            // `grains_to_string`: one decimal below ten grains a second, none from ten.
            "rate" => vec![(10.0, 0.1)],
            // The read delay's own reading, and `variation_seconds` on the buffer's span.
            "readdelay" | "delayvar" => vec![seconds(BUFFER_S)],
            "sizevar" => vec![seconds((MAX_LENGTH_S - MIN_LENGTH_S) * 0.5)],
            // `variation_semitones`: cents below a semitone, semitones to one decimal from it.
            "pitchvar" => vec![(1.0 / MAX_PITCH_SEMITONES, 0.01 / MAX_PITCH_SEMITONES)],
            // `level_variation`: "0 dB" where a tenth of a decibel rounds to zero.
            "levelvar" => vec![(0.05 / MAX_LEVEL_DROP_DB, 0.1 / MAX_LEVEL_DROP_DB)],
            // `onset_timing`: Regular, Jittered, the hand-over at half and Random, in half-percents.
            "onsettiming" => vec![(0.0, 0.005), (0.5, 0.005), (1.0, 0.005)],
            // `grain_shape`: Hard, Triangle and Smooth at their whole percents.
            "shape" => vec![(0.0, 0.01), (0.5, 0.01), (1.0, 0.01)],
            _ => Vec::new(),
        }
    }

    /// One trip through the host, as `vendor/nice-plug`'s CLAP wrapper makes it: the CLAP value is
    /// the normalized value times the step count, the text carries the unit, and the parsed text
    /// comes back through the parameter's normalized conversion before it is formatted again.
    /// Returns the failure, if the text changed or did not parse.
    ///
    /// # Safety
    ///
    /// `ptr` must point at a parameter that outlives the call.
    unsafe fn host_round_trip(id: &str, ptr: ParamPtr, clap_value: f64) -> Option<String> {
        unsafe {
            let steps = ptr.step_count().unwrap_or(1);
            let normalised = clap_value as f32 / steps as f32;
            let first = ptr.normalized_value_to_string(normalised, true);
            let Some(parsed) = ptr.string_to_normalized_value(&first) else {
                return Some(format!("{id}: {first:?} does not parse"));
            };
            let back = parsed as f64 * steps as f64;
            let second = ptr.normalized_value_to_string(back as f32 / steps as f32, true);
            (second != first).then(|| {
                format!(
                    "{id} at plain {}: {first:?} came back {second:?}",
                    ptr.preview_plain(normalised)
                )
            })
        }
    }

    /// **Every parameter's text survives the host's normalized conversion unchanged.** A host
    /// formats a value, parses the text, normalizes the result and formats that again; the text has
    /// to come back identical, and `clap-validator`'s `param-conversions` checks exactly that — on
    /// a grid of its own, so a clean validator run proves nothing about a sliver between its points.
    ///
    /// **Formatted with the unit, which is the path that actually ships.** This test once passed
    /// `include_unit: false`, which neither the validator nor the editor does: `param-conversions`
    /// asks through CLAP, which always includes the unit, and `mxm-preset`'s `ErasedParam::text`
    /// does the same because §6 makes the unit part of the value.
    ///
    /// **Branch points, not only a grid.** On the twenty-step grid alone it passed Grain rate
    /// printing `10.0 grains/s` for 9.97, which parses to ten and comes back `10 grains/s`, and Onset
    /// timing printing `Jittered 100 %` for 0.498, which parses to one half and comes back
    /// `Handed over 0 %`. Both chose their branch from the raw value.
    ///
    /// Probes, for every parameter in `param_map` rather than a list someone has to extend: the
    /// twenty-step grid; `clap-validator` 0.4.1's own grid, whose size follows the parameter count;
    /// both sides of every branch point in [`branch_points`]; and a hair either side of zero on a
    /// range that crosses it.
    #[test]
    fn text_round_trips_are_idempotent() {
        let params = MxmGrainFxParams::default();
        let map = params.param_map();
        let validator_points = 4000usize.div_ceil(map.len()).clamp(5, 100);
        let mut failures = Vec::new();
        for (id, ptr, _) in map {
            // SAFETY: `params` owns every parameter these pointers refer to and outlives the loop;
            // this is the access `param-conversions` makes through CLAP.
            unsafe {
                let steps = ptr.step_count().unwrap_or(1) as f64;
                let mut probes: Vec<f64> = (0..=19).map(|i| steps * i as f64 / 19.0).collect();
                probes.extend(
                    (0..validator_points)
                        .map(|i| steps * (i as f64 / (validator_points - 1) as f64)),
                );
                let mut plains: Vec<f32> = branch_points(&id)
                    .into_iter()
                    .flat_map(|(point, step)| around(point, step))
                    .collect();
                let (low, high) = (ptr.preview_plain(0.0), ptr.preview_plain(1.0));
                if low.min(high) < 0.0 && low.max(high) > 0.0 {
                    plains.extend([-1.0e-3, -1.0e-4, 0.0, 1.0e-4, 1.0e-3]);
                }
                probes.extend(
                    plains
                        .into_iter()
                        .map(|plain| ptr.preview_normalized(plain) as f64 * steps),
                );
                failures.extend(
                    probes
                        .into_iter()
                        .filter_map(|value| host_round_trip(&id, ptr, value)),
                );
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
