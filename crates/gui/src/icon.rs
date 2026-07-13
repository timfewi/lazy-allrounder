//! The embedded app icon (the logo mark from `assets/icon.png`), decoded
//! on demand into whichever icon a run needs — the eframe window icon in
//! overlay mode, or the tray icon in tray mode (the two are mutually
//! exclusive, so a launch decodes it once).

/// Raw RGBA pixels plus dimensions, ready for whatever icon type a backend
/// wants to build from them.
pub struct DecodedIcon {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

const ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/icon.png"
));

/// The raw embedded PNG bytes, for consumers that want the file itself
/// (e.g. installing the hicolor desktop icon) rather than decoded pixels.
#[cfg(target_os = "linux")]
pub fn png_bytes() -> &'static [u8] {
    ICON_PNG
}

/// Decodes the embedded PNG; a corrupt asset degrades to `None` (default
/// icon) instead of failing startup.
pub fn decode() -> Option<DecodedIcon> {
    match image::load_from_memory(ICON_PNG) {
        Ok(decoded) => {
            let rgba = decoded.into_rgba8();
            let (width, height) = rgba.dimensions();
            Some(DecodedIcon {
                rgba: rgba.into_raw(),
                width,
                height,
            })
        }
        Err(error) => {
            tracing::warn!("could not decode the embedded app icon: {error}");
            None
        }
    }
}

pub fn window_icon() -> Option<egui::IconData> {
    decode().map(|icon| egui::IconData {
        rgba: icon.rgba,
        width: icon.width,
        height: icon.height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icon_decodes_as_square_rgba() {
        let icon = decode().expect("the embedded icon should decode");
        assert_eq!(icon.width, icon.height);
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
    }
}
