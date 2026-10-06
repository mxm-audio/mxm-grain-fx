//! The Playback card's buffer timeline, and the Grain card's envelope.
//!
//! Both are plugin-local, following `mxm-bucket-delay`'s constellation and `mxm-chorus-06`'s Sweep.
//! Neither is a reusable type and neither belongs in `crates/ui` (mxm-kit's `mxm-ui`): design
//! system §13 keeps a widget out of the shared crate until a second instrument needs it, and
//! nothing else here draws a capture buffer.
//!
//! **Geometry comes from `mxm_ui::visual`'s tokens**, so telemetry does not quietly grow a second
//! design system, and **nothing here calls `navigation::at`** — a display is not a control, and the
//! keyboard cursor's coverage check would report it as a parameter that is not one.

use egui::{Align2, FontId, Rect, Sense, Ui, Vec2, pos2};
use mxm_ui::theme::Tokens;
use mxm_ui::visual;

use crate::params::MxmGrainFxParams;
use crate::telemetry::{ONSET_SLOTS, Telemetry};

/// How much recent history the timeline shows, in seconds.
///
/// Chosen. Long enough that the slowest onsets the panel can ask for — half a grain per second —
/// leave more than one mark on screen, and short enough that the fastest do not collapse into a
/// solid band before the eye can see their spacing.
const WINDOW_S: f32 = 3.0;

/// The mark for one grain, at its narrowest.
const MARK_MIN_WIDTH: f32 = 1.5;

const CAPTION_FONT: f32 = 10.0;
const CAPTION_INSET: f32 = 6.0;

/// The narrowest the timeline can be drawn and still say anything.
pub fn timeline_min_width() -> f32 {
    220.0
}

/// The capture buffer over the last few seconds, with the grains that actually read it.
///
/// **Two axes, and the whole point is that they are independent.** Time runs left to right, so the
/// record head is the right-hand edge and a mark's horizontal position is *when that grain started*
/// — which is what Onset timing moves. Buffer age runs top to bottom, so a mark's vertical position
/// is *how far back it read* — which is what Read delay sets and Delay variation spreads. A player
/// who cannot tell those two controls apart can see the difference here: one scatters the marks
/// sideways, the other smears them vertically.
///
/// The marks are real. They come from the engine's own onset log through [`Telemetry`], never from
/// a pattern computed off the controls — at the top of Onset timing the scheduler is a per-sample
/// draw, and a control-derived picture would draw an even comb over a Poisson cloud.
pub fn timeline(
    ui: &mut Ui,
    tokens: &Tokens,
    params: &MxmGrainFxParams,
    telemetry: &Telemetry,
    sample_rate: f32,
) {
    let width = ui.available_width().max(timeline_min_width());
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(width, visual::PLOT_HEIGHT),
        // Hover only: this is a picture, not a control.
        Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, visual::CANVAS_RADIUS, tokens.surface_1);

    let inner = rect.shrink(visual::INNER_GUTTER);
    let age_to_y = |age: f32| inner.top() + age.clamp(0.0, 1.0) * inner.height();

    // The band Delay variation reaches, around the delay Read delay sets. Drawn first, so the marks
    // and the read line sit on top of it.
    // Synced to a tempo, the read line is the division in force.
    let centre = params
        .synced_read_delay(telemetry.tempo.get())
        .unwrap_or_else(|| params.read_delay.value());
    let spread = params.delay_variation.value();
    if spread > 0.0 {
        let band = Rect::from_x_y_ranges(
            inner.left()..=inner.right(),
            age_to_y(centre - spread)..=age_to_y(centre + spread),
        );
        painter.rect_filled(band, 2.0, tokens.accent.gamma_multiply(0.16));
    }

    // The read point itself.
    painter.line_segment(
        [
            pos2(inner.left(), age_to_y(centre)),
            pos2(inner.right(), age_to_y(centre)),
        ],
        egui::Stroke::new(visual::REFERENCE_STROKE, tokens.text_secondary),
    );

    // The record head: age zero, the newest sample, and the right-hand edge of now.
    painter.line_segment(
        [
            pos2(inner.right(), inner.top()),
            pos2(inner.right(), inner.bottom()),
        ],
        egui::Stroke::new(visual::EMPHASIS_STROKE, tokens.accent),
    );

    // The grains. One mark each, placed by when it started and how far back it read, and as wide as
    // the grain is long — so a long grain is a dash and a two-millisecond one is a hair.
    let mut onsets = [(0u32, 0.0f32); ONSET_SLOTS];
    let found = telemetry.onsets(&mut onsets);
    let now = telemetry.clock() as u32;
    let window_samples = WINDOW_S * sample_rate;
    let mark_width =
        ((params.size.value() * sample_rate) / window_samples * inner.width()).max(MARK_MIN_WIDTH);
    for (at, age_fraction) in &onsets[..found] {
        // Truncated clocks, so the difference is taken in wrapping arithmetic.
        let ago = now.wrapping_sub(*at) as f32;
        if ago > window_samples {
            continue;
        }
        let x = inner.right() - (ago / window_samples) * inner.width();
        let y = age_to_y(*age_fraction);
        let mark = Rect::from_min_size(
            pos2(x - mark_width * 0.5, y - visual::TRACE_STROKE),
            Vec2::new(mark_width, visual::TRACE_STROKE * 2.0),
        );
        painter.rect_filled(mark, 0.5, tokens.accent);
    }

    // Both ends of the axis, so the picture carries its own scale.
    let caption = |text: &str, anchor: Align2, at: egui::Pos2| {
        painter.text(
            at,
            anchor,
            text,
            FontId::proportional(CAPTION_FONT),
            tokens.text_secondary,
        );
    };
    caption(
        &format!("{:.0} s ago", WINDOW_S),
        Align2::LEFT_BOTTOM,
        pos2(rect.left() + CAPTION_INSET, rect.bottom() - CAPTION_INSET),
    );
    caption(
        "now",
        Align2::RIGHT_BOTTOM,
        pos2(rect.right() - CAPTION_INSET, rect.bottom() - CAPTION_INSET),
    );
    caption(
        "oldest",
        Align2::LEFT_TOP,
        pos2(rect.left() + CAPTION_INSET, rect.top() + CAPTION_INSET),
    );
}

/// The narrowest the envelope can be drawn.
pub fn envelope_min_width() -> f32 {
    96.0
}

/// One grain's envelope, at the shape the control is sitting at.
///
/// `docs/briefs/mxm-grain-fx.md` §3: the window is a **tone control**, not a quality setting, and
/// "75 %" says nothing about that. The curve does: a near-rectangle with a one-hundredth attack is
/// visibly the clicking end, the midpoint is visibly a triangle, and the smooth end is visibly a
/// Hann. It reads the DSP's own [`mxm_grain_fx_dsp::window::window`], so it cannot drift from the
/// envelope the grains are actually given.
pub fn envelope(ui: &mut Ui, tokens: &Tokens, params: &MxmGrainFxParams) {
    let width = ui.available_width().max(envelope_min_width());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, visual::PLOT_HEIGHT), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, visual::CANVAS_RADIUS, tokens.surface_1);

    let inner = rect.shrink(visual::INNER_GUTTER);
    let shape = params.shape.value();
    let steps = (inner.width().round() as usize).clamp(16, 512);
    let points: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let phase = i as f32 / steps as f32;
            let w = mxm_grain_fx_dsp::window::window(shape, phase);
            pos2(
                inner.left() + phase * inner.width(),
                inner.bottom() - w * inner.height(),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(visual::TRACE_STROKE, tokens.accent),
    ));
    painter.line_segment(
        [
            pos2(inner.left(), inner.bottom()),
            pos2(inner.right(), inner.bottom()),
        ],
        egui::Stroke::new(visual::AXIS_STROKE, tokens.text_secondary),
    );
}
