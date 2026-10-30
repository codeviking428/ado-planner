use ado_core::{
    FormControl, FormControlKind, IdentityValue, WorkItemFormModel, WorkItemNode,
    all_editable_controls, build_form_patch, html_to_markdown, is_plain_html_field,
    markdown_to_html, missing_required_fields,
};
use serde_json::{Value, json};

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct FormDraft {
    pub model: WorkItemFormModel,
    pub values: serde_json::Map<String, Value>,
    pub html_markdown: std::collections::HashMap<String, String>,
    pub html_plain: std::collections::HashMap<String, bool>,
    pub identity_query: String,
    pub identity_results: Vec<IdentityValue>,
    pub active_page: usize,
    pub saving: bool,
}

impl FormDraft {
    pub fn from_model(model: WorkItemFormModel) -> Self {
        let mut html_markdown = std::collections::HashMap::new();
        let mut html_plain = std::collections::HashMap::new();
        let values = model.values.clone();
        for control in all_controls(&model) {
            if control.kind != FormControlKind::Html {
                continue;
            }
            let html = values
                .get(&control.reference_name)
                .and_then(Value::as_str)
                .unwrap_or("");
            let plain = is_plain_html_field(html);
            html_plain.insert(control.reference_name.clone(), plain);
            if plain {
                html_markdown.insert(
                    control.reference_name.clone(),
                    html_to_markdown(html).unwrap_or_default(),
                );
            }
        }
        Self {
            model,
            values,
            html_markdown,
            html_plain,
            identity_query: String::new(),
            identity_results: Vec::new(),
            active_page: 0,
            saving: false,
        }
    }

    pub fn title(&self) -> String {
        format!("#{} {}", self.model.id, self.model.work_item_type)
    }

    pub fn item_title(&self) -> String {
        self.values
            .get("System.Title")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_default()
    }

    pub fn set_string(&mut self, reference: &str, value: String) {
        self.values
            .insert(reference.to_string(), Value::String(value));
    }

    pub fn set_markdown(&mut self, reference: &str, markdown: String) {
        self.html_markdown
            .insert(reference.to_string(), markdown.clone());
        self.values.insert(
            reference.to_string(),
            Value::String(markdown_to_html(&markdown)),
        );
    }

    pub fn is_plain_html(&self, reference: &str) -> bool {
        self.html_plain.get(reference).copied().unwrap_or(false)
    }

    pub fn validate(&self) -> Result<(), AppError> {
        let editable: Vec<_> = all_editable_controls(&self.model)
            .into_iter()
            .cloned()
            .collect();
        let missing = missing_required_fields(&editable, &self.values);
        if let Some(first) = missing.first() {
            return Err(AppError::message(format!("Required field: {first}")));
        }
        Ok(())
    }

    pub fn patch_ops(&self) -> Vec<ado_core::JsonPatchOp> {
        let editable: Vec<_> = all_editable_controls(&self.model)
            .into_iter()
            .cloned()
            .collect();
        build_form_patch(self.model.rev, &self.model.values, &self.values, &editable)
    }

    pub fn apply_saved(&self, nodes: &mut [WorkItemNode], rev: i64) {
        if let Some(node) = nodes.iter_mut().find(|node| node.id == self.model.id) {
            node.rev = rev;
            if let Some(Value::String(title)) = self.values.get("System.Title") {
                node.title = title.clone();
            }
            if let Some(Value::String(state)) = self.values.get("System.State") {
                node.state = state.clone();
            }
            if let Some(assigned) = self.values.get("System.AssignedTo") {
                node.assigned_to = identity_from_value(assigned);
            }
            if let Some(Value::String(iteration)) = self.values.get("System.IterationPath") {
                node.iteration_path = iteration.clone();
            }
            if let Some(Value::String(area)) = self.values.get("System.AreaPath") {
                node.area_path = area.clone();
            }
            if let Some(start) = self.values.get(ado_core::START_DATE_FIELD) {
                node.start_date = gantt_date_from_draft(start);
            }
            if let Some(target) = self.values.get(ado_core::TARGET_DATE_FIELD) {
                node.target_date = gantt_date_from_draft(target);
            }
            for (key, value) in &self.values {
                node.fields.insert(key.clone(), value.clone());
            }
        }
    }
}

fn gantt_date_from_draft(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn identity_from_value(value: &Value) -> Option<IdentityValue> {
    match value {
        Value::Null => None,
        Value::String(text) if text.is_empty() => None,
        Value::String(text) => Some(IdentityValue {
            display_name: text.clone(),
            unique_name: text.clone(),
            id: None,
        }),
        Value::Object(map) => {
            let display = map.get("displayName").and_then(Value::as_str).unwrap_or("");
            let unique = map
                .get("uniqueName")
                .and_then(Value::as_str)
                .unwrap_or(display);
            if display.is_empty() && unique.is_empty() {
                None
            } else {
                Some(IdentityValue {
                    display_name: if display.is_empty() {
                        unique.to_string()
                    } else {
                        display.to_string()
                    },
                    unique_name: unique.to_string(),
                    id: map.get("id").and_then(Value::as_str).map(ToOwned::to_owned),
                })
            }
        }
        _ => None,
    }
}

pub fn all_controls(model: &WorkItemFormModel) -> Vec<FormControl> {
    let mut controls = model.system_controls.clone();
    for page in &model.pages {
        for group in &page.groups {
            controls.extend(group.controls.clone());
        }
    }
    controls
}

pub fn ado_edit_url(org: &str, project: &str, id: i64) -> String {
    ado_edit_url_on("https://dev.azure.com", org, project, id)
}

pub fn ado_edit_url_on(ado_base: &str, org: &str, project: &str, id: i64) -> String {
    let base = ado_base.trim_end_matches('/');
    let project = urlencoding::encode(project);
    if base.contains("visualstudio.com") && !base.contains("dev.azure.com") {
        format!("{base}/{project}/_workitems/edit/{id}")
    } else {
        format!("{base}/{org}/{project}/_workitems/edit/{id}")
    }
}

pub fn identity_json(value: &IdentityValue) -> Value {
    json!({
        "displayName": value.display_name,
        "uniqueName": value.unique_name,
        "id": value.id,
    })
}

/// Display text for a form control, so integers are not shown as `13.0` and
/// date-times are not dumped as raw ISO instants.
pub fn format_form_value(kind: FormControlKind, value: &Value) -> String {
    match (kind, value) {
        (FormControlKind::Integer, Value::Number(n)) => n
            .as_i64()
            .map(|v| v.to_string())
            .or_else(|| {
                n.as_f64().map(|v| {
                    if v.fract() == 0.0 {
                        format!("{}", v as i64)
                    } else {
                        v.to_string()
                    }
                })
            })
            .unwrap_or_else(|| n.to_string()),
        (FormControlKind::Double, Value::Number(n)) => n
            .as_f64()
            .map(|v| {
                if v.fract() == 0.0 {
                    format!("{}", v as i64)
                } else {
                    v.to_string()
                }
            })
            .unwrap_or_else(|| n.to_string()),
        (FormControlKind::DateTime, Value::String(text)) => ado_core::parse_iso_date(Some(text))
            .map(|date| date.to_string())
            .unwrap_or_else(|| text.clone()),
        (_, Value::String(text)) => text.clone(),
        (_, Value::Number(n)) => n.to_string(),
        (_, Value::Bool(flag)) => flag.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ado_core::FormControlKind;

    #[test]
    fn format_form_value_drops_integer_fraction_and_iso_time() {
        assert_eq!(
            format_form_value(FormControlKind::Integer, &json!(13.0)),
            "13"
        );
        assert_eq!(
            format_form_value(FormControlKind::Integer, &json!(13)),
            "13"
        );
        assert_eq!(
            format_form_value(FormControlKind::DateTime, &json!("2026-07-06T00:00:00Z")),
            "2026-07-06"
        );
    }

    #[test]
    fn apply_saved_clears_empty_or_null_dates() {
        let mut values = serde_json::Map::new();
        values.insert("System.Title".into(), Value::String("Cleared".into()));
        values.insert(
            ado_core::START_DATE_FIELD.into(),
            Value::String(String::new()),
        );
        values.insert(ado_core::TARGET_DATE_FIELD.into(), Value::Null);
        let draft = FormDraft::from_model(WorkItemFormModel {
            id: 1,
            rev: 1,
            work_item_type: "User Story".into(),
            system_controls: Vec::new(),
            pages: Vec::new(),
            values,
        });
        let mut nodes = vec![WorkItemNode {
            id: 1,
            rev: 1,
            title: "Old".into(),
            work_item_type: "User Story".into(),
            state: "New".into(),
            assigned_to: None,
            parent_id: None,
            area_path: "Shop".into(),
            iteration_path: "Shop".into(),
            start_date: Some("2026-07-06T00:00:00Z".into()),
            target_date: Some("2026-07-20T00:00:00Z".into()),
            has_date_fields: true,
            fields: serde_json::Map::new(),
        }];
        draft.apply_saved(&mut nodes, 2);
        assert_eq!(nodes[0].rev, 2);
        assert!(nodes[0].start_date.is_none());
        assert!(nodes[0].target_date.is_none());
    }

    #[test]
    fn ado_edit_url_encodes_project_and_uses_base() {
        assert_eq!(
            ado_edit_url_on("https://dev.azure.com", "contoso", "My Project", 42),
            "https://dev.azure.com/contoso/My%20Project/_workitems/edit/42"
        );
        assert_eq!(
            ado_edit_url_on("https://contoso.visualstudio.com", "contoso", "Shop", 7),
            "https://contoso.visualstudio.com/Shop/_workitems/edit/7"
        );
    }

    #[test]
    fn plain_html_edits_as_markdown() {
        let mut model = WorkItemFormModel {
            id: 1,
            rev: 1,
            work_item_type: "User Story".into(),
            system_controls: vec![FormControl {
                id: "System.Title".into(),
                reference_name: "System.Title".into(),
                label: "Title".into(),
                kind: FormControlKind::String,
                required: true,
                read_only: false,
                visible: true,
                options: None,
            }],
            pages: Vec::new(),
            values: serde_json::Map::from_iter([(
                "System.Title".into(),
                Value::String("Hello".into()),
            )]),
        };
        model.values.insert(
            "System.Description".into(),
            Value::String(
                include_str!("../../ado-core/tests/fixtures/plain-paragraphs.html").into(),
            ),
        );
        model.pages.push(ado_core::FormPage {
            id: "details".into(),
            label: "Details".into(),
            groups: vec![ado_core::FormGroup {
                id: "desc".into(),
                label: "Description".into(),
                controls: vec![FormControl {
                    id: "System.Description".into(),
                    reference_name: "System.Description".into(),
                    label: "Description".into(),
                    kind: FormControlKind::Html,
                    required: false,
                    read_only: false,
                    visible: true,
                    options: None,
                }],
            }],
        });
        let draft = FormDraft::from_model(model);
        assert_eq!(draft.item_title(), "Hello");
        assert!(draft.title().contains("#1"));
        assert!(draft.is_plain_html("System.Description"));
        assert!(
            draft
                .html_markdown
                .get("System.Description")
                .is_some_and(|md| md.to_lowercase().contains("cart"))
        );
    }
}
