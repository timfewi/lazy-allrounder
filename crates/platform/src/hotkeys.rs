//! Global hotkey parsing and registration (via the global-hotkey crate).
//!
//! Works on X11, macOS, and Windows. Wayland compositors do not allow apps
//! to grab global keys, so registration fails there — callers should treat
//! that as a soft failure and fall back to desktop-native shortcuts.

use std::collections::HashMap;

use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use lazy_allrounder_core::error::PortError;

/// Parses a user-facing binding like "Super+W", "ctrl+shift+d", or "F9" into
/// a registerable hotkey. Pure string handling — unit-testable everywhere.
pub fn parse_binding(binding: &str) -> Result<HotKey, PortError> {
    let normalized = normalize_binding(binding)?;
    normalized
        .parse::<HotKey>()
        .map_err(|error| PortError::Other {
            message: format!("invalid hotkey binding {binding:?}: {error}"),
        })
}

/// Rewrites friendly key names into the KeyW/Digit1-style codes the parser
/// expects, leaving modifiers and already-correct codes untouched.
fn normalize_binding(binding: &str) -> Result<String, PortError> {
    if binding.trim().is_empty() {
        return Err(PortError::Other {
            message: "hotkey binding is empty".to_owned(),
        });
    }

    let parts: Vec<String> = binding
        .split('+')
        .map(|part| {
            let part = part.trim();
            let lower = part.to_ascii_lowercase();
            match lower.as_str() {
                "super" | "cmd" | "command" | "win" | "meta" => "super".to_owned(),
                "ctrl" | "control" => "control".to_owned(),
                "alt" | "option" => "alt".to_owned(),
                "shift" => "shift".to_owned(),
                _ => normalize_key(part),
            }
        })
        .collect();

    Ok(parts.join("+"))
}

fn normalize_key(key: &str) -> String {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(letter), None) if letter.is_ascii_alphabetic() => {
            format!("Key{}", letter.to_ascii_uppercase())
        }
        (Some(digit), None) if digit.is_ascii_digit() => format!("Digit{digit}"),
        _ => key.to_owned(),
    }
}

/// Registered hotkeys: keeps the OS-level registration alive and maps event
/// ids back to the caller's action names.
pub struct RegisteredHotkeys {
    // Held for its Drop side effect: dropping unregisters every hotkey.
    _manager: GlobalHotKeyManager,
    actions_by_id: HashMap<u32, String>,
}

impl RegisteredHotkeys {
    pub fn action_for(&self, hotkey_id: u32) -> Option<&str> {
        self.actions_by_id.get(&hotkey_id).map(String::as_str)
    }
}

/// Registers every (action, binding) pair. Fails as a whole if the platform
/// has no global-hotkey support (e.g. Wayland) or any binding is invalid.
pub fn register_hotkeys(bindings: &[(String, String)]) -> Result<RegisteredHotkeys, PortError> {
    let manager = GlobalHotKeyManager::new().map_err(|error| PortError::Other {
        message: format!("global hotkeys are unavailable on this session: {error}"),
    })?;

    let mut actions_by_id = HashMap::new();
    for (action, binding) in bindings {
        let hotkey = parse_binding(binding)?;
        manager.register(hotkey).map_err(|error| PortError::Other {
            message: format!("could not register {binding:?} for {action}: {error}"),
        })?;
        actions_by_id.insert(hotkey.id(), action.clone());
    }

    Ok(RegisteredHotkeys {
        _manager: manager,
        actions_by_id,
    })
}

/// Blocks until the next hotkey *press* (releases are filtered out) and
/// returns its id. Returns None once the event channel closes.
pub fn next_hotkey_press() -> Option<u32> {
    loop {
        let event = GlobalHotKeyEvent::receiver().recv().ok()?;
        if event.state() == HotKeyState::Pressed {
            return Some(event.id());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_friendly_single_letter_bindings() {
        let hotkey = parse_binding("Super+W").expect("Super+W should parse");
        let same = parse_binding("super+KeyW").expect("explicit code should parse");
        assert_eq!(hotkey.id(), same.id());
    }

    #[test]
    fn parses_digits_and_function_keys() {
        parse_binding("ctrl+1").expect("digit binding should parse");
        parse_binding("F9").expect("bare function key should parse");
        parse_binding("alt+shift+Space").expect("named key should parse");
    }

    #[test]
    fn modifier_aliases_normalize_to_the_same_hotkey() {
        let cmd = parse_binding("Cmd+D").expect("cmd alias");
        let win = parse_binding("win+d").expect("win alias");
        let meta = parse_binding("META+D").expect("meta alias");
        assert_eq!(cmd.id(), win.id());
        assert_eq!(win.id(), meta.id());
    }

    #[test]
    fn rejects_empty_and_garbage_bindings() {
        assert!(parse_binding("").is_err());
        assert!(parse_binding("   ").is_err());
        assert!(parse_binding("super+NotARealKey").is_err());
    }
}
