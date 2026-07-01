//! The floating overlay: a badge window that expands into a panel on click.

mod badge;
mod panel;
pub mod theme;
pub mod viewport;

use std::time::Duration;

use eframe::NativeOptions;
use egui::{Context, Id, ViewportCommand};
use lazy_allrounder_app::{AppError, Application};
use lazy_allrounder_core::config::OverlayCorner;
use lazy_allrounder_platform::AudioPlayer;

use crate::hotkeys::HotkeyEvents;
use crate::session::Session;
use crate::state::{Activity, Mode, OverlayState};

const EXPAND_SECONDS: f32 = 0.22;

/// Whether the backing Application is usable, and if not, what the panel
/// should ask of the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupState {
    Ready,
    NeedsApiKey,
    Failed(String),
}

pub struct OverlayApp {
    state: OverlayState,
    session: Option<Session>,
    hotkeys: Option<HotkeyEvents>,
    startup: StartupState,
    corner: OverlayCorner,
    question: String,
    api_key_input: String,
    onboarding_error: Option<String>,
    autostart_enabled: bool,
    openness: f32,
    player: AudioPlayer,
}

impl OverlayApp {
    pub fn new(hotkeys: Option<HotkeyEvents>, corner: OverlayCorner, player: AudioPlayer) -> Self {
        let autostart_enabled =
            lazy_allrounder_platform::is_autostart_enabled().unwrap_or_else(|error| {
                tracing::warn!("could not check the start-on-login state: {error}");
                false
            });

        Self {
            state: OverlayState::new(),
            session: None,
            hotkeys,
            startup: StartupState::NeedsApiKey,
            corner,
            question: String::new(),
            api_key_input: String::new(),
            onboarding_error: None,
            autostart_enabled,
            openness: 0.0,
            player,
        }
    }

    /// (Re)creates the Application + session; called at startup and again
    /// after the user saves an API key through the panel.
    fn try_start_session(&mut self, ctx: &Context) {
        let outcome = lazy_allrounder_app::ensure_configuration_file(None)
            .and_then(|loaded| Application::from_loaded_configuration(&loaded));

        match outcome {
            Ok(application) => {
                let repaint_ctx = ctx.clone();
                self.session = Some(Session::spawn(
                    application,
                    self.player.clone(),
                    move || repaint_ctx.request_repaint(),
                ));
                self.startup = StartupState::Ready;
                self.onboarding_error = None;
            }
            Err(AppError::MissingApiKey) => {
                self.startup = StartupState::NeedsApiKey;
            }
            Err(error) => {
                self.startup = StartupState::Failed(error.to_string());
            }
        }
    }

    fn save_api_key(&mut self, ctx: &Context) {
        let api_key = self.api_key_input.trim().to_owned();
        if api_key.is_empty() {
            return;
        }

        match lazy_allrounder_app::store_api_key(&api_key) {
            Ok(()) => {
                self.api_key_input.clear();
                self.try_start_session(ctx);
                if self.startup == StartupState::Ready {
                    self.state.panel_open = true;
                }
            }
            Err(error) => {
                self.onboarding_error = Some(error.to_string());
            }
        }
    }

    fn set_autostart(&mut self, enabled: bool) {
        match lazy_allrounder_platform::set_autostart(enabled) {
            Ok(()) => self.autostart_enabled = enabled,
            Err(error) => {
                tracing::warn!("could not update start-on-login: {error}");
                self.onboarding_error = Some(error.to_string());
            }
        }
    }

    fn trigger(&mut self, mode: Mode, now: f64) {
        let Some(session) = &self.session else {
            let message = match &self.startup {
                StartupState::NeedsApiKey => {
                    "add your OpenRouter API key in the panel first".to_owned()
                }
                StartupState::Failed(message) => message.clone(),
                StartupState::Ready => "the app is still starting".to_owned(),
            };
            self.state.begin(mode);
            self.state.finish(mode, Err(message), now);
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
                    panel::PanelInputs {
                        state: &self.state,
                        startup: &self.startup,
                        question: &mut self.question,
                        api_key_input: &mut self.api_key_input,
                        onboarding_error: self.onboarding_error.as_deref(),
                        autostart_enabled: self.autostart_enabled,
                    },
                );
            });
        }

        if let Some(mode) = panel_response.trigger {
            self.trigger(mode, now);
        }
        if panel_response.save_key {
            self.save_api_key(&ctx);
        }
        if let Some(enabled) = panel_response.set_autostart {
            self.set_autostart(enabled);
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
    corner: OverlayCorner,
    hotkeys_config: lazy_allrounder_core::config::HotkeysConfiguration,
    player: AudioPlayer,
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

            let hotkeys = crate::hotkeys::start(&hotkeys_config, creation_context.egui_ctx.clone());
            let mut app = OverlayApp::new(hotkeys, corner, player);
            // The session's completion notifications must wake the event loop
            // even when the window is idle, so it needs the live egui context.
            app.try_start_session(&creation_context.egui_ctx);

            Ok(Box::new(app))
        }),
    )
}
