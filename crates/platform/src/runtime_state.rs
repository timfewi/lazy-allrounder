use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use lazy_allrounder_core::error::PortError;

use crate::dictate_runtime::DictateState;

use super::process_control;

const MIN_WAV_BYTES: u64 = 128;

#[derive(Debug, Clone)]
pub struct RuntimeFiles {
    state: PathBuf,
    pid: PathBuf,
    audio: PathBuf,
    lock: PathBuf,
}

impl RuntimeFiles {
    pub fn new() -> Self {
        let runtime_dir = runtime_dir();

        Self {
            state: runtime_dir.join("lazy-allrounder-dictate.state"),
            pid: runtime_dir.join("lazy-allrounder-dictate.pid"),
            audio: runtime_dir.join("lazy-allrounder-dictate.wav"),
            lock: runtime_dir.join("lazy-allrounder-dictate.lock"),
        }
    }

    pub fn pid_path(&self) -> &Path {
        &self.pid
    }

    pub fn audio_path(&self) -> &Path {
        &self.audio
    }

    pub fn audio_path_str(&self) -> Result<&str, PortError> {
        self.audio.to_str().ok_or_else(|| PortError::Other {
            message: format!("path is not valid UTF-8: {}", self.audio.display()),
        })
    }
}

#[derive(Debug)]
pub struct ActionLock {
    path: PathBuf,
}

impl ActionLock {
    pub fn acquire(runtime: &RuntimeFiles) -> Result<Self, PortError> {
        for _ in 0..2 {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&runtime.lock)
            {
                Ok(mut file) => {
                    use std::io::Write;
                    writeln!(file, "{}", std::process::id())
                        .map_err(process_control::map_io_error)?;
                    return Ok(Self {
                        path: runtime.lock.clone(),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if clear_stale_lock(runtime)? {
                        continue;
                    }

                    return Err(PortError::Other {
                        message: "previous dictation action is still finishing".to_owned(),
                    });
                }
                Err(error) => return Err(process_control::map_io_error(error)),
            }
        }

        Err(PortError::Other {
            message: "failed to acquire dictate runtime lock".to_owned(),
        })
    }
}

impl Drop for ActionLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn reconcile_runtime(runtime: &RuntimeFiles) -> Result<(), PortError> {
    if let Some(pid) = read_pid(runtime.pid_path())? {
        if process_control::is_pw_record_process(pid)? {
            if read_state(runtime)? != Some(DictateState::Recording) {
                set_state(runtime, DictateState::Recording)?;
            }
            return Ok(());
        }

        remove_file_if_exists(runtime.pid_path())?;
        remove_file_if_exists(runtime.audio_path())?;
        set_state(runtime, DictateState::Idle)?;
        return Ok(());
    }

    match read_state(runtime)? {
        Some(DictateState::Recording) => {
            remove_file_if_exists(runtime.audio_path())?;
            set_state(runtime, DictateState::Idle)?;
        }
        Some(DictateState::Transcribing) if !lock_is_active(runtime)? => {
            set_state(runtime, DictateState::Idle)?;
        }
        Some(DictateState::Idle) | Some(DictateState::Transcribing) | None => {}
    }

    Ok(())
}

pub fn current_state(runtime: &RuntimeFiles) -> Result<DictateState, PortError> {
    if let Some(pid) = read_pid(runtime.pid_path())?
        && process_control::is_pw_record_process(pid)?
    {
        return Ok(DictateState::Recording);
    }

    Ok(read_state(runtime)?.unwrap_or(DictateState::Idle))
}

pub fn set_state(runtime: &RuntimeFiles, state: DictateState) -> Result<(), PortError> {
    fs::write(&runtime.state, format!("{}\n", state.as_str()))
        .map_err(process_control::map_io_error)
}

pub fn has_valid_audio(runtime: &RuntimeFiles) -> Result<bool, PortError> {
    let metadata = match fs::metadata(runtime.audio_path()) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(process_control::map_io_error(error)),
    };

    Ok(metadata.len() >= MIN_WAV_BYTES)
}

pub fn read_pid(path: &Path) -> Result<Option<u32>, PortError> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(process_control::map_io_error(error)),
    };

    match raw.trim().parse::<u32>() {
        Ok(pid) => Ok(Some(pid)),
        Err(_) => {
            remove_file_if_exists(path)?;
            Ok(None)
        }
    }
}

fn clear_stale_lock(runtime: &RuntimeFiles) -> Result<bool, PortError> {
    let Some(pid) = read_pid(&runtime.lock)? else {
        remove_file_if_exists(&runtime.lock)?;
        return Ok(true);
    };

    if process_control::process_is_alive(pid)? {
        return Ok(false);
    }

    remove_file_if_exists(&runtime.lock)?;
    Ok(true)
}

fn lock_is_active(runtime: &RuntimeFiles) -> Result<bool, PortError> {
    let Some(pid) = read_pid(&runtime.lock)? else {
        return Ok(false);
    };

    process_control::process_is_alive(pid)
}

fn read_state(runtime: &RuntimeFiles) -> Result<Option<DictateState>, PortError> {
    let raw = match fs::read_to_string(&runtime.state) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(process_control::map_io_error(error)),
    };

    let state = match raw.trim() {
        "idle" => DictateState::Idle,
        "recording" => DictateState::Recording,
        "transcribing" => DictateState::Transcribing,
        _ => DictateState::Idle,
    };

    Ok(Some(state))
}

pub(super) fn remove_file_if_exists(path: &Path) -> Result<(), PortError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(process_control::map_io_error(error)),
    }
}

fn runtime_dir() -> PathBuf {
    match env::var_os("XDG_RUNTIME_DIR") {
        Some(value) => PathBuf::from(value),
        None => env::temp_dir(),
    }
}
