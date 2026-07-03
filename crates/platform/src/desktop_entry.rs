//! Linux desktop integration: a per-user `.desktop` entry plus a hicolor
//! icon, so the desktop shows the app's real name and waveform icon instead
//! of a letter fallback.
//!
//! GNOME (and most desktops) identify a window by matching its Wayland
//! `app_id` / X11 `WM_CLASS` against an installed `.desktop` file — the
//! in-process window icon is ignored on Wayland by design. Installing the
//! entry at startup is what makes the badge stop rendering as an "L".

use std::fs;
use std::path::PathBuf;

use lazy_allrounder_core::error::PortError;

use crate::APP_ID;

/// Entry/icon names installed by pre-rename builds; removed on sight so a
/// user who ran an older build doesn't end up with two launcher entries.
const LEGACY_APP_ID: &str = "lazy-allrounder";

/// Idempotently installs `~/.local/share/applications/{APP_ID}.desktop`
/// and the hicolor icon it references, pointing Exec at the current
/// executable. Existing files are only rewritten when their content changed
/// (e.g. the binary moved), so repeated startups are no-ops.
///
/// The file name deliberately matches the entry that the .deb/AppImage
/// packaging ships under /usr/share/applications: per the XDG data-dir
/// precedence, the per-user copy shadows the system one instead of showing
/// up as a duplicate launcher.
pub fn install_desktop_integration(icon_png: &[u8]) -> Result<(), PortError> {
    let data_home = xdg_data_home()?;

    let icon_path = data_home
        .join("icons/hicolor/512x512/apps")
        .join(format!("{APP_ID}.png"));
    write_if_changed(&icon_path, icon_png)?;

    let exe = std::env::current_exe().map_err(|error| PortError::Other {
        message: format!("could not determine the app's own path: {error}"),
    })?;
    let entry = desktop_entry(&exe.to_string_lossy());
    let entry_path = data_home
        .join("applications")
        .join(format!("{APP_ID}.desktop"));
    write_if_changed(&entry_path, entry.as_bytes())?;

    // Best-effort cleanup of the old names; a failure here is cosmetic.
    let _ = fs::remove_file(
        data_home
            .join("applications")
            .join(format!("{LEGACY_APP_ID}.desktop")),
    );
    let _ = fs::remove_file(
        data_home
            .join("icons/hicolor/512x512/apps")
            .join(format!("{LEGACY_APP_ID}.png")),
    );

    Ok(())
}

fn xdg_data_home() -> Result<PathBuf, PortError> {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME")
        && !dir.is_empty()
    {
        return Ok(PathBuf::from(dir));
    }

    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(".local/share"))
        .ok_or(PortError::Other {
            message: "neither XDG_DATA_HOME nor HOME is set".to_owned(),
        })
}

fn desktop_entry(exe: &str) -> String {
    // Exec is quote-escaped per the Desktop Entry spec so paths with spaces
    // survive; StartupWMClass ties X11/XWayland windows to this entry, and
    // the file name (app id) ties Wayland-native windows to it.
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Lazy Allrounder\n\
         Comment=Read aloud, summarize, explain, ask, and dictate anywhere\n\
         Exec={exec}\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=Utility;Accessibility;\n\
         Keywords=read aloud;summarize;dictate;tts;\n\
         StartupWMClass={APP_ID}\n\
         StartupNotify=false\n",
        exec = quote_exec(exe),
    )
}

/// Quotes an Exec value per the Desktop Entry spec's three layers: reserved
/// characters require the whole argument double-quoted with `"`, `` ` ``,
/// `$`, `\` backslash-escaped (quoting layer); every backslash of that
/// quoting-layer output is then itself doubled for the key-file string layer
/// (a literal `$` is `\\$` in file bytes, a literal `\` is `\\\\`); and a
/// literal `%` is always doubled so launchers don't expand it as a field
/// code.
fn quote_exec(path: &str) -> String {
    const RESERVED: &[char] = &[
        ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(',
        ')', '`',
    ];
    if !path.contains(RESERVED) {
        return path.replace('%', "%%");
    }

    let mut quoted = String::with_capacity(path.len() + 2);
    quoted.push('"');
    for character in path.chars() {
        match character {
            // Quoting layer: `\\` — string layer doubles both: 4 backslashes.
            '\\' => quoted.push_str("\\\\\\\\"),
            // Quoting layer: `\` + char — string layer doubles the backslash.
            '"' | '`' | '$' => {
                quoted.push_str("\\\\");
                quoted.push(character);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

fn write_if_changed(path: &PathBuf, contents: &[u8]) -> Result<(), PortError> {
    if fs::read(path).is_ok_and(|existing| existing == contents) {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| PortError::Other {
            message: format!("could not create {}: {error}", parent.display()),
        })?;
    }
    fs::write(path, contents).map_err(|error| PortError::Other {
        message: format!("could not write {}: {error}", path.display()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paths_are_left_unquoted() {
        assert_eq!(
            quote_exec("/usr/bin/lazy-allrounder"),
            "/usr/bin/lazy-allrounder"
        );
    }

    #[test]
    fn paths_with_spaces_are_quoted_and_double_escaped() {
        assert_eq!(
            quote_exec("/home/tim/My Apps/lazy-allrounder"),
            "\"/home/tim/My Apps/lazy-allrounder\""
        );
        // The quoting-layer backslash is itself string-layer escaped, so a
        // literal `"` becomes `\\"` and a literal `$` becomes `\\$` on disk.
        assert_eq!(quote_exec("/tmp/a\"b"), "\"/tmp/a\\\\\"b\"");
        assert_eq!(quote_exec("/tmp/a$b c"), "\"/tmp/a\\\\$b c\"");
        // A literal backslash escapes to `\\` at the quoting layer and each
        // of those doubles at the string layer: 4 backslashes on disk.
        assert_eq!(quote_exec("/tmp/a\\b"), "\"/tmp/a\\\\\\\\b\"");
    }

    #[test]
    fn percent_is_doubled_in_both_quoted_and_unquoted_paths() {
        assert_eq!(quote_exec("/tmp/50%off/app"), "/tmp/50%%off/app");
        assert_eq!(quote_exec("/tmp/50% off/app"), "\"/tmp/50%% off/app\"");
    }

    #[test]
    fn entry_references_the_app_id_everywhere() {
        let entry = desktop_entry("/usr/bin/lazy-allrounder");
        assert!(entry.contains("Exec=/usr/bin/lazy-allrounder\n"));
        assert!(entry.contains(&format!("Icon={APP_ID}\n")));
        assert!(entry.contains(&format!("StartupWMClass={APP_ID}\n")));
    }
}
