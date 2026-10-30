use serde::{Deserialize, Serialize};

use crate::types::AssigneeFilter;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannerPrefs {
    #[serde(default)]
    pub org: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub team: String,
    #[serde(default)]
    pub iteration_path: String,
    #[serde(default = "anyone")]
    pub assignee: AssigneeFilter,
    #[serde(default)]
    pub hidden_types: Vec<String>,
    #[serde(default)]
    pub hidden_states: Vec<String>,
    /// `None` (missing key) = never set; `Some(None)` = Show all; `Some(Some([]))` = Hide all.
    #[serde(
        default,
        deserialize_with = "deserialize_root_types",
        skip_serializing_if = "Option::is_none"
    )]
    pub root_types: Option<Option<Vec<String>>>,
}

fn deserialize_root_types<'de, D>(deserializer: D) -> Result<Option<Option<Vec<String>>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<Vec<String>>::deserialize(deserializer)?))
}

fn anyone() -> String {
    "anyone".into()
}

pub fn parse_planner_prefs(raw: Option<&str>) -> Option<PlannerPrefs> {
    let raw = raw?;
    if raw.is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    if !value.is_object() {
        return None;
    }
    let assignee = value
        .get("assignee")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or("anyone")
        .to_string();
    let root_types = match value.get("rootTypes") {
        None => None,
        Some(serde_json::Value::Null) => Some(None),
        Some(serde_json::Value::Array(items)) => Some(Some(
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .collect(),
        )),
        Some(_) => None,
    };
    Some(PlannerPrefs {
        org: string_or(&value, "org"),
        project: string_or(&value, "project"),
        team: string_or(&value, "team"),
        iteration_path: string_or(&value, "iterationPath"),
        assignee,
        hidden_types: string_array(&value, "hiddenTypes"),
        hidden_states: string_array(&value, "hiddenStates"),
        root_types,
    })
}

pub fn serialize_planner_prefs(prefs: &PlannerPrefs) -> String {
    serde_json::to_string(prefs).unwrap_or_else(|_| "{}".into())
}

pub fn choose_available(preferred: &str, available: &[String]) -> String {
    if available.is_empty() {
        return preferred.to_string();
    }
    if !preferred.is_empty() && available.iter().any(|item| item == preferred) {
        return preferred.to_string();
    }
    available.first().cloned().unwrap_or_default()
}

fn string_or(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn string_array(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_complete_selection() {
        let prefs = PlannerPrefs {
            org: "amergis".into(),
            project: "MatterWorx".into(),
            team: "Platform".into(),
            iteration_path: "MatterWorx\\Sprint 12".into(),
            assignee: "me".into(),
            hidden_types: vec!["Task".into()],
            hidden_states: vec!["Done".into()],
            root_types: Some(Some(vec!["Epic".into(), "Feature".into()])),
        };
        assert_eq!(
            parse_planner_prefs(Some(&serialize_planner_prefs(&prefs))),
            Some(prefs)
        );
    }

    #[test]
    fn missing_root_types_stays_unset() {
        let parsed = parse_planner_prefs(Some(r#"{"org":"amergis"}"#)).unwrap();
        assert_eq!(parsed.root_types, None);
    }

    #[test]
    fn null_root_types_is_show_all() {
        let parsed = parse_planner_prefs(Some(r#"{"rootTypes":null}"#)).unwrap();
        assert_eq!(parsed.root_types, Some(None));
    }

    #[test]
    fn empty_root_types_is_hide_all() {
        let parsed = parse_planner_prefs(Some(r#"{"rootTypes":[]}"#)).unwrap();
        assert_eq!(parsed.root_types, Some(Some(vec![])));
    }

    #[test]
    fn junk_or_empty_storage_is_ignored() {
        assert_eq!(parse_planner_prefs(None), None);
        assert_eq!(parse_planner_prefs(Some("not-json")), None);
    }

    #[test]
    fn blank_assignee_falls_back_to_anyone() {
        let parsed = parse_planner_prefs(Some(r#"{"assignee":1}"#)).unwrap();
        assert_eq!(parsed.assignee, "anyone");
    }

    #[test]
    fn keeps_a_team_member_unique_name() {
        let parsed = parse_planner_prefs(Some(r#"{"assignee":"ada@contoso.com"}"#)).unwrap();
        assert_eq!(parsed.assignee, "ada@contoso.com");
    }

    #[test]
    fn choose_available_keeps_a_still_valid_preference() {
        assert_eq!(
            choose_available("Platform", &["Delivery".into(), "Platform".into()]),
            "Platform"
        );
    }

    #[test]
    fn choose_available_keeps_the_preference_when_the_list_is_still_empty() {
        assert_eq!(choose_available("Platform", &[]), "Platform");
    }

    #[test]
    fn choose_available_falls_back_when_the_preference_is_gone() {
        assert_eq!(
            choose_available("Ghost", &["Delivery".into(), "Platform".into()]),
            "Delivery"
        );
    }
}
