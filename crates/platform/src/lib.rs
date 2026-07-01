mod clipboard;
mod dictate_runtime;
mod display;
mod hotkeys;
mod notifications;
mod playback;
mod text_insertion;

pub use clipboard::read_text as read_clipboard_text;
pub use dictate_runtime::{
    DictateCompletion, DictateState, DictateStatus, DictateToggleResult, PendingDictation,
    capture_microphone_until_enter, dictate_start, dictate_status, dictate_stop, dictate_toggle,
};
pub use display::{SessionKind, is_wayland_session, session_kind};
pub use hotkeys::{RegisteredHotkeys, next_hotkey_press, parse_binding, register_hotkeys};
pub use notifications::notify;
pub use playback::AudioPlayer;
pub use text_insertion::insert_text_into_focused_app;
