//! Icon SVGs for the window. GPUI paints an icon as nothing when no asset
//! source can load its path, so every `IconName` the UI uses must load here.

use std::borrow::Cow;

use gpui_kit::assets::Assets;
use gpui_kit::{AssetSource, Result, SharedString};

gpui_kit::assets::icon_assets!(ExtraIcons, [LogOut]);

/// The component default bundle plus the icons the app adds on top of it.
#[derive(Clone, Copy, Debug, Default)]
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::assets::IconName;

    fn icons_used_in(source: &str) -> Vec<&str> {
        let mut names: Vec<&str> = source
            .split("IconName::")
            .skip(1)
            .map(|rest| {
                let end = rest
                    .find(|c: char| !c.is_ascii_alphanumeric())
                    .unwrap_or(rest.len());
                &rest[..end]
            })
            .filter(|name| !name.is_empty())
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    #[test]
    fn header_theme_and_log_out_icons_load() {
        for icon in [IconName::Palette, IconName::LogOut] {
            let path = icon.path();
            assert!(
                AppAssets
                    .load(&path)
                    .unwrap()
                    .is_some_and(|b| !b.is_empty()),
                "{path} must load or its icon button paints blank"
            );
        }
    }

    #[test]
    fn every_icon_the_ui_names_loads() {
        let used = icons_used_in(include_str!("ui.rs"));
        assert!(used.contains(&"Palette"), "scan must find the theme icon");
        for name in used {
            let icon = IconName::ALL
                .iter()
                .find(|icon| format!("{icon:?}") == name)
                .unwrap_or_else(|| panic!("IconName::{name} is not a bundled icon"));
            let path = icon.path();
            assert!(
                AppAssets.load(&path).ok().flatten().is_some(),
                "IconName::{name} ({path}) is missing from AppAssets"
            );
        }
    }
}
