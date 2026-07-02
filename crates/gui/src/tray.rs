//! System-tray mode: an opt-in alternative UI (`LAZY_ALLROUNDER_UI=tray`)
//! for users who prefer a tray icon over the floating badge — e.g. desktops
//! with a proper StatusNotifierItem tray. Feedback goes through desktop
//! notifications since there is no badge to animate.
//!
//! Linux-only for now: tray-icon's Linux backend needs a GTK main loop,
//! which this module owns. On macOS/Windows the overlay is the primary UI.

#[cfg(target_os = "linux")]
pub use linux::run;

#[cfg(not(target_os = "linux"))]
pub fn run(
    _application: lazy_allrounder_app::Application,
    _player: lazy_allrounder_platform::AudioPlayer,
) {
    tracing::error!("tray mode is currently Linux-only; run the overlay instead");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::sync::mpsc::channel;
    use std::thread;

    use lazy_allrounder_app::Application;
    use lazy_allrounder_platform::{AudioPlayer, notify};
    use tray_icon::{
        Icon, TrayIconBuilder,
        menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
    };

    use crate::session::{ActionRequest, run_action};
    use crate::state::Mode;

    enum TrayCommand {
        Trigger(Mode),
        Stop,
    }

    pub fn run(application: Application, player: AudioPlayer) {
        if let Err(error) = gtk::init() {
            tracing::error!("failed to initialize GTK for the tray: {error}");
            std::process::exit(1);
        }

        let menu = Menu::new();
        let mut mode_ids: HashMap<MenuId, Mode> = HashMap::new();
        for mode in [Mode::Read, Mode::Summarize, Mode::Explain, Mode::Dictate] {
            let item = MenuItem::new(mode.label(), true, None);
            mode_ids.insert(item.id().clone(), mode);
            menu.append(&item).expect("failed to build the tray menu");
        }
        let stop_item = MenuItem::new("Stop audio", true, None);
        let stop_id = stop_item.id().clone();
        menu.append(&stop_item)
            .expect("failed to build the tray menu");
        menu.append(&PredefinedMenuItem::separator())
            .expect("failed to build the tray menu");
        let quit_item = MenuItem::new("Quit", true, None);
        let quit_id = quit_item.id().clone();
        menu.append(&quit_item)
            .expect("failed to build the tray menu");

        let _tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Lazy Allrounder")
            .with_icon(tray_icon())
            .build()
            .expect("failed to create the tray icon");

        // Worker thread: runs the actual actions so the GTK loop stays live.
        let (commands, command_receiver) = channel::<TrayCommand>();
        let worker_player = player.clone();
        thread::Builder::new()
            .name("lazy-allrounder-tray-worker".to_owned())
            .spawn(move || {
                let runtime =
                    tokio::runtime::Runtime::new().expect("failed to start the async runtime");
                while let Ok(command) = command_receiver.recv() {
                    match command {
                        TrayCommand::Stop => worker_player.stop(),
                        TrayCommand::Trigger(mode) => {
                            let request = ActionRequest {
                                mode,
                                question: None,
                            };
                            let result =
                                runtime.block_on(run_action(&application, &worker_player, request));
                            match result {
                                Ok(()) => {
                                    if mode == Mode::Dictate {
                                        notify("Dictation", "Dictation toggled.");
                                    }
                                }
                                Err(message) => notify(mode.label(), &message),
                            }
                        }
                    }
                }
            })
            .expect("failed to spawn the tray worker thread");

        // Menu events arrive on the GTK loop; forward them to the worker.
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == quit_id {
                gtk::main_quit();
            } else if event.id == stop_id {
                let _ = commands.send(TrayCommand::Stop);
            } else if let Some(mode) = mode_ids.get(&event.id) {
                let _ = commands.send(TrayCommand::Trigger(*mode));
            }
        }));

        tracing::info!("lazy-allrounder tray started");
        gtk::main();
    }

    // The waveform logo, shared with the overlay window icon. If the embedded
    // asset ever fails to decode we fall back to a solid accent-blue square so
    // the tray still has a visible icon.
    fn tray_icon() -> Icon {
        if let Some(icon) = crate::icon::decode()
            && let Ok(icon) = Icon::from_rgba(icon.rgba, icon.width, icon.height)
        {
            return icon;
        }

        const SIZE: u32 = 32;
        let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
        for _ in 0..(SIZE * SIZE) {
            rgba.extend_from_slice(&[0x4A, 0x9E, 0xE0, 0xFF]);
        }
        Icon::from_rgba(rgba, SIZE, SIZE).expect("placeholder icon dimensions should be valid")
    }
}
