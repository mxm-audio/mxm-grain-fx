//! The dynamically paged, three-card MXM editor for `mxm-grain-fx`.
//!
//! `docs/briefs/mxm-grain-fx.md` is the gating document. **This editor exists so the plan's
//! listening gate can happen**: a granular processor's open decisions are all about how its controls
//! interact, and none of them can be judged from a rendered file.

pub mod binding;
pub mod sections;
pub mod visuals;

use std::collections::HashMap;
use std::sync::Arc;

use egui::Ui;
use mxm_ui::space::SPACE_5;
use mxm_ui::theme::Tokens;
use nice_plug::context::gui::GuiContext;
use nice_plug::prelude::*;
use nice_plug_egui::{EguiEditorState, NiceEguiApp, create_egui_editor};

use crate::params::MxmGrainFxParams;
use crate::telemetry::Telemetry;

// Three cards, each two or three knob rows deep, two of them carrying a display.
//
// The opening size is the quarter-4K budget hugged to the cards (`plans/plan-editor-standard.md`
// F1), which `tests::the_opening_size_is_the_budget_hugged` holds; the minimum is one card wide at
// the widest floor plus its gutters, which `the_minimum_holds_the_widest_card` pins.
const REFERENCE: (u32, u32) = (818, 543);
const MINIMUM: (u32, u32) = (380, 360);

pub type MxmGrainFxEditor = nice_plug_egui::EguiEditor<MxmGrainFxApp>;
pub use mxm_preset::PresetUi;

pub fn create(
    params: Arc<MxmGrainFxParams>,
    telemetry: Arc<Telemetry>,
) -> Option<MxmGrainFxEditor> {
    let state = EguiEditorState::from_size(
        nice_plug::editor::dpi::LogicalSize::new(REFERENCE.0, REFERENCE.1),
        1.0,
    );
    create_egui_editor(
        state,
        nice_plug_egui::RepaintNotifier::new(),
        nice_plug_egui::EguiNiceSettings {
            title: crate::NAME.to_owned(),
            resize_hint: ResizeHint {
                size_constraints: nice_plug::editor::SizeConstraints::min_logical_size(
                    nice_plug::editor::dpi::LogicalSize::new(MINIMUM.0 as f32, MINIMUM.1 as f32),
                ),
                ..ResizeHint::RESIZABLE
            },
            ..Default::default()
        },
        MxmGrainFxApp::new(params, telemetry),
    )
}

pub struct MxmGrainFxApp {
    params: Arc<MxmGrainFxParams>,
    telemetry: Arc<Telemetry>,
    gui_context: Option<GuiContext>,
    text_entry: HashMap<&'static str, Option<String>>,
    presets: PresetUi,
    /// Where the keyboard is: a card, and a parameter inside it. Transient, like the text
    /// buffers — it is not a parameter and nothing durable reads it.
    nav: mxm_ui::navigation::State,
}

impl MxmGrainFxApp {
    pub fn new(params: Arc<MxmGrainFxParams>, telemetry: Arc<Telemetry>) -> Self {
        let presets = PresetUi::new(params.as_ref());
        Self {
            params,
            telemetry,
            gui_context: None,
            text_entry: HashMap::new(),
            presets,
            nav: mxm_ui::navigation::State::default(),
        }
    }
}

impl NiceEguiApp for MxmGrainFxApp {
    fn build(
        &mut self,
        context: egui::Context,
        gui_context: GuiContext,
        _frame: &mut nice_plug_egui::Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        mxm_ui::theme::apply(&context);
        mxm_ui::typography::apply(&context);
        context.set_theme(mxm_ui::theme::preference());
        self.gui_context = Some(gui_context);
        Ok(())
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut nice_plug_egui::Frame) {
        let Some(context) = self.gui_context.clone() else {
            return;
        };
        panel(
            ui,
            &self.params,
            &self.telemetry,
            &context.param_setter(),
            &mut self.text_entry,
            &mut self.presets,
            &mut self.nav,
        );
    }

    fn editor_closed(&mut self) {
        self.gui_context = None;
    }
}

pub fn panel(
    ui: &mut Ui,
    params: &MxmGrainFxParams,
    telemetry: &Telemetry,
    setter: &ParamSetter<'_>,
    text_entry: &mut HashMap<&'static str, Option<String>>,
    presets: &mut PresetUi,
    nav: &mut mxm_ui::navigation::State,
) {
    let tokens = tokens_for(ui);
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(50));
    let peak = telemetry.take_peak();
    let _grains = telemetry.take_grains();
    let clipped = telemetry.clipped();
    // One question, and both layers suspend on it: the paging renderer's `hold` and the cursor's
    // `inert` both ask whether another surface owns this frame's keyboard.
    let busy = presets.holds_the_keyboard() || text_entry.values().any(Option::is_some);
    mxm_ui::paging::editor::hold(ui.ctx(), busy);
    // **The cursor moves before anything is drawn**, so a navigation arrow is consumed here rather
    // than also walking egui's own focus ring. It reads the registry and the exact card rectangles
    // the previous frame built, and navigates the paging plan's own order.
    mxm_ui::navigation::paged(ui.ctx(), nav, busy);

    mxm_ui::AppBar::new(crate::NAME).show_with(
        ui,
        &tokens,
        |ui| mxm_preset::ui::preset_row(ui, &tokens, params, setter, presets),
        |ui| {
            if mxm_ui::shell::level_meter(ui, &tokens, peak, clipped) {
                telemetry.clear_clip();
            }
            mxm_ui::shell::zoom_control(ui);
            mxm_ui::shell::editor_theme_control(ui);
        },
    );
    mxm_preset::ui::overlays(ui, &tokens, params, setter, presets);
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(tokens.canvas)
                .inner_margin(egui::Margin::same(SPACE_5 as i8)),
        )
        .show(ui, |ui| {
            sections::cards(ui, &tokens, params, telemetry, setter, text_entry);
        });
}

fn tokens_for(ui: &Ui) -> Tokens {
    if ui.visuals().dark_mode {
        mxm_ui::DARK
    } else {
        mxm_ui::LIGHT
    }
}

/// The paging items as the editor computes them, from a context set up as an editor's is — three
/// passes in, so the weighted font cuts are bound — for tests, which have no editor `Ui` to hand.
#[cfg(test)]
pub(crate) fn test_items(params: &MxmGrainFxParams) -> Vec<mxm_ui::paging::Item<'static>> {
    let ctx = egui::Context::default();
    mxm_ui::typography::apply(&ctx);
    mxm_ui::theme::apply(&ctx);
    let mut items = Vec::new();
    for _ in 0..3 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            items = sections::page_items(ui, params);
        });
        output.textures_delta.clear();
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use nice_plug::params::internals::ParamPtr;
    use nice_plug::prelude::{PluginApi, PluginState};

    struct NoHost;

    impl nice_plug::context::gui::GuiContextInner for NoHost {
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Clap
        }
        unsafe fn raw_begin_set_parameter(&self, _param: ParamPtr) {}
        unsafe fn raw_set_parameter_normalized(&self, _param: ParamPtr, _normalized: f32) {}
        unsafe fn raw_end_set_parameter(&self, _param: ParamPtr) {}
        fn get_state(&self) -> PluginState {
            PluginState {
                version: String::new(),
                params: Default::default(),
                fields: Default::default(),
            }
        }
        fn set_state(&self, _state: PluginState) {}
    }

    /// Every word the panel paints, with the box it painted into.
    fn painted_boxes(theme: egui::ThemePreference) -> Vec<(String, egui::Rect)> {
        let context = egui::Context::default();
        mxm_ui::theme::apply(&context);
        mxm_ui::typography::apply(&context);
        context.set_theme(theme);
        context.all_styles_mut(|style| style.animation_time = 0.0);

        let params = MxmGrainFxParams::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32),
            )),
            ..Default::default()
        };

        let mut boxes = Vec::new();
        for pass in 0..3 {
            let mut output = context.run_ui(input.clone(), |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            });
            if pass == 2 {
                for clipped in &output.shapes {
                    collect_boxes(&clipped.shape, &mut boxes);
                }
            }
            output.textures_delta.clear();
        }
        boxes
    }

    fn collect_boxes(shape: &egui::epaint::Shape, boxes: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::epaint::Shape::Text(text) => boxes.push((
                text.galley.text().to_owned(),
                text.galley.rect.translate(text.pos.to_vec2()),
            )),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_boxes(shape, boxes);
                }
            }
            _ => {}
        }
    }

    use mxm_plugin_test::keyboard_checks;
    use mxm_plugin_test::opening_size;

    /// **The editor opens at the quarter-4K budget, hugged** (`plans/plan-editor-standard.md` F1):
    /// `REFERENCE` is derived, not typed, and this holds it.
    #[test]
    fn the_opening_size_is_the_budget_hugged() {
        let params = MxmGrainFxParams::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        opening_size::is_the_budget_hugged(
            egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32),
            &REVEAL,
            &mut |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            },
        );
    }

    /// **The app bar holds in the narrowest window**: its `…` menu whole and nothing drawn over
    /// anything else, from `MINIMUM` up (`opening_size::bar_holds_from_the_minimum`).
    #[test]
    fn the_app_bar_holds_in_the_minimum_window() {
        let params = MxmGrainFxParams::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        opening_size::bar_holds_from_the_minimum(
            egui::vec2(MINIMUM.0 as f32, MINIMUM.1 as f32),
            &mut |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            },
        );
    }

    /// What this editor keeps behind a disclosure, opened so the reachability check sees it.
    /// Nothing here: every control is on a card.
    const REVEAL: fn(&egui::Context) = |_| {};

    /// The rollout's own failure mode: a control whose `navigation::at` scope was forgotten paints
    /// exactly as before and is simply unreachable from the keyboard. Nothing else would say so.
    #[test]
    fn the_keyboard_cursor_reaches_and_operates_every_parameter() {
        let params = MxmGrainFxParams::default();
        let telemetry = Telemetry::default();
        let host = keyboard_checks::Recorder::default();
        let setter = ParamSetter::new(&host);
        let ids: Vec<&str> = sections::all_parameters(&params)
            .iter()
            .map(|bound| bound.id)
            .collect();
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        keyboard_checks::the_cursor_reaches_and_operates(
            egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32),
            &test_items(&params),
            keyboard_checks::Coverage::Exactly(&ids),
            &REVEAL,
            &host,
            &mut |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            },
        );
    }

    #[test]
    fn the_minimum_holds_the_widest_card() {
        let cards: Vec<_> = test_items(&MxmGrainFxParams::default())
            .iter()
            .map(|item| item.card)
            .collect();
        assert!(MINIMUM.0 as f32 >= mxm_ui::flow::minimum_width(&cards) + 2.0 * SPACE_5);
    }

    /// Every card, in every state that changes what it paints, passes the layout tree's checks
    /// (plans/plan-layout-tree.md §4.3, `tree_checks::card`): its computed floor holds its content
    /// with nothing painted outside the card, the content floor is exact, the height its tree states
    /// is the height it draws, and every leaf stays in the room it was given.
    ///
    /// The states are this editor's structural-state matrix. The cards have no route, disclosure or
    /// reserved alternative; what changes is what the two displays paint: the init patch; every
    /// control at its bottom, where the envelope is the near-rectangle and the timeline has no band;
    /// and every control at its top with the onset log full, where the band, the longest marks and
    /// the Hann reach furthest. The onsets are real telemetry, published here as the engine would.
    #[test]
    fn every_card_passes_the_tree_checks_in_every_state() {
        use nice_plug::params::InternalParamMut;
        for state in [
            "init",
            "every control at its bottom",
            "every control at its top",
            "both syncs on, no tempo",
            "both syncs on at a tempo",
        ] {
            let params = MxmGrainFxParams::default();
            let telemetry = Telemetry::default();
            if state.starts_with("both syncs on") {
                unsafe {
                    let _ = params.rate_sync._internal_set_normalized_value(1.0);
                    let _ = params.read_delay_sync._internal_set_normalized_value(1.0);
                }
            }
            if state == "both syncs on at a tempo" {
                telemetry.tempo.publish(Some(120.0));
            }
            let end = match state {
                "every control at its bottom" => Some(0.0),
                "every control at its top" => Some(1.0),
                _ => None,
            };
            if let Some(end) = end {
                for param in [
                    &params.size,
                    &params.rate,
                    &params.shape,
                    &params.onset_timing,
                    &params.read_delay,
                    &params.width,
                    &params.transpose,
                    &params.reverse_chance,
                    &params.delay_variation,
                    &params.pitch_variation,
                    &params.size_variation,
                    &params.level_variation,
                    &params.feedback,
                    &params.mix,
                ] {
                    unsafe {
                        let _ = param._internal_set_normalized_value(end);
                        param._internal_update_smoother(48_000.0, true);
                    }
                }
            }
            if end == Some(1.0) {
                let onsets: Vec<_> = (0..crate::telemetry::ONSET_SLOTS)
                    .map(|i| mxm_grain_fx_dsp::Onset {
                        at: i as u64 * 1_000,
                        age_fraction: (i % 7) as f32 / 6.0,
                    })
                    .collect();
                telemetry.publish_onsets(crate::telemetry::ONSET_SLOTS as u64 * 1_000, &onsets);
            }
            let floors: Vec<f32> = test_items(&params)
                .iter()
                .map(|item| item.card.floor)
                .collect();
            let host = NoHost;
            let setter = ParamSetter::new(&host);
            for (index, floor) in floors.into_iter().enumerate() {
                let mut text = HashMap::new();
                let mut live = sections::Live {
                    params: &params,
                    telemetry: &telemetry,
                    setter: &setter,
                    text: &mut text,
                };
                tree_checks::card(
                    &|_| {},
                    state,
                    sections::TITLES[index],
                    floor,
                    &|ui| sections::card(ui, index, &params),
                    &mut |ui, leaf, rect| {
                        sections::paint(ui, &mxm_ui::LIGHT, leaf, rect, &mut live);
                    },
                );
            }
        }
    }

    /// **The test that would have caught the panel the owner could not read.** It asserts the words
    /// the panel actually paints, so a control that is renamed in one list and not another, or a
    /// label squeezed into a column too narrow for its longest word, fails here rather than in a
    /// screenshot. Ported from `mxm-shimmer`, which has carried it since its own editor landed.
    #[test]
    fn both_themes_paint_every_card_and_control_inside_the_reference_window() {
        for theme in [egui::ThemePreference::Light, egui::ThemePreference::Dark] {
            let boxes = painted_boxes(theme);
            let words: Vec<_> = boxes.iter().map(|(word, _)| word.as_str()).collect();
            for expected in [
                // The three cards.
                "Grain",
                "Playback",
                "Loop & output",
                // Every control, by the name the panel actually paints.
                "Size",
                "Size variation",
                "Rate",
                "Onset timing",
                "Shape",
                "Read delay",
                "Delay variation",
                "Transpose",
                "Pitch variation",
                "Reverse chance",
                "Stereo width",
                "Level variation",
                "Feedback",
                "Mix",
                "Freeze",
            ] {
                assert!(
                    words.contains(&expected),
                    "{theme:?} never painted {expected:?}; it painted {words:?}"
                );
            }
            for (text, rect) in boxes {
                assert!(
                    rect.left() >= 0.0
                        && rect.right() <= REFERENCE.0 as f32
                        && rect.bottom() <= REFERENCE.1 as f32,
                    "{theme:?} painted {text:?} at {rect:?} outside the reference window"
                );
                if text.chars().count() > 1 {
                    assert!(
                        rect.height() <= 3.0 * rect.width().max(1.0),
                        "{theme:?} squeezed {text:?} into a {:.1} x {:.1} column",
                        rect.width(),
                        rect.height()
                    );
                }
            }
        }
    }

    use mxm_plugin_test::tree_checks;

    /// Every page at the opening size, light and dark, for the owner's review of the layout-tree
    /// conversion (plans/plan-layout-tree.md §4.3): `target/layout-tree/mxm-grain-fx/<tag>/`, where
    /// `MXM_PICTURES` names the tag — `before` on the unconverted editor, `after` on the tree.
    ///
    /// `MXM_PICTURES=after cargo test -p mxm-grain-fx --lib tree_pictures -- --ignored`
    #[test]
    #[ignore = "renders through wgpu; run by hand"]
    fn tree_pictures() {
        let tag = std::env::var("MXM_PICTURES").unwrap_or_else(|_| "after".to_owned());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/layout-tree/mxm-grain-fx")
            .join(tag);
        let params = MxmGrainFxParams::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        tree_checks::pictures(
            &|_| {},
            egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32),
            &dir,
            &mut |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            },
        );
    }
}
