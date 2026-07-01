// Linux-only focused-app insertion implementation.
// Rename this file to insertion_linux.rs when macOS or Windows insertion
// modules are added, and keep text_insertion.rs as the cross-platform entrypoint.
use std::{
    env,
    io::{self, Write},
    process::{Command, Stdio},
};

use lazy_allrounder_core::error::PortError;

pub fn insert_text_into_focused_app(text: &str) -> Result<(), PortError> {
    if text.is_empty() {
        return Err(PortError::Other {
            message: "cannot insert an empty transcript".to_owned(),
        });
    }

    match session_kind()? {
        SessionKind::Wayland => insert_on_wayland(text),
        SessionKind::X11 => insert_on_x11(text),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionKind {
    Wayland,
    X11,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandAttempt {
    Succeeded,
    Failed,
    MissingDependency,
}

fn insert_on_wayland(text: &str) -> Result<(), PortError> {
    insert_on_wayland_with(
        text,
        &mut |action| run_linux_insert_action(action, text),
        &mut |action, copied_text| run_linux_clipboard_action(action, copied_text),
    )
}

fn insert_on_x11(text: &str) -> Result<(), PortError> {
    insert_on_x11_with(
        text,
        &mut |action| run_linux_insert_action(action, text),
        &mut |action, copied_text| run_linux_clipboard_action(action, copied_text),
    )
}

fn insert_on_wayland_with<Run, Copy>(
    text: &str,
    run_action: &mut Run,
    copy_text: &mut Copy,
) -> Result<(), PortError>
where
    Run: FnMut(InsertAction) -> Result<CommandAttempt, PortError>,
    Copy: FnMut(ClipboardAction, &str) -> Result<CommandAttempt, PortError>,
{
    if run_action(InsertAction::WaylandType)? == CommandAttempt::Succeeded {
        return Ok(());
    }

    let clipboard = copy_text(ClipboardAction::WaylandClipboard, text)?;
    if clipboard == CommandAttempt::Succeeded && try_wayland_paste_with(run_action)? {
        return Ok(());
    }

    if clipboard == CommandAttempt::Succeeded {
        return Err(PortError::Other {
            message:
                "failed to paste the transcript into the focused Wayland application; the transcript was copied to the clipboard"
                    .to_owned(),
        });
    }

    Err(PortError::Other {
        message:
            "focused-app insertion on Wayland requires wtype for typing and wl-copy for clipboard fallback"
                .to_owned(),
    })
}

fn insert_on_x11_with<Run, Copy>(
    text: &str,
    run_action: &mut Run,
    copy_text: &mut Copy,
) -> Result<(), PortError>
where
    Run: FnMut(InsertAction) -> Result<CommandAttempt, PortError>,
    Copy: FnMut(ClipboardAction, &str) -> Result<CommandAttempt, PortError>,
{
    if run_action(InsertAction::X11Type)? == CommandAttempt::Succeeded {
        return Ok(());
    }

    let clipboard = copy_text(ClipboardAction::X11Clipboard, text)?;
    if clipboard == CommandAttempt::Succeeded && try_x11_paste_with(run_action)? {
        return Ok(());
    }

    if clipboard == CommandAttempt::Succeeded {
        return Err(PortError::Other {
            message:
                "failed to paste the transcript into the focused X11 application; the transcript was copied to the clipboard"
                    .to_owned(),
        });
    }

    Err(PortError::Other {
        message:
            "focused-app insertion on X11 requires xdotool for typing and xclip for clipboard fallback"
                .to_owned(),
    })
}

fn try_wayland_paste_with(
    run_action: &mut impl FnMut(InsertAction) -> Result<CommandAttempt, PortError>,
) -> Result<bool, PortError> {
    Ok(
        run_action(InsertAction::WaylandPasteCtrlShiftV)? == CommandAttempt::Succeeded
            || run_action(InsertAction::WaylandPasteCtrlV)? == CommandAttempt::Succeeded,
    )
}

fn try_x11_paste_with(
    run_action: &mut impl FnMut(InsertAction) -> Result<CommandAttempt, PortError>,
) -> Result<bool, PortError> {
    Ok(
        run_action(InsertAction::X11PasteCtrlV)? == CommandAttempt::Succeeded
            || run_action(InsertAction::X11PasteShiftInsert)? == CommandAttempt::Succeeded,
    )
}

fn session_kind() -> Result<SessionKind, PortError> {
    detect_session_kind(env::var_os("WAYLAND_DISPLAY"), env::var_os("DISPLAY"))
}

fn detect_session_kind(
    wayland_display: Option<std::ffi::OsString>,
    display: Option<std::ffi::OsString>,
) -> Result<SessionKind, PortError> {
    match crate::display::detect(wayland_display, display) {
        Some(crate::display::SessionKind::Wayland) => Ok(SessionKind::Wayland),
        Some(crate::display::SessionKind::X11) => Ok(SessionKind::X11),
        None => Err(PortError::Other {
            message:
                "focused-app insertion requires a graphical Linux session with WAYLAND_DISPLAY or DISPLAY set"
                    .to_owned(),
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InsertAction {
    WaylandType,
    WaylandPasteCtrlShiftV,
    WaylandPasteCtrlV,
    X11Type,
    X11PasteCtrlV,
    X11PasteShiftInsert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipboardAction {
    WaylandClipboard,
    X11Clipboard,
}

fn run_linux_insert_action(action: InsertAction, text: &str) -> Result<CommandAttempt, PortError> {
    match action {
        InsertAction::WaylandType => run_command(Command::new("wtype").arg(text)),
        InsertAction::WaylandPasteCtrlShiftV => {
            run_command(Command::new("wtype").args(["--mods", "ctrl,shift", "v"]))
        }
        InsertAction::WaylandPasteCtrlV => {
            run_command(Command::new("wtype").args(["--mods", "ctrl", "v"]))
        }
        InsertAction::X11Type => run_command(Command::new("xdotool").args([
            "type",
            "--clearmodifiers",
            "--delay",
            "0",
            "--",
            text,
        ])),
        InsertAction::X11PasteCtrlV => {
            run_command(Command::new("xdotool").args(["key", "--clearmodifiers", "ctrl+v"]))
        }
        InsertAction::X11PasteShiftInsert => {
            run_command(Command::new("xdotool").args(["key", "--clearmodifiers", "shift+Insert"]))
        }
    }
}

fn run_linux_clipboard_action(
    action: ClipboardAction,
    text: &str,
) -> Result<CommandAttempt, PortError> {
    match action {
        ClipboardAction::WaylandClipboard => {
            let mut command = Command::new("wl-copy");
            pipe_text_to_command(&mut command, text)
        }
        ClipboardAction::X11Clipboard => {
            let mut command = Command::new("xclip");
            command.args(["-selection", "clipboard", "-in"]);
            pipe_text_to_command(&mut command, text)
        }
    }
}

fn run_command(command: &mut Command) -> Result<CommandAttempt, PortError> {
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());

    match command.status() {
        Ok(status) if status.success() => Ok(CommandAttempt::Succeeded),
        Ok(_) => Ok(CommandAttempt::Failed),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(CommandAttempt::MissingDependency)
        }
        Err(error) => Err(map_io_error(error)),
    }
}

fn pipe_text_to_command(command: &mut Command, text: &str) -> Result<CommandAttempt, PortError> {
    command.stdin(Stdio::piped());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(CommandAttempt::MissingDependency);
        }
        Err(error) => return Err(map_io_error(error)),
    };

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes()).map_err(map_io_error)?;
    }

    match child.wait() {
        Ok(status) if status.success() => Ok(CommandAttempt::Succeeded),
        Ok(_) => Ok(CommandAttempt::Failed),
        Err(error) => Err(map_io_error(error)),
    }
}

fn map_io_error(error: io::Error) -> PortError {
    PortError::Other {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, ffi::OsString, rc::Rc};

    use super::{
        ClipboardAction, CommandAttempt, InsertAction, SessionKind, detect_session_kind,
        insert_on_wayland_with,
    };

    #[test]
    fn detect_session_kind_prefers_wayland_when_both_displays_exist() {
        let session = detect_session_kind(
            Some(OsString::from("wayland-0")),
            Some(OsString::from(":0")),
        )
        .expect("graphical session should be detected");

        assert_eq!(session, SessionKind::Wayland);
    }

    #[test]
    fn detect_session_kind_requires_graphical_environment() {
        let error = detect_session_kind(None, None).expect_err("headless session should fail");

        assert_eq!(
            error.to_string(),
            "focused-app insertion requires a graphical Linux session with WAYLAND_DISPLAY or DISPLAY set"
        );
    }

    #[test]
    fn wayland_falls_back_to_clipboard_paste_when_direct_typing_fails() {
        let seen_actions = Rc::new(RefCell::new(Vec::new()));
        let copied_text = Rc::new(RefCell::new(String::new()));
        let action_log = Rc::clone(&seen_actions);
        let copied_log = Rc::clone(&copied_text);

        let result = insert_on_wayland_with(
            "hello world",
            &mut move |action| {
                action_log.borrow_mut().push(action);
                let attempt = match action {
                    InsertAction::WaylandType => CommandAttempt::Failed,
                    InsertAction::WaylandPasteCtrlShiftV => CommandAttempt::Failed,
                    InsertAction::WaylandPasteCtrlV => CommandAttempt::Succeeded,
                    _ => unreachable!("wayland test should only run wayland actions"),
                };
                Ok(attempt)
            },
            &mut move |action, text| {
                assert_eq!(action, ClipboardAction::WaylandClipboard);
                copied_log.borrow_mut().push_str(text);
                Ok(CommandAttempt::Succeeded)
            },
        );

        result.expect("clipboard fallback should succeed");
        assert_eq!(copied_text.borrow().as_str(), "hello world");
        assert_eq!(
            seen_actions.borrow().as_slice(),
            &[
                InsertAction::WaylandType,
                InsertAction::WaylandPasteCtrlShiftV,
                InsertAction::WaylandPasteCtrlV,
            ]
        );
    }

    #[test]
    fn wayland_surfaces_missing_dependencies_when_no_insert_path_exists() {
        let error = insert_on_wayland_with(
            "hello world",
            &mut |_action| Ok(CommandAttempt::MissingDependency),
            &mut |_action, _text| Ok(CommandAttempt::MissingDependency),
        )
        .expect_err("missing insertion tools should fail");

        assert_eq!(
            error.to_string(),
            "focused-app insertion on Wayland requires wtype for typing and wl-copy for clipboard fallback"
        );
    }
}
