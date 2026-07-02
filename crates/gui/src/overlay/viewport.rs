//! Window geometry: a small badge window that grows into a panel. The badge
//! is user-moveable (OS drag), so the panel expands from wherever the badge
//! currently sits, growing toward the screen center.

use egui::{Context, Pos2, Rect, Vec2, ViewportBuilder, ViewportCommand, pos2, vec2};
use lazy_allrounder_core::config::OverlayCorner;

pub const BADGE_SIZE: Vec2 = vec2(56.0, 56.0);
pub const PANEL_SIZE: Vec2 = vec2(280.0, 380.0);
const SCREEN_MARGIN: f32 = 18.0;

pub fn initial_viewport() -> ViewportBuilder {
    let mut builder = ViewportBuilder::default()
        .with_title("Lazy Allrounder")
        .with_inner_size(BADGE_SIZE)
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_resizable(false)
        .with_taskbar(false);

    if let Some(icon) = crate::icon::window_icon() {
        builder = builder.with_icon(icon);
    }

    #[cfg(target_os = "macos")]
    {
        builder = builder.with_has_shadow(false);
    }

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

/// The screen quadrant the badge currently sits in, expressed as the corner
/// the panel should stay anchored to while expanding (so it grows toward the
/// screen center instead of off-screen).
pub fn quadrant_corner(badge_pos: Pos2, monitor: Vec2) -> OverlayCorner {
    let center = badge_pos + BADGE_SIZE / 2.0;
    match (center.x > monitor.x / 2.0, center.y > monitor.y / 2.0) {
        (false, false) => OverlayCorner::TopLeft,
        (true, false) => OverlayCorner::TopRight,
        (false, true) => OverlayCorner::BottomLeft,
        (true, true) => OverlayCorner::BottomRight,
    }
}

/// Top-left position for a window of `size` that keeps the badge's anchored
/// corner fixed at its current spot, clamped fully on-screen.
pub fn expanded_position(
    corner: OverlayCorner,
    badge_pos: Pos2,
    size: Vec2,
    monitor: Vec2,
) -> Pos2 {
    let x = match corner {
        OverlayCorner::TopLeft | OverlayCorner::BottomLeft => badge_pos.x,
        OverlayCorner::TopRight | OverlayCorner::BottomRight => badge_pos.x + BADGE_SIZE.x - size.x,
    };
    let y = match corner {
        OverlayCorner::TopLeft | OverlayCorner::TopRight => badge_pos.y,
        OverlayCorner::BottomLeft | OverlayCorner::BottomRight => {
            badge_pos.y + BADGE_SIZE.y - size.y
        }
    };

    pos2(
        x.clamp(0.0, (monitor.x - size.x).max(0.0)),
        y.clamp(0.0, (monitor.y - size.y).max(0.0)),
    )
}

/// Per-frame window geometry driver. Places the badge in the configured
/// corner once at startup, then leaves the position alone while collapsed so
/// OS drags stick; while animating or expanded it resizes and repositions
/// around the badge's last resting spot. On Wayland position commands are
/// compositor no-ops and `monitor_size`/`outer_rect` may be absent — the
/// size change still applies, so the UI remains usable there.
pub struct Geometry {
    corner: OverlayCorner,
    placed: bool,
    badge_pos: Option<Pos2>,
    prev_openness: f32,
}

impl Geometry {
    pub fn new(corner: OverlayCorner) -> Self {
        Self {
            corner,
            placed: false,
            badge_pos: None,
            prev_openness: 0.0,
        }
    }

    pub fn apply(&mut self, ctx: &Context, openness: f32) {
        let (monitor, outer_rect) = ctx.input(|input| {
            let viewport = input.viewport();
            (viewport.monitor_size, viewport.outer_rect)
        });
        let monitor = monitor.filter(|monitor| monitor.x > 0.0 && monitor.y > 0.0);

        if !self.placed
            && let Some(monitor) = monitor
        {
            let position = anchored_position(self.corner, monitor, BADGE_SIZE);
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(position));
            self.badge_pos = Some(position);
            self.placed = true;
            self.prev_openness = openness;
            return;
        }

        if openness <= 0.0 && self.prev_openness <= 0.0 {
            // Collapsed and quiescent: adopt wherever the user dragged the
            // badge, and send no commands so the OS move sticks.
            if self.placed
                && let Some(outer_rect) = outer_rect.filter(is_plausible_window_rect)
            {
                self.badge_pos = Some(outer_rect.min);
            }
            return;
        }

        let size = current_size(openness);
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
        if let (Some(badge_pos), Some(monitor)) = (self.badge_pos, monitor) {
            let corner = quadrant_corner(badge_pos, monitor);
            let position = expanded_position(corner, badge_pos, size, monitor);
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(position));
        }
        self.prev_openness = openness;
    }
}

/// Some backends briefly report a zero/negative rect during creation;
/// adopting that as the badge position would teleport the window.
fn is_plausible_window_rect(rect: &Rect) -> bool {
    rect.width() > 0.0
        && rect.height() > 0.0
        && rect.min.x > -PANEL_SIZE.x
        && rect.min.y > -PANEL_SIZE.y
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

    #[test]
    fn quadrant_corner_matches_where_the_badge_sits() {
        assert_eq!(
            quadrant_corner(pos2(10.0, 10.0), MONITOR),
            OverlayCorner::TopLeft
        );
        assert_eq!(
            quadrant_corner(pos2(1800.0, 10.0), MONITOR),
            OverlayCorner::TopRight
        );
        assert_eq!(
            quadrant_corner(pos2(10.0, 1000.0), MONITOR),
            OverlayCorner::BottomLeft
        );
        assert_eq!(
            quadrant_corner(pos2(1800.0, 1000.0), MONITOR),
            OverlayCorner::BottomRight
        );
    }

    #[test]
    fn expansion_keeps_the_badge_corner_fixed() {
        // Badge dragged into the bottom-right area: its bottom-right corner
        // must not move while the panel grows.
        let badge_pos = pos2(1400.0, 700.0);
        let corner = quadrant_corner(badge_pos, MONITOR);
        let panel = expanded_position(corner, badge_pos, PANEL_SIZE, MONITOR);

        assert_eq!(panel.x + PANEL_SIZE.x, badge_pos.x + BADGE_SIZE.x);
        assert_eq!(panel.y + PANEL_SIZE.y, badge_pos.y + BADGE_SIZE.y);
    }

    #[test]
    fn expansion_at_badge_size_returns_the_badge_position() {
        for badge_pos in [pos2(100.0, 100.0), pos2(1500.0, 900.0), pos2(900.0, 200.0)] {
            let corner = quadrant_corner(badge_pos, MONITOR);
            assert_eq!(
                expanded_position(corner, badge_pos, BADGE_SIZE, MONITOR),
                badge_pos
            );
        }
    }

    #[test]
    fn expansion_is_clamped_on_screen_from_extreme_positions() {
        // Badge dragged right up against the top-left corner while the
        // quadrant math wants to grow down-right: still fully on-screen.
        for badge_pos in [pos2(0.0, 0.0), pos2(1864.0, 1024.0), pos2(0.0, 1024.0)] {
            let corner = quadrant_corner(badge_pos, MONITOR);
            let panel = expanded_position(corner, badge_pos, PANEL_SIZE, MONITOR);
            assert!(panel.x >= 0.0 && panel.y >= 0.0, "{badge_pos:?}");
            assert!(panel.x + PANEL_SIZE.x <= MONITOR.x, "{badge_pos:?}");
            assert!(panel.y + PANEL_SIZE.y <= MONITOR.y, "{badge_pos:?}");
        }
    }
}
