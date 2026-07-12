//! GUI-side global hotkey pump.
//!
//! Registration lives in `platform::hotkeys`; this module runs the event
//! loop glue: a background thread blocks on hotkey presses, translates them
//! to [`UiEvent`]s, and wakes the egui event loop so `OverlayApp::logic` can
//! pick them up even while the window is idle and unfocused.

use std::sync::mpsc::Sender;
use std::thread;

use lazy_allrounder_core::config::HotkeysConfiguration;
use lazy_allrounder_platform::{next_hotkey_press, register_hotkeys};

use crate::state::{Mode, UiEvent};

/// Registers the configured hotkeys and starts the pump thread. A no-op
/// where global hotkeys are unsupported (e.g. Wayland) — the overlay stays
/// fully usable via mouse and the control socket, and desktop-native
/// shortcuts call the CLI instead.
pub fn start(config: &HotkeysConfiguration, ctx: egui::Context, events: Sender<UiEvent>) {
    let bindings = config.enabled_bindings();
    if bindings.is_empty() {
        tracing::info!("all hotkeys are disabled in the config");
        return;
    }

    // The manager stays registered on this thread for the app's lifetime
    // (it is not `Send` on Windows); only the id→action map crosses into
    // the pump thread.
    let actions = match register_hotkeys(&bindings) {
        Ok(registered) => registered.leak(),
        Err(error) => {
            tracing::warn!("global hotkeys are disabled: {error}");
            return;
        }
    };

    thread::Builder::new()
        .name("lazy-allrounder-hotkeys".to_owned())
        .spawn(move || {
            while let Some(hotkey_id) = next_hotkey_press() {
                let Some(mode) = actions
                    .get(&hotkey_id)
                    .and_then(|action| mode_for_action(action))
                else {
                    continue;
                };
                if events.send(UiEvent::Trigger(mode)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        })
        .expect("failed to spawn the hotkey pump thread");
}

fn mode_for_action(action: &str) -> Option<Mode> {
    match action {
        "read" => Some(Mode::Read),
        "summarize" => Some(Mode::Summarize),
        "explain" => Some(Mode::Explain),
        "ask" => Some(Mode::Ask),
        "dictate" => Some(Mode::Dictate),
        _ => None,
    }
}
