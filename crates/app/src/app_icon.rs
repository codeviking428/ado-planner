//! Window identity for Linux desktops. Windows takes the icon from the exe
//! resource that `build.rs` embeds.

use std::sync::Arc;

use image::RgbaImage;

/// Matches the `ado-planner.desktop` file and hicolor icon name that
/// cargo-packager installs, so Wayland compositors and X11 docks pair the
/// window with its launcher.
pub const APP_ID: &str = "ado-planner";

/// GPUI sets `_NET_WM_ICON` in one X11 request. Without BIG-REQUESTS a request
/// holds at most 65535 four-byte words, and a 256 px icon needs 65538.
const WINDOW_ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/png/128x128.png");

pub fn window_icon() -> Arc<RgbaImage> {
    let icon = image::load_from_memory_with_format(WINDOW_ICON_PNG, image::ImageFormat::Png)
        .expect("bundled window icon is a valid PNG");
    Arc::new(icon.into_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_icon_decodes_to_an_opaque_cornered_128_px_tile() {
        let icon = window_icon();
        assert_eq!(icon.dimensions(), (128, 128));
        assert_eq!(
            icon.get_pixel(0, 0)[3],
            0,
            "rounded corner must be transparent"
        );
        assert_eq!(icon.get_pixel(64, 64)[3], 255, "tile centre must be opaque");
    }

    #[test]
    fn app_id_matches_the_packaged_binary_and_desktop_file() {
        let manifest = include_str!("../Cargo.toml");
        assert!(manifest.contains(&format!("name = \"{APP_ID}\"")));
        assert!(manifest.contains(&format!("{{ path = \"{APP_ID}\", main = true }}")));
    }
}
