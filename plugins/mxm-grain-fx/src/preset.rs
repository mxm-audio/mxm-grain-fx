//! The preset system's view of this plugin.
//!
//! **The factory bank is deliberately empty until the listening gate.** `plans/plan-mxm-grain-fx.md`
//! puts presets at G5, after the gate, and the reason is not scheduling: a factory sound is a set of
//! parameter values, and the gate can still change *which parameters exist* — decision 9 alone
//! decides whether five spray depths become controls or two do. A bank authored now would be
//! authored against a parameter set that is provisional, which is exactly the mistake
//! `mxm-bucket-delay` made at its revision 15, when two of its presets turned out to have been
//! written against laws that had since changed.
//!
//! Init still works: `mxm-preset` generates it from the parameter defaults rather than from a file,
//! so the browser, Save As, favourites and the rest are all live from the first build. What is
//! missing is only the shipped sounds.

use mxm_preset::{Instrument, PresetIdentity};
use std::sync::RwLock;

use crate::params::MxmGrainFxParams;

pub use mxm_preset::Library;

/// **The tempo syncs this plugin gained on 2026-09-25** (`plans/plan-tempo-sync-controls.md`). A
/// preset file written before them was written unsynced, so each loads off rather than keeping the
/// instance's sync, and without reporting a missing control.
pub(crate) const TEMPO_SYNC_IDS: &[&str] = &["ratesync", "readdelaysync"];

impl Instrument for MxmGrainFxParams {
    fn clap_id(&self) -> &'static str {
        crate::CLAP_ID
    }

    fn parameters(&self) -> Vec<(&'static str, &dyn mxm_preset::ErasedParam)> {
        // Declaration order is the preset and Init write order. It is deliberately independent of
        // the editor's signal-flow order, which may change without changing host-facing writes.
        vec![
            ("mix", &self.mix),
            ("size", &self.size),
            ("rate", &self.rate),
            ("ratesync", &self.rate_sync),
            ("onsettiming", &self.onset_timing),
            ("readdelay", &self.read_delay),
            ("readdelaysync", &self.read_delay_sync),
            ("transpose", &self.transpose),
            ("reversechance", &self.reverse_chance),
            ("shape", &self.shape),
            ("width", &self.width),
            ("feedback", &self.feedback),
            ("freeze", &self.freeze),
            ("delayvar", &self.delay_variation),
            ("pitchvar", &self.pitch_variation),
            ("sizevar", &self.size_variation),
            ("levelvar", &self.level_variation),
        ]
    }

    fn identity(&self) -> &RwLock<PresetIdentity> {
        &self.preset
    }

    fn factory_files(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }

    fn default_missing_legacy_parameter(&self, id: &str) -> bool {
        TEMPO_SYNC_IDS.contains(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A project saved before the tempo syncs restores them Off** (`mxm_preset::add_switches_off`),
    /// whatever this instance had.
    #[test]
    fn an_older_state_restores_the_tempo_syncs_off() {
        use nice_plug::prelude::Plugin as _;
        let mut state = nice_plug::prelude::PluginState {
            version: String::new(),
            params: Default::default(),
            fields: Default::default(),
        };
        crate::MxmGrainFx::filter_state(&mut state);
        for id in TEMPO_SYNC_IDS {
            assert!(
                matches!(
                    state.params.get(*id),
                    Some(nice_plug::plugin::ParamValue::Bool(false))
                ),
                "{{id}} was not restored off"
            );
        }
    }

    /// **A preset saved before the tempo syncs loads them off, and cleanly** ([`TEMPO_SYNC_IDS`]).
    #[test]
    fn a_preset_from_before_the_tempo_syncs_loads_them_off() {
        let params = crate::params::MxmGrainFxParams::default();
        let mut old = mxm_preset::Preset::init(&params);
        for id in TEMPO_SYNC_IDS {
            old.params.remove(*id);
        }
        let (writes, problems) = old.resolve(&params);
        assert!(problems.is_empty(), "{{problems:?}}");
        for id in TEMPO_SYNC_IDS {
            assert!(
                writes.iter().any(|(w, _, v)| w == id && *v == 0.0),
                "{{id}} was not written off"
            );
        }
    }

    #[test]
    fn every_parameter_is_declared_exactly_once() {
        let params = MxmGrainFxParams::default();
        let declared = params.parameters();
        let mut ids: Vec<&str> = declared.iter().map(|(id, _)| *id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "a parameter id is declared twice");
        // Seventeen: ten controls, the five per-grain variation depths the gate chooses between, and
        // the two tempo syncs of 2026-09-25. It was sixteen, then fifteen when Spread and the pan
        // depth turned out to be one control — two knobs summing two draws into the same pan value —
        // and were merged.
        assert_eq!(declared.len(), 17, "the declared set changed: {ids:?}");
    }

    #[test]
    fn the_init_preset_is_the_parameter_defaults() {
        // Init has no file and must not grow one: it is generated from the defaults, so the button
        // and the host's own "reset this parameter" cannot disagree.
        let params = MxmGrainFxParams::default();
        assert!(
            params.factory_files().is_empty(),
            "the bank is authored at G5, after the gate can still move the parameter set"
        );
        for (id, param) in params.parameters() {
            assert_eq!(
                param.normalised(),
                param.default_normalised(),
                "{id} does not start at its default"
            );
        }
    }
}
