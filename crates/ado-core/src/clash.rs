//! Clash rule from ADR 0003: a Dependency clashes when the successor starts
//! before its predecessor's Target Date, judged on each end's own Start Date
//! and Target Date. Iteration Path and Rollup hint dates never count.

use std::collections::{HashMap, HashSet};

use crate::cascade::WorkItemDates;
use crate::types::WorkItemNode;

/// `(predecessor_id, successor_id)`.
pub type DependencyLink = (i64, i64);

/// The dates `node`'s own Gantt bar is drawn from. `None` for types without
/// date fields and Unscheduled Work Items.
pub fn clash_ends(node: &WorkItemNode) -> Option<WorkItemDates> {
    let dates = WorkItemDates::of(node);
    (node.has_date_fields && (dates.start.is_some() || dates.target.is_some())).then_some(dates)
}

/// The successor starts on its Start Date, or a Milestone on its Target Date.
/// Equal dates do not clash; a predecessor with no Target Date never does.
pub fn ends_clash(predecessor: WorkItemDates, successor: WorkItemDates) -> bool {
    match (predecessor.target, successor.start.or(successor.target)) {
        (Some(finish), Some(start)) => start < finish,
        _ => false,
    }
}

pub fn link_clashes(predecessor: Option<WorkItemDates>, successor: Option<WorkItemDates>) -> bool {
    match (predecessor, successor) {
        (Some(predecessor), Some(successor)) => ends_clash(predecessor, successor),
        _ => false,
    }
}

/// Every clashing Dependency whose ends are both in `nodes`.
pub fn clashes(nodes: &[WorkItemNode]) -> HashSet<DependencyLink> {
    let ends: HashMap<i64, Option<WorkItemDates>> = nodes
        .iter()
        .map(|node| (node.id, clash_ends(node)))
        .collect();
    let mut out = HashSet::new();
    for node in nodes {
        let Some(successor) = ends.get(&node.id).copied().flatten() else {
            continue;
        };
        for &from in &node.predecessors {
            if link_clashes(ends.get(&from).copied().flatten(), Some(successor)) {
                out.insert((from, node.id));
            }
        }
    }
    out
}

/// Dependencies into and out of `id` whose other end is in `nodes`: the links
/// a date change on `id` can make or clear a Clash on.
pub fn links_touching(nodes: &[WorkItemNode], id: i64) -> Vec<DependencyLink> {
    let ids: HashSet<i64> = nodes.iter().map(|node| node.id).collect();
    if !ids.contains(&id) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for node in nodes {
        for &from in &node.predecessors {
            let touches = (node.id == id || from == id) && from != node.id;
            if touches && ids.contains(&from) && !out.contains(&(from, node.id)) {
                out.push((from, node.id));
            }
        }
    }
    out
}

/// Re-judge only `links` against `ends` (tentative dates during a gesture or
/// a cascade preview), leaving the rest of `clashes` as loaded.
pub fn retest_clashes(
    clashes: &mut HashSet<DependencyLink>,
    links: &[DependencyLink],
    ends: impl Fn(i64) -> Option<WorkItemDates>,
) {
    for &(from, to) in links {
        if link_clashes(ends(from), ends(to)) {
            clashes.insert((from, to));
        } else {
            clashes.remove(&(from, to));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: i64, start: Option<&str>, target: Option<&str>, preds: &[i64]) -> WorkItemNode {
        WorkItemNode {
            id,
            rev: 1,
            title: format!("Item {id}"),
            work_item_type: "Task".into(),
            state: "Active".into(),
            assigned_to: None,
            parent_id: None,
            area_path: String::new(),
            iteration_path: "Shop\\Sprint 1".into(),
            start_date: start.map(|date| format!("{date}T00:00:00Z")),
            target_date: target.map(|date| format!("{date}T00:00:00Z")),
            has_date_fields: true,
            predecessors: preds.to_vec(),
            fields: serde_json::Map::new(),
        }
    }

    fn dates(start: Option<&str>, target: Option<&str>) -> WorkItemDates {
        WorkItemDates {
            start: start.map(|text| text.parse().unwrap()),
            target: target.map(|text| text.parse().unwrap()),
        }
    }

    #[test]
    fn successor_starting_before_the_predecessor_target_clashes() {
        let pred = node(12, Some("2026-07-01"), Some("2026-07-10"), &[]);
        let succ = node(34, Some("2026-07-09"), Some("2026-07-20"), &[12]);
        assert_eq!(clashes(&[pred, succ]), HashSet::from([(12, 34)]));
    }

    #[test]
    fn successor_starting_on_the_predecessor_target_does_not_clash() {
        let pred = node(12, Some("2026-07-01"), Some("2026-07-10"), &[]);
        let succ = node(34, Some("2026-07-10"), Some("2026-07-20"), &[12]);
        assert!(clashes(&[pred, succ]).is_empty());
    }

    #[test]
    fn successor_starting_after_the_predecessor_target_does_not_clash() {
        let pred = node(12, Some("2026-07-01"), Some("2026-07-10"), &[]);
        let succ = node(34, Some("2026-07-11"), Some("2026-07-20"), &[12]);
        assert!(clashes(&[pred, succ]).is_empty());
    }

    #[test]
    fn milestone_successor_starts_on_its_target() {
        let pred = node(12, Some("2026-07-01"), Some("2026-07-10"), &[]);
        let early = node(34, None, Some("2026-07-09"), &[12]);
        let on_time = node(56, None, Some("2026-07-10"), &[12]);
        assert_eq!(clashes(&[pred, early, on_time]), HashSet::from([(12, 34)]));
    }

    #[test]
    fn milestone_predecessor_finishes_on_its_target() {
        let pred = node(12, None, Some("2026-07-10"), &[]);
        let early = node(34, Some("2026-07-09"), Some("2026-07-20"), &[12]);
        let on_time = node(56, Some("2026-07-10"), Some("2026-07-20"), &[12]);
        assert_eq!(clashes(&[pred, early, on_time]), HashSet::from([(12, 34)]));
    }

    #[test]
    fn start_only_successor_uses_its_start_date() {
        let pred = node(12, Some("2026-07-01"), Some("2026-07-10"), &[]);
        let succ = node(34, Some("2026-07-02"), None, &[12]);
        assert_eq!(clashes(&[pred, succ]), HashSet::from([(12, 34)]));
    }

    #[test]
    fn predecessor_without_a_target_date_never_clashes() {
        let pred = node(12, Some("2026-07-01"), None, &[]);
        let succ = node(34, Some("2026-06-01"), Some("2026-06-02"), &[12]);
        assert!(clashes(&[pred, succ]).is_empty());
    }

    #[test]
    fn unscheduled_ends_never_clash_even_with_iteration_or_rollup_hints() {
        let parent = node(1, None, None, &[]);
        let mut child = node(2, Some("2026-07-01"), Some("2026-07-30"), &[]);
        child.parent_id = Some(1);
        let succ = node(34, Some("2026-07-02"), Some("2026-07-05"), &[1]);
        let unscheduled_succ = node(56, None, None, &[2]);
        assert!(clashes(&[parent, child, succ, unscheduled_succ]).is_empty());
    }

    #[test]
    fn types_without_date_fields_never_clash() {
        let mut pred = node(12, Some("2026-07-01"), Some("2026-07-10"), &[]);
        pred.has_date_fields = false;
        let succ = node(34, Some("2026-07-02"), Some("2026-07-05"), &[12]);
        let mut fieldless_succ = node(56, Some("2026-07-02"), Some("2026-07-05"), &[34]);
        fieldless_succ.has_date_fields = false;
        let late = node(78, Some("2026-07-01"), Some("2026-07-20"), &[]);
        let mut fieldless_after_late = node(90, Some("2026-07-02"), None, &[78]);
        fieldless_after_late.has_date_fields = false;
        assert!(clashes(&[pred, succ, fieldless_succ, late, fieldless_after_late]).is_empty());
    }

    #[test]
    fn off_forest_predecessors_never_clash() {
        let succ = node(34, Some("2026-07-02"), Some("2026-07-05"), &[999]);
        assert!(clashes(&[succ]).is_empty());
    }

    #[test]
    fn ends_clash_ignores_everything_but_start_and_target() {
        let pred = dates(None, Some("2026-07-10"));
        let succ = dates(Some("2026-07-09"), None);
        assert!(ends_clash(pred, succ));
        assert!(!ends_clash(succ, pred));
        assert!(!ends_clash(pred, dates(None, None)));
    }

    #[test]
    fn links_touching_lists_in_and_out_links_inside_the_forest() {
        let a = node(1, Some("2026-07-01"), Some("2026-07-10"), &[]);
        let b = node(2, Some("2026-07-11"), Some("2026-07-20"), &[1, 999]);
        let c = node(3, Some("2026-07-21"), Some("2026-07-30"), &[2]);
        let d = node(4, Some("2026-07-21"), Some("2026-07-30"), &[1]);
        let mut links = links_touching(&[a, b, c, d], 2);
        links.sort_unstable();
        assert_eq!(links, vec![(1, 2), (2, 3)]);
    }

    #[test]
    fn retest_clashes_only_rejudges_the_given_links() {
        let a = node(1, Some("2026-07-01"), Some("2026-07-10"), &[]);
        let b = node(2, Some("2026-07-11"), Some("2026-07-20"), &[1]);
        let c = node(3, Some("2026-07-05"), Some("2026-07-30"), &[2]);
        let x = node(8, Some("2026-07-01"), Some("2026-07-10"), &[]);
        let y = node(9, Some("2026-07-02"), Some("2026-07-20"), &[8]);
        let nodes = [a, b, c, x, y];
        let mut set = clashes(&nodes);
        assert_eq!(set, HashSet::from([(2, 3), (8, 9)]));
        let links = links_touching(&nodes, 2);
        let moved = Some(dates(Some("2026-07-08"), Some("2026-07-04")));
        retest_clashes(&mut set, &links, |id| {
            if id == 2 {
                moved
            } else {
                nodes.iter().find(|node| node.id == id).and_then(clash_ends)
            }
        });
        assert_eq!(set, HashSet::from([(1, 2), (8, 9)]));
    }
}
