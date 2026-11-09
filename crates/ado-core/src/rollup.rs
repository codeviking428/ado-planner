//! Hierarchy Rollup as a write. After the Date cascade moves Work Items, each
//! ancestor with date fields takes the earliest Start Date and latest Target
//! Date of its descendants, and an ancestor that moved Date-cascades its own
//! successors. Pure; the caller applies the returned writes.

use std::collections::{BTreeSet, HashMap};

use jiff::civil::Date;

use crate::cascade::{
    CascadeMode, CascadeTrigger, DateChange, DateWrite, WorkItemDates, plan_cascade,
};
use crate::types::WorkItemNode;

/// [`plan_cascade`], then the Rollup of every ancestor of a Work Item that
/// changed or moved, then the Date cascade from each ancestor that moved, and
/// so on until nothing new moves.
///
/// Only ancestors in `nodes` with date fields are written. Work Items in
/// `changes` are never written, so a parent the user moved keeps its dates,
/// and each Work Item is written at most once. A later pass that would move
/// a Work Item already written leaves it, so a Rollup that feeds back into
/// the same Work Item ends the walk.
pub fn plan_cascade_with_rollup(
    nodes: &[WorkItemNode],
    changes: &[DateChange],
    mode: CascadeMode,
    trigger: CascadeTrigger,
) -> Vec<DateWrite> {
    let forest = Forest::new(nodes);
    let mut dates: HashMap<i64, WorkItemDates> = changes
        .iter()
        .map(|change| (change.id, change.after))
        .collect();
    let mut fixed: BTreeSet<i64> = changes.iter().map(|change| change.id).collect();
    let mut writes = Vec::new();
    let mut moved = changes.to_vec();
    let mut trigger = trigger;
    loop {
        let mut pass = moved.clone();
        pass.extend(
            fixed
                .iter()
                .filter(|&&id| !moved.iter().any(|change| change.id == id))
                .map(|&id| {
                    let held = forest.dates(id, &dates);
                    DateChange {
                        id,
                        before: held,
                        after: held,
                    }
                }),
        );
        let mut touched: Vec<i64> = moved.iter().map(|change| change.id).collect();
        for write in plan_cascade(nodes, &pass, mode, trigger) {
            if fixed.insert(write.id) {
                dates.insert(
                    write.id,
                    WorkItemDates {
                        start: write.start,
                        target: write.target,
                    },
                );
                touched.push(write.id);
                writes.push(write);
            }
        }
        moved.clear();
        for id in forest.ancestors_deepest_first(&touched) {
            if fixed.contains(&id) || !forest.has_date_fields(id) {
                continue;
            }
            let before = forest.dates(id, &dates);
            let Some(after) = forest.rollup(id, &dates).filter(|&after| after != before) else {
                continue;
            };
            dates.insert(id, after);
            fixed.insert(id);
            writes.push(DateWrite {
                id,
                start: after.start,
                target: after.target,
            });
            moved.push(DateChange { id, before, after });
        }
        if moved.is_empty() {
            return writes;
        }
        trigger = CascadeTrigger::DateChange;
    }
}

struct Forest<'a> {
    by_id: HashMap<i64, &'a WorkItemNode>,
    children: HashMap<i64, Vec<i64>>,
}

impl<'a> Forest<'a> {
    fn new(nodes: &'a [WorkItemNode]) -> Self {
        let mut by_id = HashMap::with_capacity(nodes.len());
        for node in nodes {
            by_id.entry(node.id).or_insert(node);
        }
        let mut children: HashMap<i64, Vec<i64>> = HashMap::new();
        for node in by_id.values() {
            if let Some(parent) = node.parent_id {
                children.entry(parent).or_default().push(node.id);
            }
        }
        Self { by_id, children }
    }

    fn has_date_fields(&self, id: i64) -> bool {
        self.by_id.get(&id).is_some_and(|node| node.has_date_fields)
    }

    fn dates(&self, id: i64, held: &HashMap<i64, WorkItemDates>) -> WorkItemDates {
        held.get(&id).copied().unwrap_or_else(|| {
            self.by_id
                .get(&id)
                .map(|node| WorkItemDates::of(node))
                .unwrap_or_default()
        })
    }

    /// The in-forest parent chain of `id`, nearest first. Stops at a parent
    /// cycle.
    fn ancestors(&self, id: i64) -> Vec<i64> {
        let mut chain = Vec::new();
        let mut current = id;
        while let Some(parent) = self.by_id.get(&current).and_then(|node| node.parent_id) {
            if parent == id || chain.contains(&parent) || !self.by_id.contains_key(&parent) {
                break;
            }
            chain.push(parent);
            current = parent;
        }
        chain
    }

    /// Every ancestor of `ids`, once, deepest first, so a parent's Rollup
    /// reads its child parents' new Rollups.
    fn ancestors_deepest_first(&self, ids: &[i64]) -> Vec<i64> {
        let mut found = BTreeSet::new();
        for &id in ids {
            found.extend(self.ancestors(id));
        }
        let mut ordered: Vec<(usize, i64)> = found
            .into_iter()
            .map(|id| (self.ancestors(id).len(), id))
            .collect();
        ordered.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        ordered.into_iter().map(|(_, id)| id).collect()
    }

    /// Earliest Start Date and latest Target Date over the descendants of
    /// `id` that have date fields. A field no descendant has keeps `id`'s
    /// own value. `None` when no descendant has a date, or when the span
    /// would end before it starts.
    fn rollup(&self, id: i64, held: &HashMap<i64, WorkItemDates>) -> Option<WorkItemDates> {
        let mut start: Option<Date> = None;
        let mut target: Option<Date> = None;
        let mut seen = BTreeSet::from([id]);
        let mut stack = vec![id];
        while let Some(parent) = stack.pop() {
            for &child in self.children.get(&parent).into_iter().flatten() {
                if !seen.insert(child) {
                    continue;
                }
                stack.push(child);
                if !self.has_date_fields(child) {
                    continue;
                }
                let dates = self.dates(child, held);
                start = match (start, dates.start) {
                    (Some(earliest), Some(child)) => Some(earliest.min(child)),
                    (earliest, child) => earliest.or(child),
                };
                target = target.max(dates.target);
            }
        }
        if start.is_none() && target.is_none() {
            return None;
        }
        let own = self.dates(id, held);
        let span = WorkItemDates {
            start: start.or(own.start),
            target: target.or(own.target),
        };
        match (span.start, span.target) {
            (Some(start), Some(target)) if start > target => None,
            _ => Some(span),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CascadeMode::{Flexible, NoAction, Strict};
    use super::*;
    use crate::dates::iso_date_only;

    fn day(text: &str) -> Option<Date> {
        (!text.is_empty()).then(|| format!("2026-{text}").parse().unwrap())
    }

    fn span(start: &str, target: &str) -> WorkItemDates {
        WorkItemDates {
            start: day(start),
            target: day(target),
        }
    }

    fn item(
        id: i64,
        parent: Option<i64>,
        start: &str,
        target: &str,
        predecessors: &[i64],
    ) -> WorkItemNode {
        WorkItemNode {
            id,
            rev: 1,
            title: format!("WI {id}"),
            work_item_type: "Feature".into(),
            state: "New".into(),
            assigned_to: None,
            parent_id: parent,
            area_path: "Shop".into(),
            iteration_path: "Shop".into(),
            start_date: day(start).map(iso_date_only),
            target_date: day(target).map(iso_date_only),
            has_date_fields: true,
            predecessors: predecessors.to_vec(),
            fields: serde_json::Map::new(),
        }
    }

    fn change(id: i64, before: (&str, &str), after: (&str, &str)) -> DateChange {
        DateChange {
            id,
            before: span(before.0, before.1),
            after: span(after.0, after.1),
        }
    }

    fn write(id: i64, start: &str, target: &str) -> DateWrite {
        DateWrite {
            id,
            start: day(start),
            target: day(target),
        }
    }

    fn plan(nodes: &[WorkItemNode], changes: &[DateChange], mode: CascadeMode) -> Vec<DateWrite> {
        plan_cascade_with_rollup(nodes, changes, mode, CascadeTrigger::DateChange)
    }

    /// Epic 1 › Feature 2 › Story 3 and Story 4. Feature 2 precedes Feature 5.
    fn tree() -> Vec<WorkItemNode> {
        vec![
            item(1, None, "07-01", "09-30", &[]),
            item(2, Some(1), "07-06", "07-31", &[]),
            item(3, Some(2), "07-06", "07-20", &[]),
            item(4, Some(2), "07-10", "07-31", &[]),
            item(5, None, "08-01", "08-14", &[2]),
        ]
    }

    #[test]
    fn a_child_move_writes_each_ancestor_and_cascades_the_parent() {
        let changes = [change(3, ("07-06", "07-20"), ("07-11", "08-05"))];
        assert_eq!(
            plan(&tree(), &changes, Flexible),
            vec![
                write(2, "07-10", "08-05"),
                write(1, "07-10", "08-05"),
                write(5, "08-05", "08-18"),
            ],
        );
        assert_eq!(
            plan(&tree(), &changes, Strict),
            vec![
                write(2, "07-10", "08-05"),
                write(1, "07-10", "08-05"),
                write(5, "08-06", "08-19"),
            ],
        );
    }

    #[test]
    fn no_action_still_writes_rollups_but_moves_no_successor() {
        let changes = [change(3, ("07-06", "07-20"), ("07-11", "08-05"))];
        assert_eq!(
            plan(&tree(), &changes, NoAction),
            vec![write(2, "07-10", "08-05"), write(1, "07-10", "08-05")],
        );
    }

    #[test]
    fn moving_the_parent_itself_writes_no_rollup_for_it() {
        let changes = [change(2, ("07-06", "07-31"), ("07-08", "08-02"))];
        assert_eq!(
            plan(&tree(), &changes, Flexible),
            vec![write(5, "08-02", "08-15"), write(1, "07-06", "08-02")],
            "the Epic rolls up over its Feature and Stories as they stand",
        );
    }

    #[test]
    fn an_unchanged_rollup_writes_nothing() {
        let changes = [change(4, ("07-10", "07-31"), ("07-12", "07-31"))];
        let mut nodes = tree();
        nodes[0] = item(1, None, "07-06", "07-31", &[]);
        assert_eq!(plan(&nodes, &changes, Flexible), Vec::new());
    }

    #[test]
    fn rollup_skips_types_without_date_fields_and_reads_through_them() {
        let mut nodes = vec![
            item(1, None, "07-01", "07-31", &[]),
            item(2, Some(1), "", "", &[]),
            item(3, Some(2), "07-06", "07-20", &[]),
        ];
        nodes[1].has_date_fields = false;
        let changes = [change(3, ("07-06", "07-20"), ("07-08", "07-22"))];
        assert_eq!(
            plan(&nodes, &changes, Flexible),
            vec![write(1, "07-08", "07-22")],
        );
    }

    #[test]
    fn an_unscheduled_parent_gets_its_childrens_span() {
        let nodes = vec![
            item(1, None, "", "", &[]),
            item(2, Some(1), "07-06", "07-20", &[]),
        ];
        let changes = [change(2, ("07-06", "07-20"), ("07-07", "07-21"))];
        assert_eq!(
            plan(&nodes, &changes, Flexible),
            vec![write(1, "07-07", "07-21")]
        );
    }

    #[test]
    fn a_milestone_child_counts_its_target_date_only() {
        let nodes = vec![
            item(1, None, "07-01", "07-31", &[]),
            item(2, Some(1), "07-06", "07-20", &[]),
            item(3, Some(1), "", "08-10", &[]),
        ];
        let changes = [change(2, ("07-06", "07-20"), ("07-08", "07-22"))];
        assert_eq!(
            plan(&nodes, &changes, Flexible),
            vec![write(1, "07-08", "08-10")]
        );
    }

    #[test]
    fn a_cascaded_successor_rolls_up_its_own_parent() {
        let nodes = vec![
            item(1, None, "07-01", "07-20", &[]),
            item(2, None, "07-21", "07-31", &[1]),
            item(3, Some(2), "07-21", "07-31", &[]),
            item(4, Some(3), "07-21", "07-31", &[1]),
        ];
        let changes = [change(1, ("07-01", "07-20"), ("07-01", "07-25"))];
        assert_eq!(
            plan(&nodes, &changes, Flexible),
            vec![
                write(2, "07-25", "08-04"),
                write(4, "07-25", "08-04"),
                write(3, "07-25", "08-04"),
            ],
            "#3 rolls up over #4; #2 already moved with the cascade, so it is not written twice",
        );
    }

    #[test]
    fn a_rollup_feeding_back_into_the_same_work_item_ends_the_walk() {
        // Feature 1 precedes its own child Story 3, so its Rollup moves Story
        // 3, which would move Feature 1 again.
        let nodes = vec![
            item(1, None, "07-01", "07-20", &[]),
            item(2, Some(1), "07-01", "07-20", &[]),
            item(3, Some(1), "07-21", "07-25", &[1]),
        ];
        let changes = [change(2, ("07-01", "07-20"), ("07-01", "07-28"))];
        let writes = plan(&nodes, &changes, Flexible);
        assert_eq!(
            writes,
            vec![write(1, "07-01", "07-28"), write(3, "07-28", "08-01")]
        );
        assert_eq!(writes.iter().filter(|write| write.id == 1).count(), 1);
    }

    #[test]
    fn a_parent_cycle_does_not_hang() {
        let nodes = vec![
            item(1, Some(2), "07-01", "07-20", &[]),
            item(2, Some(1), "07-01", "07-20", &[]),
        ];
        let changes = [change(1, ("07-01", "07-20"), ("07-02", "07-21"))];
        assert_eq!(
            plan(&nodes, &changes, Flexible),
            vec![write(2, "07-02", "07-21")]
        );
    }

    #[test]
    fn a_new_dependency_rolls_up_the_successors_parent() {
        let nodes = vec![
            item(1, None, "07-01", "07-20", &[]),
            item(2, None, "07-06", "07-15", &[]),
            item(3, Some(2), "07-06", "07-15", &[]),
        ];
        let writes = plan_cascade_with_rollup(
            &nodes,
            &[],
            Strict,
            CascadeTrigger::NewDependency {
                predecessor: 1,
                successor: 3,
            },
        );
        assert_eq!(
            writes,
            vec![write(3, "07-20", "07-29"), write(2, "07-20", "07-29")]
        );
    }

    #[test]
    fn off_forest_parents_are_not_written() {
        let nodes = vec![item(2, Some(99), "07-06", "07-20", &[])];
        let changes = [change(2, ("07-06", "07-20"), ("07-08", "07-22"))];
        assert_eq!(plan(&nodes, &changes, Flexible), Vec::new());
    }
}
