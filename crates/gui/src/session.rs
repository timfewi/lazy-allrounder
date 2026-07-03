//! Bridges async `Application` calls to the synchronous egui update loop.
//!
//! A dedicated worker thread owns a tokio runtime and the `Application`;
//! the UI thread sends `ActionRequest`s and polls `ActionOutcome`s each
//! frame, so network/audio work never blocks rendering.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use lazy_allrounder_app::{Application, DictateCaptureOutcome, dictate_toggle_capture};
use lazy_allrounder_platform::AudioPlayer;

use crate::actions;
use crate::state::Mode;

#[derive(Debug)]
pub struct ActionRequest {
    pub mode: Mode,
    pub question: Option<String>,
}

#[derive(Debug)]
pub struct ActionOutcome {
    pub mode: Mode,
    pub result: Result<(), String>,
}

pub struct Session {
    requests: Sender<ActionRequest>,
    outcomes: Receiver<ActionOutcome>,
    player: AudioPlayer,
}

impl Session {
    /// Spawns the worker thread. `notify` is called after each outcome is
    /// queued so the UI can request a repaint even while unfocused.
    pub fn spawn(
        application: Application,
        player: AudioPlayer,
        notify: impl Fn() + Send + 'static,
    ) -> Self {
        let (requests, request_receiver) = channel::<ActionRequest>();
        let (outcome_sender, outcomes) = channel::<ActionOutcome>();
        let worker_player = player.clone();

        thread::Builder::new()
            .name("lazy-allrounder-session".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Runtime::new() {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        tracing::error!("failed to start the session runtime: {error}");
                        return;
                    }
                };

                while let Ok(request) = request_receiver.recv() {
                    let mode = request.mode;
                    let result =
                        runtime.block_on(run_action(&application, &worker_player, request));
                    if outcome_sender.send(ActionOutcome { mode, result }).is_err() {
                        break;
                    }
                    notify();
                }
            })
            .expect("failed to spawn the session thread");

        Self {
            requests,
            outcomes,
            player,
        }
    }

    pub fn dispatch(&self, mode: Mode, question: Option<String>) {
        if self
            .requests
            .send(ActionRequest { mode, question })
            .is_err()
        {
            tracing::error!("the session thread is no longer running");
        }
    }

    /// Stops any in-flight audio playback; the blocked worker action then
    /// completes and reports its outcome normally.
    pub fn stop_playback(&self) {
        self.player.stop();
    }

    pub fn poll(&self) -> Option<ActionOutcome> {
        self.outcomes.try_recv().ok()
    }
}

pub(crate) async fn run_action(
    application: &Application,
    player: &AudioPlayer,
    request: ActionRequest,
) -> Result<(), String> {
    // Selection-first: highlighting text is enough; Ctrl+C stays optional.
    let read_clipboard = lazy_allrounder_platform::read_selection_or_clipboard_text;

    match request.mode {
        Mode::Read => {
            actions::read_clipboard_aloud(application, read_clipboard, |audio| {
                player.play_and_wait(audio)
            })
            .await
        }
        Mode::Summarize => {
            let text = actions::clipboard_text(read_clipboard)?;
            let generated = application
                .summarize(text)
                .await
                .map_err(|error| error.to_string())?;
            let (_, audio) = generated.into_parts();
            player
                .play_and_wait(audio)
                .map_err(|error| error.to_string())
        }
        Mode::Explain => {
            let text = actions::clipboard_text(read_clipboard)?;
            let generated = application
                .explain(text)
                .await
                .map_err(|error| error.to_string())?;
            let (_, audio) = generated.into_parts();
            player
                .play_and_wait(audio)
                .map_err(|error| error.to_string())
        }
        Mode::Ask => {
            let question = request
                .question
                .filter(|question| !question.trim().is_empty())
                .ok_or_else(|| "type a question first".to_owned())?;
            let text = actions::clipboard_text(read_clipboard)?;
            let generated = application
                .ask(text, question)
                .await
                .map_err(|error| error.to_string())?;
            let (_, audio) = generated.into_parts();
            player
                .play_and_wait(audio)
                .map_err(|error| error.to_string())
        }
        Mode::Dictate => match dictate_toggle_capture().map_err(|error| error.to_string())? {
            DictateCaptureOutcome::Started => Ok(()),
            pending @ DictateCaptureOutcome::Pending(_) => {
                let DictateCaptureOutcome::Pending(pending) = pending else {
                    unreachable!("matched Pending above");
                };
                let transcript = application
                    .transcribe_pending_dictation(pending)
                    .await
                    .map_err(|error| error.to_string())?;
                application
                    .insert_dictated_text(&transcript)
                    .map_err(|error| format!("transcribed, but could not insert: {error}"))
            }
        },
    }
}
