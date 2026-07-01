//! The expanded panel: mode buttons, ask input, status, and footer actions.

use egui::{Button, CornerRadius, RichText, TextEdit, Ui, vec2};

use crate::overlay::theme;
use crate::state::{Activity, Mode, OverlayState};

/// What the caller (the eframe app) should do in response to panel input.
#[derive(Debug, Default)]
pub struct PanelResponse {
    pub trigger: Option<Mode>,
    pub stop: bool,
    pub close: bool,
    pub quit: bool,
}

pub fn draw(
    ui: &mut Ui,
    state: &OverlayState,
    question: &mut String,
    startup_error: Option<&str>,
) -> PanelResponse {
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

    if let Some(error) = startup_error {
        ui.colored_label(theme::FAILURE, error);
        ui.colored_label(
            theme::TEXT_MUTED,
            "Set OPENROUTER_API_KEY and create the config file, then restart.",
        );
        ui.add_space(8.0);
    }

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

    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("Quit").color(theme::TEXT_MUTED))
                .clicked()
            {
                response.quit = true;
            }
            ui.add_enabled(
                false,
                Button::new(RichText::new("Settings").color(theme::TEXT_MUTED)),
            )
            .on_disabled_hover_text("Hotkey settings arrive in the next milestone");
            ui.hyperlink_to(
                RichText::new("Help").color(theme::TEXT_MUTED),
                "https://github.com/timfewi/lazy-allrounder",
            );
        });
    });

    response
}

fn mode_button(ui: &mut Ui, label: &str, busy: bool) -> bool {
    let button = Button::new(RichText::new(label).color(theme::TEXT_PRIMARY).size(14.0))
        .fill(theme::SURFACE_RAISED)
        .corner_radius(CornerRadius::same(10))
        .min_size(vec2(ui.available_width(), 34.0));

    ui.add_enabled(!busy, button).clicked()
}
