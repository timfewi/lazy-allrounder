use arboard::Clipboard;
use lazy_allrounder_core::error::PortError;

/// Reads the current text contents of the system clipboard.
///
/// Requires a live display server (X11/Wayland on Linux, or the native
/// clipboard service on macOS/Windows) — not exercisable in a headless
/// test environment, so this has no unit tests of its own. Callers should
/// keep any branching logic that depends on the result (empty vs missing
/// vs present) in a separate, pure function they can unit test instead.
pub fn read_text() -> Result<String, PortError> {
    let mut clipboard = Clipboard::new().map_err(|error| PortError::Other {
        message: format!("failed to access the system clipboard: {error}"),
    })?;

    clipboard.get_text().map_err(|error| PortError::Other {
        message: format!("failed to read text from the clipboard: {error}"),
    })
}

/// Reads the text the user currently has highlighted, falling back to the
/// clipboard — so on Linux, selecting text is enough and Ctrl+C is optional.
///
/// The PRIMARY selection only exists on Linux, and this build reads it via
/// arboard's X11 backend — on Wayland sessions that means through XWayland,
/// where the compositor bridges selections both ways; without any X server
/// the read fails and the clipboard fallback applies. Elsewhere this is
/// exactly `read_text`. A missing or blank selection is not an error — it
/// just means "use the clipboard".
pub fn read_selection_or_clipboard() -> Result<String, PortError> {
    #[cfg(target_os = "linux")]
    match read_primary_selection() {
        Ok(text) if !text.trim().is_empty() => return Ok(text),
        Ok(_) => {}
        Err(error) => {
            tracing::debug!("no readable primary selection, using the clipboard: {error}");
        }
    }

    read_text()
}

#[cfg(target_os = "linux")]
fn read_primary_selection() -> Result<String, PortError> {
    use arboard::{GetExtLinux, LinuxClipboardKind};

    let mut clipboard = Clipboard::new().map_err(|error| PortError::Other {
        message: format!("failed to access the system clipboard: {error}"),
    })?;

    clipboard
        .get()
        .clipboard(LinuxClipboardKind::Primary)
        .text()
        .map_err(|error| PortError::Other {
            message: format!("failed to read the primary selection: {error}"),
        })
}
