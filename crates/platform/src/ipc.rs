//! Control socket for a running overlay GUI.
//!
//! Desktop-level keyboard shortcuts on Wayland cannot reach the GUI process
//! directly (the compositor owns global keys), so shortcuts run the CLI,
//! which forwards one-line commands over this Unix socket. The protocol is
//! deliberately tiny — one command per connection, `ok`/`err <reason>`
//! reply — so both ends stay trivially testable.

use std::fmt;

/// An overlay action a client may ask the GUI to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiAction {
    Read,
    Summarize,
    Explain,
    Ask,
    Dictate,
}

impl GuiAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Summarize => "summarize",
            Self::Explain => "explain",
            Self::Ask => "ask",
            Self::Dictate => "dictate",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "read" => Some(Self::Read),
            "summarize" => Some(Self::Summarize),
            "explain" => Some(Self::Explain),
            "ask" => Some(Self::Ask),
            "dictate" => Some(Self::Dictate),
            _ => None,
        }
    }
}

/// One command over the control socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiCommand {
    TogglePanel,
    Trigger(GuiAction),
    Stop,
}

/// The wire form of a command — the exact line a client sends.
pub fn format_gui_command(command: GuiCommand) -> &'static str {
    match command {
        GuiCommand::TogglePanel => "toggle-panel",
        GuiCommand::Stop => "stop",
        GuiCommand::Trigger(GuiAction::Read) => "trigger read",
        GuiCommand::Trigger(GuiAction::Summarize) => "trigger summarize",
        GuiCommand::Trigger(GuiAction::Explain) => "trigger explain",
        GuiCommand::Trigger(GuiAction::Ask) => "trigger ask",
        GuiCommand::Trigger(GuiAction::Dictate) => "trigger dictate",
    }
}

/// Parses one received line. Whitespace-tolerant and case-insensitive so a
/// hand-typed `echo TOGGLE-PANEL | nc -U …` behaves the same as the CLI.
pub fn parse_gui_command(line: &str) -> Result<GuiCommand, String> {
    let normalized = line.trim().to_ascii_lowercase();
    let mut words = normalized.split_whitespace();

    let command = match (words.next(), words.next(), words.next()) {
        (Some("toggle-panel"), None, _) => GuiCommand::TogglePanel,
        (Some("stop"), None, _) => GuiCommand::Stop,
        (Some("trigger"), Some(action), None) => GuiAction::parse(action)
            .map(GuiCommand::Trigger)
            .ok_or_else(|| format!("unknown action {action:?}"))?,
        _ => return Err(format!("unknown command {:?}", line.trim())),
    };

    Ok(command)
}

/// Why sending a command to the GUI failed. `NotRunning` is the branch
/// callers act on (fall back to a headless path or tell the user); the
/// others are genuine faults.
#[derive(Debug)]
pub enum SendError {
    /// Nothing is listening on the control socket.
    NotRunning,
    /// The GUI answered `err …` — it is running but refused the command.
    Rejected(String),
    /// Transport failure mid-conversation.
    Io(String),
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRunning => write!(f, "the overlay GUI is not running"),
            Self::Rejected(message) => write!(f, "the overlay GUI rejected the command: {message}"),
            Self::Io(message) => write!(f, "could not talk to the overlay GUI: {message}"),
        }
    }
}

impl std::error::Error for SendError {}

#[cfg(unix)]
pub use socket::{GuiCommandListener, gui_socket_path, send_gui_command};

#[cfg(not(unix))]
pub fn send_gui_command(_command: GuiCommand) -> Result<(), SendError> {
    // No control socket on this platform; callers treat this like an absent
    // GUI and use their fallback path.
    Err(SendError::NotRunning)
}

#[cfg(unix)]
mod socket {
    use std::io::{Read as _, Write as _};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use lazy_allrounder_core::error::PortError;

    use super::{GuiCommand, SendError, format_gui_command, parse_gui_command};

    /// A slow or wedged peer must not stall the accept loop (server) or a
    /// keyboard shortcut (client) for longer than this.
    const IO_TIMEOUT: Duration = Duration::from_millis(500);

    /// Longest well-formed command is well under this; anything longer is
    /// garbage and gets cut off instead of buffered.
    const MAX_LINE_BYTES: usize = 256;

    /// Same location convention as the dictate runtime files: flat under
    /// `$XDG_RUNTIME_DIR` (per-user, mode 0700), temp dir as the fallback.
    pub fn gui_socket_path() -> PathBuf {
        let dir = match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(value) => PathBuf::from(value),
            None => std::env::temp_dir(),
        };

        dir.join("lazy-allrounder-gui.sock")
    }

    /// The GUI's end of the control socket. Owns the socket file: it is
    /// removed again on drop.
    pub struct GuiCommandListener {
        listener: UnixListener,
        path: PathBuf,
    }

    impl GuiCommandListener {
        pub fn bind() -> Result<Self, PortError> {
            Self::bind_at(gui_socket_path())
        }

        /// Binds with stale-socket recovery: a leftover file from a crashed
        /// GUI refuses connections, so probe before giving up — but never
        /// unlink a socket another live instance is serving.
        fn bind_at(path: PathBuf) -> Result<Self, PortError> {
            match UnixListener::bind(&path) {
                Ok(listener) => return Ok(Self { listener, path }),
                Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {}
                Err(error) => {
                    return Err(PortError::Other {
                        message: format!(
                            "could not bind the control socket at {}: {error}",
                            path.display()
                        ),
                    });
                }
            }

            if UnixStream::connect(&path).is_ok() {
                return Err(PortError::Other {
                    message: format!(
                        "another instance is serving the control socket at {}",
                        path.display()
                    ),
                });
            }

            std::fs::remove_file(&path).map_err(|error| PortError::Other {
                message: format!(
                    "could not remove the stale control socket at {}: {error}",
                    path.display()
                ),
            })?;

            let listener = UnixListener::bind(&path).map_err(|error| PortError::Other {
                message: format!(
                    "could not bind the control socket at {}: {error}",
                    path.display()
                ),
            })?;

            Ok(Self { listener, path })
        }

        /// Serves connections until accepting fails terminally. Sequential
        /// by design: commands arrive at keyboard-shortcut rate, and the
        /// read timeout bounds how long one silent client can occupy the
        /// loop.
        pub fn run(self, mut on_command: impl FnMut(GuiCommand)) {
            loop {
                let mut stream = match self.listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        tracing::warn!("control socket accept failed, shutting it down: {error}");
                        return;
                    }
                };

                match read_command(&mut stream) {
                    Ok(command) => {
                        on_command(command);
                        let _ = stream.write_all(b"ok\n");
                    }
                    Err(message) => {
                        tracing::debug!("rejected a control socket command: {message}");
                        let _ = stream.write_all(format!("err {message}\n").as_bytes());
                    }
                }
            }
        }
    }

    impl Drop for GuiCommandListener {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn read_command(stream: &mut UnixStream) -> Result<GuiCommand, String> {
        let line = read_line(stream)?;
        parse_gui_command(&line)
    }

    fn read_line(stream: &mut UnixStream) -> Result<String, String> {
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));

        let mut buffer = [0u8; MAX_LINE_BYTES];
        let mut filled = 0;

        loop {
            match stream.read(&mut buffer[filled..]) {
                Ok(0) => break,
                Ok(read) => {
                    filled += read;
                    if buffer[..filled].contains(&b'\n') {
                        break;
                    }
                    if filled == buffer.len() {
                        return Err("command is too long".to_owned());
                    }
                }
                Err(error) => return Err(format!("could not read the command: {error}")),
            }
        }

        let raw = std::str::from_utf8(&buffer[..filled])
            .map_err(|_| "command is not valid UTF-8".to_owned())?;
        Ok(raw.lines().next().unwrap_or("").to_owned())
    }

    /// Sends one command to a running GUI and waits for its acknowledgement.
    /// An `ok` reply means the command was queued to the UI thread, not that
    /// the action has finished.
    pub fn send_gui_command(command: GuiCommand) -> Result<(), SendError> {
        send_gui_command_at(&gui_socket_path(), command)
    }

    fn send_gui_command_at(path: &Path, command: GuiCommand) -> Result<(), SendError> {
        let mut stream = match UnixStream::connect(path) {
            Ok(stream) => stream,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) =>
            {
                return Err(SendError::NotRunning);
            }
            Err(error) => return Err(SendError::Io(error.to_string())),
        };

        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
        stream
            .write_all(format!("{}\n", format_gui_command(command)).as_bytes())
            .map_err(|error| SendError::Io(error.to_string()))?;

        let reply = read_line(&mut stream).map_err(SendError::Io)?;
        let reply = reply.trim();

        if reply == "ok" {
            return Ok(());
        }

        match reply.strip_prefix("err ") {
            Some(message) => Err(SendError::Rejected(message.to_owned())),
            None => Err(SendError::Io(format!("unexpected reply {reply:?}"))),
        }
    }

    #[cfg(test)]
    mod tests {
        use std::os::unix::net::UnixListener;

        use super::*;
        use crate::ipc::GuiAction;

        /// A per-test socket path that stays under the ~104-byte sun_path
        /// limit; the kernel namespace is per-file so tests cannot collide
        /// as long as the names differ.
        fn test_socket_path(name: &str) -> PathBuf {
            let dir = std::env::temp_dir().join(format!("la-ipc-{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("test dir should be creatable");
            dir.join(format!("{name}.sock"))
        }

        #[test]
        fn bind_recovers_a_stale_socket_file() {
            let path = test_socket_path("stale");
            // A bound-then-dropped listener whose file survived (SIGKILL
            // shape): recreate that by binding and leaking the file.
            let listener = UnixListener::bind(&path).expect("first bind should work");
            drop(listener);
            assert!(path.exists(), "dropping a raw UnixListener keeps the file");

            let recovered = GuiCommandListener::bind_at(path.clone());
            assert!(recovered.is_ok(), "stale socket should be reclaimed");
            drop(recovered);
            assert!(!path.exists(), "drop should remove the socket file");
        }

        #[test]
        fn bind_refuses_when_another_listener_is_live() {
            let path = test_socket_path("live");
            let first = GuiCommandListener::bind_at(path.clone()).expect("first bind");

            // Keep `first` accepting so the probe connects: a Unix socket
            // connect succeeds while the listener exists, even before
            // accept() is called (the connection sits in the backlog).
            let second = GuiCommandListener::bind_at(path.clone());
            assert!(second.is_err(), "live socket must not be stolen");
            assert!(path.exists());
            drop(first);
        }

        #[test]
        fn client_and_server_complete_one_command() {
            let path = test_socket_path("roundtrip");
            let listener = GuiCommandListener::bind_at(path.clone()).expect("bind");

            let server = std::thread::spawn(move || {
                let mut received = None;
                let (mut stream, _) = listener.listener.accept().expect("accept");
                match read_command(&mut stream) {
                    Ok(command) => {
                        received = Some(command);
                        let _ = stream.write_all(b"ok\n");
                    }
                    Err(message) => {
                        let _ = stream.write_all(format!("err {message}\n").as_bytes());
                    }
                }
                received
            });

            let sent = send_gui_command_at(&path, GuiCommand::Trigger(GuiAction::Dictate));
            assert!(sent.is_ok(), "send should be acknowledged: {sent:?}");
            assert_eq!(
                server.join().expect("server thread"),
                Some(GuiCommand::Trigger(GuiAction::Dictate))
            );
        }

        #[test]
        fn sending_without_a_listener_reports_not_running() {
            let path = test_socket_path("absent");
            let _ = std::fs::remove_file(&path);
            let result = send_gui_command_at(&path, GuiCommand::Stop);
            assert!(matches!(result, Err(SendError::NotRunning)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_round_trips_through_its_wire_form() {
        let commands = [
            GuiCommand::TogglePanel,
            GuiCommand::Stop,
            GuiCommand::Trigger(GuiAction::Read),
            GuiCommand::Trigger(GuiAction::Summarize),
            GuiCommand::Trigger(GuiAction::Explain),
            GuiCommand::Trigger(GuiAction::Ask),
            GuiCommand::Trigger(GuiAction::Dictate),
        ];

        for command in commands {
            assert_eq!(parse_gui_command(format_gui_command(command)), Ok(command));
        }
    }

    #[test]
    fn parsing_tolerates_case_whitespace_and_a_trailing_newline() {
        assert_eq!(
            parse_gui_command("  TOGGLE-PANEL \n"),
            Ok(GuiCommand::TogglePanel)
        );
        assert_eq!(
            parse_gui_command("Trigger   Read\n"),
            Ok(GuiCommand::Trigger(GuiAction::Read))
        );
    }

    #[test]
    fn garbage_is_rejected_with_a_reason() {
        assert!(parse_gui_command("").is_err());
        assert!(parse_gui_command("open sesame").is_err());
        assert!(parse_gui_command("trigger").is_err());
        assert!(parse_gui_command("trigger dance").is_err());
        assert!(parse_gui_command("trigger read now").is_err());
        assert!(parse_gui_command("stop everything").is_err());
    }
}
