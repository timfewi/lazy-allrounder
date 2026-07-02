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

/// Top-left origin of the monitor the badge currently sits on.
///
/// egui reports the badge position (`outer_rect`) in global virtual-desktop
/// coordinates, but only exposes the *size* of the current monitor, never its
/// offset. Assuming monitors tile edge-to-edge from the origin — the common
/// layout, and exact for a single monitor — the badge's position within its
/// monitor is `badge_pos.rem_euclid(monitor)`, which recovers the origin. On
/// an irregular layout (mixed resolutions, gaps) this is an approximation, but
/// it still keeps the expanding panel near the badge instead of teleporting it
/// back onto the primary monitor.
fn monitor_origin(badge_pos: Pos2, monitor: Vec2) -> Pos2 {
    pos2(
        badge_pos.x - badge_pos.x.rem_euclid(monitor.x),
        badge_pos.y - badge_pos.y.rem_euclid(monitor.y),
    )
}

/// The screen quadrant the badge currently sits in (relative to its own
/// monitor), expressed as the corner the panel should stay anchored to while
/// expanding (so it grows toward the monitor center instead of off-screen).
pub fn quadrant_corner(badge_pos: Pos2, monitor: Vec2) -> OverlayCorner {
    let origin = monitor_origin(badge_pos, monitor);
    let center = (badge_pos + BADGE_SIZE / 2.0) - origin;
    match (center.x > monitor.x / 2.0, center.y > monitor.y / 2.0) {
        (false, false) => OverlayCorner::TopLeft,
        (true, false) => OverlayCorner::TopRight,
        (false, true) => OverlayCorner::BottomLeft,
        (true, true) => OverlayCorner::BottomRight,
    }
}

/// Top-left position for a window of `size` that keeps the badge's anchored
/// corner fixed at its current spot, clamped fully onto the badge's own
/// monitor (not just the primary one).
pub fn expanded_position(
    corner: OverlayCorner,
    badge_pos: Pos2,
    size: Vec2,
    monitor: Vec2,
) -> Pos2 {
    let origin = monitor_origin(badge_pos, monitor);
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
        x.clamp(origin.x, (origin.x + monitor.x - size.x).max(origin.x)),
        y.clamp(origin.y, (origin.y + monitor.y - size.y).max(origin.y)),
    )
}

/// What a frame's geometry state implies the window should do — computed as a
/// pure function of the inputs (see [`Geometry::plan`]) so every transition is
/// unit-testable without a real window.
#[derive(Debug, Clone, PartialEq)]
enum Plan {
    /// Not enough information yet (no monitor known before first placement).
    Wait,
    /// First placement: move the badge to the configured corner.
    Place { position: Pos2 },
    /// Collapsed and settled: send nothing (so OS drags stick), optionally
    /// adopting the current window position as the badge's resting spot.
    Idle { adopt: Option<Pos2> },
    /// Animating or expanded: resize to `size`; reposition to `position` when
    /// a monitor is known (`None` on Wayland, where positioning is a no-op).
    Show { size: Vec2, position: Option<Pos2> },
}

/// Per-frame window geometry driver. Places the badge in the configured
/// corner once at startup, then leaves the position alone while collapsed so
/// OS drags stick; while animating or expanded it resizes and repositions
/// around the badge's last resting spot. Size/position commands are only sent
/// when their target actually changes, so a settled badge or panel never
/// re-arms a repaint (egui schedules an immediate repaint on every viewport
/// command). On Wayland position commands are compositor no-ops and
/// `monitor_size`/`outer_rect` may be absent — the size change still applies,
/// so the UI remains usable there.
pub struct Geometry {
    corner: OverlayCorner,
    placed: bool,
    badge_pos: Option<Pos2>,
    prev_openness: f32,
    last_size: Option<Vec2>,
    last_position: Option<Pos2>,
}

impl Geometry {
    pub fn new(corner: OverlayCorner) -> Self {
        Self {
            corner,
            placed: false,
            badge_pos: None,
            prev_openness: 0.0,
            last_size: None,
            last_position: None,
        }
    }

    /// Decides what the window should do this frame. Pure: depends only on the
    /// current state plus the frame's `monitor`/`outer_rect`/`openness`.
    fn plan(&self, monitor: Option<Vec2>, outer_rect: Option<Rect>, openness: f32) -> Plan {
        if !self.placed {
            return match monitor {
                Some(monitor) => Plan::Place {
                    position: anchored_position(self.corner, monitor, BADGE_SIZE),
                },
                None => Plan::Wait,
            };
        }

        if openness <= 0.0 && self.prev_openness <= 0.0 {
            let adopt = outer_rect
                .filter(is_plausible_window_rect)
                .map(|rect| rect.min);
            return Plan::Idle { adopt };
        }

        let size = current_size(openness);
        let position = match (self.badge_pos, monitor) {
            (Some(badge_pos), Some(monitor)) => {
                let corner = quadrant_corner(badge_pos, monitor);
                Some(expanded_position(corner, badge_pos, size, monitor))
            }
            _ => None,
        };
        Plan::Show { size, position }
    }

    pub fn apply(&mut self, ctx: &Context, openness: f32) {
        let (monitor, outer_rect) = ctx.input(|input| {
            let viewport = input.viewport();
            (viewport.monitor_size, viewport.outer_rect)
        });
        let monitor = monitor.filter(|monitor| monitor.x > 0.0 && monitor.y > 0.0);

        match self.plan(monitor, outer_rect, openness) {
            Plan::Wait => {}
            Plan::Place { position } => {
                self.send_position(ctx, position);
                self.badge_pos = Some(position);
                self.placed = true;
                self.prev_openness = openness;
            }
            Plan::Idle { adopt } => {
                // Collapsed and quiescent: adopt wherever the user dragged the
                // badge, and send no commands so the OS move sticks.
                if let Some(position) = adopt {
                    self.badge_pos = Some(position);
                }
            }
            Plan::Show { size, position } => {
                self.send_size(ctx, size);
                if let Some(position) = position {
                    self.send_position(ctx, position);
                }
                self.prev_openness = openness;
            }
        }
    }

    /// Sends an `InnerSize` only when the target changes; a settled window
    /// (fully open or fully closed) therefore issues nothing and stays
    /// quiescent instead of spinning at full frame rate.
    fn send_size(&mut self, ctx: &Context, size: Vec2) {
        if self.last_size != Some(size) {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
            self.last_size = Some(size);
        }
    }

    /// Sends an `OuterPosition` only when the target changes (same quiescence
    /// rationale as [`Self::send_size`]).
    fn send_position(&mut self, ctx: &Context, position: Pos2) {
        if self.last_position != Some(position) {
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(position));
            self.last_position = Some(position);
        }
    }
}

/// Some backends briefly report a zero-size rect during window creation;
/// adopting that as the badge position would teleport the window. Negative
/// coordinates are legitimate (a monitor placed left of / above the primary),
/// so only non-finite or empty rects are rejected.
fn is_plausible_window_rect(rect: &Rect) -> bool {
    rect.width() > 0.0 && rect.height() > 0.0 && rect.min.x.is_finite() && rect.min.y.is_finite()
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

    #[test]
    fn monitor_origin_is_zero_on_primary_and_offset_on_neighbors() {
        assert_eq!(monitor_origin(pos2(100.0, 100.0), MONITOR), pos2(0.0, 0.0));
        assert_eq!(
            monitor_origin(pos2(2000.0, 100.0), MONITOR),
            pos2(1920.0, 0.0)
        );
        assert_eq!(
            monitor_origin(pos2(-1500.0, 100.0), MONITOR),
            pos2(-1920.0, 0.0)
        );
    }

    #[test]
    fn expansion_stays_on_a_monitor_to_the_right() {
        // Badge dragged onto a second 1920-wide monitor right of the primary:
        // the panel must stay in [1920, 3840), never fold back onto primary.
        let badge_pos = pos2(2000.0, 200.0);
        let corner = quadrant_corner(badge_pos, MONITOR);
        let panel = expanded_position(corner, badge_pos, PANEL_SIZE, MONITOR);
        assert!(panel.x >= 1920.0, "panel.x={} folded onto primary", panel.x);
        assert!(panel.x + PANEL_SIZE.x <= 3840.0, "{panel:?}");
    }

    #[test]
    fn expansion_stays_on_a_monitor_to_the_left() {
        // Second monitor left of the primary occupies negative coordinates.
        let badge_pos = pos2(-1500.0, 200.0);
        let corner = quadrant_corner(badge_pos, MONITOR);
        let panel = expanded_position(corner, badge_pos, PANEL_SIZE, MONITOR);
        assert!(panel.x >= -1920.0, "{panel:?}");
        assert!(
            panel.x + PANEL_SIZE.x <= 0.0,
            "panel right edge {} spilled onto primary",
            panel.x + PANEL_SIZE.x
        );
    }

    #[test]
    fn plan_waits_for_a_monitor_then_places_in_the_corner() {
        let geometry = Geometry::new(OverlayCorner::BottomRight);
        assert_eq!(geometry.plan(None, None, 0.0), Plan::Wait);
        assert_eq!(
            geometry.plan(Some(MONITOR), None, 0.0),
            Plan::Place {
                position: anchored_position(OverlayCorner::BottomRight, MONITOR, BADGE_SIZE),
            }
        );
    }

    #[test]
    fn plan_stays_hands_off_while_collapsed_and_adopts_a_dragged_position() {
        let mut geometry = Geometry::new(OverlayCorner::TopLeft);
        geometry.placed = true;
        geometry.badge_pos = Some(pos2(20.0, 20.0));

        // A plausible dragged rect is adopted; no commands are implied.
        let dragged = Rect::from_min_size(pos2(640.0, 480.0), BADGE_SIZE);
        assert_eq!(
            geometry.plan(Some(MONITOR), Some(dragged), 0.0),
            Plan::Idle {
                adopt: Some(pos2(640.0, 480.0))
            }
        );

        // A degenerate (empty) rect is ignored rather than teleporting.
        let bogus = Rect::from_min_size(pos2(0.0, 0.0), Vec2::ZERO);
        assert_eq!(
            geometry.plan(Some(MONITOR), Some(bogus), 0.0),
            Plan::Idle { adopt: None }
        );
    }

    #[test]
    fn plan_resizes_around_the_badge_while_expanding() {
        let mut geometry = Geometry::new(OverlayCorner::TopLeft);
        geometry.placed = true;
        geometry.badge_pos = Some(pos2(1400.0, 700.0));

        let corner = quadrant_corner(pos2(1400.0, 700.0), MONITOR);
        assert_eq!(
            geometry.plan(Some(MONITOR), None, 1.0),
            Plan::Show {
                size: PANEL_SIZE,
                position: Some(expanded_position(
                    corner,
                    pos2(1400.0, 700.0),
                    PANEL_SIZE,
                    MONITOR
                )),
            }
        );
    }

    #[test]
    fn plan_expands_in_place_without_a_monitor() {
        // Wayland: monitor size is absent, so resize but issue no position.
        let mut geometry = Geometry::new(OverlayCorner::TopLeft);
        geometry.placed = true;
        geometry.badge_pos = Some(pos2(10.0, 10.0));
        assert_eq!(
            geometry.plan(None, None, 1.0),
            Plan::Show {
                size: PANEL_SIZE,
                position: None,
            }
        );
    }
}
