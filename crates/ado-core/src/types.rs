use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ADO_RESOURCE_SCOPE: &str = "499b84ac-1321-427f-aa17-267ca6975798/.default";
pub const ENTRA_AUTHORITY: &str = "https://login.microsoftonline.com/organizations";
pub const START_DATE_FIELD: &str = "Microsoft.VSTS.Scheduling.StartDate";
pub const TARGET_DATE_FIELD: &str = "Microsoft.VSTS.Scheduling.TargetDate";
pub const WIQL_RESULT_CAP: usize = 20_000;
pub const BATCH_SIZE: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityValue {
    pub display_name: String,
    pub unique_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemNode {
    pub id: i64,
    pub rev: i64,
    pub title: String,
    #[serde(rename = "type")]
    pub work_item_type: String,
    pub state: String,
    pub assigned_to: Option<IdentityValue>,
    pub parent_id: Option<i64>,
    pub area_path: String,
    pub iteration_path: String,
    pub start_date: Option<String>,
    pub target_date: Option<String>,
    pub has_date_fields: bool,
    pub fields: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HierarchyResult {
    pub nodes: Vec<WorkItemNode>,
    pub types: Vec<String>,
    pub top_backlog_types: Vec<String>,
    pub truncated: bool,
}

pub const ASSIGNEE_SPECIALS: [&str; 3] = ["anyone", "me", "unassigned"];

pub type AssigneeFilter = String;

pub fn is_assignee_special(value: &str) -> bool {
    ASSIGNEE_SPECIALS.contains(&value)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayFilter {
    pub types: Option<Vec<String>>,
    pub states: Option<Vec<String>>,
    /// `None` skips the Root-type overlay (`undefined` or `null` in prefs).
    /// `Some([])` matches nothing. `Some(names)` keeps those Root types.
    pub root_types: Option<Vec<String>>,
    pub assignee: AssigneeFilter,
    pub iteration_path: Option<String>,
    pub current_user_unique_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeSelection {
    pub org: String,
    pub project: String,
    pub team: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iteration_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonPatchOp {
    pub op: PatchOp,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PatchOp {
    Test,
    Add,
    Remove,
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormControlKind {
    String,
    Html,
    Identity,
    Picklist,
    Integer,
    Double,
    Boolean,
    DateTime,
    PlainText,
    TreePath,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormControl {
    pub id: String,
    pub reference_name: String,
    pub label: String,
    pub kind: FormControlKind,
    pub required: bool,
    pub read_only: bool,
    pub visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormGroup {
    pub id: String,
    pub label: String,
    pub controls: Vec<FormControl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormPage {
    pub id: String,
    pub label: String,
    pub groups: Vec<FormGroup>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkItemFormModel {
    pub id: i64,
    pub rev: i64,
    #[serde(rename = "type")]
    pub work_item_type: String,
    pub system_controls: Vec<FormControl>,
    pub pages: Vec<FormPage>,
    pub values: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    Entra,
    Pat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub signed_in: bool,
    pub display_name: Option<String>,
    pub username: Option<String>,
    pub auth_mode: AuthMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    pub account_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IterationNode {
    pub path: String,
    pub name: String,
    pub start_date: Option<String>,
    pub finish_date: Option<String>,
    pub children: Vec<IterationNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterPrompt {
    pub version: String,
    pub release_notes: Option<String>,
}
