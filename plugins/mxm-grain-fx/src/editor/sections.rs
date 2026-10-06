//! The Grain, Playback and Loop & output cards, in signal-flow order.
//!
//! `docs/briefs/mxm-grain-fx.md` §5 is the gating document for what is on which card and why.
//!
//! **Each variation sits beside the property it varies**, which is brief revision 2's organising
//! rule and the fix for the panel the owner could not read: collecting the depths on a card of
//! their own made them look like a subsystem, and the pairing is the only thing that says whether a
//! control changes *when a grain starts* or *what a grain is*.

use std::collections::HashMap;

use egui::{Rect, Ui};
use mxm_ui::control::Size;
use mxm_ui::theme::Tokens;
use mxm_ui::tree::{self, Height, Kind, Node};
use nice_plug::prelude::ParamSetter;

use super::binding::{Bound, toggle_labelled};
use crate::params::MxmGrainFxParams;
use crate::telemetry::Telemetry;

use super::visuals;

const KNOB: Size = Size::Standard;

/// The width of one knob column.
///
/// The collection's one knob column (76), for Shape standing beside the envelope; the knob rows are
/// `tree::knob_row`'s. This panel once passed 56, and a narrower column pushed "variation" — which
/// four of these names carry — out of its box.
const KNOB_COLUMN: f32 = mxm_ui::control::KNOB_COLUMN_MIN;

/// The sample rate the timeline's axis assumes.
///
/// The editor is never told the host's rate - `activate` knows it and the panel does not - so the
/// axis is drawn against the common one. What this costs is the *label*: at 96 kHz the window holds
/// half the seconds the caption claims. The marks themselves stay correct relative to one another,
/// which is what the picture is for, and carrying the real rate across would mean another telemetry
/// field for a caption. Revisit if the gate says the caption matters.
const SAMPLE_RATE_HINT: f32 = 48_000.0;

pub fn all_parameters(params: &MxmGrainFxParams) -> Vec<Bound<'_>> {
    vec![
        Bound::new(
            "size",
            &params.size,
            "How long each grain is: short grains buzz, middle ones blend into a texture, long ones are heard one by one.",
        )
        // The Grain card says what the three are of (B1); a host, the tooltip and a screen reader
        // read *Grain size*, *Grain rate* and *Grain shape*.
        .labelled("Size"),
        Bound::new(
            "rate",
            &params.rate,
            "How many grains start each second.",
        )
        .labelled("Rate"),
        Bound::new("ratesync", &params.rate_sync, super::binding::SYNC_DESCRIPTION),
        Bound::new(
            "shape",
            &params.shape,
            "The shape of each grain, from clicky to smooth; harder is brighter.",
        )
        .labelled("Shape"),
        Bound::new(
            "onsettiming",
            &params.onset_timing,
            "How irregularly the grains start, from steady to scattered.",
        ),
        Bound::new(
            "readdelay",
            &params.read_delay,
            "How far back in time the grains read from.",
        ),
        Bound::new(
            "readdelaysync",
            &params.read_delay_sync,
            super::binding::SYNC_DESCRIPTION,
        ),
        Bound::new(
            "width",
            &params.width,
            "Spreads the grains across the stereo field.",
        ),
        // The one bipolar control. Without this it paints a half-filled arc at 0 st, which reads
        // as "half on" where it means "no transposition" — the failure `binding.rs` documents and
        // this panel had gone without.
        Bound::new(
            "transpose",
            &params.transpose,
            "Transposes the grains; sweeping it steps rather than glides.",
        )
        .bipolar()
        .law(mxm_preset::StepLaw::Semitones),
        Bound::new(
            "reversechance",
            &params.reverse_chance,
            "How often a grain plays backwards.",
        ),
        Bound::new(
            "delayvar",
            &params.delay_variation,
            "Scatters where each grain reads from, for a thicker cloud.",
        ),
        Bound::new(
            "pitchvar",
            &params.pitch_variation,
            "Scatters each grain's pitch, from a chorus to a cloud.",
        ),
        Bound::new(
            "sizevar",
            &params.size_variation,
            "Scatters how long each grain is, for a looser texture.",
        ),
        Bound::new(
            "levelvar",
            &params.level_variation,
            "Scatters how loud each grain is.",
        ),
        Bound::new(
            "feedback",
            &params.feedback,
            "Feeds the grains back in, so each pass is scattered again.",
        ),
        Bound::new(
            "mix",
            &params.mix,
            "The balance of dry sound and grains; at zero the effect is off.",
        ),
        Bound::new(
            "freeze",
            &params.freeze,
            "Stops taking in new sound, so the grains keep playing what they have.",
        ),
    ]
}

fn bound<'a>(id: &str, params: &'a MxmGrainFxParams) -> Bound<'a> {
    all_parameters(params)
        .into_iter()
        .find(|binding| binding.id == id)
        .unwrap_or_else(|| panic!("no binding for {id}"))
}

/// The cards' titles, in signal-flow order.
pub const TITLES: [&str; 3] = ["Grain", "Playback", "Loop & output"];

/// What a leaf of this editor's cards draws. Hashed by what it names, which keeps its widget ids
/// stable when a card is re-paged.
#[derive(Clone, Copy, Debug, Hash)]
pub enum Leaf {
    Knob(&'static str),
    /// A control's tempo sync, the quarter note beside it.
    Picture(&'static str),
    Freeze,
    Envelope,
    Timeline,
}

/// A knob standing in `column`: `0.0` in the collection's knob row, which sizes the columns, or
/// [`KNOB_COLUMN`] standing alone.
fn knob(params: &MxmGrainFxParams, id: &'static str, column: f32) -> Node<Leaf> {
    let bound = bound(id, params);
    let param = bound.param;
    // A syncable control's column holds its free readings and its divisions.
    let widest = match id {
        "rate" => super::binding::synced_widest(param, crate::params::RATE_SYNC.span),
        "readdelay" => super::binding::synced_widest(param, crate::params::READ_DELAY_SYNC.span),
        _ => mxm_ui::control::widest_value(|n| param.format(n as f32)),
    };
    tree::leaf(
        Leaf::Knob(id),
        Kind::Knob {
            name: bound.painted().to_owned(),
            widest,
            size: KNOB,
            column,
        },
    )
}

/// A synced knob and its tempo sync's quarter note, then the knobs after it: one flat row, so a
/// wide card's spare width stays at its end (`plans/plan-tempo-sync-controls.md`).
fn synced_row(
    ui: &Ui,
    params: &MxmGrainFxParams,
    id: &'static str,
    sync: &'static str,
    after: &[&'static str],
) -> Node<Leaf> {
    tree::row_gap(
        ui.spacing().item_spacing.x,
        vec![
            knob_row(ui, params, &[id]),
            mxm_ui::tree::switch_beside_knob(
                KNOB,
                tree::leaf(Leaf::Picture(sync), Kind::SyncToggle),
            ),
            knob_row(ui, params, after),
        ],
    )
}

/// A row of knobs, the row's spacing apart.
fn knob_row(ui: &Ui, params: &MxmGrainFxParams, ids: &[&'static str]) -> Node<Leaf> {
    mxm_ui::tree::knob_row(
        ui,
        ids.iter()
            .map(|&id| (KNOB, knob(params, id, 0.0)))
            .collect(),
    )
}

/// A display that fills the width it is given at `visual::PLOT_HEIGHT`, at least `min_width`.
fn display(key: Leaf, min_width: f32) -> Node<Leaf> {
    tree::leaf(
        key,
        Kind::Custom {
            min_width,
            height: Height::Fixed(mxm_ui::visual::PLOT_HEIGHT),
            fills: true,
        },
    )
}

/// Card `index`'s body, as a tree (plans/plan-layout-tree.md): described once, and that one
/// description is both measured — the card's floor and height — and drawn, leaf by leaf, through
/// the bindings ([`paint`]).
///
/// Exhaustive on purpose. A catch-all arm would draw the last card for any index the list grew
/// past — silently, and with every parameter still registering somewhere, so nothing would fail.
pub fn card(ui: &Ui, index: usize, params: &MxmGrainFxParams) -> Node<Leaf> {
    match index {
        // What one grain is, then how often one starts — each beside what varies it. Onset timing
        // is paired with Grain rate rather than with a variation because it varies *the clock*, not
        // a property of a grain, and that is the distinction the whole panel is organised around.
        // The envelope fills the rest of Shape's row, because the window is a tone control and a
        // percentage does not say which tone. Brief 3.
        0 => tree::stack(vec![
            knob_row(ui, params, &["size", "sizevar"]),
            synced_row(ui, params, "rate", "ratesync", &["onsettiming"]),
            tree::row_gap(
                ui.spacing().item_spacing.x,
                vec![
                    knob(params, "shape", KNOB_COLUMN),
                    display(Leaf::Envelope, visuals::envelope_min_width()),
                ],
            ),
        ]),
        // The buffer, and the grains that actually read it. It sits above the controls because it
        // is what they are all about: time across, buffer age down, so Onset timing scatters the
        // marks sideways and Delay variation smears them vertically. Then where in the buffer each
        // grain reads, and how far each departs from the last.
        1 => tree::stack(vec![
            display(Leaf::Timeline, visuals::timeline_min_width()),
            synced_row(ui, params, "readdelay", "readdelaysync", &["delayvar"]),
            knob_row(ui, params, &["transpose", "pitchvar"]),
            knob_row(ui, params, &["reversechance", "width", "levelvar"]),
        ]),
        2 => tree::stack(vec![
            knob_row(ui, params, &["feedback", "mix"]),
            // The collection's on/off (design system §7.2), in the parameter's own word: a host
            // reads *Freeze* on or off, and so does the panel.
            tree::leaf(
                Leaf::Freeze,
                Kind::Toggle {
                    label: bound("freeze", params).painted().to_owned(),
                },
            ),
        ]),
        other => unreachable!("card {other} has no body"),
    }
}

/// The authored cards, each floor computed from its tree in `ui`'s fonts. Also what the keyboard
/// cursor is given: `mxm_ui::navigation::paged` reads the plan's own order from the last frame's
/// report and falls back to these before one exists.
pub fn page_items(ui: &Ui, params: &MxmGrainFxParams) -> Vec<mxm_ui::paging::Item<'static>> {
    use mxm_ui::paging::{Category, Item, Key};
    TITLES
        .iter()
        .enumerate()
        .map(|(index, title)| Item {
            key: Key(index as u64),
            // As wide as its controls and no wider (`plans/plan-editor-standard.md` A1).
            card: {
                let floor = tree::card_floor(ui, title, &card(ui, index, params));
                mxm_ui::flow::Card::new(title, floor).capped(floor)
            },
            category: Category::Effects,
            kind: title,
        })
        .collect()
}

/// Everything a leaf draws with.
pub struct Live<'a, 'b> {
    pub params: &'a MxmGrainFxParams,
    pub telemetry: &'a Telemetry,
    pub setter: &'a ParamSetter<'b>,
    pub text: &'a mut HashMap<&'static str, Option<String>>,
}

/// Draws one leaf, in the `Ui` the tree bounded to `rect`, through the bindings — so the controls,
/// their gestures and their names are exactly what they were.
pub fn paint(ui: &mut Ui, tokens: &Tokens, leaf: &Leaf, rect: Rect, live: &mut Live<'_, '_>) {
    let params = live.params;
    match *leaf {
        // Synced to a tempo, a knob reads its division; the host still reads its value.
        Leaf::Knob(id) => {
            let tempo = live.telemetry.tempo.get();
            let division = match id {
                "rate" if params.rate_sync.value() => {
                    use nice_plug::prelude::Param as _;
                    let rate = &params.rate;
                    crate::params::RATE_SYNC.shown(
                        rate.unmodulated_normalized_value(),
                        tempo,
                        f64::from(rate.preview_plain(0.0)),
                        f64::from(rate.preview_plain(1.0)),
                    )
                }
                "readdelay" if params.read_delay_sync.value() => {
                    use nice_plug::prelude::Param as _;
                    crate::params::READ_DELAY_SYNC.shown(
                        params.read_delay.unmodulated_normalized_value(),
                        tempo,
                        0.0,
                        f64::from(mxm_grain_fx_dsp::BUFFER_S),
                    )
                }
                _ => None,
            };
            let bound = bound(id, params);
            match division {
                Some(division) => bound.knob_with_reading(
                    ui,
                    tokens,
                    live.setter,
                    KNOB,
                    rect.width(),
                    live.text,
                    division.label(),
                ),
                None => bound.knob(ui, tokens, live.setter, KNOB, rect.width(), live.text),
            }
        }
        Leaf::Picture(id) => {
            super::binding::sync_picture(ui, tokens, id, bound(id, params).param, live.setter);
        }
        Leaf::Freeze => {
            let binding = bound("freeze", params);
            toggle_labelled(
                ui,
                tokens,
                "freeze",
                binding.param,
                binding.painted(),
                binding.description,
                live.setter,
                0.0,
            );
        }
        Leaf::Envelope => visuals::envelope(ui, tokens, params),
        Leaf::Timeline => {
            visuals::timeline(ui, tokens, params, live.telemetry, SAMPLE_RATE_HINT);
        }
    }
}

pub fn cards(
    ui: &mut Ui,
    tokens: &Tokens,
    params: &MxmGrainFxParams,
    telemetry: &Telemetry,
    setter: &ParamSetter<'_>,
    text: &mut HashMap<&'static str, Option<String>>,
) -> f32 {
    let items = page_items(ui, params);
    let text_editing = text.values().any(Option::is_some);
    let mut live = Live {
        params,
        telemetry,
        setter,
        text,
    };
    let report = mxm_ui::paging::editor::show(
        ui,
        tokens,
        &items,
        &[],
        text_editing,
        &mut |ui, index| card(ui, index, params),
        &mut |ui, _, leaf, rect| paint(ui, tokens, leaf, rect, &mut live),
    );
    report
        .visible
        .iter()
        .map(|(_, rect)| rect.bottom())
        .fold(ui.min_rect().bottom(), f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nice_plug::prelude::Params;

    /// Every parameter is bound exactly once, and nothing is bound that is not a parameter.
    ///
    /// **This is the test that makes a rename safe.** The permanent ids are written out by hand in
    /// three places — `params.rs`'s `#[id]`, `preset.rs`'s `parameters()`, and `all_parameters()`
    /// here — and none of the three is checked against another by the compiler. Ported from
    /// `mxm-bucket-delay`, which has carried it since its own rename.
    #[test]
    fn every_parameter_is_bound_exactly_once() {
        let params = MxmGrainFxParams::default();
        let bound: Vec<&str> = all_parameters(&params).iter().map(|b| b.id).collect();
        for (id, _, _) in params.param_map() {
            assert!(
                bound.contains(&id.as_str()),
                "{id} is not bound to a control"
            );
        }
        assert_eq!(
            bound.len(),
            params.param_map().len(),
            "a control is bound that is not a parameter"
        );
        for (i, id) in bound.iter().enumerate() {
            assert!(!bound[i + 1..].contains(id), "{id} is bound twice");
        }
    }

    /// Every binding carries a sentence, because design system §7.1 requires one in every tooltip
    /// and CLAP has no field a host could supply it from.
    #[test]
    fn every_binding_carries_a_description() {
        let params = MxmGrainFxParams::default();
        for bound in all_parameters(&params) {
            assert!(
                bound.description.len() > 20 && bound.description.ends_with('.'),
                "{} has no usable description",
                bound.id
            );
        }
    }

    /// Every card body is reachable, and no index falls through to a neighbour.
    ///
    /// `card()` dispatches on a positional index. A catch-all arm there would build the last card
    /// for any stale index and the compiler would say nothing, so every title's index is built here,
    /// and each body names different parameters.
    #[test]
    fn every_card_has_a_body() {
        let params = MxmGrainFxParams::default();
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let bodies: Vec<String> = (0..TITLES.len())
                .map(|index| format!("{:?}", card(ui, index, &params).keys()))
                .collect();
            for (i, body) in bodies.iter().enumerate() {
                assert!(
                    !bodies[i + 1..].contains(body),
                    "{} shares a body with a later card",
                    TITLES[i]
                );
            }
        });
        output.textures_delta.clear();
    }
}
