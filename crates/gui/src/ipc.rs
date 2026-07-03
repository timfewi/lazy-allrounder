//! GUI-side control socket pump.
//!
//! Mirrors the hotkey pump: a background thread serves the Unix control
//! socket (`lazy-allrounder gui …` on the CLI side), translates commands to
//! [`UiEvent`]s, and wakes the egui event loop. This is the only trigger
//! path that works on GNOME Wayland, where the compositor swallows global
//! keys before any app sees them — desktop shortcuts run the CLI, and the
//! CLI talks to this socket.

use std::sync::mpsc::Sender;
#[cfg(unix)]
use std::thread;

#[cfg(unix)]
use lazy_allrounder_platform::{GuiAction, GuiCommand, GuiCommandListener};

#[cfg(unix)]
use crate::state::Mode;
use crate::state::UiEvent;

/// The control socket is Unix-only; elsewhere desktop shortcuts and global
/// hotkeys are the compositor's job anyway.
#[cfg(not(unix))]
pub fn start(_ctx: egui::Context, _events: Sender<UiEvent>) {}

#[cfg(unix)]
pub fn start(ctx: egui::Context, events: Sender<UiEvent>) {
    let listener = match GuiCommandListener::bind() {
        Ok(listener) => listener,
        Err(error) => {
            // Most likely a second GUI instance; the overlay still works by
            // mouse, so degrade instead of failing startup.
            tracing::warn!("CLI gui commands will not reach this instance: {error}");
            return;
        }
    };

    thread::Builder::new()
        .name("lazy-allrounder-ipc".to_owned())
        .spawn(move || {
            listener.run(|command| {
                let event = match command {
                    GuiCommand::TogglePanel => UiEvent::TogglePanel,
                    GuiCommand::Stop => UiEvent::Stop,
                    GuiCommand::Trigger(action) => UiEvent::Trigger(mode_for(action)),
                };
                if events.send(event).is_ok() {
                    ctx.request_repaint();
                }
            });
        })
        .expect("failed to spawn the ipc pump thread");
}

#[cfg(unix)]
fn mode_for(action: GuiAction) -> Mode {
    match action {
        GuiAction::Read => Mode::Read,
        GuiAction::Summarize => Mode::Summarize,
        GuiAction::Explain => Mode::Explain,
        GuiAction::Ask => Mode::Ask,
        GuiAction::Dictate => Mode::Dictate,
    }
}
