mod autostart;
mod clipboard;
#[cfg(target_os = "linux")]
mod desktop_entry;
mod dictate_runtime;
mod display;
mod hotkeys;
mod ipc;
mod notifications;
mod playback;
mod secrets;
mod text_insertion;

/// The window's Wayland `app_id` / X11 `WM_CLASS` and the basename of its
/// `.desktop` entry. Matches the entry the .deb/AppImage packaging ships
/// (derived from the `lazy-allrounder-gui` package name), so the desktop can
/// associate the running window with that entry on every install path.
pub const APP_ID: &str = "lazy-allrounder-gui";

pub use autostart::{is_autostart_enabled, set_autostart};
pub use clipboard::read_selection_or_clipboard as read_selection_or_clipboard_text;
pub use clipboard::read_text as read_clipboard_text;
#[cfg(target_os = "linux")]
pub use desktop_entry::install_desktop_integration;
pub use dictate_runtime::{
    DictateCompletion, DictateState, DictateStatus, DictateToggleResult, PendingDictation,
    capture_microphone_until_enter, dictate_start, dictate_status, dictate_stop, dictate_toggle,
};
pub use display::{SessionKind, is_wayland_session, session_kind};
pub use hotkeys::{RegisteredHotkeys, next_hotkey_press, parse_binding, register_hotkeys};
pub use ipc::{GuiAction, GuiCommand, SendError, send_gui_command};
#[cfg(unix)]
pub use ipc::{GuiCommandListener, gui_socket_path};
pub use notifications::notify;
pub use playback::AudioPlayer;
pub use secrets::{delete_api_key, load_api_key, store_api_key};
pub use text_insertion::insert_text_into_focused_app;
