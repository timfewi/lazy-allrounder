//! Start-on-login registration (XDG autostart on Linux, LaunchAgents on
//! macOS, the registry Run key on Windows) via the auto-launch crate.
//!
//! Touches real OS state — integration behavior, no unit tests here.

use auto_launch::AutoLaunchBuilder;
use lazy_allrounder_core::error::PortError;

fn launcher() -> Result<auto_launch::AutoLaunch, PortError> {
    let exe = std::env::current_exe().map_err(|error| PortError::Other {
        message: format!("could not determine the app's own path: {error}"),
    })?;

    AutoLaunchBuilder::new()
        .set_app_name("lazy-allrounder")
        .set_app_path(&exe.to_string_lossy())
        .build()
        .map_err(|error| PortError::Other {
            message: format!("could not prepare the start-on-login entry: {error}"),
        })
}

pub fn is_autostart_enabled() -> Result<bool, PortError> {
    launcher()?.is_enabled().map_err(|error| PortError::Other {
        message: format!("could not check the start-on-login entry: {error}"),
    })
}

pub fn set_autostart(enabled: bool) -> Result<(), PortError> {
    let launcher = launcher()?;
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };

    result.map_err(|error| PortError::Other {
        message: format!("could not update the start-on-login entry: {error}"),
    })
}
