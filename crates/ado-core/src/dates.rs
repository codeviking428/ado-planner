use std::collections::HashMap;

use jiff::ToSpan;
use jiff::civil::Date;
use serde_json::{Value, json};

use crate::types::{JsonPatchOp, PatchOp, START_DATE_FIELD, TARGET_DATE_FIELD, WorkItemNode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IterationHint {
    pub start_date: Option<String>,
    pub finish_date: Option<String>,
}

pub fn is_unscheduled(start_date: Option<&str>, target_date: Option<&str>) -> bool {
    start_date.is_none() && target_date.is_none()
}

/// Inclusive length of a start-only bar when Target Date is still empty.
pub const DEFAULT_BAR_DAYS: i32 = 7;

pub fn to_gantt_inclusive_target(target_date: Date) -> Date {
    target_date.saturating_add(1.day())
}

pub fn from_gantt_exclusive_end(end: Date) -> Date {
    end.saturating_sub(1.day())
}

pub fn iso_date_only(date: Date) -> String {
    format!("{date}T00:00:00Z")
}

pub fn parse_iso_date(value: Option<&str>) -> Option<Date> {
    let value = value?;
    if value.is_empty() {
        return None;
    }
    let ymd = value.get(..10)?;
    ymd.parse().ok()
}

const MONTH_ABBREVS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Parse a typed date. Display copy is compact (`Jul 6`); ISO `YYYY-MM-DD` is
/// still accepted as input, with an optional year on compact forms.
///
/// Year-less compact copy (`Jul 6`) uses `fallback_year` so confirming or
/// editing a 2025/2027 bar does not PATCH today's year. `None` falls back to
/// the current calendar year (new / unscheduled dates).
pub fn parse_user_date(value: Option<&str>, fallback_year: Option<i16>) -> Option<Date> {
    let value = value?.trim();
    if value.is_empty() || value == "…" {
        return None;
    }
    if let Some(date) = parse_iso_date(Some(value)) {
        return Some(date);
    }
    parse_compact_date(value, fallback_year)
}

fn parse_compact_date(value: &str, fallback_year: Option<i16>) -> Option<Date> {
    let mut parts = value.split_whitespace();
    let month_token = parts.next()?;
    let day_token = parts.next()?;
    let year_token = parts.next();
    let month = MONTH_ABBREVS
        .iter()
        .position(|name| name.eq_ignore_ascii_case(month_token))? as i8
        + 1;
    let day: i8 = day_token.trim_end_matches(',').parse().ok()?;
    let year: i16 = match year_token {
        Some(year) => year.trim_end_matches(',').parse().ok()?,
        None => fallback_year.unwrap_or_else(|| jiff::Zoned::now().date().year()),
    };
    Date::new(year, month, day).ok()
}

/// Id and parent → children lookups over one node slice, built once so each
/// Rollup or child lookup walks only its subtree instead of the whole slice.
/// Positions refer to the slice passed to [`HierarchyIndex::new`]; pass that
/// same slice to the lookups.
#[derive(Debug, Clone, Default)]
pub struct HierarchyIndex {
    by_id: HashMap<i64, usize>,
    children: HashMap<i64, Vec<usize>>,
}

impl HierarchyIndex {
    pub fn new(nodes: &[WorkItemNode]) -> Self {
        let mut by_id = HashMap::with_capacity(nodes.len());
        let mut children: HashMap<i64, Vec<usize>> = HashMap::new();
        for (index, node) in nodes.iter().enumerate() {
            by_id.entry(node.id).or_insert(index);
            if let Some(parent) = node.parent_id {
                children.entry(parent).or_default().push(index);
            }
        }
        Self { by_id, children }
    }

    pub fn get<'a>(&self, nodes: &'a [WorkItemNode], id: i64) -> Option<&'a WorkItemNode> {
        self.by_id.get(&id).and_then(|&index| nodes.get(index))
    }

    pub fn children<'a>(
        &'a self,
        nodes: &'a [WorkItemNode],
        id: i64,
    ) -> impl Iterator<Item = &'a WorkItemNode> + 'a {
        self.children
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|&index| nodes.get(index))
    }

    /// Same as [`client_rollup`] without rebuilding the children map.
    pub fn rollup(&self, nodes: &[WorkItemNode], parent_id: i64) -> Option<(Date, Date)> {
        let mut span: Option<(Date, Date)> = None;
        let mut stack = vec![parent_id];
        // A parent cycle would otherwise loop forever.
        let mut visits_left = nodes.len();
        while let Some(id) = stack.pop() {
            for child in self.children(nodes, id) {
                if visits_left == 0 {
                    return span;
                }
                visits_left -= 1;
                if let (Some(start), Some(target)) = (
                    parse_iso_date(child.start_date.as_deref()),
                    parse_iso_date(child.target_date.as_deref()),
                ) {
                    span = Some(match span {
                        Some((min, max)) => (min.min(start), max.max(target)),
                        None => (start, target),
                    });
                }
                stack.push(child.id);
            }
        }
        span
    }
}

pub fn client_rollup(nodes: &[WorkItemNode], parent_id: i64) -> Option<(Date, Date)> {
    HierarchyIndex::new(nodes).rollup(nodes, parent_id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GanttBarKind {
    Scheduled,
    Unscheduled,
    NoFields,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GanttBarDisplay {
    pub kind: GanttBarKind,
    pub start: Option<Date>,
    pub end: Option<Date>,
    pub milestone: bool,
}

pub fn gantt_bar_display(
    node: &WorkItemNode,
    nodes: &[WorkItemNode],
    iteration: Option<&IterationHint>,
) -> GanttBarDisplay {
    bar_display(node, iteration, || client_rollup(nodes, node.id))
}

/// [`gantt_bar_display`] with a prebuilt index over `nodes`.
pub fn gantt_bar_display_indexed(
    node: &WorkItemNode,
    nodes: &[WorkItemNode],
    index: &HierarchyIndex,
    iteration: Option<&IterationHint>,
) -> GanttBarDisplay {
    bar_display(node, iteration, || index.rollup(nodes, node.id))
}

fn bar_display(
    node: &WorkItemNode,
    iteration: Option<&IterationHint>,
    rollup: impl FnOnce() -> Option<(Date, Date)>,
) -> GanttBarDisplay {
    if !node.has_date_fields {
        return GanttBarDisplay {
            kind: GanttBarKind::NoFields,
            start: None,
            end: None,
            milestone: false,
        };
    }
    let start = parse_iso_date(node.start_date.as_deref());
    let target = parse_iso_date(node.target_date.as_deref());
    if let (Some(start), Some(target)) = (start, target) {
        return GanttBarDisplay {
            kind: GanttBarKind::Scheduled,
            start: Some(start),
            end: Some(target),
            milestone: false,
        };
    }
    if let Some(start) = start {
        let end = start.saturating_add((DEFAULT_BAR_DAYS - 1).days());
        return GanttBarDisplay {
            kind: GanttBarKind::Scheduled,
            start: Some(start),
            end: Some(end),
            milestone: false,
        };
    }
    if let Some(target) = target {
        return GanttBarDisplay {
            kind: GanttBarKind::Scheduled,
            start: Some(target),
            end: Some(target),
            milestone: true,
        };
    }
    if let Some((start, target)) = rollup() {
        return GanttBarDisplay {
            kind: GanttBarKind::Unscheduled,
            start: Some(start),
            end: Some(target),
            milestone: false,
        };
    }
    if let Some(iteration) = iteration
        && iteration.start_date.is_some()
        && iteration.finish_date.is_some()
    {
        return GanttBarDisplay {
            kind: GanttBarKind::Unscheduled,
            start: parse_iso_date(iteration.start_date.as_deref()),
            end: parse_iso_date(iteration.finish_date.as_deref()),
            milestone: false,
        };
    }
    GanttBarDisplay {
        kind: GanttBarKind::Unscheduled,
        start: None,
        end: None,
        milestone: false,
    }
}

pub fn build_clear_dates_patch(rev: i64) -> Vec<JsonPatchOp> {
    vec![
        JsonPatchOp {
            op: PatchOp::Test,
            path: "/rev".into(),
            value: Some(json!(rev)),
        },
        JsonPatchOp {
            op: PatchOp::Add,
            path: format!("/fields/{START_DATE_FIELD}"),
            value: Some(json!(null)),
        },
        JsonPatchOp {
            op: PatchOp::Add,
            path: format!("/fields/{TARGET_DATE_FIELD}"),
            value: Some(json!(null)),
        },
    ]
}

pub fn schedule_from_iteration(iteration: &IterationHint) -> Option<(Date, Date)> {
    let start = parse_iso_date(iteration.start_date.as_deref())?;
    let finish = parse_iso_date(iteration.finish_date.as_deref())?;
    Some((start, finish))
}

pub fn build_field_patch(rev: i64, reference: &str, value: Value) -> Vec<JsonPatchOp> {
    vec![
        JsonPatchOp {
            op: PatchOp::Test,
            path: "/rev".into(),
            value: Some(json!(rev)),
        },
        JsonPatchOp {
            op: PatchOp::Add,
            path: format!("/fields/{reference}"),
            value: Some(value),
        },
    ]
}

pub fn build_start_target_patch(rev: i64, start_date: Date, target_date: Date) -> Vec<JsonPatchOp> {
    build_dates_patch(rev, Some(start_date), Some(target_date))
}

pub fn build_dates_patch(
    rev: i64,
    start_date: Option<Date>,
    target_date: Option<Date>,
) -> Vec<JsonPatchOp> {
    let mut ops = vec![JsonPatchOp {
        op: PatchOp::Test,
        path: "/rev".into(),
        value: Some(json!(rev)),
    }];
    if let Some(start_date) = start_date {
        ops.push(JsonPatchOp {
            op: PatchOp::Add,
            path: format!("/fields/{START_DATE_FIELD}"),
            value: Some(json!(iso_date_only(start_date))),
        });
    }
    if let Some(target_date) = target_date {
        ops.push(JsonPatchOp {
            op: PatchOp::Add,
            path: format!("/fields/{TARGET_DATE_FIELD}"),
            value: Some(json!(iso_date_only(target_date))),
        });
    }
    ops
}

/// Write both date fields, using JSON null to clear a side. History undo/redo
/// needs this so a start-only item can lose a Target Date again.
pub fn build_replace_dates_patch(
    rev: i64,
    start_date: Option<Date>,
    target_date: Option<Date>,
) -> Vec<JsonPatchOp> {
    vec![
        JsonPatchOp {
            op: PatchOp::Test,
            path: "/rev".into(),
            value: Some(json!(rev)),
        },
        JsonPatchOp {
            op: PatchOp::Add,
            path: format!("/fields/{START_DATE_FIELD}"),
            value: Some(
                start_date
                    .map(|date| json!(iso_date_only(date)))
                    .unwrap_or(Value::Null),
            ),
        },
        JsonPatchOp {
            op: PatchOp::Add,
            path: format!("/fields/{TARGET_DATE_FIELD}"),
            value: Some(
                target_date
                    .map(|date| json!(iso_date_only(date)))
                    .unwrap_or(Value::Null),
            ),
        },
    ]
}

pub fn parent_relation_op(parent_url: &str) -> JsonPatchOp {
    JsonPatchOp {
        op: PatchOp::Add,
        path: "/relations/-".into(),
        value: Some(json!({
            "rel": "System.LinkTypes.Hierarchy-Reverse",
            "url": parent_url,
        })),
    }
}

pub fn build_add_dependency_patch(rev: i64, predecessor_url: &str) -> Vec<JsonPatchOp> {
    vec![
        JsonPatchOp {
            op: PatchOp::Test,
            path: "/rev".into(),
            value: Some(json!(rev)),
        },
        JsonPatchOp {
            op: PatchOp::Add,
            path: "/relations/-".into(),
            value: Some(json!({
                "rel": "System.LinkTypes.Dependency-Reverse",
                "url": predecessor_url,
            })),
        },
    ]
}

pub fn build_remove_dependency_patch(rev: i64, relation_index: usize) -> Vec<JsonPatchOp> {
    vec![
        JsonPatchOp {
            op: PatchOp::Test,
            path: "/rev".into(),
            value: Some(json!(rev)),
        },
        JsonPatchOp {
            op: PatchOp::Remove,
            path: format!("/relations/{relation_index}"),
            value: None,
        },
    ]
}

pub fn type_has_start_and_target(field_reference_names: &[impl AsRef<str>]) -> bool {
    let has_start = field_reference_names
        .iter()
        .any(|name| name.as_ref() == START_DATE_FIELD);
    let has_target = field_reference_names
        .iter()
        .any(|name| name.as_ref() == TARGET_DATE_FIELD);
    has_start && has_target
}

/// Short month-and-day copy for tree cells and form fields (`Jul 6`).
pub fn compact_date(date: Date) -> String {
    let month = MONTH_ABBREVS[(date.month() as u8).saturating_sub(1).min(11) as usize];
    format!("{month} {}", date.day())
}

/// Start/target copy using `→`, matching toasts and the Gantt Dates column.
pub fn format_date_range(start: Option<Date>, target: Option<Date>) -> String {
    match (start, target) {
        (Some(start), Some(target)) => {
            format!("{} → {}", compact_date(start), compact_date(target))
        }
        (Some(start), None) => format!("{} → …", compact_date(start)),
        (None, Some(target)) => format!("◆ {}", compact_date(target)),
        (None, None) => String::new(),
    }
}

/// Tree-column copy. Iteration/rollup hints are display-only — never PATCHed.
pub fn dates_column_text(
    node: &WorkItemNode,
    nodes: &[WorkItemNode],
    iteration: Option<&IterationHint>,
) -> String {
    dates_text(node, iteration, || client_rollup(nodes, node.id))
}

/// [`dates_column_text`] with a prebuilt index over `nodes`.
pub fn dates_column_text_indexed(
    node: &WorkItemNode,
    nodes: &[WorkItemNode],
    index: &HierarchyIndex,
    iteration: Option<&IterationHint>,
) -> String {
    dates_text(node, iteration, || index.rollup(nodes, node.id))
}

fn dates_text(
    node: &WorkItemNode,
    iteration: Option<&IterationHint>,
    rollup: impl FnOnce() -> Option<(Date, Date)>,
) -> String {
    if !node.has_date_fields {
        return "No Start/Target on type".into();
    }
    let start = parse_iso_date(node.start_date.as_deref());
    let target = parse_iso_date(node.target_date.as_deref());
    let scheduled = format_date_range(start, target);
    if !scheduled.is_empty() {
        return scheduled;
    }
    if let Some((start, target)) = rollup() {
        return format!(
            "Unscheduled · rollup {} → {}",
            compact_date(start),
            compact_date(target)
        );
    }
    if let Some(iteration) = iteration
        && let (Some(start), Some(target)) = (
            parse_iso_date(iteration.start_date.as_deref()),
            parse_iso_date(iteration.finish_date.as_deref()),
        )
    {
        return format!(
            "Unscheduled · iteration {} → {}",
            compact_date(start),
            compact_date(target)
        );
    }
    "Unscheduled".into()
}

pub fn iteration_hint_for_path(
    path: &str,
    iterations: &[crate::types::IterationNode],
) -> Option<IterationHint> {
    iterations
        .iter()
        .find(|row| row.path == path)
        .map(|row| IterationHint {
            start_date: row.start_date.clone(),
            finish_date: row.finish_date.clone(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::WorkItemNode;
    use serde_json::json;

    fn node(id: i64) -> WorkItemNode {
        WorkItemNode {
            id,
            rev: 1,
            title: "WI".into(),
            work_item_type: "Feature".into(),
            state: "New".into(),
            assigned_to: None,
            parent_id: None,
            area_path: "Shop".into(),
            iteration_path: "Shop\\Sprint 12".into(),
            start_date: None,
            target_date: None,
            has_date_fields: true,
            predecessors: Vec::new(),
            fields: serde_json::Map::new(),
        }
    }

    #[test]
    fn parse_user_date_accepts_iso_and_compact() {
        let july_6 = Date::new(2026, 7, 6).unwrap();
        assert_eq!(parse_user_date(Some("2026-07-06"), None), Some(july_6));
        assert_eq!(
            parse_user_date(Some("2026-07-06T00:00:00Z"), None),
            Some(july_6)
        );
        assert_eq!(parse_user_date(Some("Jul 6, 2026"), None), Some(july_6));
        let year = jiff::Zoned::now().date().year();
        assert_eq!(
            parse_user_date(Some("Jul 6"), None),
            Some(Date::new(year, 7, 6).unwrap())
        );
        assert_eq!(parse_user_date(Some("…"), None), None);
    }

    #[test]
    fn yearless_compact_keeps_the_fallback_year() {
        assert_eq!(
            parse_user_date(Some("Jul 6"), Some(2025)),
            Some(Date::new(2025, 7, 6).unwrap()),
            "confirming compact copy on a 2025 bar must not jump to this year"
        );
        assert_eq!(
            parse_user_date(Some("Jul 6"), Some(2027)),
            Some(Date::new(2027, 7, 6).unwrap())
        );
        assert_eq!(
            parse_user_date(Some("Jul 6, 2024"), Some(2025)),
            Some(Date::new(2024, 7, 6).unwrap()),
            "an explicit year must win over the item year"
        );
    }

    #[test]
    fn empty_start_or_target_is_unscheduled() {
        assert!(is_unscheduled(None, None));
        assert!(!is_unscheduled(Some("2026-07-01T00:00:00Z"), None));
        assert!(!is_unscheduled(None, Some("2026-07-10T00:00:00Z")));
        assert!(!is_unscheduled(
            Some("2026-07-01T00:00:00Z"),
            Some("2026-07-10T00:00:00Z")
        ));
    }

    #[test]
    fn start_only_gets_a_default_length_bar() {
        let mut item = node(1);
        item.start_date = Some("2026-07-06T00:00:00Z".into());
        let display = gantt_bar_display(&item, &[item.clone()], None);
        assert_eq!(display.kind, GanttBarKind::Scheduled);
        assert!(!display.milestone);
        assert_eq!(
            display.start.map(|d| d.to_string()).as_deref(),
            Some("2026-07-06")
        );
        assert_eq!(
            display.end.map(|d| d.to_string()).as_deref(),
            Some("2026-07-12")
        );
        assert_eq!(dates_column_text(&item, &[item.clone()], None), "Jul 6 → …");
    }

    #[test]
    fn target_only_is_a_milestone() {
        let mut item = node(1);
        item.target_date = Some("2026-07-20T00:00:00Z".into());
        let display = gantt_bar_display(&item, &[item.clone()], None);
        assert!(display.milestone);
        assert_eq!(display.start, display.end);
        assert_eq!(dates_column_text(&item, &[item.clone()], None), "◆ Jul 20");
    }

    #[test]
    fn drag_patch_writes_start_and_target_with_test_rev() {
        let document = build_start_target_patch(
            5,
            "2026-07-06".parse().unwrap(),
            "2026-07-20".parse().unwrap(),
        );
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
                    path: format!("/fields/{START_DATE_FIELD}"),
                    value: Some(json!("2026-07-06T00:00:00Z")),
                },
                JsonPatchOp {
                    op: PatchOp::Add,
                    path: format!("/fields/{TARGET_DATE_FIELD}"),
                    value: Some(json!("2026-07-20T00:00:00Z")),
                },
            ]
        );
        let encoded = serde_json::to_string(&document).unwrap();
        assert!(!encoded.contains("IterationPath"));
        let start_only = build_dates_patch(5, Some("2026-07-06".parse().unwrap()), None);
        assert_eq!(start_only.len(), 2);
        assert_eq!(start_only[1].path, format!("/fields/{START_DATE_FIELD}"));
        let link = build_add_dependency_patch(3, "https://example/_apis/wit/workitems/1003");
        assert_eq!(link[1].path, "/relations/-");
    }

    #[test]
    fn exclusive_end_converts_to_inclusive_target_date() {
        let target: Date = "2026-07-20".parse().unwrap();
        let exclusive = to_gantt_inclusive_target(target);
        assert_eq!(iso_date_only(exclusive), "2026-07-21T00:00:00Z");
        assert_eq!(
            iso_date_only(from_gantt_exclusive_end(exclusive)),
            "2026-07-20T00:00:00Z"
        );
    }

    #[test]
    fn parent_rollup_is_min_child_start_max_child_target() {
        let mut parent = node(1);
        parent.work_item_type = "Epic".into();
        parent.title = "Epic".into();
        parent.iteration_path = "Shop".into();
        let mut a = node(2);
        a.title = "A".into();
        a.parent_id = Some(1);
        a.state = "Active".into();
        a.iteration_path = "Shop".into();
        a.start_date = Some("2026-07-10T00:00:00Z".into());
        a.target_date = Some("2026-07-12T00:00:00Z".into());
        let mut b = node(3);
        b.title = "B".into();
        b.parent_id = Some(1);
        b.state = "Active".into();
        b.iteration_path = "Shop".into();
        b.start_date = Some("2026-07-01T00:00:00Z".into());
        b.target_date = Some("2026-08-01T00:00:00Z".into());
        let nodes = vec![parent, a, b];
        let (start, target) = client_rollup(&nodes, 1).unwrap();
        assert_eq!(iso_date_only(start), "2026-07-01T00:00:00Z");
        assert_eq!(iso_date_only(target), "2026-08-01T00:00:00Z");
    }

    #[test]
    fn indexed_rollup_walks_grandchildren_like_client_rollup() {
        let epic = node(1);
        let mut feature = node(2);
        feature.parent_id = Some(1);
        let mut story = node(3);
        story.parent_id = Some(2);
        story.start_date = Some("2026-07-01T00:00:00Z".into());
        story.target_date = Some("2026-07-09T00:00:00Z".into());
        let mut task = node(4);
        task.parent_id = Some(3);
        task.start_date = Some("2026-07-05T00:00:00Z".into());
        task.target_date = Some("2026-08-02T00:00:00Z".into());
        let mut start_only = node(5);
        start_only.parent_id = Some(2);
        start_only.start_date = Some("2026-06-01T00:00:00Z".into());
        let nodes = vec![epic, feature, story, task, start_only];
        let index = HierarchyIndex::new(&nodes);
        let span = Some((
            Date::new(2026, 7, 1).unwrap(),
            Date::new(2026, 8, 2).unwrap(),
        ));
        assert_eq!(client_rollup(&nodes, 1), span);
        assert_eq!(index.rollup(&nodes, 1), span);
        assert_eq!(index.rollup(&nodes, 4), None);
        for node in &nodes {
            assert_eq!(
                gantt_bar_display_indexed(node, &nodes, &index, None),
                gantt_bar_display(node, &nodes, None)
            );
            assert_eq!(
                dates_column_text_indexed(node, &nodes, &index, None),
                dates_column_text(node, &nodes, None)
            );
        }
        let children: Vec<i64> = index.children(&nodes, 2).map(|n| n.id).collect();
        assert_eq!(children, vec![3, 5]);
        assert_eq!(index.get(&nodes, 4).map(|n| n.id), Some(4));
        assert!(index.get(&nodes, 99).is_none());
    }

    #[test]
    fn rollup_stops_on_a_parent_cycle() {
        let mut a = node(1);
        a.parent_id = Some(2);
        a.start_date = Some("2026-07-01T00:00:00Z".into());
        a.target_date = Some("2026-07-02T00:00:00Z".into());
        let mut b = node(2);
        b.parent_id = Some(1);
        let nodes = vec![a, b];
        assert!(client_rollup(&nodes, 2).is_some());
    }

    #[test]
    fn replace_dates_patch_writes_null_to_clear_a_side() {
        let document = build_replace_dates_patch(4, Some("2026-07-06".parse().unwrap()), None);
        assert_eq!(document[0].path, "/rev");
        assert_eq!(document[1].value, Some(json!("2026-07-06T00:00:00Z")));
        assert_eq!(document[2].value, Some(Value::Null));
        let parent = parent_relation_op("https://example/_apis/wit/workitems/1");
        assert_eq!(parent.path, "/relations/-");
        assert_eq!(
            parent.value.as_ref().unwrap()["rel"],
            "System.LinkTypes.Hierarchy-Reverse"
        );
    }

    #[test]
    fn types_without_start_and_target_cannot_be_dragged() {
        assert!(!type_has_start_and_target(&[
            "System.Title",
            "System.State"
        ]));
        assert!(type_has_start_and_target(&[
            START_DATE_FIELD,
            TARGET_DATE_FIELD
        ]));
    }

    #[test]
    fn unscheduled_parent_shows_client_side_rollup_as_display_hint() {
        let parent = node(1);
        let mut child = node(2);
        child.parent_id = Some(1);
        child.start_date = Some("2026-08-17T00:00:00Z".into());
        child.target_date = Some("2026-09-04T00:00:00Z".into());
        assert_eq!(
            dates_column_text(&parent, &[parent.clone(), child], None),
            "Unscheduled · rollup Aug 17 → Sep 4"
        );
    }

    #[test]
    fn unscheduled_leaf_shows_iteration_path_dates_as_muted_display_hint() {
        let mut leaf = node(5);
        leaf.work_item_type = "Task".into();
        leaf.title = "Cart UI".into();
        assert_eq!(
            dates_column_text(
                &leaf,
                &[leaf.clone()],
                Some(&IterationHint {
                    start_date: Some("2026-08-10T00:00:00Z".into()),
                    finish_date: Some("2026-08-21T00:00:00Z".into()),
                })
            ),
            "Unscheduled · iteration Aug 10 → Aug 21"
        );
    }

    #[test]
    fn scheduled_work_item_shows_own_start_and_target_not_iteration_hint() {
        let mut scheduled = node(3);
        scheduled.start_date = Some("2026-07-06T00:00:00Z".into());
        scheduled.target_date = Some("2026-07-20T00:00:00Z".into());
        assert_eq!(
            dates_column_text(
                &scheduled,
                &[scheduled.clone()],
                Some(&IterationHint {
                    start_date: Some("2026-07-13T00:00:00Z".into()),
                    finish_date: Some("2026-07-24T00:00:00Z".into()),
                })
            ),
            "Jul 6 → Jul 20"
        );
    }

    #[test]
    fn schedule_from_iteration_keeps_inclusive_finish_date() {
        let (start, target) = schedule_from_iteration(&IterationHint {
            start_date: Some("2026-07-13T00:00:00Z".into()),
            finish_date: Some("2026-07-24T00:00:00Z".into()),
        })
        .unwrap();
        assert_eq!(start.to_string(), "2026-07-13");
        assert_eq!(target.to_string(), "2026-07-24");
    }

    #[test]
    fn type_without_start_target_is_not_treated_as_unscheduled() {
        let mut item = node(11);
        item.has_date_fields = false;
        item.work_item_type = "Task".into();
        assert_eq!(
            dates_column_text(&item, &[item.clone()], None),
            "No Start/Target on type"
        );
    }
}
