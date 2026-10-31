use ado_core::{Flavor, is_dark_flavor};
use gpui_kit::component::theme::{Theme, ThemeColor, ThemeMode};
use gpui_kit::{App, Hsla, hsla};

#[derive(Clone, Copy)]
struct Palette {
    background: (f32, f32, f32),
    foreground: (f32, f32, f32),
    muted: (f32, f32, f32),
    muted_foreground: (f32, f32, f32),
    primary: (f32, f32, f32),
    primary_foreground: (f32, f32, f32),
    secondary: (f32, f32, f32),
    border: (f32, f32, f32),
    danger: (f32, f32, f32),
    sidebar: (f32, f32, f32),
}

impl Palette {
    fn color(self, triple: (f32, f32, f32)) -> Hsla {
        let (h, s, l) = triple;
        hsla(h / 360.0, s / 100.0, l / 100.0, 1.0)
    }
}

fn palette(flavor: Flavor) -> Palette {
    match flavor {
        Flavor::Latte => Palette {
            background: (220.0, 23.08, 94.9),
            foreground: (234.0, 16.02, 35.49),
            muted: (223.0, 15.91, 82.75),
            muted_foreground: (233.0, 12.8, 41.37),
            primary: (266.0, 85.05, 58.04),
            primary_foreground: (220.0, 23.08, 94.9),
            secondary: (223.0, 15.91, 82.75),
            border: (225.0, 13.56, 76.86),
            danger: (347.0, 86.67, 44.12),
            sidebar: (218.0, 25.1, 93.73),
        },
        Flavor::Frappe => Palette {
            background: (229.0, 19.0, 23.0),
            foreground: (227.0, 70.0, 87.0),
            muted: (227.0, 22.0, 32.0),
            muted_foreground: (228.0, 20.0, 65.0),
            primary: (277.0, 59.0, 76.0),
            primary_foreground: (229.0, 19.0, 23.0),
            secondary: (227.0, 22.0, 32.0),
            border: (228.0, 13.0, 36.0),
            danger: (359.0, 68.0, 71.0),
            sidebar: (231.0, 19.0, 20.0),
        },
        Flavor::Macchiato => Palette {
            background: (232.0, 23.0, 18.0),
            foreground: (227.0, 68.0, 88.0),
            muted: (230.0, 19.0, 26.0),
            muted_foreground: (228.0, 26.0, 72.0),
            primary: (267.0, 83.0, 80.0),
            primary_foreground: (232.0, 23.0, 18.0),
            secondary: (230.0, 19.0, 26.0),
            border: (230.0, 14.0, 29.0),
            danger: (351.0, 74.0, 73.0),
            sidebar: (232.0, 23.0, 16.0),
        },
        Flavor::Mocha => Palette {
            background: (240.0, 21.05, 14.9),
            foreground: (226.0, 63.93, 88.04),
            muted: (237.0, 16.24, 22.94),
            muted_foreground: (227.0, 35.29, 80.0),
            primary: (267.0, 83.51, 80.98),
            primary_foreground: (240.0, 21.05, 14.9),
            secondary: (237.0, 16.24, 22.94),
            border: (234.0, 13.21, 31.18),
            danger: (343.0, 81.25, 74.9),
            sidebar: (240.0, 21.18, 14.12),
        },
    }
}

pub fn theme_color(flavor: Flavor) -> ThemeColor {
    let pal = palette(flavor);
    let mut colors = if is_dark_flavor(flavor) {
        *ThemeColor::dark()
    } else {
        *ThemeColor::light()
    };
    let bg = pal.color(pal.background);
    let fg = pal.color(pal.foreground);
    let muted = pal.color(pal.muted);
    let muted_fg = pal.color(pal.muted_foreground);
    let primary = pal.color(pal.primary);
    let primary_fg = pal.color(pal.primary_foreground);
    let secondary = pal.color(pal.secondary);
    let border = pal.color(pal.border);
    let danger = pal.color(pal.danger);
    let sidebar = pal.color(pal.sidebar);
    colors.background = bg;
    colors.foreground = fg;
    colors.muted = muted;
    colors.muted_foreground = muted_fg;
    colors.primary = primary;
    colors.primary_foreground = primary_fg;
    colors.primary_hover = primary;
    colors.primary_active = primary;
    colors.secondary = secondary;
    colors.secondary_foreground = fg;
    colors.border = border;
    colors.input = border;
    colors.ring = fg;
    colors.accent = secondary;
    colors.accent_foreground = fg;
    colors.popover = muted;
    colors.popover_foreground = fg;
    colors.danger = danger;
    colors.danger_foreground = primary_fg;
    colors.sidebar = sidebar;
    colors.sidebar_foreground = fg;
    colors.sidebar_primary = primary;
    colors.sidebar_primary_foreground = primary_fg;
    colors.sidebar_border = border;
    colors.sidebar_accent = bg;
    colors.sidebar_accent_foreground = fg;
    colors.title_bar = bg;
    colors.title_bar_border = border;
    colors.window_border = border;
    colors.list = bg;
    colors.list_hover = muted;
    colors.list_active = pal.color(pal.secondary);
    colors.tab = bg;
    colors.tab_active = muted;
    colors.tab_foreground = fg;
    colors.tab_active_foreground = fg;
    colors.table = bg;
    colors.table_head = muted;
    colors
}

pub fn apply_flavor(cx: &mut App, flavor: Flavor) {
    let colors = theme_color(flavor);
    let dark = is_dark_flavor(flavor);
    Theme::update(cx, |theme| {
        theme.mode = if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        theme.colors = colors;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mocha_is_dark_and_latte_is_light() {
        let mocha = theme_color(Flavor::Mocha);
        let latte = theme_color(Flavor::Latte);
        assert!(mocha.background.l < latte.background.l);
        assert_ne!(mocha.popover, mocha.background);
    }
}
