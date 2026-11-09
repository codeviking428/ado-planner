use std::collections::{HashMap, HashSet, VecDeque};

use crate::types::WorkItemNode;

/// The loop that linking `predecessor_id → successor_id` would close, as
/// `[pred, succ, …, pred]`, or `None` when the link is safe.
///
/// Walks `predecessors` back from the new predecessor over every node given,
/// with or without a Gantt bar. Breadth-first, so the shortest loop wins.
/// Predecessor ids that are not loaded end that branch of the walk.
pub fn dependency_loop(
    nodes: &[WorkItemNode],
    predecessor_id: i64,
    successor_id: i64,
) -> Option<Vec<i64>> {
    if predecessor_id == successor_id {
        return Some(vec![predecessor_id, predecessor_id]);
    }
    let by_id: HashMap<i64, &WorkItemNode> = nodes.iter().map(|node| (node.id, node)).collect();
    let mut came_from: HashMap<i64, i64> = HashMap::new();
    let mut visited: HashSet<i64> = HashSet::from([predecessor_id]);
    let mut queue = VecDeque::from([predecessor_id]);
    while let Some(id) = queue.pop_front() {
        let Some(node) = by_id.get(&id) else {
            continue;
        };
        for &next in &node.predecessors {
            if !visited.insert(next) {
                continue;
            }
            came_from.insert(next, id);
            if next == successor_id {
                let mut path = vec![predecessor_id, successor_id];
                let mut step = successor_id;
                while let Some(&back) = came_from.get(&step) {
                    path.push(back);
                    step = back;
                }
                return Some(path);
            }
            queue.push_back(next);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: i64, predecessors: &[i64]) -> WorkItemNode {
        WorkItemNode {
            id,
            rev: 1,
            title: format!("WI {id}"),
            work_item_type: "Task".into(),
            state: "Active".into(),
            assigned_to: None,
            parent_id: None,
            area_path: "Shop".into(),
            iteration_path: "Shop\\Sprint 1".into(),
            start_date: Some("2026-07-01".into()),
            target_date: Some("2026-07-03".into()),
            has_date_fields: true,
            predecessors: predecessors.to_vec(),
            fields: serde_json::Map::new(),
        }
    }

    #[test]
    fn dependency_loop_table() {
        struct Case {
            name: &'static str,
            nodes: Vec<WorkItemNode>,
            predecessor: i64,
            successor: i64,
            expected: Option<Vec<i64>>,
        }
        let unscheduled = {
            let mut item = node(2, &[3]);
            item.start_date = None;
            item.target_date = None;
            item
        };
        let cases = [
            Case {
                name: "direct loop: 1 already waits on 2",
                nodes: vec![node(1, &[2]), node(2, &[])],
                predecessor: 1,
                successor: 2,
                expected: Some(vec![1, 2, 1]),
            },
            Case {
                name: "transitive loop: 1 ← 2 ← 3, linking 1 → 3",
                nodes: vec![node(1, &[2]), node(2, &[3]), node(3, &[])],
                predecessor: 1,
                successor: 3,
                expected: Some(vec![1, 3, 2, 1]),
            },
            Case {
                name: "transitive loop through a Work Item without a bar",
                nodes: vec![node(1, &[2]), unscheduled, node(3, &[])],
                predecessor: 1,
                successor: 3,
                expected: Some(vec![1, 3, 2, 1]),
            },
            Case {
                name: "diamond is not a loop: 4 waits on 2 and 3, both wait on 1",
                nodes: vec![node(1, &[]), node(2, &[1]), node(3, &[1]), node(4, &[2, 3])],
                predecessor: 4,
                successor: 5,
                expected: None,
            },
            Case {
                name: "closing a diamond is not a loop: link 1 → 4 again via 3",
                nodes: vec![node(1, &[]), node(2, &[1]), node(3, &[1]), node(4, &[2])],
                predecessor: 3,
                successor: 4,
                expected: None,
            },
            Case {
                name: "loop already in the data ends the walk",
                nodes: vec![node(1, &[2]), node(2, &[3]), node(3, &[1]), node(4, &[])],
                predecessor: 1,
                successor: 4,
                expected: None,
            },
            Case {
                name: "loop already in the data still finds a new loop",
                nodes: vec![node(1, &[2]), node(2, &[3, 4]), node(3, &[1]), node(4, &[])],
                predecessor: 1,
                successor: 4,
                expected: Some(vec![1, 4, 2, 1]),
            },
            Case {
                name: "unloaded predecessor ends that branch",
                nodes: vec![node(1, &[99]), node(2, &[])],
                predecessor: 1,
                successor: 2,
                expected: None,
            },
            Case {
                name: "shortest loop wins",
                nodes: vec![
                    node(1, &[2, 5]),
                    node(2, &[3]),
                    node(3, &[4]),
                    node(4, &[]),
                    node(5, &[4]),
                ],
                predecessor: 1,
                successor: 4,
                expected: Some(vec![1, 4, 5, 1]),
            },
        ];
        for case in cases {
            assert_eq!(
                dependency_loop(&case.nodes, case.predecessor, case.successor),
                case.expected,
                "{}",
                case.name
            );
        }
    }
}
