use serde::{Deserialize, Serialize};

pub const KNOWN_TYPE_COLORS: &[(&str, &str)] = &[
    ("Epic", "#cba6f7"),
    ("Feature", "#89b4fa"),
    ("Story", "#a6e3a1"),
    ("User Story", "#a6e3a1"),
    ("Product Backlog Item", "#a6e3a1"),
    ("Issue", "#a6e3a1"),
    ("Requirement", "#a6e3a1"),
    ("Task", "#fab387"),
    ("Bug", "#f38ba8"),
];

const FALLBACK: [&str; 5] = ["#94e2d5", "#f9e2af", "#b4befe", "#f5c2e7", "#74c7ec"];

pub fn color_for_type(work_item_type: &str) -> &'static str {
    for (name, color) in KNOWN_TYPE_COLORS {
        if *name == work_item_type {
            return color;
        }
    }
    let mut hash: u32 = 0;
    for ch in work_item_type.chars() {
        hash = hash.wrapping_mul(31).wrapping_add(u32::from(ch as u8));
    }
    FALLBACK[(hash as usize) % FALLBACK.len()]
}

pub const FLAVORS: [Flavor; 4] = [
    Flavor::Latte,
    Flavor::Frappe,
    Flavor::Macchiato,
    Flavor::Mocha,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Flavor {
    Latte,
    Frappe,
    Macchiato,
    #[default]
    Mocha,
}

impl Flavor {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Latte => "latte",
            Self::Frappe => "frappe",
            Self::Macchiato => "macchiato",
            Self::Mocha => "mocha",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Latte => "Latte",
            Self::Frappe => "Frappé",
            Self::Macchiato => "Macchiato",
            Self::Mocha => "Mocha",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "latte" => Some(Self::Latte),
            "frappe" => Some(Self::Frappe),
            "macchiato" => Some(Self::Macchiato),
            "mocha" => Some(Self::Mocha),
            _ => None,
        }
    }
}

pub fn is_dark_flavor(flavor: Flavor) -> bool {
    flavor != Flavor::Latte
}

pub fn default_flavor() -> Flavor {
    Flavor::Mocha
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_types_have_stable_colors() {
        assert_eq!(color_for_type("Epic"), "#cba6f7");
        assert_eq!(color_for_type("Task"), "#fab387");
    }

    #[test]
    fn unknown_types_hash_into_fallback() {
        let color = color_for_type("Technical Roadmap Item");
        assert!(FALLBACK.contains(&color));
    }

    #[test]
    fn mocha_is_default_and_dark() {
        assert_eq!(default_flavor(), Flavor::Mocha);
        assert!(is_dark_flavor(Flavor::Mocha));
        assert!(!is_dark_flavor(Flavor::Latte));
    }
}
