use std::path::Path;

use ado_core::{Flavor, PlannerPrefs, default_flavor, parse_planner_prefs};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPrefs {
    #[serde(flatten)]
    pub planner: PlannerPrefs,
    #[serde(default = "default_flavor")]
    pub flavor: Flavor,
}

impl Default for AppPrefs {
    fn default() -> Self {
        Self {
            planner: PlannerPrefs {
                org: String::new(),
                project: String::new(),
                team: String::new(),
                iteration_path: String::new(),
                assignee: "anyone".into(),
                hidden_types: Vec::new(),
                hidden_states: Vec::new(),
                root_types: None,
            },
            flavor: default_flavor(),
        }
    }
}

impl AppPrefs {
    pub fn load(path: &Path) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        if let Ok(prefs) = serde_json::from_str::<AppPrefs>(&raw) {
            return prefs;
        }
        if let Some(planner) = parse_planner_prefs(Some(&raw)) {
            return Self {
                planner,
                flavor: default_flavor(),
            };
        }
        Self::default()
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(raw) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, raw);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flavor_lives_beside_planner_prefs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        let mut prefs = AppPrefs::default();
        prefs.planner.org = "contoso".into();
        prefs.flavor = Flavor::Latte;
        prefs.save(&path);
        let loaded = AppPrefs::load(&path);
        assert_eq!(loaded.planner.org, "contoso");
        assert_eq!(loaded.flavor, Flavor::Latte);
    }

    #[test]
    fn show_all_roots_round_trips_as_some_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        let mut prefs = AppPrefs::default();
        prefs.planner.root_types = Some(None);
        prefs.save(&path);
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"rootTypes\": null") || raw.contains("\"rootTypes\":null"));
        let loaded = AppPrefs::load(&path);
        assert_eq!(loaded.planner.root_types, Some(None));
    }
}
