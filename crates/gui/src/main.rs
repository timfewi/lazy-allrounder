use lazy_allrounder_app::{Application, load_configuration};
use lazy_allrounder_platform::AudioPlayer;
use tracing_subscriber::{EnvFilter, fmt};

mod actions;
mod hotkeys;
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

    // Configuration/credential problems must not kill the GUI before it can
    // tell the user what is wrong — the overlay starts either way and shows
    // the error in its panel.
    let (application, startup_error, overlay_config, hotkeys_config) = match load_application() {
        Ok((application, overlay_config, hotkeys_config)) => {
            (Some(application), None, overlay_config, hotkeys_config)
        }
        Err(error) => {
            tracing::error!("starting unconfigured: {error}");
            (
                None,
                Some(error.to_string()),
                Default::default(),
                Default::default(),
            )
        }
    };

    // The overlay is the default UI everywhere. Tray mode is an explicit
    // opt-in for desktops with a real StatusNotifierItem tray — on stock
    // GNOME the tray icon would be invisible without an extension, which is
    // strictly worse than a floating window.
    let force_tray = std::env::var("LAZY_ALLROUNDER_UI").is_ok_and(|ui| ui == "tray");
    if force_tray {
        let Some(application) = application else {
            tracing::error!("tray mode requires a working configuration");
            std::process::exit(1);
        };
        tray::run(application, player);
        return;
    }

    if lazy_allrounder_platform::is_wayland_session() {
        tracing::info!(
            "Wayland session: the compositor controls window position and \
             stacking, and global hotkeys need desktop-native shortcuts \
             (see the README); the badge still works with the mouse"
        );
    }

    if let Err(error) = overlay::run(
        application,
        startup_error,
        overlay_config.corner,
        hotkeys_config,
        player,
    ) {
        tracing::error!("the overlay window failed: {error}");
        std::process::exit(1);
    }
}

type LoadOutcome = (
    Application,
    lazy_allrounder_core::config::OverlayConfiguration,
    lazy_allrounder_core::config::HotkeysConfiguration,
);

fn load_application() -> Result<LoadOutcome, Box<dyn std::error::Error>> {
    let loaded = load_configuration(None)?;
    let overlay_config = loaded.config.overlay;
    let hotkeys_config = loaded.config.hotkeys.clone();
    let application = Application::from_loaded_configuration(&loaded)?;
    Ok((application, overlay_config, hotkeys_config))
}
