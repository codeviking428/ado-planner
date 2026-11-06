use std::borrow::Cow;

use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::{AssetSource, Result, SharedString};

// Icons the app names directly. gpui-component's default bundle omits them,
// and an icon-only button with a missing SVG paints as an empty box.
icon_assets!(AppIcons, [LogOut, Palette]);

/// The app's icons plus gpui-component's default bundle.
#[derive(Clone, Copy, Debug, Default)]
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = AppIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = AppIcons.list(path)?;
        paths.extend(Assets.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::assets::IconName;

    fn icon_names_in_src() -> Vec<String> {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut names = Vec::new();
        for entry in std::fs::read_dir(src).expect("read src") {
            let path = entry.expect("src entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs")
                || path.file_name() == Some("assets.rs".as_ref())
            {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read source");
            for (at, _) in text.match_indices("IconName::") {
                let name: String = text[at + "IconName::".len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                if name.starts_with(|c: char| c.is_ascii_uppercase()) && name != "ALL" {
                    names.push(name);
                }
            }
        }
        names.sort();
        names.dedup();
        names
    }

    // Debug builds of the default bundle read every icon from disk, so only
    // `AppIcons` proves an icon is embedded in a release build.
    #[test]
    fn every_icon_the_app_names_is_embedded() {
        let names = icon_names_in_src();
        assert!(
            names.iter().any(|n| n == "Palette"),
            "the theme trigger's icon must be found by the scan, got {names:?}"
        );
        for name in names {
            let icon = IconName::ALL
                .iter()
                .find(|icon| format!("{icon:?}") == name)
                .unwrap_or_else(|| panic!("IconName::{name} is not a bundled icon"));
            let bytes = AppIcons
                .load(&icon.path())
                .expect("load")
                .unwrap_or_else(|| panic!("add IconName::{name} to AppIcons"));
            assert!(
                bytes.starts_with(b"<svg"),
                "IconName::{name} must load an SVG"
            );
        }
    }

    #[test]
    fn component_default_icons_still_load() {
        for icon in [IconName::Check, IconName::ChevronDown, IconName::Close] {
            let bytes = AppAssets
                .load(&icon.path())
                .expect("load")
                .expect("embedded");
            assert!(bytes.starts_with(b"<svg"), "{icon:?} must load an SVG");
        }
    }

    #[test]
    fn list_includes_app_and_default_icons() {
        let paths = AppAssets.list("icons/").expect("list");
        assert!(paths.iter().any(|p| p.as_ref() == "icons/palette.svg"));
        assert!(paths.iter().any(|p| p.as_ref() == "icons/check.svg"));
    }
}
