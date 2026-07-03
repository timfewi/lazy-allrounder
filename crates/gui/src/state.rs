//! Pure overlay state machine — no egui/window types so every transition is
//! unit-testable headlessly.

/// How long a Done/Error result stays visible on the badge before returning
/// to Idle.
const RESULT_LINGER_SECONDS: f64 = 2.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Read,
    Summarize,
    Explain,
    Ask,
    Dictate,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Read => "Read clipboard",
            Mode::Summarize => "Summarize",
            Mode::Explain => "Explain",
            Mode::Ask => "Ask",
            Mode::Dictate => "Dictate",
        }
    }

    pub fn busy_label(self) -> &'static str {
        match self {
            Mode::Read => "Reading…",
            Mode::Summarize => "Summarizing…",
            Mode::Explain => "Explaining…",
            Mode::Ask => "Answering…",
            Mode::Dictate => "Dictating…",
        }
    }
}

/// An input that arrived from outside the window: a registered global
/// hotkey, or a CLI command over the control socket. Both pumps feed one
/// channel so the overlay has a single point where external inputs meet
/// its state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiEvent {
    Trigger(Mode),
    TogglePanel,
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Activity {
    Idle,
    Processing {
        mode: Mode,
    },
    Done {
        mode: Mode,
        at: f64,
    },
    Error {
        mode: Mode,
        message: String,
        at: f64,
    },
}

#[derive(Debug)]
pub struct OverlayState {
    pub panel_open: bool,
    pub activity: Activity,
}

impl OverlayState {
    pub fn new() -> Self {
        Self {
            panel_open: false,
            activity: Activity::Idle,
        }
    }

    pub fn close_panel(&mut self) {
        self.panel_open = false;
    }

    pub fn is_busy(&self) -> bool {
        matches!(self.activity, Activity::Processing { .. })
    }

    /// Starts a mode if nothing is already running. Returns false (and leaves
    /// the state untouched) while another action is in flight.
    pub fn begin(&mut self, mode: Mode) -> bool {
        if self.is_busy() {
            return false;
        }

        self.activity = Activity::Processing { mode };
        true
    }

    /// Records the outcome of the in-flight action. Outcomes for a mode that
    /// is no longer the active one are ignored (e.g. a stale result arriving
    /// after the linger period already reset the badge).
    pub fn finish(&mut self, mode: Mode, result: Result<(), String>, now: f64) {
        if !matches!(self.activity, Activity::Processing { mode: active } if active == mode) {
            return;
        }

        self.activity = match result {
            Ok(()) => Activity::Done { mode, at: now },
            Err(message) => Activity::Error {
                mode,
                message,
                at: now,
            },
        };
    }

    /// Advances time-based transitions: Done/Error return to Idle once the
    /// linger period has passed.
    pub fn tick(&mut self, now: f64) {
        let at = match &self.activity {
            Activity::Done { at, .. } | Activity::Error { at, .. } => *at,
            _ => return,
        };

        if now - at >= RESULT_LINGER_SECONDS {
            self.activity = Activity::Idle;
        }
    }

    /// One-line status for the panel footer / badge tooltip.
    pub fn status_line(&self) -> Option<String> {
        match &self.activity {
            Activity::Idle => None,
            Activity::Processing { mode } => Some(mode.busy_label().to_owned()),
            Activity::Done { mode, .. } => Some(format!("{} finished", mode.label())),
            Activity::Error { message, .. } => Some(message.clone()),
        }
    }
}

impl Default for OverlayState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_blocks_while_processing() {
        let mut state = OverlayState::new();
        assert!(state.begin(Mode::Read));
        assert!(!state.begin(Mode::Summarize));
        assert_eq!(state.activity, Activity::Processing { mode: Mode::Read });
    }

    #[test]
    fn finish_transitions_to_done_then_idle_after_linger() {
        let mut state = OverlayState::new();
        state.begin(Mode::Read);
        state.finish(Mode::Read, Ok(()), 10.0);
        assert_eq!(
            state.activity,
            Activity::Done {
                mode: Mode::Read,
                at: 10.0
            }
        );

        state.tick(11.0);
        assert!(matches!(state.activity, Activity::Done { .. }));

        state.tick(12.6);
        assert_eq!(state.activity, Activity::Idle);
    }

    #[test]
    fn finish_records_error_message() {
        let mut state = OverlayState::new();
        state.begin(Mode::Ask);
        state.finish(Mode::Ask, Err("clipboard is empty".to_owned()), 5.0);
        assert_eq!(state.status_line().as_deref(), Some("clipboard is empty"));
    }

    #[test]
    fn finish_for_a_different_mode_is_ignored() {
        let mut state = OverlayState::new();
        state.begin(Mode::Read);
        state.finish(Mode::Summarize, Ok(()), 1.0);
        assert_eq!(state.activity, Activity::Processing { mode: Mode::Read });
    }

    #[test]
    fn finish_without_processing_is_ignored() {
        let mut state = OverlayState::new();
        state.finish(Mode::Read, Ok(()), 1.0);
        assert_eq!(state.activity, Activity::Idle);
    }

    #[test]
    fn panel_open_state_is_independent_of_activity() {
        let mut state = OverlayState::new();
        state.begin(Mode::Read);
        state.panel_open = true;
        assert!(state.is_busy());
        state.close_panel();
        assert!(!state.panel_open);
        assert!(
            state.is_busy(),
            "closing the panel does not stop the action"
        );
    }
}
