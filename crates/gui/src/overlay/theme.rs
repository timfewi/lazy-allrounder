//! Shared colors and panel chrome for the overlay.

use egui::{Color32, CornerRadius, Shadow, Stroke};

pub const ACCENT: Color32 = Color32::from_rgb(0x4A, 0x9E, 0xE0);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x2E, 0x6D, 0xA3);
pub const SUCCESS: Color32 = Color32::from_rgb(0x4C, 0xC3, 0x8A);
pub const FAILURE: Color32 = Color32::from_rgb(0xE0, 0x5A, 0x5A);
pub const SURFACE: Color32 = Color32::from_rgba_premultiplied(24, 26, 32, 242);
pub const SURFACE_RAISED: Color32 = Color32::from_rgb(38, 41, 50);
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xEC, 0xEE, 0xF2);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x9A, 0xA1, 0xAD);

// The root `App::ui` surface already has no background, so the collapsed
// badge needs no frame at all; only the expanded panel draws chrome.
pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(16))
        .stroke(Stroke::new(
            1.0_f32,
            Color32::from_rgba_premultiplied(255, 255, 255, 18),
        ))
        .shadow(Shadow {
            offset: [0, 4],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(120),
        })
        .inner_margin(14.0)
}
