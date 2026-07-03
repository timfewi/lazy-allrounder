//! The expanded panel: onboarding, mode buttons, ask input, status, and
//! footer actions.

use egui::{Button, CornerRadius, RichText, TextEdit, Ui, vec2};

use crate::overlay::StartupState;
use crate::overlay::theme;
use crate::state::{Activity, Mode, OverlayState};

/// What the caller (the eframe app) should do in response to panel input.
#[derive(Debug, Default)]
pub struct PanelResponse {
    pub trigger: Option<Mode>,
    pub save_key: bool,
    pub set_autostart: Option<bool>,
    /// The slider's current speed, reported whenever no interaction is in
    /// flight (drag released, no keyboard focus). The app dedups against the
    /// last committed value, so repeated reports are no-ops.
    pub set_speed: Option<f32>,
    pub stop: bool,
    pub close: bool,
    pub quit: bool,
}

pub struct PanelInputs<'a> {
    pub state: &'a OverlayState,
    pub startup: &'a StartupState,
    pub question: &'a mut String,
    pub api_key_input: &'a mut String,
    pub onboarding_error: Option<&'a str>,
    pub autostart_enabled: bool,
    pub speech_speed: &'a mut f32,
}

pub fn draw(ui: &mut Ui, inputs: PanelInputs<'_>) -> PanelResponse {
    let mut response = PanelResponse::default();

    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Lazy Allrounder")
                .color(theme::TEXT_PRIMARY)
                .strong()
                .size(16.0),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(RichText::new("✕").color(theme::TEXT_MUTED))
                .clicked()
            {
                response.close = true;
            }
        });
    });
    ui.add_space(4.0);

    match inputs.startup {
        StartupState::NeedsApiKey => {
            draw_onboarding(
                ui,
                inputs.api_key_input,
                inputs.onboarding_error,
                &mut response,
            );
        }
        StartupState::Failed(message) => {
            ui.colored_label(theme::FAILURE, message);
            ui.add_space(8.0);
        }
        StartupState::Ready => {
            draw_modes(
                ui,
                inputs.state,
                inputs.question,
                inputs.speech_speed,
                &mut response,
            );
        }
    }

    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("Quit").color(theme::TEXT_MUTED))
                .clicked()
            {
                response.quit = true;
            }

            let mut autostart = inputs.autostart_enabled;
            if ui
                .checkbox(
                    &mut autostart,
                    RichText::new("Start on login").color(theme::TEXT_MUTED),
                )
                .changed()
            {
                response.set_autostart = Some(autostart);
            }

            ui.hyperlink_to(
                RichText::new("Help").color(theme::TEXT_MUTED),
                "https://github.com/timfewi/lazy-allrounder",
            );
        });
    });

    response
}

/// First-run flow: paste the OpenRouter key, stored in the OS keyring — no
/// terminal or environment variables involved.
fn draw_onboarding(
    ui: &mut Ui,
    api_key_input: &mut String,
    onboarding_error: Option<&str>,
    response: &mut PanelResponse,
) {
    ui.label(
        RichText::new("Almost there — paste your OpenRouter API key:").color(theme::TEXT_PRIMARY),
    );
    ui.add_space(4.0);
    ui.hyperlink_to(
        RichText::new("Get a key at openrouter.ai/keys")
            .color(theme::ACCENT)
            .size(12.0),
        "https://openrouter.ai/keys",
    );
    ui.add_space(6.0);

    let submitted = ui
        .add(
            TextEdit::singleline(api_key_input)
                .hint_text("sk-or-…")
                .password(true)
                .desired_width(f32::INFINITY),
        )
        .lost_focus()
        && ui.input(|input| input.key_pressed(egui::Key::Enter));

    ui.add_space(6.0);
    let can_save = !api_key_input.trim().is_empty();
    if ui
        .add_enabled(can_save, styled_button(ui, "Save key"))
        .clicked()
        || (submitted && can_save)
    {
        response.save_key = true;
    }

    if let Some(error) = onboarding_error {
        ui.add_space(4.0);
        ui.colored_label(theme::FAILURE, error);
    }

    ui.add_space(8.0);
    ui.colored_label(
        theme::TEXT_MUTED,
        "The key is stored in your system keyring, never in a plain file.",
    );
}

fn draw_modes(
    ui: &mut Ui,
    state: &OverlayState,
    question: &mut String,
    speech_speed: &mut f32,
    response: &mut PanelResponse,
) {
    let busy = state.is_busy();

    for mode in [Mode::Read, Mode::Summarize, Mode::Explain] {
        if mode_button(ui, mode.label(), busy) {
            response.trigger = Some(mode);
        }
    }

    ui.add_space(6.0);
    ui.label(
        RichText::new("Ask about the clipboard")
            .color(theme::TEXT_MUTED)
            .size(12.0),
    );
    ui.add(
        TextEdit::singleline(question)
            .hint_text("Type a question…")
            .desired_width(f32::INFINITY),
    );
    if mode_button(ui, Mode::Ask.label(), busy) {
        response.trigger = Some(Mode::Ask);
    }

    ui.add_space(6.0);
    if mode_button(ui, Mode::Dictate.label(), busy) {
        response.trigger = Some(Mode::Dictate);
    }

    // Range mirrors core's TTS_SPEED_RANGE so a hand-edited config value is
    // representable rather than silently clamped into a narrower band. The
    // value is reported once interaction settles (drag released, keyboard
    // focus gone) — never on `changed()`, which egui also fires for its own
    // per-draw rounding and for every keystroke of an edit. The busy gate
    // means a commit can never yank the session out from under an action.
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Voice speed")
                .color(theme::TEXT_MUTED)
                .size(12.0),
        );
        let slider = ui.add_enabled(
            !busy,
            egui::Slider::new(speech_speed, lazy_allrounder_core::config::TTS_SPEED_RANGE)
                .max_decimals(2)
                .suffix("×"),
        );
        let settled = !slider.dragged() && !slider.has_focus();
        if !busy && (slider.drag_stopped() || settled) {
            response.set_speed = Some(*speech_speed);
        }
    });

    ui.add_space(8.0);
    match &state.activity {
        Activity::Processing { mode } => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.colored_label(theme::TEXT_MUTED, mode.busy_label());
                if ui.button("Stop").clicked() {
                    response.stop = true;
                }
            });
        }
        Activity::Done { mode, .. } => {
            ui.colored_label(theme::SUCCESS, format!("{} finished", mode.label()));
        }
        Activity::Error { message, .. } => {
            ui.colored_label(theme::FAILURE, message);
        }
        Activity::Idle => {
            ui.colored_label(theme::TEXT_MUTED, "Select or copy text, then pick a mode.");
        }
    }
}

fn styled_button(ui: &Ui, label: &str) -> Button<'static> {
    let _ = ui;
    Button::new(
        RichText::new(label.to_owned())
            .color(theme::TEXT_PRIMARY)
            .size(14.0),
    )
    .fill(theme::ACCENT_SOFT)
    .corner_radius(CornerRadius::same(10))
}

fn mode_button(ui: &mut Ui, label: &str, busy: bool) -> bool {
    let button = Button::new(RichText::new(label).color(theme::TEXT_PRIMARY).size(14.0))
        .fill(theme::SURFACE_RAISED)
        .corner_radius(CornerRadius::same(10))
        .min_size(vec2(ui.available_width(), 34.0));

    ui.add_enabled(!busy, button).clicked()
}
