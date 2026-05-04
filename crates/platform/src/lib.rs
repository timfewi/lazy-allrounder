mod dictate_runtime;
mod text_insertion;

pub use dictate_runtime::{
    DictateCompletion, DictateState, DictateStatus, DictateToggleResult, PendingDictation,
    capture_microphone_until_enter, dictate_start, dictate_status, dictate_stop, dictate_toggle,
};
pub use text_insertion::insert_text_into_focused_app;
