//! Date cascade planning: which successors move, and to which dates, after a
//! Work Item's dates change or a Dependency is created. Pure; the caller
//! applies the returned writes.

use std::collections::{BTreeSet, HashMap, VecDeque};

use jiff::ToSpan;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::dates::parse_iso_date;
use crate::types::WorkItemNode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CascadeMode {
    /// Push successors later until they start on or after every predecessor's
    /// Target Date, keeping duration. Never pull earlier.
    #[default]
    Flexible,
    /// Shift successors by the predecessor's Target Date change, either way.
    Strict,
    NoAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkItemDates {
    pub start: Option<Date>,
    pub target: Option<Date>,
}

impl WorkItemDates {
    pub fn of(node: &WorkItemNode) -> Self {
        Self {
            start: parse_iso_date(node.start_date.as_deref()),
            target: parse_iso_date(node.target_date.as_deref()),
        }
    }
}

/// A Work Item's dates before and after the user's gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateChange {
    pub id: i64,
    pub before: WorkItemDates,
    pub after: WorkItemDates,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CascadeTrigger {
    /// The Work Items in `changes` got new dates.
    DateChange,
    /// `successor` now waits on `predecessor`. The link need not be in `nodes`
    /// yet.
    NewDependency { predecessor: i64, successor: i64 },
}

/// New Start Date and Target Date for one Work Item. `None` leaves the field
/// empty, as on a Milestone's Start Date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateWrite {
    pub id: i64,
    pub start: Option<Date>,
    pub target: Option<Date>,
}

/// Plan the Date cascade over the forest in `nodes`.
///
/// `nodes` holds dates as loaded; `changes` overrides them with the user's
/// `after` dates. Work Items in `changes`, and the predecessor of a new
/// Dependency, are never written. Each Work Item
/// is written at most once, and writes come in walk order: a successor after
/// all of its predecessors on the walk.
///
/// A new Dependency pushes its successor as Flexible does, in Flexible and
/// Strict alike, then walks on in `mode`. Under Strict, a successor reached
/// from predecessors that moved by different amounts takes the largest move,
/// later winning a tie. A Work Item on a Dependency loop is never reached, so
/// the walk ends there.
pub fn plan_cascade(
    nodes: &[WorkItemNode],
    changes: &[DateChange],
    mode: CascadeMode,
    trigger: CascadeTrigger,
) -> Vec<DateWrite> {
    if mode == CascadeMode::NoAction {
        return Vec::new();
    }
    let mut planner = Planner::new(nodes, changes);
    let roots: BTreeSet<i64> = match trigger {
        CascadeTrigger::DateChange => {
            let mut roots = BTreeSet::new();
            for change in changes {
                if planner.has_date_fields(change.id) {
                    planner
                        .moves
                        .insert(change.id, (change.before.target, change.after.target));
                    roots.insert(change.id);
                }
            }
            roots
        }
        CascadeTrigger::NewDependency {
            predecessor,
            successor,
        } => {
            if predecessor == successor
                || !planner.has_date_fields(predecessor)
                || planner.dates(predecessor).target.is_none()
            {
                return Vec::new();
            }
            planner.link(predecessor, successor);
            if planner.place(successor, CascadeMode::Flexible, &[predecessor]) {
                BTreeSet::from([predecessor, successor])
            } else {
                BTreeSet::new()
            }
        }
    };
    planner.walk(&roots, mode);
    planner.writes
}

struct Planner<'a> {
    by_id: HashMap<i64, &'a WorkItemNode>,
    predecessors: HashMap<i64, BTreeSet<i64>>,
    successors: HashMap<i64, BTreeSet<i64>>,
    dates: HashMap<i64, WorkItemDates>,
    /// Target Date before and after, for each Work Item that moved.
    moves: HashMap<i64, (Option<Date>, Option<Date>)>,
    writes: Vec<DateWrite>,
}

impl<'a> Planner<'a> {
    fn new(nodes: &'a [WorkItemNode], changes: &[DateChange]) -> Self {
        let mut by_id = HashMap::with_capacity(nodes.len());
        for node in nodes {
            by_id.entry(node.id).or_insert(node);
        }
        let mut planner = Self {
            by_id,
            predecessors: HashMap::new(),
            successors: HashMap::new(),
            dates: HashMap::new(),
            moves: HashMap::new(),
            writes: Vec::new(),
        };
        let links: Vec<(i64, i64)> = planner
            .by_id
            .values()
            .flat_map(|node| node.predecessors.iter().map(|&pred| (pred, node.id)))
            .collect();
        for (predecessor, successor) in links {
            planner.link(predecessor, successor);
        }
        for change in changes {
            if planner.by_id.contains_key(&change.id) {
                planner.dates.insert(change.id, change.after);
            }
        }
        planner
    }

    /// Links with an off-forest end are dropped: their dates are unknown.
    fn link(&mut self, predecessor: i64, successor: i64) {
        if !self.by_id.contains_key(&predecessor) || !self.by_id.contains_key(&successor) {
            return;
        }
        self.predecessors
            .entry(successor)
            .or_default()
            .insert(predecessor);
        self.successors
            .entry(predecessor)
            .or_default()
            .insert(successor);
    }

    fn has_date_fields(&self, id: i64) -> bool {
        self.by_id.get(&id).is_some_and(|node| node.has_date_fields)
    }

    fn dates(&self, id: i64) -> WorkItemDates {
        self.dates.get(&id).copied().unwrap_or_else(|| {
            self.by_id
                .get(&id)
                .map(|node| WorkItemDates::of(node))
                .unwrap_or_default()
        })
    }

    fn successors_of(&self, id: i64) -> impl Iterator<Item = i64> + '_ {
        self.successors.get(&id).into_iter().flatten().copied()
    }

    fn predecessors_of(&self, id: i64) -> impl Iterator<Item = i64> + '_ {
        self.predecessors.get(&id).into_iter().flatten().copied()
    }

    fn latest_target(&self, ids: impl Iterator<Item = i64>) -> Option<Date> {
        ids.filter_map(|id| self.dates(id).target).max()
    }

    /// Visit every Work Item downstream of `roots` once, after all of its
    /// predecessors on the walk. Roots are never revisited, and Work Items on
    /// a loop never become ready.
    fn walk(&mut self, roots: &BTreeSet<i64>, mode: CascadeMode) {
        let mut reached = BTreeSet::new();
        let mut stack: Vec<i64> = roots.iter().copied().collect();
        while let Some(id) = stack.pop() {
            for successor in self.successors_of(id).collect::<Vec<_>>() {
                if !roots.contains(&successor) && reached.insert(successor) {
                    stack.push(successor);
                }
            }
        }
        let mut waiting: HashMap<i64, usize> = reached
            .iter()
            .map(|&id| {
                let count = self
                    .predecessors_of(id)
                    .filter(|pred| reached.contains(pred) || roots.contains(pred))
                    .count();
                (id, count)
            })
            .collect();
        let mut ready = VecDeque::new();
        let mut release = |id: i64, planner: &Self, ready: &mut VecDeque<i64>| {
            for successor in planner.successors_of(id) {
                if let Some(count) = waiting.get_mut(&successor) {
                    *count -= 1;
                    if *count == 0 {
                        ready.push_back(successor);
                    }
                }
            }
        };
        for &root in roots {
            release(root, self, &mut ready);
        }
        while let Some(id) = ready.pop_front() {
            let moved: Vec<i64> = self
                .predecessors_of(id)
                .filter(|pred| self.moves.contains_key(pred))
                .collect();
            if !moved.is_empty() {
                self.place(id, mode, &moved);
            }
            release(id, self, &mut ready);
        }
    }

    /// Move `id` for the predecessors in `moved`. Returns whether it moved.
    fn place(&mut self, id: i64, mode: CascadeMode, moved: &[i64]) -> bool {
        if !self.has_date_fields(id) {
            return false;
        }
        let current = self.dates(id);
        let Some(latest) = self.latest_target(self.predecessors_of(id)) else {
            return false;
        };
        let next = match mode {
            CascadeMode::Flexible => {
                let needed =
                    self.latest_target(moved.iter().copied().filter(|&pred| self.pushes(pred)));
                needed.and_then(|needed| flexible_push(current, needed, latest))
            }
            CascadeMode::Strict => moved
                .iter()
                .filter_map(|&pred| self.strict_shift(pred))
                .max_by_key(|&days| (days.abs(), days))
                .map(|days| strict_shift(current, days, latest)),
            CascadeMode::NoAction => None,
        };
        let Some(next) = next.filter(|&next| next != current) else {
            return false;
        };
        self.dates.insert(id, next);
        self.moves.insert(id, (current.target, next.target));
        self.writes.push(DateWrite {
            id,
            start: next.start,
            target: next.target,
        });
        true
    }

    /// A Flexible push carries on from a Target Date that moved later or
    /// was set. The trigger of a new Dependency always pushes.
    fn pushes(&self, id: i64) -> bool {
        match self.moves.get(&id) {
            Some(&(before, Some(after))) => before < Some(after),
            Some((_, None)) => false,
            None => self.dates(id).target.is_some(),
        }
    }

    fn strict_shift(&self, id: i64) -> Option<i32> {
        match self.moves.get(&id) {
            Some(&(Some(before), Some(after))) => {
                Some(days_between(before, after)).filter(|&days| days != 0)
            }
            _ => None,
        }
    }
}

/// `needed` is the latest Target Date among the predecessors that pushed, and
/// decides whether to move at all; `latest` over every predecessor decides
/// where to. A clash with a predecessor that did not move is left alone.
fn flexible_push(current: WorkItemDates, needed: Date, latest: Date) -> Option<WorkItemDates> {
    match (current.start, current.target) {
        (Some(start), target) if start < needed => {
            let days = days_between(start, latest);
            Some(WorkItemDates {
                start: Some(latest),
                target: target.map(|target| target.saturating_add(days.days())),
            })
        }
        (None, Some(target)) if target < needed => Some(WorkItemDates {
            start: None,
            target: Some(latest),
        }),
        (None, None) => Some(WorkItemDates {
            start: Some(latest),
            target: Some(latest),
        }),
        _ => None,
    }
}

fn strict_shift(current: WorkItemDates, days: i32, latest: Date) -> WorkItemDates {
    if current.start.is_none() && current.target.is_none() {
        return WorkItemDates {
            start: Some(latest),
            target: Some(latest),
        };
    }
    WorkItemDates {
        start: current.start.map(|start| start.saturating_add(days.days())),
        target: current
            .target
            .map(|target| target.saturating_add(days.days())),
    }
}

fn days_between(from: Date, to: Date) -> i32 {
    (to - from).get_days()
}

#[cfg(test)]
mod tests {
    use super::CascadeMode::{Flexible, NoAction, Strict};
    use super::CascadeTrigger::{DateChange as Dates, NewDependency};
    use super::*;
    use crate::dates::iso_date_only;

    /// `MM-DD` in 2026, or empty for no date.
    fn day(text: &str) -> Option<Date> {
        (!text.is_empty()).then(|| format!("2026-{text}").parse().unwrap())
    }

    fn node(id: i64, dates: WorkItemDates, predecessors: &[i64]) -> WorkItemNode {
        WorkItemNode {
            id,
            rev: 1,
            title: format!("WI {id}"),
            work_item_type: "Feature".into(),
            state: "New".into(),
            assigned_to: None,
            parent_id: None,
            area_path: "Shop".into(),
            iteration_path: "Shop".into(),
            start_date: dates.start.map(iso_date_only),
            target_date: dates.target.map(iso_date_only),
            has_date_fields: true,
            predecessors: predecessors.to_vec(),
            fields: serde_json::Map::new(),
        }
    }

    fn item(id: i64, start: &str, target: &str, predecessors: &[i64]) -> WorkItemNode {
        node(id, span(start, target), predecessors)
    }

    fn no_fields(id: i64, predecessors: &[i64]) -> WorkItemNode {
        WorkItemNode {
            has_date_fields: false,
            ..item(id, "", "", predecessors)
        }
    }

    fn span(start: &str, target: &str) -> WorkItemDates {
        WorkItemDates {
            start: day(start),
            target: day(target),
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

    fn link(predecessor: i64, successor: i64) -> CascadeTrigger {
        NewDependency {
            predecessor,
            successor,
        }
    }

    struct Case {
        name: &'static str,
        nodes: Vec<WorkItemNode>,
        changes: Vec<DateChange>,
        modes: &'static [CascadeMode],
        trigger: CascadeTrigger,
        expected: Vec<DateWrite>,
    }

    fn run(cases: Vec<Case>) {
        for case in cases {
            for &mode in case.modes {
                assert_eq!(
                    plan_cascade(&case.nodes, &case.changes, mode, case.trigger),
                    case.expected,
                    "{} ({mode:?})",
                    case.name
                );
            }
        }
    }

    /// Work Item 1 runs Jul 1 → Jul 10; its Target Date moves to `target`.
    fn a_moves_to(target: &str) -> Vec<DateChange> {
        vec![change(1, ("07-01", "07-10"), ("07-01", target))]
    }

    fn a() -> WorkItemNode {
        item(1, "07-01", "07-10", &[])
    }

    #[test]
    fn cascade_mode_serializes_snake_case_and_defaults_to_flexible() {
        assert_eq!(CascadeMode::default(), Flexible);
        for (mode, text) in [
            (Flexible, "\"flexible\""),
            (Strict, "\"strict\""),
            (NoAction, "\"no_action\""),
        ] {
            assert_eq!(serde_json::to_string(&mode).unwrap(), text);
            assert_eq!(serde_json::from_str::<CascadeMode>(text).unwrap(), mode);
        }
    }

    #[test]
    fn flexible_pushes_later_only() {
        run(vec![
            Case {
                name: "pushes a clashing successor later and keeps its duration",
                nodes: vec![a(), item(2, "07-11", "07-15", &[1])],
                changes: a_moves_to("07-14"),
                modes: &[Flexible],
                trigger: Dates,
                expected: vec![write(2, "07-14", "07-18")],
            },
            Case {
                name: "a successor may start on the predecessor's Target Date",
                nodes: vec![a(), item(2, "07-11", "07-15", &[1])],
                changes: a_moves_to("07-11"),
                modes: &[Flexible],
                trigger: Dates,
                expected: vec![],
            },
            Case {
                name: "never pulls earlier",
                nodes: vec![a(), item(2, "07-11", "07-15", &[1])],
                changes: a_moves_to("07-05"),
                modes: &[Flexible],
                trigger: Dates,
                expected: vec![],
            },
            Case {
                name: "the latest predecessor Target Date wins",
                nodes: vec![
                    a(),
                    item(3, "07-01", "07-16", &[]),
                    item(2, "07-12", "07-13", &[1, 3]),
                ],
                changes: a_moves_to("07-14"),
                modes: &[Flexible],
                trigger: Dates,
                expected: vec![write(2, "07-16", "07-17")],
            },
            Case {
                name: "a loaded clash with a predecessor that did not move stays",
                nodes: vec![
                    a(),
                    item(3, "07-01", "07-16", &[]),
                    item(2, "07-12", "07-13", &[1, 3]),
                ],
                changes: a_moves_to("07-11"),
                modes: &[Flexible],
                trigger: Dates,
                expected: vec![],
            },
            Case {
                name: "walks downstream until a successor is clear",
                nodes: vec![
                    a(),
                    item(2, "07-11", "07-15", &[1]),
                    item(3, "07-16", "07-20", &[2]),
                    item(4, "07-25", "07-26", &[3]),
                ],
                changes: a_moves_to("07-14"),
                modes: &[Flexible],
                trigger: Dates,
                expected: vec![write(2, "07-14", "07-18"), write(3, "07-18", "07-22")],
            },
            Case {
                name: "a predecessor that gains a Target Date pushes",
                nodes: vec![a(), item(2, "07-11", "07-15", &[1])],
                changes: vec![change(1, ("07-01", ""), ("07-01", "07-14"))],
                modes: &[Flexible],
                trigger: Dates,
                expected: vec![write(2, "07-14", "07-18")],
            },
            Case {
                name: "Work Items in changes are not rewritten",
                nodes: vec![
                    a(),
                    item(2, "07-11", "07-15", &[1]),
                    item(3, "07-15", "07-20", &[2]),
                ],
                changes: vec![
                    change(1, ("07-01", "07-10"), ("07-01", "07-14")),
                    change(2, ("07-11", "07-15"), ("07-12", "07-16")),
                ],
                modes: &[Flexible],
                trigger: Dates,
                expected: vec![write(3, "07-16", "07-21")],
            },
        ]);
    }

    #[test]
    fn strict_shifts_by_the_target_date_change() {
        run(vec![
            Case {
                name: "shifts later even without a clash",
                nodes: vec![a(), item(2, "07-20", "07-22", &[1])],
                changes: a_moves_to("07-14"),
                modes: &[Strict],
                trigger: Dates,
                expected: vec![write(2, "07-24", "07-26")],
            },
            Case {
                name: "shifts earlier down the chain",
                nodes: vec![
                    a(),
                    item(2, "07-11", "07-15", &[1]),
                    item(3, "07-20", "07-22", &[2]),
                ],
                changes: a_moves_to("07-07"),
                modes: &[Strict],
                trigger: Dates,
                expected: vec![write(2, "07-08", "07-12"), write(3, "07-17", "07-19")],
            },
            Case {
                name: "a Start Date change alone carries nothing",
                nodes: vec![a(), item(2, "07-11", "07-15", &[1])],
                changes: vec![change(1, ("07-01", "07-10"), ("07-03", "07-10"))],
                modes: &[Strict, Flexible],
                trigger: Dates,
                expected: vec![],
            },
            Case {
                name: "a predecessor that gains a Target Date has no shift to carry",
                nodes: vec![a(), item(2, "07-11", "07-15", &[1])],
                changes: vec![change(1, ("07-01", ""), ("07-01", "07-14"))],
                modes: &[Strict],
                trigger: Dates,
                expected: vec![],
            },
            Case {
                name: "two moved predecessors: the larger move wins, once",
                nodes: vec![
                    a(),
                    item(3, "07-01", "07-10", &[]),
                    item(2, "07-20", "07-22", &[1, 3]),
                ],
                changes: vec![
                    change(1, ("07-01", "07-10"), ("07-01", "07-12")),
                    change(3, ("07-01", "07-10"), ("07-01", "07-05")),
                ],
                modes: &[Strict],
                trigger: Dates,
                expected: vec![write(2, "07-15", "07-17")],
            },
        ]);
    }

    #[test]
    fn unscheduled_successor_gets_the_predecessor_target_date() {
        run(vec![
            Case {
                name: "flexible walks on from it",
                nodes: vec![a(), item(2, "", "", &[1]), item(3, "07-12", "07-13", &[2])],
                changes: a_moves_to("07-14"),
                modes: &[Flexible],
                trigger: Dates,
                expected: vec![write(2, "07-14", "07-14"), write(3, "07-14", "07-15")],
            },
            Case {
                name: "strict stops there",
                nodes: vec![a(), item(2, "", "", &[1]), item(3, "07-20", "07-22", &[2])],
                changes: a_moves_to("07-14"),
                modes: &[Strict],
                trigger: Dates,
                expected: vec![write(2, "07-14", "07-14")],
            },
        ]);
    }

    #[test]
    fn milestone_successor_moves_target_date_only() {
        run(vec![
            Case {
                name: "flexible",
                nodes: vec![
                    a(),
                    item(2, "", "07-12", &[1]),
                    item(3, "07-13", "07-15", &[2]),
                ],
                changes: a_moves_to("07-14"),
                modes: &[Flexible],
                trigger: Dates,
                expected: vec![write(2, "", "07-14"), write(3, "07-14", "07-16")],
            },
            Case {
                name: "strict",
                nodes: vec![
                    a(),
                    item(2, "", "07-12", &[1]),
                    item(3, "07-20", "07-21", &[2]),
                ],
                changes: a_moves_to("07-14"),
                modes: &[Strict],
                trigger: Dates,
                expected: vec![write(2, "", "07-16"), write(3, "07-24", "07-25")],
            },
        ]);
    }

    #[test]
    fn the_walk_stops_where_dates_are_missing_or_unknown() {
        let both: &[CascadeMode] = &[Flexible, Strict];
        run(vec![
            Case {
                name: "predecessor loses its Target Date",
                nodes: vec![a(), item(2, "07-05", "07-06", &[1])],
                changes: vec![change(1, ("07-01", "07-10"), ("07-01", ""))],
                modes: both,
                trigger: Dates,
                expected: vec![],
            },
            Case {
                name: "start-only successor moves its Start Date, then the walk stops",
                nodes: vec![a(), item(2, "07-11", "", &[1]), item(3, "", "", &[2])],
                changes: a_moves_to("07-14"),
                modes: &[Flexible],
                trigger: Dates,
                expected: vec![write(2, "07-14", "")],
            },
            Case {
                name: "start-only successor shifts, then the walk stops",
                nodes: vec![
                    a(),
                    item(2, "07-11", "", &[1]),
                    item(3, "07-20", "07-21", &[2]),
                ],
                changes: a_moves_to("07-14"),
                modes: &[Strict],
                trigger: Dates,
                expected: vec![write(2, "07-15", "")],
            },
            Case {
                name: "type without date fields",
                nodes: vec![a(), no_fields(2, &[1]), item(3, "07-12", "07-13", &[2])],
                changes: a_moves_to("07-14"),
                modes: both,
                trigger: Dates,
                expected: vec![],
            },
            Case {
                name: "changed Work Item off the forest",
                nodes: vec![item(2, "07-11", "07-15", &[99])],
                changes: vec![change(99, ("07-01", "07-10"), ("07-01", "07-14"))],
                modes: both,
                trigger: Dates,
                expected: vec![],
            },
            Case {
                name: "off-forest Work Item between two in the forest",
                nodes: vec![a(), item(3, "07-11", "07-15", &[99])],
                changes: a_moves_to("07-14"),
                modes: both,
                trigger: Dates,
                expected: vec![],
            },
        ]);
    }

    #[test]
    fn loops_end_the_walk() {
        let both: &[CascadeMode] = &[Flexible, Strict];
        run(vec![
            Case {
                name: "loop downstream of the change",
                nodes: vec![
                    a(),
                    item(2, "07-11", "07-15", &[1, 3]),
                    item(3, "07-16", "07-20", &[2]),
                ],
                changes: a_moves_to("07-14"),
                modes: both,
                trigger: Dates,
                expected: vec![],
            },
            Case {
                name: "loop back to the changed Work Item",
                nodes: vec![
                    item(1, "07-01", "07-10", &[2]),
                    item(2, "07-11", "07-15", &[1]),
                ],
                changes: a_moves_to("07-14"),
                modes: &[Flexible],
                trigger: Dates,
                expected: vec![write(2, "07-14", "07-18")],
            },
            Case {
                name: "self loop",
                nodes: vec![item(1, "07-01", "07-10", &[1])],
                changes: a_moves_to("07-14"),
                modes: both,
                trigger: Dates,
                expected: vec![],
            },
            Case {
                name: "new Dependency that closes a loop",
                nodes: vec![
                    item(1, "07-01", "07-14", &[2]),
                    item(2, "07-10", "07-12", &[]),
                ],
                changes: vec![],
                modes: both,
                trigger: link(1, 2),
                expected: vec![write(2, "07-14", "07-16")],
            },
        ]);
    }

    #[test]
    fn new_dependency_uses_flexible_push() {
        let predecessor = || item(1, "07-01", "07-14", &[]);
        run(vec![
            Case {
                name: "flexible pushes the successor and walks on",
                nodes: vec![
                    predecessor(),
                    item(2, "07-10", "07-12", &[]),
                    item(3, "07-15", "07-16", &[2]),
                ],
                changes: vec![],
                modes: &[Flexible],
                trigger: link(1, 2),
                expected: vec![write(2, "07-14", "07-16"), write(3, "07-16", "07-17")],
            },
            Case {
                name: "strict pushes the successor, then shifts downstream",
                nodes: vec![
                    predecessor(),
                    item(2, "07-10", "07-12", &[]),
                    item(3, "07-20", "07-21", &[2]),
                ],
                changes: vec![],
                modes: &[Strict],
                trigger: link(1, 2),
                expected: vec![write(2, "07-14", "07-16"), write(3, "07-24", "07-25")],
            },
            Case {
                name: "link already in nodes",
                nodes: vec![predecessor(), item(2, "07-10", "07-12", &[1])],
                changes: vec![],
                modes: &[Flexible, Strict],
                trigger: link(1, 2),
                expected: vec![write(2, "07-14", "07-16")],
            },
            Case {
                name: "successor already clear",
                nodes: vec![predecessor(), item(2, "07-14", "07-16", &[])],
                changes: vec![],
                modes: &[Flexible, Strict],
                trigger: link(1, 2),
                expected: vec![],
            },
            Case {
                name: "unscheduled successor: strict stops there",
                nodes: vec![
                    predecessor(),
                    item(2, "", "", &[]),
                    item(3, "07-20", "07-21", &[2]),
                ],
                changes: vec![],
                modes: &[Strict],
                trigger: link(1, 2),
                expected: vec![write(2, "07-14", "07-14")],
            },
            Case {
                name: "milestone successor",
                nodes: vec![predecessor(), item(2, "", "07-12", &[])],
                changes: vec![],
                modes: &[Flexible, Strict],
                trigger: link(1, 2),
                expected: vec![write(2, "", "07-14")],
            },
            Case {
                name: "predecessor with no Target Date",
                nodes: vec![item(1, "07-01", "", &[]), item(2, "", "", &[])],
                changes: vec![],
                modes: &[Flexible, Strict],
                trigger: link(1, 2),
                expected: vec![],
            },
            Case {
                name: "successor without date fields",
                nodes: vec![predecessor(), no_fields(2, &[])],
                changes: vec![],
                modes: &[Flexible, Strict],
                trigger: link(1, 2),
                expected: vec![],
            },
            Case {
                name: "off-forest predecessor",
                nodes: vec![item(2, "07-10", "07-12", &[])],
                changes: vec![],
                modes: &[Flexible, Strict],
                trigger: link(99, 2),
                expected: vec![],
            },
            Case {
                name: "off-forest successor",
                nodes: vec![predecessor()],
                changes: vec![],
                modes: &[Flexible, Strict],
                trigger: link(1, 99),
                expected: vec![],
            },
        ]);
    }

    #[test]
    fn no_action_writes_nothing() {
        run(vec![
            Case {
                name: "date change",
                nodes: vec![a(), item(2, "07-11", "07-15", &[1])],
                changes: a_moves_to("07-14"),
                modes: &[NoAction],
                trigger: Dates,
                expected: vec![],
            },
            Case {
                name: "new Dependency",
                nodes: vec![
                    item(1, "07-01", "07-14", &[]),
                    item(2, "07-10", "07-12", &[]),
                ],
                changes: vec![],
                modes: &[NoAction],
                trigger: link(1, 2),
                expected: vec![],
            },
        ]);
    }

    #[test]
    fn long_chain_moves_every_link_once_in_order() {
        let first = Date::new(2026, 6, 1).unwrap();
        let at = |offset: i32| first.saturating_add(offset.days());
        let length = 60;
        let nodes: Vec<WorkItemNode> = (0..length)
            .map(|index| {
                let dates = WorkItemDates {
                    start: Some(at(2 * index)),
                    target: Some(at(2 * index + 2)),
                };
                let predecessors: Vec<i64> =
                    (index > 0).then(|| i64::from(index)).into_iter().collect();
                node(i64::from(index) + 1, dates, &predecessors)
            })
            .collect();
        let changes = [DateChange {
            id: 1,
            before: WorkItemDates::of(&nodes[0]),
            after: WorkItemDates {
                start: Some(at(0)),
                target: Some(at(7)),
            },
        }];
        let expected: Vec<DateWrite> = (1..length)
            .map(|index| DateWrite {
                id: i64::from(index) + 1,
                start: Some(at(2 * index + 5)),
                target: Some(at(2 * index + 7)),
            })
            .collect();
        for mode in [Flexible, Strict] {
            assert_eq!(
                plan_cascade(&nodes, &changes, mode, Dates),
                expected,
                "{mode:?}"
            );
        }
    }

    #[test]
    fn diamond_moves_the_join_once() {
        let nodes = vec![
            a(),
            item(2, "07-10", "07-12", &[1]),
            item(3, "07-10", "07-15", &[1]),
            item(4, "07-20", "07-21", &[2, 3]),
            item(5, "07-16", "07-17", &[2, 3]),
        ];
        run(vec![
            Case {
                name: "flexible pushes the join past its latest predecessor",
                nodes: nodes.clone(),
                changes: a_moves_to("07-13"),
                modes: &[Flexible],
                trigger: Dates,
                expected: vec![
                    write(2, "07-13", "07-15"),
                    write(3, "07-13", "07-18"),
                    write(5, "07-18", "07-19"),
                ],
            },
            Case {
                name: "strict shifts each join by the change, not twice",
                nodes,
                changes: a_moves_to("07-13"),
                modes: &[Strict],
                trigger: Dates,
                expected: vec![
                    write(2, "07-13", "07-15"),
                    write(3, "07-13", "07-18"),
                    write(4, "07-23", "07-24"),
                    write(5, "07-19", "07-20"),
                ],
            },
        ]);
    }
}
