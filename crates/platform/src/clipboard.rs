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
