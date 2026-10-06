//! `mxm-grain-fx` — a granular processor on a live capture buffer.
//!
//! Grains are short windowed reads of a buffer the input is still being written into, scattered in
//! time by a scheduler that reaches the periodic, jittered and stochastic families through one
//! control, and optionally recycled so each pass is granulated again. The DSP is
//! `crates/mxm-grain-fx-dsp`; the product is `plans/plan-mxm-grain-fx.md`.
//!
//! **The parameter ids here are provisional until the plan's listening gate.** The shell exists so
//! the gate can happen at all — a granular processor cannot be judged without turning its knobs —
//! and nothing is released, so changing an id costs nothing yet. See this plugin's AGENTS.md.

macro_rules! plugin_name {
    () => {
        "mxm-grain-fx"
    };
}

pub const NAME: &str = plugin_name!();
pub const CLAP_ID: &str = concat!("dk.mxm.", plugin_name!());

pub mod editor;
pub mod params;
pub mod preset;
pub mod telemetry;

use mxm_grain_fx_dsp::{Activity, Controls, GrainEngine, Onset, Randomisation};
use nice_plug::prelude::*;
use params::MxmGrainFxParams;
use std::sync::Arc;
use telemetry::Telemetry;

pub struct MxmGrainFx {
    pub params: Arc<MxmGrainFxParams>,
    telemetry: Arc<Telemetry>,
    engine: GrainEngine,
    sample_rate: f32,
    input_channels: usize,
    /// Scratch for the mono-in layout, so the engine always sees two channels.
    scratch: Vec<f32>,
    /// Where the onset telemetry has published up to, so a block copies only what is new.
    published_onsets_at: u64,
    /// Fixed landing space for those onsets. Preallocated: `process` never allocates.
    onset_scratch: Vec<Onset>,
    /// Grain rate and read delay as their syncs resolved them for this block, or `None` for their
    /// free values (`plans/plan-tempo-sync-controls.md`).
    synced: [Option<f32>; 2],
}

impl Default for MxmGrainFx {
    fn default() -> Self {
        Self {
            params: Arc::new(MxmGrainFxParams::default()),
            telemetry: Telemetry::shared(),
            engine: GrainEngine::new(48_000.0),
            sample_rate: 48_000.0,
            input_channels: 1,
            scratch: Vec::new(),
            published_onsets_at: 0,
            onset_scratch: Vec::new(),
            synced: [None; 2],
        }
    }
}

impl MxmGrainFx {
    fn prepare(&mut self, sample_rate: f32, input_channels: usize, max_block: usize) {
        // Forget the last activation's tempo too: nice-plug resets right after activating, and a
        // division resolved from a tempo the host may since have changed would seed the engine.
        self.telemetry.tempo.publish(None);
        // A restored state is resolved afresh by the next block: activation must not seed the
        // engine with the division the previous state was synced to.
        self.synced = [None; 2];
        self.sample_rate = valid_sample_rate(sample_rate);
        self.input_channels = input_channels.clamp(1, 2);
        self.engine = GrainEngine::new(self.sample_rate);
        // Every allocation happens here, never in `process`.
        self.scratch = vec![0.0; max_block.max(1)];
        self.onset_scratch = vec![
            Onset {
                at: 0,
                age_fraction: 0.0
            };
            telemetry::ONSET_SLOTS
        ];
        self.published_onsets_at = 0;
        // State may have been restored before activation, so the engine begins at that state rather
        // than at the Rust default followed by an audible transition.
        self.engine.set_controls(&self.target_controls());
        self.engine.reset();
    }

    /// The controls the parameters are currently asking for.
    ///
    /// Read once per block from each parameter's value rather than from its smoother: the engine
    /// smooths what has to be smoothed — the crossfade, the feedback gain and the normalisation —
    /// per sample and internally, and everything else is drawn once per grain at its onset, where a
    /// smoothed value would mean nothing.
    fn target_controls(&self) -> Controls {
        let p = &self.params;
        // **The names either side of each colon are different vocabularies, deliberately.** On the
        // left are the DSP's, which describe the quantity in its own units; on the right the
        // panel's, which describe what a player is reaching for. The one pair worth pausing over is
        // `rate`: the plugin's `rate` is how many grains start per second, while the DSP's
        // `Randomisation::rate` is the *playback*-rate draw the panel calls Pitch variation.
        Controls {
            mix: p.mix.value(),
            length_s: p.size.value(),
            density_hz: self.synced[0].unwrap_or_else(|| p.rate.value()),
            scatter: p.onset_timing.value(),
            position: self.synced[1].unwrap_or_else(|| p.read_delay.value()),
            pitch_semitones: p.transpose.value(),
            reverse: p.reverse_chance.value(),
            window_shape: p.shape.value(),
            feedback: p.feedback.value(),
            freeze: p.freeze.value(),
            randomisation: Randomisation {
                position: p.delay_variation.value(),
                rate: p.pitch_variation.value(),
                // Stereo width: one control, one draw. It used to be two parameters summing two
                // draws into the same pan value.
                pan: p.width.value(),
                length: p.size_variation.value(),
                level: p.level_variation.value(),
            },
        }
    }

    pub fn prepare_for_test(&mut self, sample_rate: f32, input_channels: usize, max_block: usize) {
        self.prepare(sample_rate, input_channels, max_block);
    }

    pub fn process_block_for_test(&mut self, channels: &mut [&mut [f32]]) -> ProcessStatus {
        self.process_block(channels)
    }

    fn process_block(&mut self, channels: &mut [&mut [f32]]) -> ProcessStatus {
        let Some(first) = channels.first() else {
            return ProcessStatus::Normal;
        };
        let samples = first.len();
        if samples == 0 {
            return ProcessStatus::Normal;
        }
        let mono = self.input_channels == 1 || channels.len() < 2;
        let inputs = self.input_channels.min(channels.len());

        // Flush denormals and anything non-finite before it reaches recursive state, and notice
        // whether there is anything to do at all.
        let mut has_input = false;
        for channel in &mut channels[..inputs] {
            for sample in &mut channel[..samples] {
                if !sample.is_finite() || sample.abs() < f32::MIN_POSITIVE {
                    *sample = 0.0;
                } else {
                    has_input = true;
                }
            }
        }

        self.engine.set_controls(&self.target_controls());

        // Doing nothing costs nothing: with no input and an emptied engine the block is a copy.
        if !has_input && self.engine.is_parked() {
            if mono && channels.len() > 1 {
                let (left, rest) = channels.split_at_mut(1);
                rest[0][..samples].copy_from_slice(&left[0][..samples]);
            }
            return ProcessStatus::Normal;
        }

        let activity = if channels.len() >= 2 {
            let (left, rest) = channels.split_at_mut(1);
            if mono {
                rest[0][..samples].copy_from_slice(&left[0][..samples]);
            }
            self.engine
                .process(&mut left[0][..samples], &mut rest[0][..samples])
        } else {
            // A single-channel buffer: the engine still wants two, so the second is scratch.
            if self.scratch.len() < samples {
                // Cannot allocate here. Report the block untouched rather than break the contract.
                return ProcessStatus::Normal;
            }
            let (head, _) = self.scratch.split_at_mut(samples);
            head.copy_from_slice(&channels[0][..samples]);
            self.engine.process(&mut channels[0][..samples], head)
        };

        // Anything above a stereo pair is fed the left channel, as the siblings do.
        if channels.len() > 2 {
            let (left, rest) = channels.split_at_mut(1);
            for channel in rest.iter_mut().skip(1) {
                channel[..samples].copy_from_slice(&left[0][..samples]);
            }
        }

        let mut peak = 0.0f32;
        for channel in channels.iter().take(2) {
            for sample in &channel[..samples] {
                peak = peak.max(sample.abs());
            }
        }
        self.telemetry.publish(peak, self.engine.active_grains());
        // The onsets this block produced, and the clock to place them against. Copying only what is
        // new keeps this a handful of entries rather than the whole log.
        let fresh = self
            .engine
            .onsets_since(self.published_onsets_at, &mut self.onset_scratch);
        self.published_onsets_at = self.engine.clock();
        self.telemetry
            .publish_onsets(self.published_onsets_at, &self.onset_scratch[..fresh]);

        match activity {
            // A held texture and an empty engine are both `Normal`: a host may truncate a tail and
            // must not truncate the thing somebody is holding.
            Activity::Parked | Activity::Sustaining => ProcessStatus::Normal,
            Activity::Decaying { tail_seconds } => ProcessStatus::Tail(
                (tail_seconds * self.sample_rate).clamp(0.0, u32::MAX as f32) as u32,
            ),
        }
    }
}

impl Plugin for MxmGrainFx {
    const NAME: &'static str = NAME;
    const VENDOR: &'static str = "mxm";
    const URL: &'static str = "https://mxm.dk";
    const EMAIL: &'static str = "plugins@mxm.dk";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
    ];
    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type Editor = editor::MxmGrainFxEditor;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        editor::create(self.params.clone(), self.telemetry.clone())
    }

    fn activate(
        &mut self,
        layout: &AudioIOLayout,
        config: &BufferConfig,
        _context: &mut impl ActivateContext<Self>,
    ) -> bool {
        self.prepare(
            config.sample_rate,
            layout
                .main_input_channels
                .map_or(1, |channels| channels.get() as usize),
            config.max_buffer_size as usize,
        );
        true
    }

    fn reset(&mut self) {
        // Parameter flushes arrive while a host has the effect bypassed and unprocessed, so the
        // targets are synchronised before the reset: the next excitation must not begin in the
        // previous topology.
        // A host resets without a callback between (a bypass, a transport restart), and a parameter
        // flush may have moved a sync meanwhile: re-resolve every sync from the parameters as they
        // stand and the last tempo seen, so nothing is seeded from the previous division.
        let tempo = self.telemetry.tempo.get();
        self.synced = [
            self.params.synced_rate(tempo),
            self.params.synced_read_delay(tempo),
        ];
        self.engine.set_controls(&self.target_controls());
        self.engine.reset();
    }

    /// **A project saved before the tempo syncs** restores each Off rather than keeping this
    /// instance's, and a loaded preset's baseline gains it, so the preset stays clean
    /// (`mxm_preset::add_switches_off`).
    fn filter_state(state: &mut PluginState) {
        mxm_preset::add_switches_off(state, crate::preset::TEMPO_SYNC_IDS);
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // The tempo syncs, once a block, and the tempo in force for the editor's readings. Both are
        // drawn once per grain at its onset, so a division needs no smoothing.
        let tempo = context.transport().tempo;
        self.synced = [
            self.params.synced_rate(tempo),
            self.params.synced_read_delay(tempo),
        ];
        self.telemetry.tempo.publish(tempo);
        self.process_block(buffer.as_slice())
    }
}

impl ClapPlugin for MxmGrainFx {
    const CLAP_ID: &'static str = CLAP_ID;
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Turns incoming sound into clouds of tiny grains");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Delay,
        ClapFeature::Stereo,
    ];
}

nice_export_clap!(MxmGrainFx);

fn valid_sample_rate(sample_rate: f32) -> f32 {
    if sample_rate.is_finite() {
        sample_rate.clamp(8_000.0, 384_000.0)
    } else {
        48_000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nice_plug::params::{InternalParamMut, Param};

    fn prepared(input_channels: usize) -> MxmGrainFx {
        let mut plugin = MxmGrainFx::default();
        plugin.prepare(48_000.0, input_channels, 4096);
        plugin
    }

    fn set(param: &FloatParam, value: f32) {
        unsafe {
            let normalised = param.preview_normalized(value);
            let _ = param._internal_set_normalized_value(normalised);
            param._internal_update_smoother(48_000.0, true);
        }
    }

    #[test]
    fn mix_zero_is_the_dry_to_the_bit_in_both_layouts() {
        let left_input: Vec<f32> = (0..4096).map(|n| (n as f32 * 0.13).sin() * 0.4).collect();
        let right_input: Vec<f32> = (0..4096).map(|n| (n as f32 * 0.07).cos() * 0.3).collect();
        for input_channels in [1, 2] {
            let mut plugin = prepared(input_channels);
            set(&plugin.params.mix, 0.0);
            let mut left = left_input.clone();
            let mut right = right_input.clone();
            let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
            plugin.process_block(&mut channels);
            assert_eq!(left, left_input, "{input_channels}-in left");
            let expected_right = if input_channels == 1 {
                left_input.as_slice()
            } else {
                right_input.as_slice()
            };
            assert_eq!(
                right.as_slice(),
                expected_right,
                "{input_channels}-in right"
            );
        }
    }

    #[test]
    fn an_engaged_effect_changes_the_signal() {
        // The effects' init-patch rule: inserted with no gesture it must demonstrate itself.
        let mut plugin = prepared(2);
        let input: Vec<f32> = (0..48_000).map(|n| (n as f32 * 0.05).sin() * 0.4).collect();
        let mut left = input.clone();
        let mut right = input.clone();
        {
            let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
            plugin.process_block(&mut channels);
        }
        let difference: f32 = left
            .iter()
            .zip(input.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(
            difference > 1.0,
            "the default patch was inaudible: {difference}"
        );
    }

    #[test]
    fn a_tail_is_reported_and_a_live_freeze_is_normal() {
        let mut plugin = prepared(2);
        let mut left = vec![0.0; 4096];
        left[0] = 0.8;
        let mut right = vec![0.0; 4096];
        {
            let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
            plugin.process_block(&mut channels);
        }
        left.fill(0.0);
        right.fill(0.0);
        {
            let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
            assert!(matches!(
                plugin.process_block(&mut channels),
                ProcessStatus::Tail(_)
            ));
        }
    }

    #[test]
    fn a_non_finite_input_is_flushed_before_it_reaches_the_engine() {
        let mut plugin = prepared(2);
        let mut left = vec![f32::NAN, f32::INFINITY, 1.0e-42, 0.5];
        let mut right = left.clone();
        {
            let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
            plugin.process_block(&mut channels);
        }
        assert!(left.iter().all(|v| v.is_finite()));
        assert!(right.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn the_name_and_id_have_one_source() {
        assert_eq!(NAME, "mxm-grain-fx");
        assert_eq!(CLAP_ID, format!("dk.mxm.{NAME}"));
    }

    #[test]
    fn the_bundle_is_named_after_this_plugin() {
        mxm_plugin_test::bundle::is_named(env!("CARGO_MANIFEST_DIR"), env!("CARGO_PKG_NAME"), NAME);
    }
}

/// **A reset re-resolves the tempo syncs**: a sync turned off while the host held the effect
/// unprocessed does not seed the reset from the previous division, and one still on stays on it.
#[cfg(test)]
mod reset_resolves_the_syncs {
    use super::*;
    use nice_plug::params::InternalParamMut;

    #[test]
    fn a_reset_re_resolves_the_tempo_syncs() {
        let mut plugin = MxmGrainFx {
            synced: [Some(9.0), Some(0.5)],
            ..Default::default()
        };
        plugin.reset();
        assert_eq!(plugin.synced, [None; 2], "sync off: the knobs");
        plugin.telemetry.tempo.publish(Some(120.0));
        unsafe {
            let _ = plugin.params.rate_sync._internal_set_normalized_value(1.0);
        }
        plugin.reset();
        assert_eq!(plugin.synced[0], plugin.params.synced_rate(Some(120.0)));
    }

    /// **Reactivation forgets the old tempo**: nice-plug resets right after activating, and that
    /// reset must not resolve from the tempo the host reported before it was deactivated — the first
    /// callback's tempo is the first one used.
    #[test]
    fn reactivation_forgets_the_previous_tempo() {
        let mut plugin = MxmGrainFx::default();
        plugin.telemetry.tempo.publish(Some(120.0));
        unsafe {
            let _ = plugin.params.rate_sync._internal_set_normalized_value(1.0);
        }
        plugin.prepare(48_000.0, 2, 512);
        plugin.reset();
        assert_eq!(plugin.synced, [None; 2]);
    }
}

/// What a player reads — on hover in the editor, and in a host's plugin browser — speaks to the
/// player about the sound, never about the machine or the code (`mxm_plugin_test::hover_text`).
#[cfg(test)]
mod speaks_to_the_player {
    #[test]
    fn hover_text() {
        mxm_plugin_test::hover_text::speaks_to_the_player(env!("CARGO_MANIFEST_DIR"));
    }

    #[test]
    fn host_description() {
        mxm_plugin_test::hover_text::host_description_speaks_to_the_player(env!(
            "CARGO_MANIFEST_DIR"
        ));
    }
}
