use lazy_allrounder_core::error::PortError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictateState {
    Idle,
    Recording,
    Transcribing,
}

impl DictateState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Recording => "recording",
            Self::Transcribing => "transcribing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DictateStatus {
    pub state: DictateState,
}

#[derive(Debug)]
pub struct PendingDictation {
    audio: Vec<u8>,
    completion: platform::CompletionGuard,
}

impl PendingDictation {
    pub fn into_parts(self) -> (Vec<u8>, DictateCompletion) {
        (
            self.audio,
            DictateCompletion {
                inner: self.completion,
            },
        )
    }
}

#[derive(Debug)]
pub struct DictateCompletion {
    inner: platform::CompletionGuard,
}

impl DictateCompletion {
    pub fn finish(self) -> Result<(), PortError> {
        self.inner.finish()
    }
}

#[derive(Debug)]
pub enum DictateToggleResult {
    Started,
    Pending(PendingDictation),
}

pub fn dictate_start() -> Result<(), PortError> {
    platform::dictate_start()
}

pub fn dictate_stop() -> Result<PendingDictation, PortError> {
    let (audio, completion) = platform::dictate_stop()?;
    Ok(PendingDictation { audio, completion })
}

pub fn dictate_toggle() -> Result<DictateToggleResult, PortError> {
    match platform::dictate_toggle()? {
        platform::ToggleOutcome::Started => Ok(DictateToggleResult::Started),
        platform::ToggleOutcome::Pending { audio, completion } => {
            Ok(DictateToggleResult::Pending(PendingDictation {
                audio,
                completion,
            }))
        }
    }
}

pub fn dictate_status() -> Result<DictateStatus, PortError> {
    Ok(DictateStatus {
        state: platform::dictate_status()?,
    })
}

pub fn capture_microphone_until_enter() -> Result<PendingDictation, PortError> {
    platform::ensure_interactive_terminal()?;
    dictate_start()?;

    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(platform::map_io_error)?;

    dictate_stop()
}

#[cfg(target_os = "linux")]
#[path = "platform.rs"]
mod platform;

#[cfg(not(target_os = "linux"))]
mod platform {
    use lazy_allrounder_core::error::PortError;

    use super::DictateState;

    #[derive(Debug)]
    pub struct CompletionGuard;

    impl CompletionGuard {
        pub fn finish(self) -> Result<(), PortError> {
            Err(PortError::unsupported("dictate runtime"))
        }
    }

    #[derive(Debug)]
    pub enum ToggleOutcome {
        Started,
        Pending {
            audio: Vec<u8>,
            completion: CompletionGuard,
        },
    }

    pub fn dictate_start() -> Result<(), PortError> {
        Err(PortError::unsupported("dictate runtime"))
    }

    pub fn dictate_stop() -> Result<(Vec<u8>, CompletionGuard), PortError> {
        Err(PortError::unsupported("dictate runtime"))
    }

    pub fn dictate_toggle() -> Result<ToggleOutcome, PortError> {
        Err(PortError::unsupported("dictate runtime"))
    }

    pub fn dictate_status() -> Result<DictateState, PortError> {
        Err(PortError::unsupported("dictate runtime"))
    }

    pub fn ensure_interactive_terminal() -> Result<(), PortError> {
        Err(PortError::unsupported("microphone capture"))
    }

    pub fn map_io_error(error: std::io::Error) -> PortError {
        PortError::Other {
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DictateState;

    #[test]
    fn dictate_state_strings_are_stable() {
        assert_eq!(DictateState::Idle.as_str(), "idle");
        assert_eq!(DictateState::Recording.as_str(), "recording");
        assert_eq!(DictateState::Transcribing.as_str(), "transcribing");
    }
}
