//! Window geometry: a small badge window that grows into a panel, anchored
//! to a screen corner so the badge corner stays fixed while expanding.

use egui::{Context, Pos2, Vec2, ViewportBuilder, ViewportCommand, pos2, vec2};
use lazy_allrounder_core::config::OverlayCorner;

pub const BADGE_SIZE: Vec2 = vec2(56.0, 56.0);
pub const PANEL_SIZE: Vec2 = vec2(280.0, 380.0);
const SCREEN_MARGIN: f32 = 18.0;

pub fn initial_viewport() -> ViewportBuilder {
    let builder = ViewportBuilder::default()
        .with_title("Lazy Allrounder")
        .with_inner_size(BADGE_SIZE)
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_resizable(false)
        .with_taskbar(false);

    #[cfg(target_os = "macos")]
    let builder = builder.with_has_shadow(false);

    builder
}

/// Linear size between badge and panel for the current animation factor.
pub fn current_size(openness: f32) -> Vec2 {
    vec2(
        egui::lerp(BADGE_SIZE.x..=PANEL_SIZE.x, openness),
        egui::lerp(BADGE_SIZE.y..=PANEL_SIZE.y, openness),
    )
}

/// Top-left outer position that keeps the anchored corner fixed for a window
/// of `size` on a monitor of `monitor` points.
pub fn anchored_position(corner: OverlayCorner, monitor: Vec2, size: Vec2) -> Pos2 {
    let x = match corner {
        OverlayCorner::TopLeft | OverlayCorner::BottomLeft => SCREEN_MARGIN,
        OverlayCorner::TopRight | OverlayCorner::BottomRight => monitor.x - size.x - SCREEN_MARGIN,
    };
    let y = match corner {
        OverlayCorner::TopLeft | OverlayCorner::TopRight => SCREEN_MARGIN,
        OverlayCorner::BottomLeft | OverlayCorner::BottomRight => {
            monitor.y - size.y - SCREEN_MARGIN
        }
    };

    pos2(x, y)
}

/// Resizes/repositions the window each frame while animating. On Wayland the
/// position command is a compositor no-op and `monitor_size` may be absent —
/// the size change still applies, so the UI remains usable there.
pub fn apply_geometry(ctx: &Context, corner: OverlayCorner, openness: f32) {
    let size = current_size(openness);
    ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));

    let monitor = ctx.input(|input| input.viewport().monitor_size);
    if let Some(monitor) = monitor
        && monitor.x > 0.0
        && monitor.y > 0.0
    {
        let position = anchored_position(corner, monitor, size);
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(position));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: Vec2 = vec2(1920.0, 1080.0);

    #[test]
    fn size_interpolates_between_badge_and_panel() {
        assert_eq!(current_size(0.0), BADGE_SIZE);
        assert_eq!(current_size(1.0), PANEL_SIZE);
        let half = current_size(0.5);
        assert!(half.x > BADGE_SIZE.x && half.x < PANEL_SIZE.x);
    }

    #[test]
    fn bottom_right_anchor_keeps_bottom_right_fixed_while_growing() {
        let badge = anchored_position(OverlayCorner::BottomRight, MONITOR, BADGE_SIZE);
        let panel = anchored_position(OverlayCorner::BottomRight, MONITOR, PANEL_SIZE);

        assert_eq!(badge.x + BADGE_SIZE.x, panel.x + PANEL_SIZE.x);
        assert_eq!(badge.y + BADGE_SIZE.y, panel.y + PANEL_SIZE.y);
    }

    #[test]
    fn top_left_anchor_is_margin_offset_regardless_of_size() {
        let badge = anchored_position(OverlayCorner::TopLeft, MONITOR, BADGE_SIZE);
        let panel = anchored_position(OverlayCorner::TopLeft, MONITOR, PANEL_SIZE);
        assert_eq!(badge, panel);
    }

    #[test]
    fn windows_stay_on_screen_for_every_corner() {
        for corner in [
            OverlayCorner::TopLeft,
            OverlayCorner::TopRight,
            OverlayCorner::BottomLeft,
            OverlayCorner::BottomRight,
        ] {
            let position = anchored_position(corner, MONITOR, PANEL_SIZE);
            assert!(position.x >= 0.0 && position.y >= 0.0, "{corner:?}");
            assert!(position.x + PANEL_SIZE.x <= MONITOR.x, "{corner:?}");
            assert!(position.y + PANEL_SIZE.y <= MONITOR.y, "{corner:?}");
        }
    }
}
