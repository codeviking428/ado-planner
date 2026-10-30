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

pub fn client_rollup(nodes: &[WorkItemNode], parent_id: i64) -> Option<(Date, Date)> {
    let mut dates: Vec<(Date, Date)> = Vec::new();
    let mut children: std::collections::HashMap<i64, Vec<usize>> = std::collections::HashMap::new();
    for (index, node) in nodes.iter().enumerate() {
        if let Some(parent) = node.parent_id {
            children.entry(parent).or_default().push(index);
        }
    }

    fn walk(
        nodes: &[WorkItemNode],
        children: &std::collections::HashMap<i64, Vec<usize>>,
        id: i64,
        dates: &mut Vec<(Date, Date)>,
    ) {
        let Some(indexes) = children.get(&id) else {
            return;
        };
        for &index in indexes {
            let child = &nodes[index];
            if let (Some(start), Some(target)) = (
                parse_iso_date(child.start_date.as_deref()),
                parse_iso_date(child.target_date.as_deref()),
            ) {
                dates.push((start, target));
            }
            walk(nodes, children, child.id, dates);
        }
    }

    walk(nodes, &children, parent_id, &mut dates);
    if dates.is_empty() {
        return None;
    }
    let start = dates.iter().map(|(s, _)| *s).min()?;
    let target = dates.iter().map(|(_, t)| *t).max()?;
    Some((start, target))
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
    if let Some((start, target)) = client_rollup(nodes, node.id) {
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
    vec![
        JsonPatchOp {
            op: PatchOp::Test,
            path: "/rev".into(),
            value: Some(json!(rev)),
        },
        JsonPatchOp {
            op: PatchOp::Add,
            path: format!("/fields/{START_DATE_FIELD}"),
            value: Some(json!(iso_date_only(start_date))),
        },
        JsonPatchOp {
            op: PatchOp::Add,
            path: format!("/fields/{TARGET_DATE_FIELD}"),
            value: Some(json!(iso_date_only(target_date))),
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

fn ymd(value: Option<&str>) -> String {
    parse_iso_date(value)
        .map(|date| date.to_string())
        .unwrap_or_default()
}

/// Tree-column copy. Iteration/rollup hints are display-only — never PATCHed.
pub fn dates_column_text(
    node: &WorkItemNode,
    nodes: &[WorkItemNode],
    iteration: Option<&IterationHint>,
) -> String {
    if !node.has_date_fields {
        return "No Start/Target on type".into();
    }
    let start = parse_iso_date(node.start_date.as_deref());
    let target = parse_iso_date(node.target_date.as_deref());
    if let (Some(start), Some(target)) = (start, target) {
        return format!("{start} → {target}");
    }
    if let Some(start) = start {
        return format!("{start} → …");
    }
    if let Some(target) = target {
        return format!("◆ {target}");
    }
    if let Some((start, target)) = client_rollup(nodes, node.id) {
        return format!("Unscheduled · rollup {start}–{target}");
    }
    if let Some(iteration) = iteration
        && iteration.start_date.is_some()
        && iteration.finish_date.is_some()
    {
        return format!(
            "Unscheduled · iteration {}–{}",
            ymd(iteration.start_date.as_deref()),
            ymd(iteration.finish_date.as_deref())
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
        assert_eq!(
            dates_column_text(&item, &[item.clone()], None),
            "2026-07-06 → …"
        );
    }

    #[test]
    fn target_only_is_a_milestone() {
        let mut item = node(1);
        item.target_date = Some("2026-07-20T00:00:00Z".into());
        let display = gantt_bar_display(&item, &[item.clone()], None);
        assert!(display.milestone);
        assert_eq!(display.start, display.end);
        assert_eq!(
            dates_column_text(&item, &[item.clone()], None),
            "◆ 2026-07-20"
        );
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
            "Unscheduled · rollup 2026-08-17–2026-09-04"
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
            "Unscheduled · iteration 2026-08-10–2026-08-21"
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
            "2026-07-06 → 2026-07-20"
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
