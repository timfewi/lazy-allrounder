//! The collapsed floating badge: a circular button carrying the app's
//! waveform mark, whose ring color and pulse reflect the current activity.
//! The badge senses drags so the window can be moved (the caller turns a
//! drag into an OS window move).

use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};

use crate::overlay::theme;
use crate::state::{Activity, OverlayState};

/// Relative bar heights of the waveform mark, center bar tallest. Keep in
/// sync with `heights` in the icon generator (`tools/gen_icon.py`) so the
/// badge and the window/tray icon read as the same logo.
const BAR_HEIGHTS: [f32; 5] = [0.42, 0.72, 1.0, 0.72, 0.42];

pub fn draw(ui: &mut Ui, state: &OverlayState, time: f64) -> Response {
    let size = ui.available_size().min(Vec2::splat(56.0));
    // click_and_drag so the caller can distinguish a tap (toggle the panel)
    // from a drag (move the window).
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter();
    let center = rect.center();
    let base_radius = rect.width().min(rect.height()) * 0.5 - 4.0;

    let (ring_color, pulse, animated) = match &state.activity {
        Activity::Idle => (theme::ACCENT_SOFT, 0.0, false),
        Activity::Processing { .. } => {
            let pulse = ((time * 4.0).sin() * 0.5 + 0.5) as f32;
            (theme::ACCENT, pulse, true)
        }
        Activity::Done { .. } => (theme::SUCCESS, 0.0, false),
        Activity::Error { .. } => (theme::FAILURE, 0.0, false),
    };

    // Soft outer pulse ring while processing.
    if pulse > 0.0 {
        let ring_radius = base_radius + 1.0 + pulse * 4.0;
        let alpha = (90.0 * (1.0 - pulse)) as u8 + 40;
        painter.circle_stroke(
            center,
            ring_radius,
            Stroke::new(
                2.0,
                Color32::from_rgba_unmultiplied(
                    ring_color.r(),
                    ring_color.g(),
                    ring_color.b(),
                    alpha,
                ),
            ),
        );
    }

    let fill = if response.hovered() {
        theme::SURFACE_RAISED
    } else {
        theme::SURFACE
    };
    painter.circle_filled(center, base_radius, fill);
    painter.circle_stroke(center, base_radius, Stroke::new(2.0, ring_color));

    // The mark itself picks up the accent while idle, and the activity color
    // otherwise, so the whole badge reads as one status light.
    let mark_color = if matches!(state.activity, Activity::Idle) {
        theme::ACCENT
    } else {
        ring_color
    };
    draw_waveform(painter, center, base_radius, mark_color, animated, time);

    if let Some(status) = state.status_line() {
        response.clone().on_hover_text(status);
    }

    response
}

/// Paints the five-bar waveform mark centered in the badge. While `animated`,
/// each bar bounces on its own phase so the mark visibly "speaks" during
/// processing; otherwise the bars hold their resting heights.
fn draw_waveform(
    painter: &egui::Painter,
    center: Pos2,
    base_radius: f32,
    color: Color32,
    animated: bool,
    time: f64,
) {
    let count = BAR_HEIGHTS.len();
    let bar_width = base_radius * 0.22;
    let gap = base_radius * 0.16;
    let span = count as f32 * bar_width + (count as f32 - 1.0) * gap;
    let left = center.x - span / 2.0 + bar_width / 2.0;
    // Tallest bar spans most of the disc's diameter.
    let max_height = base_radius * 1.15;
    let rounding = bar_width / 2.0;

    for (index, &rest) in BAR_HEIGHTS.iter().enumerate() {
        let factor = if animated {
            // Bounce around the resting height on a per-bar phase offset.
            let wave = (time * 7.0 + index as f64 * 1.3).sin() as f32;
            (rest + wave * 0.28).clamp(0.16, 1.0)
        } else {
            rest
        };

        let half = (factor * max_height / 2.0).max(rounding);
        let x = left + index as f32 * (bar_width + gap);
        let bar = Rect::from_center_size(pos2(x, center.y), vec2(bar_width, half * 2.0));
        painter.rect_filled(bar, rounding, color);
    }
}
