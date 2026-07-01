//! Display-server/session detection, shared by focused-app insertion and the
//! GUI's mode selection.

use std::env;
use std::ffi::OsString;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Wayland,
    X11,
}

/// The graphical session kind, if one is detectable from the environment.
/// Always None on platforms without X11/Wayland (macOS, Windows).
pub fn session_kind() -> Option<SessionKind> {
    detect(env::var_os("WAYLAND_DISPLAY"), env::var_os("DISPLAY"))
}

pub fn is_wayland_session() -> bool {
    matches!(session_kind(), Some(SessionKind::Wayland))
}

pub(crate) fn detect(
    wayland_display: Option<OsString>,
    x11_display: Option<OsString>,
) -> Option<SessionKind> {
    if wayland_display.is_some_and(|value| !value.is_empty()) {
        return Some(SessionKind::Wayland);
    }
    if x11_display.is_some_and(|value| !value.is_empty()) {
        return Some(SessionKind::X11);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_wayland_when_both_displays_exist() {
        let kind = detect(Some("wayland-0".into()), Some(":0".into()));
        assert_eq!(kind, Some(SessionKind::Wayland));
    }

    #[test]
    fn falls_back_to_x11() {
        let kind = detect(None, Some(":0".into()));
        assert_eq!(kind, Some(SessionKind::X11));
    }

    #[test]
    fn empty_values_count_as_absent() {
        assert_eq!(detect(Some("".into()), Some("".into())), None);
        assert_eq!(detect(None, None), None);
    }
}
