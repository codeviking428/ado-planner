use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::{BATCH_SIZE, IdentityValue, START_DATE_FIELD, TARGET_DATE_FIELD, WorkItemNode};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AreaPathValue {
    pub value: String,
    pub include_children: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchWorkItem {
    pub id: i64,
    pub rev: Option<i64>,
    pub fields: Option<serde_json::Map<String, Value>>,
    #[serde(default)]
    pub relations: Option<Vec<Value>>,
}

pub fn build_hierarchy_wiql(project: &str, types: &[String], areas: &[AreaPathValue]) -> String {
    if types.is_empty() {
        return format!(
            "SELECT [System.Id] FROM WorkItems WHERE [System.TeamProject] = '{}' AND [System.Id] = 0",
            escape_wiql(project)
        );
    }
    hierarchy_wiql(project, Some(types), areas)
}

/// WIQL for a team's area paths with no work-item-type filter. Used when the
/// team's backlog settings are invalid (TF400497) so we can still list work.
pub fn build_untyped_hierarchy_wiql(project: &str, areas: &[AreaPathValue]) -> String {
    hierarchy_wiql(project, None, areas)
}

fn hierarchy_wiql(project: &str, types: Option<&[String]>, areas: &[AreaPathValue]) -> String {
    let area_clause = areas
        .iter()
        .map(|area| {
            let path = escape_wiql(&area.value);
            if area.include_children {
                format!("[System.AreaPath] UNDER '{path}'")
            } else {
                format!("[System.AreaPath] = '{path}'")
            }
        })
        .collect::<Vec<_>>()
        .join(" OR ");

    let mut parts = vec![
        "SELECT [System.Id] FROM WorkItems WHERE".to_string(),
        format!("[System.TeamProject] = '{}'", escape_wiql(project)),
    ];
    if let Some(types) = types.filter(|types| !types.is_empty()) {
        let type_list = types
            .iter()
            .map(|ty| format!("'{}'", escape_wiql(ty)))
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("AND [System.WorkItemType] IN ({type_list})"));
    }
    parts.push("AND [System.State] <> 'Removed'".to_string());
    if !area_clause.is_empty() {
        parts.push(format!("AND ({area_clause})"));
    }
    parts.join(" ")
}

pub fn escape_wiql(value: &str) -> String {
    value.replace('\'', "''")
}

pub fn chunk_ids(ids: &[i64], size: usize) -> Vec<Vec<i64>> {
    let size = if size == 0 { BATCH_SIZE } else { size };
    ids.chunks(size).map(<[i64]>::to_vec).collect()
}

pub fn identity_from_field(value: &Value) -> Option<IdentityValue> {
    if is_js_falsy(value) {
        return None;
    }
    match value {
        Value::String(text) => Some(IdentityValue {
            display_name: text.clone(),
            unique_name: text.clone(),
            id: None,
        }),
        Value::Object(map) => {
            let display_name = string_field(map, "displayName");
            let unique_name = string_field(map, "uniqueName");
            if display_name.is_none() && unique_name.is_none() {
                return None;
            }
            let display = display_name
                .clone()
                .or_else(|| unique_name.clone())
                .unwrap_or_default();
            let unique = unique_name.or(display_name).unwrap_or_default();
            Some(IdentityValue {
                display_name: display,
                unique_name: unique,
                id: string_field(map, "id"),
            })
        }
        _ => None,
    }
}

pub fn parent_id_from_fields(fields: &serde_json::Map<String, Value>) -> Option<i64> {
    let raw = fields.get("System.Parent")?;
    let number = raw.as_i64().or_else(|| raw.as_u64().map(|n| n as i64))?;
    if number <= 0 { None } else { Some(number) }
}

pub fn iso_date_from_field(value: &Value) -> Option<String> {
    if is_js_falsy(value) {
        return None;
    }
    match value {
        Value::String(text) => Some(text.clone()),
        _ => None,
    }
}

pub fn map_batch_work_item(
    item: &BatchWorkItem,
    type_date_fields: &std::collections::HashSet<String>,
) -> WorkItemNode {
    let empty = serde_json::Map::new();
    let fields = item.fields.as_ref().unwrap_or(&empty);
    let work_item_type = string_or(fields, "System.WorkItemType", "");
    WorkItemNode {
        id: item.id,
        rev: item
            .rev
            .or_else(|| fields.get("System.Rev").and_then(Value::as_i64))
            .unwrap_or(1),
        title: string_or(fields, "System.Title", ""),
        work_item_type: work_item_type.clone(),
        state: string_or(fields, "System.State", ""),
        assigned_to: fields
            .get("System.AssignedTo")
            .and_then(identity_from_field),
        parent_id: parent_id_from_fields(fields),
        area_path: string_or(fields, "System.AreaPath", ""),
        iteration_path: string_or(fields, "System.IterationPath", ""),
        start_date: fields.get(START_DATE_FIELD).and_then(iso_date_from_field),
        target_date: fields.get(TARGET_DATE_FIELD).and_then(iso_date_from_field),
        has_date_fields: type_date_fields.contains(&work_item_type),
        predecessors: predecessor_ids(item.relations.as_deref()),
        fields: fields.clone(),
    }
}

const DEPENDENCY_REVERSE: &str = "System.LinkTypes.Dependency-Reverse";
const DEPENDENCY: &str = "System.LinkTypes.Dependency";

/// Index of the dependency relation that points at `predecessor_id`.
pub fn dependency_relation_index(
    relations: Option<&[Value]>,
    predecessor_id: i64,
) -> Option<usize> {
    let relations = relations?;
    relations.iter().position(|relation| {
        let rel = relation.get("rel").and_then(Value::as_str).unwrap_or("");
        if rel != DEPENDENCY_REVERSE && rel != DEPENDENCY {
            return false;
        }
        let url = relation.get("url").and_then(Value::as_str).unwrap_or("");
        url.trim_end_matches('/')
            .rsplit('/')
            .next()
            .and_then(|id| id.parse::<i64>().ok())
            == Some(predecessor_id)
    })
}

/// Predecessor IDs from ADO relations (`Dependency-Reverse` / `Dependency`).
pub fn predecessor_ids(relations: Option<&[Value]>) -> Vec<i64> {
    let Some(relations) = relations else {
        return Vec::new();
    };
    relations
        .iter()
        .filter_map(|relation| {
            let rel = relation.get("rel")?.as_str()?;
            if rel != DEPENDENCY_REVERSE && rel != DEPENDENCY {
                return None;
            }
            let url = relation.get("url")?.as_str()?;
            url.trim_end_matches('/').rsplit('/').next()?.parse().ok()
        })
        .collect()
}

/// Unparented nodes and nodes whose parent is outside the loaded ID set become roots.
pub fn assemble_forest(nodes: &[WorkItemNode]) -> Vec<WorkItemNode> {
    let ids: std::collections::HashSet<i64> = nodes.iter().map(|node| node.id).collect();
    nodes
        .iter()
        .map(|node| {
            let parent_id = match node.parent_id {
                Some(parent) if ids.contains(&parent) => Some(parent),
                _ => None,
            };
            let mut next = node.clone();
            next.parent_id = parent_id;
            next
        })
        .collect()
}

pub const HIERARCHY_FIELDS: [&str; 11] = [
    "System.Id",
    "System.Rev",
    "System.Title",
    "System.WorkItemType",
    "System.State",
    "System.AssignedTo",
    "System.Parent",
    "System.AreaPath",
    "System.IterationPath",
    START_DATE_FIELD,
    TARGET_DATE_FIELD,
];

fn string_field(map: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    match map.get(key)? {
        Value::String(text) => Some(text.clone()),
        _ => None,
    }
}

fn string_or(map: &serde_json::Map<String, Value>, key: &str, fallback: &str) -> String {
    map.get(key)
        .and_then(|value| match value {
            Value::String(text) => Some(text.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .unwrap_or_else(|| fallback.to_string())
}

fn is_js_falsy(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(false) => true,
        Value::Number(n) => n.as_f64().is_some_and(|v| v == 0.0),
        Value::String(s) => s.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(_) => false,
        Value::Bool(true) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::START_DATE_FIELD;
    use serde_json::json;
    use std::collections::HashSet;

    fn node(id: i64, parent_id: Option<i64>) -> WorkItemNode {
        WorkItemNode {
            id,
            rev: 1,
            title: format!("WI {id}"),
            work_item_type: "Task".into(),
            state: "Active".into(),
            assigned_to: None,
            parent_id,
            area_path: "Shop\\Platform".into(),
            iteration_path: "Shop\\Sprint 1".into(),
            start_date: None,
            target_date: None,
            has_date_fields: true,
            predecessors: Vec::new(),
            fields: serde_json::Map::new(),
        }
    }

    #[test]
    fn unparented_work_items_become_roots() {
        let forest = assemble_forest(&[node(1, None), node(2, Some(1)), node(3, Some(0))]);
        assert_eq!(forest.iter().find(|n| n.id == 1).unwrap().parent_id, None);
        assert_eq!(
            forest.iter().find(|n| n.id == 2).unwrap().parent_id,
            Some(1)
        );
        assert_eq!(forest.iter().find(|n| n.id == 3).unwrap().parent_id, None);
    }

    #[test]
    fn out_of_area_parents_are_not_kept_child_becomes_a_root() {
        let forest = assemble_forest(&[node(20, Some(99))]);
        assert_eq!(forest[0].id, 20);
        assert_eq!(forest[0].parent_id, None);
    }

    #[test]
    fn same_type_nesting_stays_in_the_forest() {
        let mut parent = node(1, None);
        parent.work_item_type = "User Story".into();
        let mut child = node(2, Some(1));
        child.work_item_type = "User Story".into();
        let forest = assemble_forest(&[parent, child]);
        assert_eq!(
            forest.iter().find(|n| n.id == 2).unwrap().parent_id,
            Some(1)
        );
    }

    #[test]
    fn uses_under_vs_eq_from_include_children_and_excludes_removed() {
        let wiql = build_hierarchy_wiql(
            "O'Malley",
            &["Epic".into(), "Task".into()],
            &[
                AreaPathValue {
                    value: "O'Malley\\Area".into(),
                    include_children: true,
                },
                AreaPathValue {
                    value: "O'Malley\\Exact".into(),
                    include_children: false,
                },
            ],
        );
        assert!(wiql.contains("[System.TeamProject] = 'O''Malley'"));
        assert!(wiql.contains("[System.AreaPath] UNDER 'O''Malley\\Area'"));
        assert!(wiql.contains("[System.AreaPath] = 'O''Malley\\Exact'"));
        assert!(wiql.contains("[System.State] <> 'Removed'"));
        assert!(wiql.contains("IN ('Epic', 'Task')"));
    }

    #[test]
    fn empty_type_list_does_not_emit_invalid_in_clause() {
        let wiql = build_hierarchy_wiql("Shop", &[], &[]);
        assert!(!wiql.contains("IN ()"));
        assert!(wiql.contains("[System.Id] = 0"));
    }

    #[test]
    fn untyped_wiql_omits_the_type_filter() {
        let wiql = build_untyped_hierarchy_wiql(
            "Shop",
            &[AreaPathValue {
                value: "Shop\\Area".into(),
                include_children: true,
            }],
        );
        assert!(!wiql.contains("WorkItemType"));
        assert!(wiql.contains("[System.AreaPath] UNDER 'Shop\\Area'"));
        assert!(wiql.contains("[System.State] <> 'Removed'"));
    }

    #[test]
    fn batches_200_ids() {
        let ids: Vec<i64> = (1..=401).collect();
        let chunks = chunk_ids(&ids, BATCH_SIZE);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 200);
        assert_eq!(chunks[1].len(), 200);
        assert_eq!(chunks[2].len(), 1);
    }

    #[test]
    fn maps_system_parent_and_scheduling_fields() {
        let mut fields = serde_json::Map::new();
        fields.insert("System.Title".into(), json!("Cart"));
        fields.insert("System.WorkItemType".into(), json!("Feature"));
        fields.insert("System.State".into(), json!("Active"));
        fields.insert("System.Parent".into(), json!(1));
        fields.insert(
            "System.AssignedTo".into(),
            json!({ "displayName": "Ada", "uniqueName": "ada@contoso" }),
        );
        fields.insert(START_DATE_FIELD.into(), json!("2026-07-01T00:00:00Z"));
        fields.insert(TARGET_DATE_FIELD.into(), json!("2026-07-10T00:00:00Z"));
        let mapped = map_batch_work_item(
            &BatchWorkItem {
                id: 10,
                rev: Some(4),
                fields: Some(fields),
                relations: None,
            },
            &HashSet::from(["Feature".into()]),
        );
        assert_eq!(mapped.parent_id, Some(1));
        assert!(mapped.has_date_fields);
        assert_eq!(
            mapped.assigned_to,
            Some(IdentityValue {
                display_name: "Ada".into(),
                unique_name: "ada@contoso".into(),
                id: None,
            })
        );
    }

    #[test]
    fn missing_parent_is_null() {
        assert_eq!(parent_id_from_fields(&serde_json::Map::new()), None);
        let mut fields = serde_json::Map::new();
        fields.insert("System.Parent".into(), json!(0));
        assert_eq!(parent_id_from_fields(&fields), None);
    }

    #[test]
    fn predecessor_ids_read_dependency_reverse_and_dependency() {
        let relations = vec![
            json!({
                "rel": "System.LinkTypes.Dependency-Reverse",
                "url": "https://dev.azure.com/contoso/Shop/_apis/wit/workItems/1003"
            }),
            json!({
                "rel": "System.LinkTypes.Hierarchy-Reverse",
                "url": "https://dev.azure.com/contoso/Shop/_apis/wit/workItems/1"
            }),
            json!({
                "rel": "System.LinkTypes.Dependency",
                "url": "https://dev.azure.com/contoso/_apis/wit/workItems/1010/"
            }),
        ];
        assert_eq!(predecessor_ids(Some(&relations)), vec![1003, 1010]);
        assert!(predecessor_ids(None).is_empty());
    }
}
