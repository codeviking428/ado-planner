use std::collections::HashSet;

use ado_core::{
    FormControl, FormControlKind, IdentityValue, PatchOp, WorkItemFormModel, WorkItemNode,
    all_editable_controls, build_form_patch, html_to_markdown, identity_from_field,
    is_plain_html_field, markdown_to_html, missing_required_fields,
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
    pub show_empty: bool,
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
            show_empty: false,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.patch_ops().iter().any(|op| op.op != PatchOp::Test)
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

    /// Dialog heading: the title as of open/save, not the live Title field.
    pub fn heading_title(&self) -> String {
        self.model
            .values
            .get("System.Title")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| self.item_title())
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

    /// Write both date fields exactly, including JSON null to clear a side.
    pub fn set_exact_range_dates(&mut self, start: Option<String>, end: Option<String>) {
        self.values.insert(
            ado_core::START_DATE_FIELD.into(),
            start.map(|value| json!(value)).unwrap_or(Value::Null),
        );
        self.values.insert(
            ado_core::TARGET_DATE_FIELD.into(),
            end.map(|value| json!(value)).unwrap_or(Value::Null),
        );
    }

    /// Keep the open dialog's cached `/rev` and dates aligned with the live
    /// Gantt node so a later Save does not 412 after Undo or Clear dates.
    pub fn adopt_live_node(&mut self, node: &WorkItemNode) {
        if self.model.id != node.id {
            return;
        }
        self.model.rev = node.rev;
        self.set_exact_range_dates(node.start_date.clone(), node.target_date.clone());
        if let Some(start) = self.values.get(ado_core::START_DATE_FIELD).cloned() {
            self.model
                .values
                .insert(ado_core::START_DATE_FIELD.into(), start);
        }
        if let Some(target) = self.values.get(ado_core::TARGET_DATE_FIELD).cloned() {
            self.model
                .values
                .insert(ado_core::TARGET_DATE_FIELD.into(), target);
        }
    }

    /// Write Start/Target from a range picker. A missing side is left as-is
    /// unless both sides are empty (the user cleared the range).
    pub fn set_range_dates(&mut self, start: Option<String>, end: Option<String>) {
        match (start, end) {
            (None, None) => {
                self.values
                    .insert(ado_core::START_DATE_FIELD.into(), Value::Null);
                self.values
                    .insert(ado_core::TARGET_DATE_FIELD.into(), Value::Null);
            }
            (start, end) => {
                if let Some(start) = start {
                    self.values
                        .insert(ado_core::START_DATE_FIELD.into(), json!(start));
                }
                if let Some(end) = end {
                    self.values
                        .insert(ado_core::TARGET_DATE_FIELD.into(), json!(end));
                }
            }
        }
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
                node.assigned_to = identity_from_field(assigned);
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

    /// Fill Assigned To from the Gantt row when the form payload has no usable identity.
    pub fn hydrate_assigned_to(&mut self, node: &WorkItemNode) {
        if self
            .values
            .get("System.AssignedTo")
            .and_then(identity_from_field)
            .is_some()
        {
            return;
        }
        let Some(assigned) = node.assigned_to.as_ref() else {
            return;
        };
        let value = identity_json(assigned);
        self.values
            .insert("System.AssignedTo".into(), value.clone());
        self.model.values.insert("System.AssignedTo".into(), value);
    }
}

fn gantt_date_from_draft(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

const PRIMARY_FIELDS: [&str; 3] = ["System.Title", "System.State", "System.AssignedTo"];

pub struct FormDialogLayout {
    pub dates: Vec<FormControl>,
    pub primary: Vec<FormControl>,
    pub system_rest: Vec<FormControl>,
    pub groups: Vec<(String, Vec<FormControl>)>,
    pub extra: Vec<FormControl>,
}

pub fn control_is_empty(control: &FormControl, values: &serde_json::Map<String, Value>) -> bool {
    let value = values.get(&control.reference_name).unwrap_or(&Value::Null);
    match control.kind {
        FormControlKind::Boolean => matches!(value, Value::Null),
        FormControlKind::Identity => identity_from_field(value).is_none(),
        FormControlKind::Integer | FormControlKind::Double => match value {
            Value::Null => true,
            Value::String(text) => text.trim().is_empty(),
            _ => false,
        },
        _ => format_form_value(control.kind, value).trim().is_empty(),
    }
}

fn is_date_control(control: &FormControl) -> bool {
    control.kind == FormControlKind::DateTime
        && (control.reference_name == ado_core::START_DATE_FIELD
            || control.reference_name == ado_core::TARGET_DATE_FIELD)
}

fn date_value_is_empty(values: &serde_json::Map<String, Value>, reference: &str) -> bool {
    match values.get(reference) {
        None | Some(Value::Null) => true,
        Some(Value::String(text)) => text.trim().is_empty(),
        _ => false,
    }
}

fn classify_control(
    control: &FormControl,
    values: &serde_json::Map<String, Value>,
    has_start: bool,
    target_only: bool,
) -> FieldSlot {
    if control.reference_name == ado_core::TARGET_DATE_FIELD && has_start && !target_only {
        return FieldSlot::Skip;
    }
    if control.reference_name == ado_core::START_DATE_FIELD && target_only {
        return FieldSlot::Skip;
    }
    if is_date_control(control) {
        return FieldSlot::Date;
    }
    if PRIMARY_FIELDS.contains(&control.reference_name.as_str()) {
        return FieldSlot::Primary;
    }
    if !control.required
        && control.kind != FormControlKind::Boolean
        && control_is_empty(control, values)
    {
        return FieldSlot::Extra;
    }
    FieldSlot::Keep
}

enum FieldSlot {
    Date,
    Primary,
    Extra,
    Keep,
    Skip,
}

pub fn form_dialog_layout(form: &FormDraft) -> FormDialogLayout {
    let has_start = all_controls(&form.model)
        .iter()
        .any(|control| control.reference_name == ado_core::START_DATE_FIELD);
    let target_only = has_start
        && date_value_is_empty(&form.values, ado_core::START_DATE_FIELD)
        && !date_value_is_empty(&form.values, ado_core::TARGET_DATE_FIELD);
    let mut seen = HashSet::new();
    let mut dates = Vec::new();
    let mut primary = Vec::new();
    let mut system_rest = Vec::new();
    let mut extra = Vec::new();
    let mut take = |control: &FormControl, keep: &mut Vec<FormControl>| {
        if !control.visible || !seen.insert(control.reference_name.clone()) {
            return;
        }
        match classify_control(control, &form.values, has_start, target_only) {
            FieldSlot::Date => dates.push(control.clone()),
            FieldSlot::Primary => primary.push(control.clone()),
            FieldSlot::Extra => extra.push(control.clone()),
            FieldSlot::Keep => keep.push(control.clone()),
            FieldSlot::Skip => {}
        }
    };
    for control in &form.model.system_controls {
        take(control, &mut system_rest);
    }
    let mut groups = Vec::new();
    if let Some(page) = form.model.pages.get(form.active_page) {
        for group in &page.groups {
            let mut kept = Vec::new();
            for control in &group.controls {
                take(control, &mut kept);
            }
            if !kept.is_empty() {
                groups.push((group.label.clone(), kept));
            }
        }
    }
    FormDialogLayout {
        dates,
        primary,
        system_rest,
        groups,
        extra,
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
    fn unchanged_dialog_does_not_patch_assigned_to() {
        let mut values = serde_json::Map::new();
        values.insert("System.Title".into(), json!("Persist cart"));
        values.insert(
            "System.AssignedTo".into(),
            json!({
                "displayName": "Ada Lovelace",
                "uniqueName": "ada@contoso.com",
                "_links": { "avatar": { "href": "https://dev.azure.com/avatar" } }
            }),
        );
        let draft = FormDraft::from_model(WorkItemFormModel {
            id: 52917,
            rev: 4,
            work_item_type: "User Story".into(),
            system_controls: vec![
                FormControl {
                    id: "System.Title".into(),
                    reference_name: "System.Title".into(),
                    label: "Title".into(),
                    kind: FormControlKind::String,
                    required: true,
                    read_only: false,
                    visible: true,
                    options: None,
                },
                assigned_control(),
            ],
            pages: Vec::new(),
            values,
        });
        let ops = draft.patch_ops();
        assert!(
            ops.iter().all(|op| op.path != "/fields/System.AssignedTo"),
            "saving an unchanged dialog must not send System.AssignedTo: {ops:?}"
        );
    }

    #[test]
    fn hydrate_assigned_to_fills_empty_form_from_the_gantt_row() {
        let draft_model = WorkItemFormModel {
            id: 52917,
            rev: 4,
            work_item_type: "User Story".into(),
            system_controls: vec![assigned_control()],
            pages: Vec::new(),
            values: serde_json::Map::from_iter([(
                "System.AssignedTo".into(),
                json!({ "id": "guid-only", "descriptor": "aad.1" }),
            )]),
        };
        let mut draft = FormDraft::from_model(draft_model);
        let node = WorkItemNode {
            id: 52917,
            rev: 4,
            title: "Cart".into(),
            work_item_type: "User Story".into(),
            state: "Active".into(),
            assigned_to: Some(IdentityValue {
                display_name: "Ada Lovelace".into(),
                unique_name: "ada@contoso.com".into(),
                id: Some("guid-ada".into()),
            }),
            parent_id: None,
            area_path: "Shop".into(),
            iteration_path: "Shop".into(),
            start_date: None,
            target_date: None,
            has_date_fields: true,
            predecessors: Vec::new(),
            fields: serde_json::Map::new(),
        };
        draft.hydrate_assigned_to(&node);
        assert_eq!(
            identity_from_field(draft.values.get("System.AssignedTo").expect("value"))
                .expect("identity")
                .display_name,
            "Ada Lovelace"
        );
        let ops = draft.patch_ops();
        assert!(
            ops.iter().all(|op| op.path != "/fields/System.AssignedTo"),
            "hydrating from the row must not count as an Assigned To edit: {ops:?}"
        );
    }

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
            predecessors: Vec::new(),
            fields: serde_json::Map::new(),
        }];
        draft.apply_saved(&mut nodes, 2);
        assert_eq!(nodes[0].rev, 2);
        assert!(nodes[0].start_date.is_none());
        assert!(nodes[0].target_date.is_none());
    }

    #[test]
    fn range_picker_start_only_leaves_target() {
        let mut values = serde_json::Map::new();
        values.insert(
            ado_core::START_DATE_FIELD.into(),
            json!("2026-07-06T00:00:00Z"),
        );
        values.insert(
            ado_core::TARGET_DATE_FIELD.into(),
            json!("2026-07-20T00:00:00Z"),
        );
        let mut draft = FormDraft::from_model(WorkItemFormModel {
            id: 1,
            rev: 1,
            work_item_type: "User Story".into(),
            system_controls: Vec::new(),
            pages: Vec::new(),
            values,
        });
        draft.set_range_dates(Some("2026-08-10T00:00:00Z".into()), None);
        assert_eq!(
            draft.values.get(ado_core::START_DATE_FIELD),
            Some(&json!("2026-08-10T00:00:00Z"))
        );
        assert_eq!(
            draft.values.get(ado_core::TARGET_DATE_FIELD),
            Some(&json!("2026-07-20T00:00:00Z")),
            "picking only Start must not clear Target Date"
        );
        draft.set_range_dates(None, None);
        assert_eq!(
            draft.values.get(ado_core::START_DATE_FIELD),
            Some(&Value::Null)
        );
        assert_eq!(
            draft.values.get(ado_core::TARGET_DATE_FIELD),
            Some(&Value::Null)
        );
        draft.set_exact_range_dates(Some("2026-08-10T00:00:00Z".into()), None);
        assert_eq!(
            draft.values.get(ado_core::START_DATE_FIELD),
            Some(&json!("2026-08-10T00:00:00Z"))
        );
        assert_eq!(
            draft.values.get(ado_core::TARGET_DATE_FIELD),
            Some(&Value::Null),
            "an explicit start-only commit must clear Target Date"
        );
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
        let mut draft = FormDraft::from_model(model);
        assert_eq!(draft.item_title(), "Hello");
        assert_eq!(draft.heading_title(), "Hello");
        draft.set_string("System.Title", "Typed while open".into());
        assert_eq!(draft.item_title(), "Typed while open");
        assert_eq!(
            draft.heading_title(),
            "Hello",
            "the dialog heading must stay on the opened title until save"
        );
        assert!(draft.title().contains("#1"));
        assert!(draft.is_plain_html("System.Description"));
        assert!(
            draft
                .html_markdown
                .get("System.Description")
                .is_some_and(|md| md.to_lowercase().contains("cart"))
        );
    }

    fn control(reference: &str, label: &str, kind: FormControlKind) -> FormControl {
        FormControl {
            id: reference.into(),
            reference_name: reference.into(),
            label: label.into(),
            kind,
            required: false,
            read_only: false,
            visible: true,
            options: None,
        }
    }

    #[test]
    fn unchanged_form_is_not_dirty() {
        let mut values = serde_json::Map::new();
        values.insert("System.Title".into(), json!("Persist cart"));
        let draft = FormDraft::from_model(WorkItemFormModel {
            id: 1,
            rev: 1,
            work_item_type: "User Story".into(),
            system_controls: vec![control("System.Title", "Title", FormControlKind::String)],
            pages: Vec::new(),
            values,
        });
        assert!(!draft.is_dirty());
    }

    #[test]
    fn dialog_layout_hoists_dates_and_hides_empty_custom_fields() {
        let mut values = serde_json::Map::new();
        values.insert("System.Title".into(), json!("Persist cart"));
        values.insert("System.State".into(), json!("New"));
        values.insert(
            ado_core::START_DATE_FIELD.into(),
            json!("2026-07-06T00:00:00Z"),
        );
        values.insert(
            ado_core::TARGET_DATE_FIELD.into(),
            json!("2026-07-20T00:00:00Z"),
        );
        let draft = FormDraft::from_model(WorkItemFormModel {
            id: 1,
            rev: 1,
            work_item_type: "User Story".into(),
            system_controls: vec![
                control("System.Title", "Title", FormControlKind::String),
                control("System.State", "State", FormControlKind::String),
            ],
            pages: vec![ado_core::FormPage {
                id: "details".into(),
                label: "Details".into(),
                groups: vec![ado_core::FormGroup {
                    id: "planning".into(),
                    label: "Planning".into(),
                    controls: vec![
                        control(
                            "Custom.TShirtSize",
                            "T-Shirt Size",
                            FormControlKind::Picklist,
                        ),
                        control(
                            ado_core::START_DATE_FIELD,
                            "Start Date",
                            FormControlKind::DateTime,
                        ),
                        control(
                            ado_core::TARGET_DATE_FIELD,
                            "Target Date",
                            FormControlKind::DateTime,
                        ),
                        control(
                            "Microsoft.VSTS.Scheduling.StoryPoints",
                            "Story Points",
                            FormControlKind::Double,
                        ),
                    ],
                }],
            }],
            values,
        });
        let layout = form_dialog_layout(&draft);
        assert_eq!(layout.dates.len(), 1);
        assert_eq!(layout.dates[0].reference_name, ado_core::START_DATE_FIELD);
        assert!(
            layout
                .primary
                .iter()
                .any(|control| control.reference_name == "System.Title")
        );
        assert!(
            layout
                .extra
                .iter()
                .any(|control| control.reference_name == "Custom.TShirtSize"),
            "empty custom fields belong under More: {:?}",
            layout
                .extra
                .iter()
                .map(|c| &c.reference_name)
                .collect::<Vec<_>>()
        );
        assert!(
            !layout
                .groups
                .iter()
                .flat_map(|(_, controls)| controls)
                .any(|control| control.reference_name == "Custom.TShirtSize")
        );
    }

    #[test]
    fn milestone_dialog_keeps_the_target_date_picker() {
        let mut values = serde_json::Map::new();
        values.insert("System.Title".into(), json!("Ship"));
        values.insert(
            ado_core::TARGET_DATE_FIELD.into(),
            json!("2026-11-02T00:00:00Z"),
        );
        let draft = FormDraft::from_model(WorkItemFormModel {
            id: 2,
            rev: 1,
            work_item_type: "Milestone".into(),
            system_controls: vec![control("System.Title", "Title", FormControlKind::String)],
            pages: vec![ado_core::FormPage {
                id: "details".into(),
                label: "Details".into(),
                groups: vec![ado_core::FormGroup {
                    id: "planning".into(),
                    label: "Planning".into(),
                    controls: vec![
                        control(
                            ado_core::START_DATE_FIELD,
                            "Start Date",
                            FormControlKind::DateTime,
                        ),
                        control(
                            ado_core::TARGET_DATE_FIELD,
                            "Target Date",
                            FormControlKind::DateTime,
                        ),
                    ],
                }],
            }],
            values,
        });
        let layout = form_dialog_layout(&draft);
        assert_eq!(layout.dates.len(), 1);
        assert_eq!(
            layout.dates[0].reference_name,
            ado_core::TARGET_DATE_FIELD,
            "a target-only item must keep the date picker, got {:?}",
            layout
                .dates
                .iter()
                .map(|c| &c.reference_name)
                .collect::<Vec<_>>()
        );
    }
}
