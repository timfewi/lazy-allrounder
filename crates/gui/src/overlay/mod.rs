//! The floating overlay: a badge window that expands into a panel on click.

mod badge;
mod panel;
pub mod theme;
pub mod viewport;

use std::time::Duration;

use eframe::NativeOptions;
use egui::{Context, Id, ViewportCommand};
use lazy_allrounder_core::config::OverlayCorner;

use crate::hotkeys::HotkeyEvents;
use crate::session::Session;
use crate::state::{Activity, Mode, OverlayState};

const EXPAND_SECONDS: f32 = 0.22;

pub struct OverlayApp {
    state: OverlayState,
    session: Option<Session>,
    hotkeys: Option<HotkeyEvents>,
    startup_error: Option<String>,
    corner: OverlayCorner,
    question: String,
    openness: f32,
}

impl OverlayApp {
    pub fn new(
        session: Option<Session>,
        hotkeys: Option<HotkeyEvents>,
        startup_error: Option<String>,
        corner: OverlayCorner,
    ) -> Self {
        Self {
            state: OverlayState::new(),
            session,
            hotkeys,
            startup_error,
            corner,
            question: String::new(),
            openness: 0.0,
        }
    }

    fn trigger(&mut self, mode: Mode, now: f64) {
        let Some(session) = &self.session else {
            self.state.begin(mode);
            self.state.finish(
                mode,
                Err(self
                    .startup_error
                    .clone()
                    .unwrap_or_else(|| "the app is not configured yet".to_owned())),
                now,
            );
            return;
        };

        if !self.state.begin(mode) {
            return;
        }

        let question = (mode == Mode::Ask).then(|| self.question.clone());
        session.dispatch(mode, question);
    }
}

impl eframe::App for OverlayApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|input| input.time);

        if let Some(session) = &self.session {
            while let Some(outcome) = session.poll() {
                self.state.finish(outcome.mode, outcome.result, now);
            }
        }
        self.state.tick(now);

        // Hotkey-triggered actions: Ask needs a typed question, so its
        // hotkey opens the panel instead of firing blindly.
        while let Some(mode) = self.hotkeys.as_ref().and_then(HotkeyEvents::poll) {
            if mode == Mode::Ask {
                self.state.panel_open = true;
            } else if self.state.is_busy() {
                // Pressing a hotkey while an action runs stops the audio —
                // the same toggle feel as the old GNOME bindings.
                if let Some(session) = &self.session {
                    session.stop_playback();
                }
            } else {
                self.trigger(mode, now);
            }
        }

        self.openness = ctx.animate_bool_with_time(
            Id::new("panel-openness"),
            self.state.panel_open,
            EXPAND_SECONDS,
        );
        viewport::apply_geometry(ctx, self.corner, self.openness);

        // Click-away: closing when the window loses focus while expanded.
        if self.state.panel_open && ctx.input(|input| input.viewport().focused) == Some(false) {
            self.state.close_panel();
        }

        // Keep animating the pulse while an action runs or a result lingers;
        // stay quiescent (no repaints) when idle and collapsed.
        if self.state.activity != Activity::Idle {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|input| input.time);

        let mut panel_response = panel::PanelResponse::default();
        if self.openness < 0.05 {
            if badge::draw(ui, &self.state, now).clicked() {
                self.state.toggle_panel();
            }
        } else {
            theme::panel_frame().show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                panel_response = panel::draw(
                    ui,
                    &self.state,
                    &mut self.question,
                    self.startup_error.as_deref(),
                );
            });
        }

        if let Some(mode) = panel_response.trigger {
            self.trigger(mode, now);
        }
        if panel_response.stop
            && let Some(session) = &self.session
        {
            session.stop_playback();
        }
        if panel_response.close {
            self.state.close_panel();
        }
        if panel_response.quit {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
}

pub fn run(
    application: Option<lazy_allrounder_app::Application>,
    startup_error: Option<String>,
    corner: OverlayCorner,
    hotkeys_config: lazy_allrounder_core::config::HotkeysConfiguration,
    player: lazy_allrounder_platform::AudioPlayer,
) -> eframe::Result<()> {
    let options = NativeOptions {
        viewport: viewport::initial_viewport(),
        ..Default::default()
    };

    eframe::run_native(
        "lazy-allrounder",
        options,
        Box::new(move |creation_context| {
            creation_context.egui_ctx.set_visuals(egui::Visuals::dark());

            // The session's completion notifications must wake the event loop
            // even when the window is idle, so it needs the live egui context.
            let session = application.map(|application| {
                let repaint_ctx = creation_context.egui_ctx.clone();
                Session::spawn(application, player, move || repaint_ctx.request_repaint())
            });
            let hotkeys = crate::hotkeys::start(&hotkeys_config, creation_context.egui_ctx.clone());

            Ok(Box::new(OverlayApp::new(
                session,
                hotkeys,
                startup_error,
                corner,
            )))
        }),
    )
}
