use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::error::Error;
use crate::types::{
    FormControl, FormControlKind, FormGroup, FormPage, JsonPatchOp, PatchOp, WorkItemFormModel,
};

const SKIP_CONTROL_TYPES: &[&str] = &[
    "WorkItemLogControl",
    "LinksControl",
    "AttachmentsControl",
    "History",
    "Links",
    "Attachments",
];

const SKIP_SYSTEM_IDS: &[&str] = &["System.History", "System.Links", "System.AttachedFiles"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessLayout {
    pub system_controls: Option<Vec<LayoutControl>>,
    pub pages: Option<Vec<LayoutPage>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutPage {
    pub id: Option<String>,
    pub label: Option<String>,
    pub visible: Option<bool>,
    pub page_type: Option<String>,
    pub sections: Option<Vec<LayoutSection>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSection {
    pub groups: Option<Vec<LayoutGroup>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutGroup {
    pub id: Option<String>,
    pub label: Option<String>,
    pub visible: Option<bool>,
    pub is_contribution: Option<bool>,
    pub controls: Option<Vec<LayoutControl>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutControl {
    pub id: Option<String>,
    pub label: Option<String>,
    pub visible: Option<bool>,
    pub read_only: Option<bool>,
    pub control_type: Option<String>,
    pub is_contribution: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldMetadata {
    pub reference_name: String,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub field_type: Option<String>,
    pub is_identity: Option<bool>,
    pub is_picklist: Option<bool>,
    pub always_required: Option<bool>,
    pub read_only: Option<bool>,
    pub allowed_values: Option<Vec<String>>,
}

pub fn strip_mnemonic(label: &str) -> String {
    let placeholder = "\u{0}";
    label
        .replace("&&", placeholder)
        .replace('&', "")
        .replace(placeholder, "&")
}

pub fn kind_from_metadata(
    control: &LayoutControl,
    meta: Option<&FieldMetadata>,
) -> FormControlKind {
    if meta.is_some_and(|m| m.is_identity == Some(true)) {
        return FormControlKind::Identity;
    }
    if control.control_type.as_deref() == Some("HtmlFieldControl")
        || meta.is_some_and(|m| m.field_type.as_deref() == Some("html"))
    {
        return FormControlKind::Html;
    }
    if control.control_type.as_deref() == Some("WorkItemClassificationControl")
        || meta.is_some_and(|m| m.field_type.as_deref() == Some("treePath"))
    {
        return FormControlKind::TreePath;
    }
    if meta.is_some_and(|m| {
        m.is_picklist == Some(true) || m.allowed_values.as_ref().is_some_and(|v| !v.is_empty())
    }) {
        return FormControlKind::Picklist;
    }
    match meta.and_then(|m| m.field_type.as_deref()) {
        Some("integer") => FormControlKind::Integer,
        Some("double") => FormControlKind::Double,
        Some("boolean") => FormControlKind::Boolean,
        Some("dateTime") => FormControlKind::DateTime,
        Some("plainText") => FormControlKind::PlainText,
        Some("html") => FormControlKind::Html,
        _ => FormControlKind::String,
    }
}

fn to_form_control(
    control: &LayoutControl,
    meta_by_ref: &HashMap<&str, &FieldMetadata>,
) -> Option<FormControl> {
    let id = control.id.as_deref()?;
    if SKIP_SYSTEM_IDS.contains(&id)
        || SKIP_CONTROL_TYPES.contains(&control.control_type.as_deref().unwrap_or(""))
    {
        return None;
    }
    if control.is_contribution == Some(true) || control.visible == Some(false) {
        return None;
    }
    let meta = meta_by_ref.get(id).copied();
    Some(FormControl {
        id: id.to_string(),
        reference_name: id.to_string(),
        label: strip_mnemonic(
            control
                .label
                .as_deref()
                .or_else(|| meta.and_then(|m| m.name.as_deref()))
                .unwrap_or(id),
        ),
        kind: kind_from_metadata(control, meta),
        required: meta.is_some_and(|m| m.always_required == Some(true)),
        read_only: control.read_only == Some(true)
            || meta.is_some_and(|m| m.read_only == Some(true)),
        visible: true,
        options: meta.and_then(|m| m.allowed_values.clone()),
    })
}

pub fn flatten_layout(
    layout: &ProcessLayout,
    fields: &[FieldMetadata],
) -> (Vec<FormControl>, Vec<FormPage>) {
    let meta_by_ref: HashMap<&str, &FieldMetadata> = fields
        .iter()
        .map(|field| (field.reference_name.as_str(), field))
        .collect();
    let system_controls = layout
        .system_controls
        .iter()
        .flatten()
        .filter_map(|control| to_form_control(control, &meta_by_ref))
        .collect();

    let mut pages = Vec::new();
    for page in layout.pages.iter().flatten() {
        if page.visible == Some(false) || page.page_type.as_deref() != Some("custom") {
            continue;
        }
        let mut groups = Vec::new();
        for section in page.sections.iter().flatten() {
            for group in section.groups.iter().flatten() {
                if group.visible == Some(false) || group.is_contribution == Some(true) {
                    continue;
                }
                let controls: Vec<FormControl> = group
                    .controls
                    .iter()
                    .flatten()
                    .filter_map(|control| to_form_control(control, &meta_by_ref))
                    .collect();
                if controls.is_empty() {
                    continue;
                }
                groups.push(FormGroup {
                    id: group
                        .id
                        .clone()
                        .or_else(|| group.label.clone())
                        .unwrap_or_else(|| "group".into()),
                    label: strip_mnemonic(group.label.as_deref().unwrap_or("")),
                    controls,
                });
            }
        }
        pages.push(FormPage {
            id: page
                .id
                .clone()
                .or_else(|| page.label.clone())
                .unwrap_or_else(|| "page".into()),
            label: strip_mnemonic(page.label.as_deref().unwrap_or("Details")),
            groups,
        });
    }
    (system_controls, pages)
}

pub fn all_editable_controls(model: &WorkItemFormModel) -> Vec<&FormControl> {
    model
        .system_controls
        .iter()
        .chain(
            model
                .pages
                .iter()
                .flat_map(|page| page.groups.iter().flat_map(|group| group.controls.iter())),
        )
        .filter(|control| control.visible && !control.read_only)
        .collect()
}

pub fn build_form_patch(
    rev: i64,
    original: &serde_json::Map<String, Value>,
    draft: &serde_json::Map<String, Value>,
    editable: &[FormControl],
) -> Vec<JsonPatchOp> {
    let mut ops = vec![JsonPatchOp {
        op: PatchOp::Test,
        path: "/rev".into(),
        value: Some(json!(rev)),
    }];
    for control in editable {
        let next = draft.get(&control.reference_name);
        let prev = original.get(&control.reference_name);
        if json_equal(next, prev) {
            continue;
        }
        let value = patch_value(control.kind, next);
        ops.push(JsonPatchOp {
            op: PatchOp::Add,
            path: format!("/fields/{}", control.reference_name),
            value: Some(value),
        });
    }
    ops
}

fn patch_value(kind: FormControlKind, next: Option<&Value>) -> Value {
    match next {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(text))
            if text.is_empty()
                && !matches!(
                    kind,
                    FormControlKind::String | FormControlKind::PlainText | FormControlKind::Html
                ) =>
        {
            Value::Null
        }
        Some(value) => value.clone(),
    }
}

fn json_equal(next: Option<&Value>, prev: Option<&Value>) -> bool {
    match (next, prev) {
        (None, None) => true,
        (None, Some(Value::Null)) | (Some(Value::Null), None) => true,
        (Some(a), Some(b)) => values_equal(a, b),
        _ => false,
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    if let (Some(left), Some(right)) = (as_f64(a), as_f64(b)) {
        return (left - right).abs() < f64::EPSILON;
    }
    if let (Some(left), Some(right)) = (date_prefix(a), date_prefix(b)) {
        return left == right;
    }
    if let (Some(left), Some(right)) = (identity_key(a), identity_key(b)) {
        return left.eq_ignore_ascii_case(&right);
    }
    false
}

fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn date_prefix(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|text| text.len() >= 10 && text.as_bytes()[4] == b'-' && text.as_bytes()[7] == b'-')
        .map(|text| &text[..10])
}

fn identity_key(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Object(map) => map
            .get("uniqueName")
            .and_then(Value::as_str)
            .or_else(|| map.get("displayName").and_then(Value::as_str))
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned),
        _ => None,
    }
}

/// TanStack Form treated `.` as a nested path. ADO reference names stay flat.
pub fn form_field_name(reference_name: &str) -> String {
    reference_name.replace('.', "::")
}

pub fn reference_name_from_form_field(name: &str) -> String {
    name.replace("::", ".")
}

pub fn work_item_form_defaults(
    values: &serde_json::Map<String, Value>,
    controls: &[FormControl],
) -> serde_json::Map<String, Value> {
    let mut defaults = serde_json::Map::new();
    for control in controls {
        let value = values
            .get(&control.reference_name)
            .cloned()
            .unwrap_or(Value::Null);
        defaults.insert(form_field_name(&control.reference_name), value);
    }
    defaults
}

pub fn draft_from_form_values(
    form_values: &serde_json::Map<String, Value>,
) -> serde_json::Map<String, Value> {
    form_values
        .iter()
        .map(|(key, value)| (reference_name_from_form_field(key), value.clone()))
        .collect()
}

pub fn missing_required_fields(
    controls: &[FormControl],
    values: &serde_json::Map<String, Value>,
) -> Vec<String> {
    let mut missing = Vec::new();
    for control in controls {
        if !control.required {
            continue;
        }
        if control.kind == FormControlKind::Identity {
            continue;
        }
        let value = values.get(&control.reference_name);
        let ok = match control.kind {
            FormControlKind::Boolean => matches!(value, Some(Value::Bool(_))),
            _ => match value {
                Some(Value::String(s)) => !s.is_empty(),
                Some(Value::Number(_)) => true,
                _ => false,
            },
        };
        if !ok {
            missing.push(control.reference_name.clone());
        }
    }
    missing
}

pub fn validate_form_values(
    controls: &[FormControl],
    values: &serde_json::Map<String, Value>,
) -> Result<(), Error> {
    let missing = missing_required_fields(controls, values);
    if let Some(first) = missing.first() {
        Err(Error::RequiredField(first.clone()))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FormControlKind;

    fn title_control() -> FormControl {
        FormControl {
            id: "System.Title".into(),
            reference_name: "System.Title".into(),
            label: "Title".into(),
            kind: FormControlKind::String,
            required: true,
            read_only: false,
            visible: true,
            options: None,
        }
    }

    fn assigned_control() -> FormControl {
        FormControl {
            id: "System.AssignedTo".into(),
            reference_name: "System.AssignedTo".into(),
            label: "Assigned To".into(),
            kind: FormControlKind::Identity,
            required: false,
            read_only: false,
            visible: true,
            options: None,
        }
    }

    #[test]
    fn keeps_visible_custom_pages_and_skips_history_links_attachments_contributions() {
        let layout = ProcessLayout {
            system_controls: Some(vec![
                LayoutControl {
                    id: Some("System.Title".into()),
                    label: Some("Titl&e".into()),
                    visible: Some(true),
                    read_only: None,
                    control_type: Some("FieldControl".into()),
                    is_contribution: None,
                },
                LayoutControl {
                    id: Some("System.History".into()),
                    label: Some("History".into()),
                    visible: Some(true),
                    read_only: None,
                    control_type: Some("WorkItemLogControl".into()),
                    is_contribution: None,
                },
            ]),
            pages: Some(vec![
                LayoutPage {
                    id: Some("details".into()),
                    label: Some("Detai&ls".into()),
                    visible: Some(true),
                    page_type: Some("custom".into()),
                    sections: Some(vec![LayoutSection {
                        groups: Some(vec![
                            LayoutGroup {
                                id: Some("planning".into()),
                                label: Some("Planning".into()),
                                visible: Some(true),
                                is_contribution: None,
                                controls: Some(vec![
                                    LayoutControl {
                                        id: Some("System.State".into()),
                                        label: Some("Stat&e".into()),
                                        visible: Some(true),
                                        read_only: None,
                                        control_type: Some("FieldControl".into()),
                                        is_contribution: None,
                                    },
                                    LayoutControl {
                                        id: Some("System.AssignedTo".into()),
                                        label: Some("Assigned To".into()),
                                        visible: Some(true),
                                        read_only: None,
                                        control_type: Some("FieldControl".into()),
                                        is_contribution: None,
                                    },
                                    LayoutControl {
                                        id: Some("hidden".into()),
                                        label: Some("Hidden".into()),
                                        visible: Some(false),
                                        read_only: None,
                                        control_type: Some("FieldControl".into()),
                                        is_contribution: None,
                                    },
                                ]),
                            },
                            LayoutGroup {
                                id: Some("marketplace".into()),
                                label: Some("Extension".into()),
                                visible: Some(true),
                                is_contribution: Some(true),
                                controls: Some(vec![LayoutControl {
                                    id: Some("ext.field".into()),
                                    label: None,
                                    visible: Some(true),
                                    read_only: None,
                                    control_type: None,
                                    is_contribution: None,
                                }]),
                            },
                        ]),
                    }]),
                },
                LayoutPage {
                    id: Some("history".into()),
                    label: Some("History".into()),
                    visible: Some(true),
                    page_type: Some("history".into()),
                    sections: None,
                },
                LayoutPage {
                    id: Some("links".into()),
                    label: Some("Links".into()),
                    visible: Some(true),
                    page_type: Some("links".into()),
                    sections: None,
                },
            ]),
        };
        let fields = vec![
            FieldMetadata {
                reference_name: "System.Title".into(),
                name: None,
                field_type: Some("string".into()),
                is_identity: None,
                is_picklist: None,
                always_required: Some(true),
                read_only: None,
                allowed_values: None,
            },
            FieldMetadata {
                reference_name: "System.State".into(),
                name: None,
                field_type: Some("string".into()),
                is_identity: None,
                is_picklist: Some(true),
                always_required: None,
                read_only: None,
                allowed_values: Some(vec!["New".into(), "Active".into()]),
            },
            FieldMetadata {
                reference_name: "System.AssignedTo".into(),
                name: None,
                field_type: Some("string".into()),
                is_identity: Some(true),
                is_picklist: None,
                always_required: None,
                read_only: None,
                allowed_values: None,
            },
        ];
        let (system_controls, pages) = flatten_layout(&layout, &fields);
        assert_eq!(
            system_controls
                .iter()
                .map(|c| c.reference_name.as_str())
                .collect::<Vec<_>>(),
            vec!["System.Title"]
        );
        assert_eq!(system_controls[0].label, "Title");
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].label, "Details");
        assert_eq!(pages[0].groups.len(), 1);
        let kinds: Vec<_> = pages[0].groups[0].controls.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            vec![FormControlKind::Picklist, FormControlKind::Identity]
        );
        assert!(!pages[0].groups[0].controls.iter().any(|c| c.id == "hidden"));
    }

    #[test]
    fn strips_ado_mnemonics() {
        assert_eq!(strip_mnemonic("Stat&e"), "State");
        assert_eq!(strip_mnemonic("R&&D Notes"), "R&D Notes");
    }

    #[test]
    fn save_patch_ignores_numeric_and_date_equivalents() {
        let mut original = serde_json::Map::new();
        original.insert("Microsoft.VSTS.Scheduling.StoryPoints".into(), json!(5));
        original.insert(
            "Microsoft.VSTS.Scheduling.StartDate".into(),
            json!("2026-07-06T00:00:00Z"),
        );
        let mut draft = original.clone();
        draft.insert("Microsoft.VSTS.Scheduling.StoryPoints".into(), json!(5.0));
        draft.insert(
            "Microsoft.VSTS.Scheduling.StartDate".into(),
            json!("2026-07-06T00:00:00Z"),
        );
        draft.insert("System.Title".into(), json!("New title"));
        original.insert("System.Title".into(), json!("Old"));
        let document = build_form_patch(
            3,
            &original,
            &draft,
            &[
                title_control(),
                FormControl {
                    id: "Microsoft.VSTS.Scheduling.StoryPoints".into(),
                    reference_name: "Microsoft.VSTS.Scheduling.StoryPoints".into(),
                    label: "Story Points".into(),
                    kind: FormControlKind::Double,
                    required: false,
                    read_only: false,
                    visible: true,
                    options: None,
                },
                FormControl {
                    id: "Microsoft.VSTS.Scheduling.StartDate".into(),
                    reference_name: "Microsoft.VSTS.Scheduling.StartDate".into(),
                    label: "Start Date".into(),
                    kind: FormControlKind::DateTime,
                    required: false,
                    read_only: false,
                    visible: true,
                    options: None,
                },
            ],
        );
        assert_eq!(
            document
                .iter()
                .filter_map(|op| op.path.strip_prefix("/fields/"))
                .collect::<Vec<_>>(),
            vec!["System.Title"]
        );
    }

    #[test]
    fn save_patch_tests_rev_and_only_dirty_editable_fields() {
        let mut original = serde_json::Map::new();
        original.insert("System.Title".into(), json!("Old"));
        original.insert("System.State".into(), json!("New"));
        let mut draft = original.clone();
        draft.insert("System.Title".into(), json!("New title"));
        let document = build_form_patch(
            9,
            &original,
            &draft,
            &[
                title_control(),
                FormControl {
                    id: "System.State".into(),
                    reference_name: "System.State".into(),
                    label: "State".into(),
                    kind: FormControlKind::Picklist,
                    required: true,
                    read_only: false,
                    visible: true,
                    options: None,
                },
            ],
        );
        assert_eq!(
            document,
            vec![
                JsonPatchOp {
                    op: PatchOp::Test,
                    path: "/rev".into(),
                    value: Some(json!(9)),
                },
                JsonPatchOp {
                    op: PatchOp::Add,
                    path: "/fields/System.Title".into(),
                    value: Some(json!("New title")),
                },
            ]
        );
    }

    #[test]
    fn encodes_dotted_ado_reference_names() {
        assert_eq!(form_field_name("System.Title"), "System::Title");
        assert_eq!(
            reference_name_from_form_field("System::Title"),
            "System.Title"
        );
        assert_eq!(
            form_field_name("Microsoft.VSTS.Scheduling.StartDate"),
            "Microsoft::VSTS::Scheduling::StartDate"
        );
    }

    #[test]
    fn default_values_and_submitted_draft_round_trip_ado_field_names() {
        let mut values = serde_json::Map::new();
        values.insert("System.Title".into(), json!("Persist cart"));
        values.insert(
            "System.AssignedTo".into(),
            json!({ "displayName": "Ada", "uniqueName": "ada@contoso.com" }),
        );
        let defaults = work_item_form_defaults(&values, &[title_control(), assigned_control()]);
        assert_eq!(defaults.get("System::Title"), Some(&json!("Persist cart")));
        let mut submitted = defaults.clone();
        submitted.insert("System::Title".into(), json!("Persist cart (saved)"));
        let draft = draft_from_form_values(&submitted);
        assert_eq!(
            draft.get("System.Title"),
            Some(&json!("Persist cart (saved)"))
        );
        let document = build_form_patch(5, &values, &draft, &[title_control(), assigned_control()]);
        assert_eq!(
            document,
            vec![
                JsonPatchOp {
                    op: PatchOp::Test,
                    path: "/rev".into(),
                    value: Some(json!(5)),
                },
                JsonPatchOp {
                    op: PatchOp::Add,
                    path: "/fields/System.Title".into(),
                    value: Some(json!("Persist cart (saved)")),
                },
            ]
        );
    }

    #[test]
    fn missing_or_empty_non_string_fields_patch_as_null() {
        let mut original = serde_json::Map::new();
        original.insert("Custom.Count".into(), json!(3));
        original.insert("System.AssignedTo".into(), json!({"displayName": "Ada"}));
        let mut draft = serde_json::Map::new();
        draft.insert("Custom.Count".into(), json!(""));
        let document = build_form_patch(
            2,
            &original,
            &draft,
            &[
                FormControl {
                    id: "Custom.Count".into(),
                    reference_name: "Custom.Count".into(),
                    label: "Count".into(),
                    kind: FormControlKind::Integer,
                    required: false,
                    read_only: false,
                    visible: true,
                    options: None,
                },
                assigned_control(),
            ],
        );
        assert!(
            document
                .iter()
                .any(|op| { op.path == "/fields/Custom.Count" && op.value == Some(Value::Null) })
        );
        assert!(
            document.iter().any(|op| {
                op.path == "/fields/System.AssignedTo" && op.value == Some(Value::Null)
            })
        );
    }

    #[test]
    fn required_field_checks_replace_zod() {
        let mut empty = serde_json::Map::new();
        empty.insert("System.Title".into(), json!(""));
        empty.insert("System.AssignedTo".into(), Value::Null);
        assert!(
            !missing_required_fields(&[title_control(), assigned_control()], &empty).is_empty()
        );
        let mut filled = serde_json::Map::new();
        filled.insert("System.Title".into(), json!("Cart"));
        filled.insert("System.AssignedTo".into(), Value::Null);
        assert!(validate_form_values(&[title_control(), assigned_control()], &filled).is_ok());
    }
}
