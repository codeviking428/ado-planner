use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../e2e/fixtures")
}

pub fn load_json_from(dir: &Path, name: &str) -> Value {
    let path = dir.join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!("failed to read fixture {}: {err}", path.display());
    });
    serde_json::from_str(&text).unwrap_or_else(|err| {
        panic!("failed to parse fixture {}: {err}", path.display());
    })
}

#[derive(Clone)]
pub struct Fixtures {
    pub work_items: Value,
    pub layout: Value,
    pub type_fields: Value,
    pub connection_data: Value,
    pub profile: Value,
    pub accounts: Value,
    pub projects: Value,
    pub teams: Value,
    pub members: Value,
    pub iterations: Value,
    pub team_field_values: Value,
    pub backlogs: Value,
    pub identities: Value,
    pub process: Value,
}

impl Fixtures {
    pub fn load() -> Self {
        Self::load_from(&fixtures_dir())
    }

    pub fn load_from(dir: &Path) -> Self {
        Self {
            work_items: load_json_from(dir, "work-items.json"),
            layout: load_json_from(dir, "layout.json"),
            type_fields: load_json_from(dir, "type-fields.json"),
            connection_data: load_json_from(dir, "connection-data.json"),
            profile: load_json_from(dir, "profile.json"),
            accounts: load_json_from(dir, "accounts.json"),
            projects: load_json_from(dir, "projects.json"),
            teams: load_json_from(dir, "teams.json"),
            members: load_json_from(dir, "members.json"),
            iterations: load_json_from(dir, "iterations.json"),
            team_field_values: load_json_from(dir, "team-field-values.json"),
            backlogs: load_json_from(dir, "backlogs.json"),
            identities: load_json_from(dir, "identities.json"),
            process: load_json_from(dir, "process.json"),
        }
    }
}
