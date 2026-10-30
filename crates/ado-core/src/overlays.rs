use crate::types::{OverlayFilter, WorkItemNode};

pub fn apply_overlays(nodes: &[WorkItemNode], filter: &OverlayFilter) -> Vec<WorkItemNode> {
    let by_id: std::collections::HashMap<i64, &WorkItemNode> =
        nodes.iter().map(|node| (node.id, node)).collect();
    let mut matching = std::collections::HashSet::new();

    for node in nodes {
        if matches_overlay(node, filter, &by_id) {
            matching.insert(node.id);
        }
    }

    if matching.is_empty() {
        return Vec::new();
    }

    let mut visible = matching.clone();
    let roots = filter.root_types.as_deref();

    for id in &matching {
        let mut current = by_id.get(id).copied();
        while let Some(node) = current {
            let Some(parent_id) = node.parent_id else {
                break;
            };
            if let Some(roots) = roots
                && !roots.is_empty()
                && roots.iter().any(|ty| ty == &node.work_item_type)
            {
                break;
            }
            visible.insert(parent_id);
            current = by_id.get(&parent_id).copied();
        }
    }

    nodes
        .iter()
        .filter(|node| visible.contains(&node.id))
        .map(|node| {
            let mut next = node.clone();
            if let Some(parent) = node.parent_id
                && !visible.contains(&parent)
            {
                next.parent_id = None;
            }
            next
        })
        .collect()
}

fn is_under_root_type(
    node: &WorkItemNode,
    by_id: &std::collections::HashMap<i64, &WorkItemNode>,
    root_types: &[String],
) -> bool {
    let mut current = Some(node);
    while let Some(item) = current {
        if root_types.iter().any(|ty| ty == &item.work_item_type) {
            return true;
        }
        current = item.parent_id.and_then(|id| by_id.get(&id).copied());
    }
    false
}

fn matches_overlay(
    node: &WorkItemNode,
    filter: &OverlayFilter,
    by_id: &std::collections::HashMap<i64, &WorkItemNode>,
) -> bool {
    if let Some(root_types) = &filter.root_types
        && (root_types.is_empty() || !is_under_root_type(node, by_id, root_types))
    {
        return false;
    }
    if let Some(types) = &filter.types
        && !types.iter().any(|ty| ty == &node.work_item_type)
    {
        return false;
    }
    if let Some(states) = &filter.states
        && !states.iter().any(|state| state == &node.state)
    {
        return false;
    }
    if let Some(path) = &filter.iteration_path
        && &node.iteration_path != path
    {
        return false;
    }
    match filter.assignee.as_str() {
        "anyone" => true,
        "unassigned" => node.assigned_to.is_none(),
        other => {
            let wanted = if other == "me" {
                filter
                    .current_user_unique_name
                    .as_deref()
                    .map(str::to_lowercase)
            } else {
                Some(other.to_lowercase())
            };
            match (wanted, &node.assigned_to) {
                (Some(wanted), Some(assigned)) => assigned.unique_name.to_lowercase() == wanted,
                _ => false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{IdentityValue, OverlayFilter, WorkItemNode};

    fn node(id: i64) -> WorkItemNode {
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
            start_date: None,
            target_date: None,
            has_date_fields: true,
            fields: serde_json::Map::new(),
        }
    }

    fn forest() -> Vec<WorkItemNode> {
        let mut epic = node(1);
        epic.work_item_type = "Epic".into();
        epic.iteration_path = "Shop\\FY26".into();
        epic.state = "New".into();
        let mut feature = node(2);
        feature.work_item_type = "Feature".into();
        feature.parent_id = Some(1);
        feature.iteration_path = "Shop\\Sprint 2".into();
        feature.state = "Active".into();
        let mut task = node(3);
        task.work_item_type = "Task".into();
        task.parent_id = Some(2);
        task.iteration_path = "Shop\\Sprint 2".into();
        task.state = "Closed".into();
        task.assigned_to = Some(IdentityValue {
            display_name: "Ada".into(),
            unique_name: "ada@contoso".into(),
            id: None,
        });
        let mut other = node(4);
        other.work_item_type = "Bug".into();
        other.iteration_path = "Shop\\Sprint 1".into();
        other.state = "Active".into();
        vec![epic, feature, task, other]
    }

    fn ids(nodes: &[WorkItemNode]) -> Vec<i64> {
        let mut values: Vec<i64> = nodes.iter().map(|n| n.id).collect();
        values.sort_unstable();
        values
    }

    fn base_filter() -> OverlayFilter {
        OverlayFilter {
            types: None,
            states: None,
            root_types: None,
            assignee: "anyone".into(),
            iteration_path: None,
            current_user_unique_name: None,
        }
    }

    #[test]
    fn iteration_overlay_keeps_matching_nodes_and_in_forest_ancestors() {
        let mut filter = base_filter();
        filter.iteration_path = Some("Shop\\Sprint 2".into());
        assert_eq!(ids(&apply_overlays(&forest(), &filter)), vec![1, 2, 3]);
    }

    #[test]
    fn type_overlay_keeps_ancestors_so_the_tree_does_not_orphan() {
        let mut filter = base_filter();
        filter.types = Some(vec!["Task".into()]);
        assert_eq!(ids(&apply_overlays(&forest(), &filter)), vec![1, 2, 3]);
    }

    #[test]
    fn state_overlay_can_hide_closed_without_dropping_ancestors_of_remaining_matches() {
        let mut filter = base_filter();
        filter.states = Some(vec!["Active".into(), "New".into()]);
        assert_eq!(ids(&apply_overlays(&forest(), &filter)), vec![1, 2, 4]);
    }

    #[test]
    fn assignee_me_keeps_only_that_identity_and_ancestors() {
        let mut filter = base_filter();
        filter.assignee = "me".into();
        filter.current_user_unique_name = Some("ada@contoso".into());
        assert_eq!(ids(&apply_overlays(&forest(), &filter)), vec![1, 2, 3]);
    }

    #[test]
    fn unassigned_hides_assigned_work_items() {
        let mut filter = base_filter();
        filter.assignee = "unassigned".into();
        assert_eq!(ids(&apply_overlays(&forest(), &filter)), vec![1, 2, 4]);
    }

    #[test]
    fn assignee_unique_name_keeps_that_identity_and_ancestors() {
        let mut filter = base_filter();
        filter.assignee = "ada@contoso".into();
        assert_eq!(ids(&apply_overlays(&forest(), &filter)), vec![1, 2, 3]);
    }

    #[test]
    fn root_types_keep_descendants_and_hide_work_items_with_no_root_type_ancestor() {
        let mut filter = base_filter();
        filter.root_types = Some(vec!["Epic".into()]);
        assert_eq!(ids(&apply_overlays(&forest(), &filter)), vec![1, 2, 3]);
    }

    #[test]
    fn root_type_sits_at_the_top_when_its_parent_is_not_a_root_type() {
        let mut filter = base_filter();
        filter.root_types = Some(vec!["Feature".into()]);
        let visible: Vec<(i64, Option<i64>)> = apply_overlays(&forest(), &filter)
            .into_iter()
            .map(|n| (n.id, n.parent_id))
            .collect();
        assert_eq!(visible, vec![(2, None), (3, Some(2))]);
    }

    #[test]
    fn empty_root_types_overlay_matches_nothing() {
        let mut filter = base_filter();
        filter.root_types = Some(vec![]);
        assert!(apply_overlays(&forest(), &filter).is_empty());
    }

    #[test]
    fn empty_type_overlay_matches_nothing() {
        let mut filter = base_filter();
        filter.types = Some(vec![]);
        assert!(apply_overlays(&forest(), &filter).is_empty());
    }
}
