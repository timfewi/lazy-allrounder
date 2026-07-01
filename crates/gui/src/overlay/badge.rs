//! The collapsed floating badge: a circular logo button whose ring color and
//! pulse reflect the current activity.

use egui::{Color32, FontId, Response, Sense, Stroke, Ui, Vec2};

use crate::overlay::theme;
use crate::state::{Activity, OverlayState};

pub fn draw(ui: &mut Ui, state: &OverlayState, time: f64) -> Response {
    let size = ui.available_size().min(Vec2::splat(56.0));
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    let center = rect.center();
    let base_radius = rect.width().min(rect.height()) * 0.5 - 4.0;

    let (ring_color, pulse) = match &state.activity {
        Activity::Idle => (theme::ACCENT_SOFT, 0.0),
        Activity::Processing { .. } => {
            let pulse = ((time * 4.0).sin() * 0.5 + 0.5) as f32;
            (theme::ACCENT, pulse)
        }
        Activity::Done { .. } => (theme::SUCCESS, 0.0),
        Activity::Error { .. } => (theme::FAILURE, 0.0),
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

    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        "L",
        FontId::proportional(base_radius),
        theme::TEXT_PRIMARY,
    );

    if let Some(status) = state.status_line() {
        response.clone().on_hover_text(status);
    }

    response
}
