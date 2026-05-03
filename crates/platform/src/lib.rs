use lazy_allrounder_core::error::PortError;

pub fn capture_microphone_until_enter() -> Result<Vec<u8>, PortError> {
    capture::capture_microphone_until_enter()
}

#[cfg(target_os = "linux")]
mod capture {
    use std::{
        env, fs,
        io::{self, IsTerminal},
        path::PathBuf,
        process::{Command, Stdio},
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use lazy_allrounder_core::error::PortError;

    const CAPTURE_RATE: &str = "16000";
    const CAPTURE_CHANNELS: &str = "1";
    const STARTUP_WAIT: Duration = Duration::from_millis(200);

    pub fn capture_microphone_until_enter() -> Result<Vec<u8>, PortError> {
        ensure_interactive_terminal(io::stdin().is_terminal())?;

        let temp_file = TemporaryAudioFile::new()?;
        let mut child = Command::new("pw-record")
            .args([
                "--rate",
                CAPTURE_RATE,
                "--channels",
                CAPTURE_CHANNELS,
                temp_file.path_as_str()?,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(map_spawn_error)?;

        thread::sleep(STARTUP_WAIT);
        if let Some(status) = child.try_wait().map_err(map_io_error)? {
            let output = child.wait_with_output().map_err(map_io_error)?;
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let detail = if stderr.is_empty() {
                format!("pw-record exited early with status {status}")
            } else {
                format!("pw-record exited early: {stderr}")
            };
            return Err(PortError::Other { message: detail });
        }

        let mut line = String::new();
        io::stdin().read_line(&mut line).map_err(map_io_error)?;
        send_interrupt(child.id())?;

        let output = child.wait_with_output().map_err(map_io_error)?;
        let audio = fs::read(temp_file.path()).map_err(map_io_error)?;
        if audio.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let message = if stderr.is_empty() {
                "pw-record produced no audio".to_owned()
            } else {
                format!("pw-record produced no audio: {stderr}")
            };
            return Err(PortError::Other { message });
        }

        Ok(audio)
    }

    fn send_interrupt(pid: u32) -> Result<(), PortError> {
        let status = Command::new("kill")
            .arg("-INT")
            .arg(pid.to_string())
            .status()
            .map_err(map_io_error)?;

        if status.success() {
            return Ok(());
        }

        Err(PortError::Other {
            message: format!("failed to stop pw-record cleanly for pid {pid}"),
        })
    }

    fn map_spawn_error(error: io::Error) -> PortError {
        if error.kind() == io::ErrorKind::NotFound {
            return PortError::Other {
                message:
                    "pw-record was not found; install PipeWire tools to use --microphone on Linux"
                        .to_owned(),
            };
        }

        map_io_error(error)
    }

    fn map_io_error(error: io::Error) -> PortError {
        PortError::Other {
            message: error.to_string(),
        }
    }

    pub(super) fn ensure_interactive_terminal(is_interactive: bool) -> Result<(), PortError> {
        if is_interactive {
            return Ok(());
        }

        Err(PortError::Other {
            message: "microphone capture requires an interactive terminal".to_owned(),
        })
    }

    struct TemporaryAudioFile {
        path: PathBuf,
    }

    impl TemporaryAudioFile {
        fn new() -> Result<Self, PortError> {
            let mut path = runtime_dir();
            path.push(format!(
                "lazy-allrounder-dictate-{}-{}.wav",
                std::process::id(),
                unix_timestamp_millis()?
            ));

            Ok(Self { path })
        }

        fn path(&self) -> &PathBuf {
            &self.path
        }

        fn path_as_str(&self) -> Result<&str, PortError> {
            self.path.to_str().ok_or_else(|| PortError::Other {
                message: "temporary capture path is not valid UTF-8".to_owned(),
            })
        }
    }

    impl Drop for TemporaryAudioFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    fn runtime_dir() -> PathBuf {
        match env::var_os("XDG_RUNTIME_DIR") {
            Some(value) => PathBuf::from(value),
            None => env::temp_dir(),
        }
    }

    fn unix_timestamp_millis() -> Result<u128, PortError> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .map_err(|error| PortError::Other {
                message: error.to_string(),
            })
    }
}

#[cfg(not(target_os = "linux"))]
mod capture {
    use lazy_allrounder_core::error::PortError;

    pub fn capture_microphone_until_enter() -> Result<Vec<u8>, PortError> {
        Err(PortError::unsupported("microphone capture"))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::capture::ensure_interactive_terminal;
    #[cfg(not(target_os = "linux"))]
    use super::capture_microphone_until_enter;
    use lazy_allrounder_core::error::PortError;

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_capture_requires_an_interactive_terminal() {
        let error = ensure_interactive_terminal(false)
            .expect_err("non-interactive terminal use should be rejected");

        assert!(matches!(
            error,
            PortError::Other { message } if message.contains("interactive terminal")
        ));
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn non_linux_capture_is_marked_unsupported() {
        let error = capture_microphone_until_enter()
            .expect_err("non-linux platforms should report microphone capture as unsupported");

        assert!(matches!(
            error,
            PortError::UnsupportedCapability {
                capability: "microphone capture"
            }
        ));
    }
}
