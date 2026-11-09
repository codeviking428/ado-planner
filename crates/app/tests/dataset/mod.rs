//! Synthetic ADO-shaped Hierarchy shared by the speed and launch tests.

use ado_core::{IdentityValue, WorkItemNode};
use jiff::civil::Date;
use serde_json::json;

/// Epic → Feature → User Story → Task forest shaped like a loaded Team:
/// Epics and Features are unscheduled (their bars are Rollups), most Stories
/// and Tasks are scheduled, every node carries the hierarchy batch fields, and
/// sibling Stories chain as Dependencies.
pub fn ado_shaped_hierarchy(count: usize) -> Vec<WorkItemNode> {
    const FEATURES_PER_EPIC: usize = 4;
    const STORIES_PER_FEATURE: usize = 5;
    const TASKS_PER_STORY: usize = 4;
    let states = ["New", "Active", "Resolved", "Closed"];
    let people = ["Ada Lovelace", "Grace Hopper", "Alan Turing"];
    let mut nodes = Vec::with_capacity(count);
    let push = |nodes: &mut Vec<WorkItemNode>, parent: Option<i64>, ty: &str, dated: bool| {
        let id = nodes.len() as i64 + 1;
        let n = id as usize;
        let start = Date::new(2026, 1, 5)
            .unwrap()
            .saturating_add(jiff::Span::new().days((n % 300) as i64));
        let target = start.saturating_add(jiff::Span::new().days(3 + (n % 20) as i64));
        let (start, target) = if dated {
            (
                Some(format!("{start}T00:00:00Z")),
                Some(format!("{target}T00:00:00Z")),
            )
        } else {
            (None, None)
        };
        let person = people[n % people.len()];
        let assigned = (!n.is_multiple_of(5)).then(|| IdentityValue {
            display_name: person.into(),
            unique_name: format!("{}@contoso.com", person.to_lowercase().replace(' ', ".")),
            id: None,
        });
        let title = format!("{ty} {id} for the checkout and catalog rework");
        let state = states[n % states.len()];
        let mut fields = serde_json::Map::new();
        fields.insert("System.Id".into(), json!(id));
        fields.insert("System.Rev".into(), json!(3));
        fields.insert("System.Title".into(), json!(title));
        fields.insert("System.WorkItemType".into(), json!(ty));
        fields.insert("System.State".into(), json!(state));
        if let Some(assigned) = &assigned {
            fields.insert(
                "System.AssignedTo".into(),
                json!({ "displayName": assigned.display_name, "uniqueName": assigned.unique_name }),
            );
        }
        if let Some(parent) = parent {
            fields.insert("System.Parent".into(), json!(parent));
        }
        fields.insert("System.AreaPath".into(), json!("Shop\\Platform"));
        fields.insert("System.IterationPath".into(), json!("Shop\\Sprint 12"));
        if let (Some(start), Some(target)) = (&start, &target) {
            fields.insert("Microsoft.VSTS.Scheduling.StartDate".into(), json!(start));
            fields.insert("Microsoft.VSTS.Scheduling.TargetDate".into(), json!(target));
        }
        nodes.push(WorkItemNode {
            id,
            rev: 3,
            title,
            work_item_type: ty.into(),
            state: state.into(),
            assigned_to: assigned,
            parent_id: parent,
            area_path: "Shop\\Platform".into(),
            iteration_path: "Shop\\Sprint 12".into(),
            start_date: start,
            target_date: target,
            has_date_fields: true,
            predecessors: Vec::new(),
            fields,
        });
        id
    };
    'epics: loop {
        let epic = push(&mut nodes, None, "Epic", false);
        for _ in 0..FEATURES_PER_EPIC {
            let feature = push(&mut nodes, Some(epic), "Feature", false);
            let mut previous_story = None;
            for s in 0..STORIES_PER_FEATURE {
                if nodes.len() >= count {
                    break 'epics;
                }
                let story = push(&mut nodes, Some(feature), "User Story", s % 4 != 3);
                if let Some(previous) = previous_story {
                    nodes[story as usize - 1].predecessors.push(previous);
                }
                previous_story = Some(story);
                for t in 0..TASKS_PER_STORY {
                    if nodes.len() >= count {
                        break 'epics;
                    }
                    push(&mut nodes, Some(story), "Task", t % 3 != 2);
                }
            }
        }
        if nodes.len() >= count {
            break;
        }
    }
    nodes.truncate(count);
    nodes
}
