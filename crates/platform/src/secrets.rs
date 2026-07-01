//! API-key storage in the OS keyring (Keychain on macOS, Credential Manager
//! on Windows, Secret Service/libsecret on Linux), so non-terminal users
//! never have to manage environment variables.
//!
//! Touches the real OS keyring — integration behavior, no unit tests here;
//! callers keep their decision logic (env-var precedence, onboarding) in
//! separately testable code.

use keyring::Entry;
use lazy_allrounder_core::error::PortError;

const SERVICE: &str = "lazy-allrounder";
const KEY_NAME: &str = "openrouter-api-key";

fn entry() -> Result<Entry, PortError> {
    Entry::new(SERVICE, KEY_NAME).map_err(|error| PortError::Other {
        message: format!("the OS keyring is unavailable: {error}"),
    })
}

pub fn store_api_key(api_key: &str) -> Result<(), PortError> {
    entry()?
        .set_password(api_key)
        .map_err(|error| PortError::Other {
            message: format!("could not save the API key to the OS keyring: {error}"),
        })
}

/// The stored API key, or None if none has been saved yet.
pub fn load_api_key() -> Result<Option<String>, PortError> {
    match entry()?.get_password() {
        Ok(api_key) => Ok(Some(api_key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(PortError::Other {
            message: format!("could not read the API key from the OS keyring: {error}"),
        }),
    }
}

pub fn delete_api_key() -> Result<(), PortError> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(PortError::Other {
            message: format!("could not remove the API key from the OS keyring: {error}"),
        }),
    }
}
