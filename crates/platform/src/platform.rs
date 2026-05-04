mod process_control;
mod runtime_state;

use std::{
    io::{self, IsTerminal},
    thread,
    time::Duration,
};

use lazy_allrounder_core::error::PortError;

use crate::dictate_runtime::DictateState;

use self::{
    process_control::map_spawn_error,
    runtime_state::{
        ActionLock, RuntimeFiles, current_state, has_valid_audio, read_pid, reconcile_runtime,
        remove_file_if_exists, set_state,
    },
};

const CAPTURE_RATE: &str = "16000";
const CAPTURE_CHANNELS: &str = "1";
const CAPTURE_FORMAT: &str = "s16";
const STARTUP_WAIT: Duration = Duration::from_millis(150);

#[derive(Debug)]
pub struct CompletionGuard {
    lock: Option<ActionLock>,
    runtime: RuntimeFiles,
    finished: bool,
}

impl CompletionGuard {
    fn new(runtime: RuntimeFiles, lock: ActionLock) -> Self {
        Self {
            lock: Some(lock),
            runtime,
            finished: false,
        }
    }

    pub fn finish(mut self) -> Result<(), PortError> {
        let result = set_state(&self.runtime, DictateState::Idle);
        self.finished = result.is_ok();
        result
    }
}

impl Drop for CompletionGuard {
    fn drop(&mut self) {
        if !self.finished {
            let _ = set_state(&self.runtime, DictateState::Idle);
        }
        self.lock.take();
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
    let runtime = RuntimeFiles::new();
    let _lock = ActionLock::acquire(&runtime)?;
    reconcile_runtime(&runtime)?;
    start_with_lock(&runtime)
}

pub fn dictate_stop() -> Result<(Vec<u8>, CompletionGuard), PortError> {
    let runtime = RuntimeFiles::new();
    let lock = ActionLock::acquire(&runtime)?;
    reconcile_runtime(&runtime)?;

    if current_state(&runtime)? != DictateState::Recording {
        return Err(PortError::Other {
            message: "dictation is not currently recording".to_owned(),
        });
    }

    stop_with_lock(runtime, lock)
}

pub fn dictate_toggle() -> Result<ToggleOutcome, PortError> {
    let runtime = RuntimeFiles::new();
    let lock = ActionLock::acquire(&runtime)?;
    reconcile_runtime(&runtime)?;

    match current_state(&runtime)? {
        DictateState::Idle => {
            start_with_lock(&runtime)?;
            drop(lock);
            Ok(ToggleOutcome::Started)
        }
        DictateState::Recording => {
            let (audio, completion) = stop_with_lock(runtime, lock)?;
            Ok(ToggleOutcome::Pending { audio, completion })
        }
        DictateState::Transcribing => Err(PortError::Other {
            message: "dictation is already transcribing".to_owned(),
        }),
    }
}

pub fn dictate_status() -> Result<DictateState, PortError> {
    let runtime = RuntimeFiles::new();
    reconcile_runtime(&runtime)?;
    current_state(&runtime)
}

pub fn ensure_interactive_terminal() -> Result<(), PortError> {
    if io::stdin().is_terminal() {
        return Ok(());
    }

    Err(PortError::Other {
        message: "microphone capture requires an interactive terminal".to_owned(),
    })
}

pub fn map_io_error(error: io::Error) -> PortError {
    process_control::map_io_error(error)
}

fn start_with_lock(runtime: &RuntimeFiles) -> Result<(), PortError> {
    if current_state(runtime)? == DictateState::Recording {
        return Err(PortError::Other {
            message: "dictation is already recording".to_owned(),
        });
    }

    remove_file_if_exists(runtime.audio_path())?;
    let child = process_control::spawn_capture(
        runtime.audio_path_str()?,
        CAPTURE_RATE,
        CAPTURE_CHANNELS,
        CAPTURE_FORMAT,
    )
    .map_err(map_spawn_error)?;

    thread::sleep(STARTUP_WAIT);
    if !process_control::process_is_alive(child.id())? {
        remove_file_if_exists(runtime.audio_path())?;
        set_state(runtime, DictateState::Idle)?;
        return Err(PortError::Other {
            message: "pw-record exited early".to_owned(),
        });
    }

    std::fs::write(runtime.pid_path(), format!("{}\n", child.id())).map_err(map_io_error)?;
    set_state(runtime, DictateState::Recording)
}

fn stop_with_lock(
    runtime: RuntimeFiles,
    lock: ActionLock,
) -> Result<(Vec<u8>, CompletionGuard), PortError> {
    let pid = read_pid(runtime.pid_path())?.ok_or_else(|| PortError::Other {
        message: "dictation is not currently recording".to_owned(),
    })?;

    process_control::stop_capture_process(pid)?;
    remove_file_if_exists(runtime.pid_path())?;

    if !has_valid_audio(&runtime)? {
        set_state(&runtime, DictateState::Idle)?;
        return Err(PortError::Other {
            message: "no audio was recorded".to_owned(),
        });
    }

    set_state(&runtime, DictateState::Transcribing)?;
    let audio = std::fs::read(runtime.audio_path()).map_err(map_io_error)?;
    remove_file_if_exists(runtime.audio_path())?;

    Ok((audio, CompletionGuard::new(runtime, lock)))
}
