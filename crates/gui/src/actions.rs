use lazy_allrounder_app::Application;
use lazy_allrounder_core::error::PortError;

/// Outcome of validating clipboard contents before sending them anywhere —
/// kept separate from the async `Application` call so it stays pure and
/// unit-testable with a stubbed clipboard reader.
#[derive(Debug, PartialEq, Eq)]
pub enum ClipboardCheck {
    Ready(String),
    Empty,
    Unavailable(String),
}

pub fn check_clipboard(
    read_clipboard: impl FnOnce() -> Result<String, PortError>,
) -> ClipboardCheck {
    match read_clipboard() {
        Ok(text) if !text.trim().is_empty() => ClipboardCheck::Ready(text),
        Ok(_) => ClipboardCheck::Empty,
        Err(error) => ClipboardCheck::Unavailable(error.to_string()),
    }
}

pub fn clipboard_text(
    read_clipboard: impl FnOnce() -> Result<String, PortError>,
) -> Result<String, String> {
    match check_clipboard(read_clipboard) {
        ClipboardCheck::Ready(text) => Ok(text),
        ClipboardCheck::Empty => Err("nothing to read — select text or copy it first".to_owned()),
        ClipboardCheck::Unavailable(message) => Err(format!("could not read text: {message}")),
    }
}

/// Reads the clipboard, synthesizes speech through `Application::read`, and
/// hands the audio bytes to the player callback (blocking until playback
/// finishes). The player is injected so this stays testable without a device.
pub async fn read_clipboard_aloud(
    app: &Application,
    read_clipboard: impl FnOnce() -> Result<String, PortError>,
    play: impl FnOnce(Vec<u8>) -> Result<(), PortError>,
) -> Result<(), String> {
    let text = clipboard_text(read_clipboard)?;
    let audio = app.read(text).await.map_err(|error| error.to_string())?;
    play(audio).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_when_clipboard_has_non_blank_text() {
        let outcome = check_clipboard(|| Ok("hello".to_owned()));
        assert_eq!(outcome, ClipboardCheck::Ready("hello".to_owned()));
    }

    #[test]
    fn empty_when_clipboard_is_blank() {
        let outcome = check_clipboard(|| Ok("   \n\t".to_owned()));
        assert_eq!(outcome, ClipboardCheck::Empty);
    }

    #[test]
    fn empty_when_clipboard_has_no_text() {
        let outcome = check_clipboard(|| Ok(String::new()));
        assert_eq!(outcome, ClipboardCheck::Empty);
    }

    #[test]
    fn unavailable_when_clipboard_read_fails() {
        let outcome = check_clipboard(|| {
            Err(PortError::Other {
                message: "no display server".to_owned(),
            })
        });
        assert_eq!(
            outcome,
            ClipboardCheck::Unavailable("no display server".to_owned())
        );
    }

    #[test]
    fn clipboard_text_maps_outcomes_to_user_facing_errors() {
        assert_eq!(
            clipboard_text(|| Ok("text".to_owned())),
            Ok("text".to_owned())
        );
        assert_eq!(
            clipboard_text(|| Ok(String::new())),
            Err("nothing to read — select text or copy it first".to_owned())
        );
        assert_eq!(
            clipboard_text(|| Err(PortError::Other {
                message: "boom".to_owned()
            })),
            Err("could not read text: boom".to_owned())
        );
    }
}
