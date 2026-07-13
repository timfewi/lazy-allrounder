//! The collapsed floating badge: a circular button carrying the app's logo
//! mark, ringed by a status color that pulses while working. The badge senses
//! drags so the window can be moved (the caller turns a drag into an OS window
//! move).

use egui::{Color32, Rect, Response, Sense, Stroke, TextureHandle, Ui, Vec2, pos2};

use crate::overlay::theme;
use crate::state::{Activity, OverlayState};

pub fn draw(ui: &mut Ui, state: &OverlayState, time: f64) -> Response {
    let size = ui.available_size().min(Vec2::splat(56.0));
    // click_and_drag so the caller can distinguish a tap (toggle the panel)
    // from a drag (move the window).
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter();
    let center = rect.center();
    // Ease the badge slightly larger under the pointer so it answers hover with
    // motion, not just a color change.
    let grow =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover-grow"), response.hovered(), 0.12);
    let base_radius = (rect.width().min(rect.height()) * 0.5 - 4.0) * (0.95 + 0.05 * grow);

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
                2.0_f32,
                Color32::from_rgba_unmultiplied(
                    ring_color.r(),
                    ring_color.g(),
                    ring_color.b(),
                    alpha,
                ),
            ),
        );
    }

    // The logo mark itself, in true color; a corrupt embedded asset degrades to
    // a filled accent disc so the badge stays visible.
    match logo_texture(ui.ctx()) {
        Some(texture) => {
            let mark = Rect::from_center_size(center, Vec2::splat(base_radius * 2.0));
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.id(), mark, uv, Color32::WHITE);
        }
        None => {
            painter.circle_filled(center, base_radius * 0.8, theme::ACCENT);
        }
    }

    // Status ring hugging the mark: accent while idle, activity color otherwise.
    painter.circle_stroke(center, base_radius, Stroke::new(2.0_f32, ring_color));

    if let Some(status) = state.status_line() {
        response.clone().on_hover_text(status);
    }

    response
}

/// Uploads the embedded logo to a GPU texture once and caches the handle in
/// egui memory, so the badge does not re-upload the image every frame.
fn logo_texture(ctx: &egui::Context) -> Option<TextureHandle> {
    let id = egui::Id::new("lazy-allrounder-badge-logo");
    if let Some(handle) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(handle);
    }
    let icon = crate::icon::decode()?;
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    let handle = ctx.load_texture("badge-logo", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    Some(handle)
}
