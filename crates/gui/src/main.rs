use lazy_allrounder_app::{Application, ensure_configuration_file};
use lazy_allrounder_core::config::AppConfiguration;
use lazy_allrounder_platform::AudioPlayer;
use tracing_subscriber::{EnvFilter, fmt};

mod actions;
mod hotkeys;
mod icon;
mod overlay;
mod session;
mod state;
mod tray;

fn main() {
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .without_time()
        .init();

    let player = AudioPlayer::new();

    // A missing config file is auto-provisioned with defaults; a broken one
    // falls back to defaults here and surfaces its error in the panel when
    // the overlay retries. The GUI must always be able to start.
    let config = match ensure_configuration_file(None) {
        Ok(loaded) => loaded.config,
        Err(error) => {
            tracing::warn!("using default configuration: {error}");
            AppConfiguration::default()
        }
    };

    // The overlay is the default UI everywhere. Tray mode is an explicit
    // opt-in for desktops with a real StatusNotifierItem tray — on stock
    // GNOME the tray icon would be invisible without an extension, which is
    // strictly worse than a floating window.
    let force_tray = std::env::var("LAZY_ALLROUNDER_UI").is_ok_and(|ui| ui == "tray");
    if force_tray {
        let application = ensure_configuration_file(None)
            .and_then(|loaded| Application::from_loaded_configuration(&loaded));
        match application {
            Ok(application) => tray::run(application, player),
            Err(error) => {
                tracing::error!("tray mode requires a working configuration: {error}");
                std::process::exit(1);
            }
        }
        return;
    }

    if lazy_allrounder_platform::is_wayland_session() {
        tracing::info!(
            "Wayland session: the compositor controls window position and \
             stacking, and global hotkeys need desktop-native shortcuts \
             (see the README); the badge still works with the mouse"
        );
    }

    if let Err(error) = overlay::run(config.overlay.corner, config.hotkeys.clone(), player) {
        tracing::error!("the overlay window failed: {error}");
        std::process::exit(1);
    }
}
