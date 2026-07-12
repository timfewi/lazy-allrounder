//! The floating overlay: a badge window that expands into a panel on click.

mod badge;
mod panel;
pub mod theme;
pub mod viewport;

use std::sync::mpsc::Receiver;
use std::time::Duration;

use eframe::NativeOptions;
use egui::{Context, Id, ViewportCommand, WindowLevel};
use lazy_allrounder_app::{AppError, Application};
use lazy_allrounder_core::config::{
    AppConfiguration, OverlayCorner, clamp_tts_speed, round_tts_speed,
};
use lazy_allrounder_platform::AudioPlayer;

use crate::session::Session;
use crate::state::{Activity, Mode, OverlayState, UiEvent};

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
    /// External inputs: global hotkeys and CLI control-socket commands.
    events: Receiver<UiEvent>,
    startup: StartupState,
    geometry: viewport::Geometry,
    question: String,
    api_key_input: String,
    onboarding_error: Option<String>,
    autostart_enabled: bool,
    speech_speed: f32,
    /// The last speed actually persisted + applied. The panel reports the
    /// slider value every idle frame; only a genuine difference from this
    /// commits, which makes egui's per-draw value rewrites harmless.
    committed_speed: f32,
    openness: f32,
    was_focused: Option<bool>,
    was_occluded: Option<bool>,
    /// A panel opened by an event (hotkey/CLI) starts unfocused; click-away
    /// close stays disarmed until the window has actually gained focus once,
    /// or the panel would slam shut before the focus grab lands. Sticky on
    /// purpose: if the compositor refuses focus, not auto-closing is the
    /// lesser evil.
    suppress_click_away: bool,
    /// One-shot request: the Ask field grabs keyboard focus on the next
    /// frame that actually draws the panel.
    focus_question_field: bool,
    player: AudioPlayer,
}

impl OverlayApp {
    pub fn new(
        events: Receiver<UiEvent>,
        corner: OverlayCorner,
        player: AudioPlayer,
        speech_speed: Option<f32>,
    ) -> Self {
        let autostart_enabled =
            lazy_allrounder_platform::is_autostart_enabled().unwrap_or_else(|error| {
                tracing::warn!("could not check the start-on-login state: {error}");
                false
            });

        // Rounded to the slider's own display precision so the first panel
        // draw (egui re-writes slider values through clamp + max_decimals
        // every frame) is a no-op instead of a phantom "change".
        let speech_speed = round_tts_speed(clamp_tts_speed(speech_speed).unwrap_or(1.0));

        Self {
            state: OverlayState::new(),
            session: None,
            events,
            startup: StartupState::NeedsApiKey,
            geometry: viewport::Geometry::new(corner),
            question: String::new(),
            api_key_input: String::new(),
            onboarding_error: None,
            autostart_enabled,
            speech_speed,
            committed_speed: speech_speed,
            openness: 0.0,
            was_focused: None,
            was_occluded: None,
            suppress_click_away: false,
            focus_question_field: false,
            player,
        }
    }

    /// Opens the panel in response to an external event (hotkey or CLI
    /// command). Unlike a badge click, the window may be unfocused, so grab
    /// focus (keys should land in the panel) and disarm click-away until the
    /// grab is observed.
    fn open_panel_via_event(&mut self, ctx: &Context) {
        self.state.panel_open = true;
        self.suppress_click_away = true;
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    /// The one close path: every way of closing the panel must also clear
    /// the event-open bookkeeping, or a stale flag would leak into the next
    /// open.
    fn close_panel(&mut self) {
        self.state.close_panel();
        self.suppress_click_away = false;
        self.focus_question_field = false;
    }

    fn stop_playback(&self) {
        if let Some(session) = &self.session {
            session.stop_playback();
        }
    }

    fn spawn_session(&mut self, application: Application, ctx: &Context) {
        let repaint_ctx = ctx.clone();
        self.session = Some(Session::spawn(
            application,
            self.player.clone(),
            move || repaint_ctx.request_repaint(),
        ));
    }

    /// The one recipe for constructing an `Application` from the on-disk
    /// configuration — startup, key onboarding, and speed changes must not
    /// drift apart in how they build it.
    fn build_application() -> Result<Application, AppError> {
        lazy_allrounder_app::ensure_configuration_file(None)
            .and_then(|loaded| Application::from_loaded_configuration(&loaded))
    }

    /// (Re)creates the Application + session; called at startup and again
    /// after the user saves an API key through the panel.
    fn try_start_session(&mut self, ctx: &Context) {
        match Self::build_application() {
            Ok(application) => {
                self.spawn_session(application, ctx);
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

    /// Persists a new speaking speed and swaps in a session at the new pace
    /// (the TTS client bakes the speed in at construction). Callers gate on
    /// `!is_busy()`, so no in-flight work is dropped by the swap. A failed
    /// save keeps the running session and snaps the slider back to the
    /// committed value, so the display never lies about the pace in use.
    fn set_speech_speed(&mut self, speed: f32, ctx: &Context) {
        if let Err(error) = lazy_allrounder_app::store_tts_speed(speed) {
            tracing::warn!("could not save the voice speed: {error}");
            self.speech_speed = self.committed_speed;
            return;
        }
        self.speech_speed = speed;
        self.committed_speed = speed;

        // Swap only when the replacement can actually be built; a transient
        // failure (keyring hiccup, config race) keeps the old session alive
        // instead of tearing the panel down to onboarding.
        match Self::build_application() {
            Ok(application) => self.spawn_session(application, ctx),
            Err(error) => tracing::warn!(
                "voice speed saved, but the session could not be rebuilt \
                 (the old pace stays active until the next action succeeds): {error}"
            ),
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

        // External inputs (global hotkeys, CLI control socket). Ask needs a
        // typed question, so its trigger opens the panel instead of firing
        // blindly.
        while let Ok(event) = self.events.try_recv() {
            match event {
                UiEvent::TogglePanel => {
                    if self.state.panel_open {
                        self.close_panel();
                    } else {
                        self.open_panel_via_event(ctx);
                    }
                }
                UiEvent::Stop => self.stop_playback(),
                UiEvent::Trigger(Mode::Ask) => {
                    self.open_panel_via_event(ctx);
                    self.focus_question_field = true;
                }
                UiEvent::Trigger(_) if self.state.is_busy() => {
                    // A trigger while an action runs stops the audio — the
                    // same toggle feel as the old GNOME bindings.
                    self.stop_playback();
                }
                UiEvent::Trigger(mode) => self.trigger(mode, now),
            }
        }

        self.openness = ctx.animate_bool_with_time(
            Id::new("panel-openness"),
            self.state.panel_open,
            EXPAND_SECONDS,
        );
        self.geometry.apply(ctx, self.openness);

        // Keep the badge on top (best effort): winit sets the level once at
        // creation, but another always-on-top window raising itself can still
        // bury us. Re-assert the level on two transitions, each of which wakes
        // the loop via its own event so this costs nothing while idle and
        // never steals focus: losing input focus, and becoming occluded (the
        // case where a peer covers an already-unfocused badge — focus alone
        // misses it). Occlusion reporting is platform-limited (best on
        // macOS/Wayland; X11 rarely reports it), so the creation-time
        // always_on_top flag remains the baseline.
        let (focused, occluded) =
            ctx.input(|input| (input.viewport().focused, input.viewport().occluded));
        let lost_focus = focused == Some(false) && self.was_focused != Some(false);
        let newly_occluded = occluded == Some(true) && self.was_occluded != Some(true);
        if lost_focus || newly_occluded {
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
        }
        self.was_focused = focused;
        self.was_occluded = occluded;

        // Click-away: closing when the window loses focus while expanded.
        // An event-opened panel is exempt until its focus grab has landed —
        // it starts life unfocused, which is not a click-away.
        if self.suppress_click_away && focused == Some(true) {
            self.suppress_click_away = false;
        }
        if self.state.panel_open && focused == Some(false) && !self.suppress_click_away {
            self.close_panel();
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

        // Keyboard input is read (and consumed) before any widget draws, but
        // only while no text field owns the keyboard — typing a question
        // must never fire an action. `wants_keyboard_input` reflects the
        // previous frame, which costs one frame of accuracy around focus
        // changes and avoids stealing keys from the field.
        let typing = ctx.egui_wants_keyboard_input();
        let escape_pressed = !typing
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let accelerator = if !typing && self.openness >= 0.05 && self.startup == StartupState::Ready
        {
            consume_accelerator(&ctx)
        } else {
            None
        };

        let mut panel_response = panel::PanelResponse::default();
        if self.openness < 0.05 {
            let badge = badge::draw(ui, &self.state, now);
            // A primary-button press-and-move drags the whole window (OS-driven
            // move, so it works even where we can't set positions ourselves); a
            // clean tap toggles the panel. StartDrag is issued once, at drag
            // start, and only for the primary button so a middle/right drag
            // can't hijack the window into a move.
            if badge.drag_started_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            } else if badge.clicked() {
                if self.state.panel_open {
                    self.close_panel();
                } else {
                    // A click already carries focus with it, so the plain
                    // open path needs no click-away suppression.
                    self.state.panel_open = true;
                }
            }
        } else {
            // Fade the panel (frame and contents) in with the expansion, so
            // opening reads as one motion instead of a resize plus a pop.
            ui.multiply_opacity(self.openness);
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
                        speech_speed: &mut self.speech_speed,
                        focus_question: self.focus_question_field,
                    },
                );
            });
            // The request reached a drawn panel this frame; a fresh one (set
            // below or by an event) survives until the panel next draws.
            self.focus_question_field = false;
        }

        // Accelerators merge into the same dispatch as the panel buttons.
        if let Some(key) = accelerator {
            match keyboard_intent(key, self.state.is_busy()) {
                KeyIntent::Trigger(mode) => {
                    if panel_response.trigger.is_none() {
                        panel_response.trigger = Some(mode);
                    }
                }
                KeyIntent::FocusQuestion => self.focus_question_field = true,
                KeyIntent::Nothing => {}
            }
        }
        if escape_pressed {
            if self.state.is_busy() {
                self.stop_playback();
            } else if self.state.panel_open {
                self.close_panel();
            }
        }

        if let Some(mode) = panel_response.trigger {
            self.trigger(mode, now);
        }
        // The panel reports the slider value on every settled frame; egui
        // itself rewrites slider values during draw (clamping, decimal
        // rounding — even while disabled), so only a real difference from
        // the committed value counts, and never while an action is running.
        if let Some(speed) = panel_response.set_speed {
            let speed = round_tts_speed(speed);
            if speed != self.committed_speed && !self.state.is_busy() {
                self.set_speech_speed(speed, &ctx);
            }
        }
        if panel_response.save_key {
            self.save_api_key(&ctx);
        }
        if let Some(enabled) = panel_response.set_autostart {
            self.set_autostart(enabled);
        }
        if panel_response.start_drag {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if panel_response.stop {
            self.stop_playback();
        }
        if panel_response.close {
            self.close_panel();
        }
        if panel_response.quit {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
}

/// The single-letter accelerators available while the panel is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelKey {
    Read,
    Summarize,
    Explain,
    Ask,
    Dictate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyIntent {
    Trigger(Mode),
    FocusQuestion,
    Nothing,
}

/// What a consumed accelerator should do given the current activity. Busy
/// blocks new triggers — exactly like the disabled buttons — while focusing
/// the Ask field stays allowed (typing during playback is harmless).
fn keyboard_intent(key: PanelKey, busy: bool) -> KeyIntent {
    match key {
        PanelKey::Ask => KeyIntent::FocusQuestion,
        _ if busy => KeyIntent::Nothing,
        PanelKey::Read => KeyIntent::Trigger(Mode::Read),
        PanelKey::Summarize => KeyIntent::Trigger(Mode::Summarize),
        PanelKey::Explain => KeyIntent::Trigger(Mode::Explain),
        PanelKey::Dictate => KeyIntent::Trigger(Mode::Dictate),
    }
}

fn consume_accelerator(ctx: &Context) -> Option<PanelKey> {
    const KEYS: [(egui::Key, PanelKey); 5] = [
        (egui::Key::R, PanelKey::Read),
        (egui::Key::S, PanelKey::Summarize),
        (egui::Key::E, PanelKey::Explain),
        (egui::Key::A, PanelKey::Ask),
        (egui::Key::D, PanelKey::Dictate),
    ];

    ctx.input_mut(|input| {
        KEYS.iter()
            .find(|(key, _)| input.consume_key(egui::Modifiers::NONE, *key))
            .map(|(_, panel_key)| *panel_key)
    })
}

pub fn run(config: AppConfiguration, player: AudioPlayer) -> eframe::Result<()> {
    // winit permits exactly one event loop per process (the created-flag is
    // set even when creation fails), so the backend must be chosen correctly
    // up front — a failed X11 attempt cannot be retried on Wayland. Hence
    // the preflight: only force X11 when the display socket actually accepts
    // a connection.
    #[cfg(target_os = "linux")]
    let force_x11 = should_force_x11(
        lazy_allrounder_platform::session_kind(),
        x11_display_reachable(),
        std::env::var("LAZY_ALLROUNDER_BACKEND").ok().as_deref(),
    );
    #[cfg(not(target_os = "linux"))]
    let force_x11 = false;

    if force_x11 {
        tracing::info!(
            "Wayland session: running the overlay through XWayland so it can \
             stay on top, sit in its corner, and be dragged \
             (set LAZY_ALLROUNDER_BACKEND=wayland to opt out)"
        );
    }

    run_native(&config, &player, force_x11).inspect_err(|_| {
        if force_x11 {
            tracing::error!(
                "the overlay failed on the X11 (XWayland) backend; \
                 set LAZY_ALLROUNDER_BACKEND=wayland to use the native backend"
            );
        }
    })
}

/// One eframe lifecycle with the chosen backend.
fn run_native(
    config: &AppConfiguration,
    player: &AudioPlayer,
    force_x11: bool,
) -> eframe::Result<()> {
    #[cfg_attr(not(target_os = "linux"), expect(unused_mut))]
    let mut options = NativeOptions {
        viewport: viewport::initial_viewport(),
        ..Default::default()
    };

    #[cfg(target_os = "linux")]
    if force_x11 {
        use winit::platform::x11::EventLoopBuilderExtX11 as _;
        options.event_loop_builder = Some(Box::new(|builder| {
            builder.with_x11();
        }));
    }
    #[cfg(not(target_os = "linux"))]
    let _ = force_x11;

    let corner = config.overlay.corner;
    let hotkeys_config = config.hotkeys.clone();
    let speech_speed = config.tts.speed;
    let player = player.clone();

    eframe::run_native(
        viewport::APP_ID,
        options,
        Box::new(move |creation_context| {
            creation_context.egui_ctx.set_visuals(egui::Visuals::dark());

            // Both external-input pumps feed one channel: global hotkeys
            // (where the OS supports them) and the CLI control socket (the
            // only trigger path on GNOME Wayland).
            let (event_sender, events) = std::sync::mpsc::channel::<UiEvent>();
            crate::hotkeys::start(
                &hotkeys_config,
                creation_context.egui_ctx.clone(),
                event_sender.clone(),
            );
            crate::ipc::start(creation_context.egui_ctx.clone(), event_sender);
            let mut app = OverlayApp::new(events, corner, player, speech_speed);
            // The session's completion notifications must wake the event loop
            // even when the window is idle, so it needs the live egui context.
            app.try_start_session(&creation_context.egui_ctx);

            Ok(Box::new(app))
        }),
    )
}

/// Whether an X server (usually XWayland) is actually accepting connections,
/// not merely advertised by `DISPLAY`. Local displays (`:N` / `:N.S`) are
/// probed by connecting to their Unix socket. Host-qualified or malformed
/// values are never XWayland (which is always local), so they do NOT force
/// the X11 backend — a stale `DISPLAY=localhost:10.0` from an old SSH
/// forward must not brick a Wayland session, given a failed X11 event loop
/// cannot be retried. `LAZY_ALLROUNDER_BACKEND=x11` still overrides for
/// genuine remote-X setups.
#[cfg(target_os = "linux")]
fn x11_display_reachable() -> bool {
    let Some(display) = std::env::var_os("DISPLAY").filter(|display| !display.is_empty()) else {
        return false;
    };

    match x11_socket_path(&display.to_string_lossy()) {
        Some(socket) => std::os::unix::net::UnixStream::connect(socket).is_ok(),
        None => false,
    }
}

/// The Unix socket path for a local `DISPLAY` value, or None when the
/// display names a remote host (TCP) and cannot be probed this way.
#[cfg(target_os = "linux")]
fn x11_socket_path(display: &str) -> Option<String> {
    let number = display.strip_prefix(':')?;
    let number = number.split('.').next().unwrap_or(number);
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    Some(format!("/tmp/.X11-unix/X{number}"))
}

/// Whether to force winit's X11 backend. Native Wayland windows cannot stay
/// always-on-top, position themselves, or (on some compositors) even be
/// dragged — the compositor owns all of that — so on Wayland sessions the
/// overlay prefers XWayland, where GNOME and friends honor all three. The
/// `LAZY_ALLROUNDER_BACKEND` variable (`x11`/`wayland`) overrides in either
/// direction.
#[cfg(target_os = "linux")]
fn should_force_x11(
    session: Option<lazy_allrounder_platform::SessionKind>,
    x11_display: bool,
    override_value: Option<&str>,
) -> bool {
    match override_value.map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("wayland") => false,
        Some(value) if value.eq_ignore_ascii_case("x11") => true,
        _ => {
            matches!(
                session,
                Some(lazy_allrounder_platform::SessionKind::Wayland)
            ) && x11_display
        }
    }
}

#[cfg(test)]
mod keyboard_tests {
    use super::{KeyIntent, PanelKey, keyboard_intent};
    use crate::state::Mode;

    #[test]
    fn idle_letters_trigger_their_modes() {
        assert_eq!(
            keyboard_intent(PanelKey::Read, false),
            KeyIntent::Trigger(Mode::Read)
        );
        assert_eq!(
            keyboard_intent(PanelKey::Summarize, false),
            KeyIntent::Trigger(Mode::Summarize)
        );
        assert_eq!(
            keyboard_intent(PanelKey::Explain, false),
            KeyIntent::Trigger(Mode::Explain)
        );
        assert_eq!(
            keyboard_intent(PanelKey::Dictate, false),
            KeyIntent::Trigger(Mode::Dictate)
        );
    }

    #[test]
    fn busy_blocks_every_trigger() {
        for key in [
            PanelKey::Read,
            PanelKey::Summarize,
            PanelKey::Explain,
            PanelKey::Dictate,
        ] {
            assert_eq!(keyboard_intent(key, true), KeyIntent::Nothing);
        }
    }

    #[test]
    fn ask_focuses_the_question_field_even_while_busy() {
        assert_eq!(
            keyboard_intent(PanelKey::Ask, false),
            KeyIntent::FocusQuestion
        );
        assert_eq!(
            keyboard_intent(PanelKey::Ask, true),
            KeyIntent::FocusQuestion
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod backend_tests {
    use lazy_allrounder_platform::SessionKind;

    use super::{should_force_x11, x11_socket_path};

    #[test]
    fn local_displays_map_to_their_unix_socket() {
        assert_eq!(x11_socket_path(":0"), Some("/tmp/.X11-unix/X0".to_owned()));
        assert_eq!(
            x11_socket_path(":10.2"),
            Some("/tmp/.X11-unix/X10".to_owned())
        );
    }

    #[test]
    fn remote_or_malformed_displays_are_not_probed() {
        assert_eq!(x11_socket_path("localhost:10.0"), None);
        assert_eq!(x11_socket_path(":"), None);
        assert_eq!(x11_socket_path(":abc"), None);
    }

    #[test]
    fn wayland_with_xwayland_prefers_x11() {
        assert!(should_force_x11(Some(SessionKind::Wayland), true, None));
    }

    #[test]
    fn wayland_without_xwayland_stays_native() {
        assert!(!should_force_x11(Some(SessionKind::Wayland), false, None));
    }

    #[test]
    fn x11_sessions_never_need_forcing() {
        assert!(!should_force_x11(Some(SessionKind::X11), true, None));
    }

    #[test]
    fn override_wins_in_both_directions() {
        assert!(!should_force_x11(
            Some(SessionKind::Wayland),
            true,
            Some("wayland")
        ));
        assert!(should_force_x11(Some(SessionKind::X11), true, Some("X11")));
        // Unknown values fall back to the default heuristic.
        assert!(should_force_x11(
            Some(SessionKind::Wayland),
            true,
            Some("cosmic")
        ));
    }
}
