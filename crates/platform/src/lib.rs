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
    completion: capture::CompletionGuard,
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
    inner: capture::CompletionGuard,
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
    capture::dictate_start()
}

pub fn dictate_stop() -> Result<PendingDictation, PortError> {
    let (audio, completion) = capture::dictate_stop()?;
    Ok(PendingDictation { audio, completion })
}

pub fn dictate_toggle() -> Result<DictateToggleResult, PortError> {
    match capture::dictate_toggle()? {
        capture::ToggleOutcome::Started => Ok(DictateToggleResult::Started),
        capture::ToggleOutcome::Pending { audio, completion } => {
            Ok(DictateToggleResult::Pending(PendingDictation {
                audio,
                completion,
            }))
        }
    }
}

pub fn dictate_status() -> Result<DictateStatus, PortError> {
    Ok(DictateStatus {
        state: capture::dictate_status()?,
    })
}

pub fn capture_microphone_until_enter() -> Result<PendingDictation, PortError> {
    capture::ensure_interactive_terminal()?;
    dictate_start()?;

    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(capture::map_io_error)?;

    dictate_stop()
}

#[cfg(target_os = "linux")]
mod capture {
    use std::{
        env, fs,
        io::{self, IsTerminal, Write},
        path::PathBuf,
        process::{Command, Stdio},
        thread,
        time::Duration,
    };

    use lazy_allrounder_core::error::PortError;

    use crate::DictateState;

    const CAPTURE_RATE: &str = "16000";
    const CAPTURE_CHANNELS: &str = "1";
    const CAPTURE_FORMAT: &str = "s16";
    const STARTUP_WAIT: Duration = Duration::from_millis(150);
    const STOP_WAIT: Duration = Duration::from_millis(100);
    const MIN_WAV_BYTES: u64 = 128;

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

    #[derive(Debug, Clone)]
    struct RuntimeFiles {
        state: PathBuf,
        pid: PathBuf,
        audio: PathBuf,
        lock: PathBuf,
    }

    impl RuntimeFiles {
        fn new() -> Self {
            let runtime_dir = runtime_dir();

            Self {
                state: runtime_dir.join("lazy-allrounder-dictate.state"),
                pid: runtime_dir.join("lazy-allrounder-dictate.pid"),
                audio: runtime_dir.join("lazy-allrounder-dictate.wav"),
                lock: runtime_dir.join("lazy-allrounder-dictate.lock"),
            }
        }
    }

    #[derive(Debug)]
    struct ActionLock {
        path: PathBuf,
    }

    impl ActionLock {
        fn acquire(runtime: &RuntimeFiles) -> Result<Self, PortError> {
            for _ in 0..2 {
                match fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&runtime.lock)
                {
                    Ok(mut file) => {
                        writeln!(file, "{}", std::process::id()).map_err(map_io_error)?;
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
                    Err(error) => return Err(map_io_error(error)),
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

    pub fn dictate_start() -> Result<(), PortError> {
        let runtime = RuntimeFiles::new();
        let _lock = ActionLock::acquire(&runtime)?;
        reconcile_runtime(&runtime)?;

        if current_state(&runtime)? == DictateState::Recording {
            return Err(PortError::Other {
                message: "dictation is already recording".to_owned(),
            });
        }

        let _ = fs::remove_file(&runtime.audio);
        let child = Command::new("pw-record")
            .args([
                "--rate",
                CAPTURE_RATE,
                "--channels",
                CAPTURE_CHANNELS,
                "--format",
                CAPTURE_FORMAT,
                path_as_str(&runtime.audio)?,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(map_spawn_error)?;

        thread::sleep(STARTUP_WAIT);
        if let Some(status) = process_status(child.id())? {
            let _ = fs::remove_file(&runtime.audio);
            set_state(&runtime, DictateState::Idle)?;
            return Err(PortError::Other {
                message: format!("pw-record exited early with status {status}"),
            });
        }

        fs::write(&runtime.pid, format!("{}\n", child.id())).map_err(map_io_error)?;
        set_state(&runtime, DictateState::Recording)
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

    fn start_with_lock(runtime: &RuntimeFiles) -> Result<(), PortError> {
        if current_state(runtime)? == DictateState::Recording {
            return Err(PortError::Other {
                message: "dictation is already recording".to_owned(),
            });
        }

        let _ = fs::remove_file(&runtime.audio);
        let child = Command::new("pw-record")
            .args([
                "--rate",
                CAPTURE_RATE,
                "--channels",
                CAPTURE_CHANNELS,
                "--format",
                CAPTURE_FORMAT,
                path_as_str(&runtime.audio)?,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(map_spawn_error)?;

        thread::sleep(STARTUP_WAIT);
        if let Some(status) = process_status(child.id())? {
            let _ = fs::remove_file(&runtime.audio);
            set_state(runtime, DictateState::Idle)?;
            return Err(PortError::Other {
                message: format!("pw-record exited early with status {status}"),
            });
        }

        fs::write(&runtime.pid, format!("{}\n", child.id())).map_err(map_io_error)?;
        set_state(runtime, DictateState::Recording)
    }

    fn stop_with_lock(
        runtime: RuntimeFiles,
        lock: ActionLock,
    ) -> Result<(Vec<u8>, CompletionGuard), PortError> {
        let pid = read_pid(&runtime.pid)?.ok_or_else(|| PortError::Other {
            message: "dictation is not currently recording".to_owned(),
        })?;

        stop_capture_process(pid)?;
        let _ = fs::remove_file(&runtime.pid);

        if !has_valid_audio(&runtime)? {
            set_state(&runtime, DictateState::Idle)?;
            return Err(PortError::Other {
                message: "no audio was recorded".to_owned(),
            });
        }

        set_state(&runtime, DictateState::Transcribing)?;
        let audio = fs::read(&runtime.audio).map_err(map_io_error)?;
        let _ = fs::remove_file(&runtime.audio);

        Ok((audio, CompletionGuard::new(runtime, lock)))
    }

    fn reconcile_runtime(runtime: &RuntimeFiles) -> Result<(), PortError> {
        if let Some(pid) = read_pid(&runtime.pid)? {
            if is_pw_record_process(pid)? {
                if read_state(runtime)? != Some(DictateState::Recording) {
                    set_state(runtime, DictateState::Recording)?;
                }
                return Ok(());
            }

            let _ = fs::remove_file(&runtime.pid);
            let _ = fs::remove_file(&runtime.audio);
            set_state(runtime, DictateState::Idle)?;
            return Ok(());
        }

        match read_state(runtime)? {
            Some(DictateState::Recording) => {
                let _ = fs::remove_file(&runtime.audio);
                set_state(runtime, DictateState::Idle)?;
            }
            Some(DictateState::Transcribing) if !lock_is_active(runtime)? => {
                set_state(runtime, DictateState::Idle)?;
            }
            Some(DictateState::Idle) | Some(DictateState::Transcribing) | None => {}
        }

        Ok(())
    }

    fn current_state(runtime: &RuntimeFiles) -> Result<DictateState, PortError> {
        if let Some(pid) = read_pid(&runtime.pid)?
            && is_pw_record_process(pid)?
        {
            return Ok(DictateState::Recording);
        }

        Ok(read_state(runtime)?.unwrap_or(DictateState::Idle))
    }

    fn read_state(runtime: &RuntimeFiles) -> Result<Option<DictateState>, PortError> {
        let raw = match fs::read_to_string(&runtime.state) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(map_io_error(error)),
        };

        let state = match raw.trim() {
            "idle" => DictateState::Idle,
            "recording" => DictateState::Recording,
            "transcribing" => DictateState::Transcribing,
            _ => DictateState::Idle,
        };

        Ok(Some(state))
    }

    fn set_state(runtime: &RuntimeFiles, state: DictateState) -> Result<(), PortError> {
        fs::write(&runtime.state, format!("{}\n", state.as_str())).map_err(map_io_error)
    }

    fn has_valid_audio(runtime: &RuntimeFiles) -> Result<bool, PortError> {
        let metadata = match fs::metadata(&runtime.audio) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(map_io_error(error)),
        };

        Ok(metadata.len() >= MIN_WAV_BYTES)
    }

    fn read_pid(path: &PathBuf) -> Result<Option<u32>, PortError> {
        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(map_io_error(error)),
        };

        let pid = raw
            .trim()
            .parse::<u32>()
            .map_err(|error| PortError::Other {
                message: format!("invalid pid file {}: {error}", path.display()),
            })?;

        Ok(Some(pid))
    }

    fn clear_stale_lock(runtime: &RuntimeFiles) -> Result<bool, PortError> {
        let Some(pid) = read_pid(&runtime.lock)? else {
            let _ = fs::remove_file(&runtime.lock);
            return Ok(true);
        };

        if process_is_alive(pid)? {
            return Ok(false);
        }

        let _ = fs::remove_file(&runtime.lock);
        Ok(true)
    }

    fn lock_is_active(runtime: &RuntimeFiles) -> Result<bool, PortError> {
        let Some(pid) = read_pid(&runtime.lock)? else {
            return Ok(false);
        };

        process_is_alive(pid)
    }

    fn process_is_alive(pid: u32) -> Result<bool, PortError> {
        let status = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(map_io_error)?;

        Ok(status.success())
    }

    fn process_status(pid: u32) -> Result<Option<std::process::ExitStatus>, PortError> {
        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "stat="])
            .output()
            .map_err(map_io_error)?;

        if !output.status.success() || output.stdout.is_empty() {
            return Ok(Some(Command::new("false").status().map_err(map_io_error)?));
        }

        Ok(None)
    }

    fn is_pw_record_process(pid: u32) -> Result<bool, PortError> {
        if !process_is_alive(pid)? {
            return Ok(false);
        }

        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "args="])
            .output()
            .map_err(map_io_error)?;

        if !output.status.success() {
            return Ok(false);
        }

        Ok(String::from_utf8_lossy(&output.stdout).contains("pw-record"))
    }

    fn stop_capture_process(pid: u32) -> Result<(), PortError> {
        send_signal(pid, "INT")?;

        for _ in 0..30 {
            if !process_is_alive(pid)? {
                return Ok(());
            }
            thread::sleep(STOP_WAIT);
        }

        send_signal(pid, "TERM")?;
        thread::sleep(Duration::from_millis(200));

        if process_is_alive(pid)? {
            send_signal(pid, "KILL")?;
        }

        Ok(())
    }

    fn send_signal(pid: u32, signal: &str) -> Result<(), PortError> {
        let status = Command::new("kill")
            .arg(format!("-{signal}"))
            .arg(pid.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(map_io_error)?;

        if status.success() {
            return Ok(());
        }

        Err(PortError::Other {
            message: format!("failed to send SIG{signal} to pid {pid}"),
        })
    }

    fn runtime_dir() -> PathBuf {
        match env::var_os("XDG_RUNTIME_DIR") {
            Some(value) => PathBuf::from(value),
            None => env::temp_dir(),
        }
    }

    fn path_as_str(path: &PathBuf) -> Result<&str, PortError> {
        path.to_str().ok_or_else(|| PortError::Other {
            message: format!("path is not valid UTF-8: {}", path.display()),
        })
    }

    pub fn ensure_interactive_terminal() -> Result<(), PortError> {
        if io::stdin().is_terminal() {
            return Ok(());
        }

        Err(PortError::Other {
            message: "microphone capture requires an interactive terminal".to_owned(),
        })
    }

    fn map_spawn_error(error: io::Error) -> PortError {
        if error.kind() == io::ErrorKind::NotFound {
            return PortError::Other {
                message: "pw-record was not found; install PipeWire tools to use dictate capture on Linux"
                    .to_owned(),
            };
        }

        map_io_error(error)
    }

    pub fn map_io_error(error: io::Error) -> PortError {
        PortError::Other {
            message: error.to_string(),
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod capture {
    use lazy_allrounder_core::error::PortError;

    use crate::DictateState;

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
