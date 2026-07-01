//! GUI-side global hotkey pump.
//!
//! Registration lives in `platform::hotkeys`; this module runs the event
//! loop glue: a background thread blocks on hotkey presses, translates them
//! to modes, and wakes the egui event loop so `OverlayApp::logic` can pick
//! them up even while the window is idle and unfocused.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use lazy_allrounder_core::config::HotkeysConfiguration;
use lazy_allrounder_platform::{next_hotkey_press, register_hotkeys};

use crate::state::Mode;

pub struct HotkeyEvents {
    incoming: Receiver<Mode>,
}

impl HotkeyEvents {
    pub fn poll(&self) -> Option<Mode> {
        self.incoming.try_recv().ok()
    }
}

/// Registers the configured hotkeys and starts the pump thread. Returns None
/// where global hotkeys are unsupported (e.g. Wayland) — the overlay stays
/// fully usable via mouse, and desktop-native shortcuts can call the CLI.
pub fn start(config: &HotkeysConfiguration, ctx: egui::Context) -> Option<HotkeyEvents> {
    let bindings = config.enabled_bindings();
    if bindings.is_empty() {
        tracing::info!("all hotkeys are disabled in the config");
        return None;
    }

    let registered = match register_hotkeys(&bindings) {
        Ok(registered) => registered,
        Err(error) => {
            tracing::warn!("global hotkeys are disabled: {error}");
            return None;
        }
    };

    let (sender, incoming): (Sender<Mode>, Receiver<Mode>) = channel();

    thread::Builder::new()
        .name("lazy-allrounder-hotkeys".to_owned())
        .spawn(move || {
            // `registered` moves in so the OS registration lives exactly as
            // long as the pump.
            while let Some(hotkey_id) = next_hotkey_press() {
                let Some(mode) = registered.action_for(hotkey_id).and_then(mode_for_action) else {
                    continue;
                };
                if sender.send(mode).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        })
        .expect("failed to spawn the hotkey pump thread");

    Some(HotkeyEvents { incoming })
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
