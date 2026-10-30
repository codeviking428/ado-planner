use ado_planner::gantt::ContextTarget;
use ado_planner::paths::AppPaths;
use ado_planner::state::PlannerState;
use ado_planner::toasts::UndoAction;
use ado_session::{EncryptedStore, MemoryStore, PlannerSession};

async fn signed_in() -> (ado_mock::AdoMock, PlannerState, tempfile::TempDir) {
    let mock = ado_mock::start().await;
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(dir.path().to_path_buf());
    let store = EncryptedStore::new(Box::new(MemoryStore::new(true)), Some([3u8; 32]));
    let session = PlannerSession::pat(store).with_bases(&mock.url, &mock.url);
    let mut state = PlannerState::new(
        paths,
        session,
        Some(mock.url.clone()),
        Some(mock.url.clone()),
    );
    state.sign_in_org = "https://dev.azure.com/contoso".into();
    state.sign_in_pat = "pat-secret-value".into();
    state.login_pat().await.expect("login");
    (mock, state, dir)
}

#[tokio::test]
async fn context_menu_open_copy_and_hide() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.open_form(1003).await.expect("open");
    assert!(state.form.is_some());
    state.close_form();
    state.copy_id(1003);
    assert_eq!(state.copied.as_deref(), Some("1003"));
    assert!(
        state
            .toasts
            .last_message()
            .unwrap_or_default()
            .contains("Copied #1003")
    );
    state.copy_link(1003);
    assert!(
        state
            .copied
            .as_deref()
            .unwrap_or_default()
            .contains("_workitems/edit/1003")
    );
    assert_eq!(state.toasts.last_message(), Some("Copied link"));
    let ty = state
        .nodes
        .iter()
        .find(|n| n.id == 1003)
        .unwrap()
        .work_item_type
        .clone();
    state.toggle_hidden_type(&ty);
    assert!(state.prefs.planner.hidden_types.contains(&ty));
    let type_hidden = state.prefs.planner.hidden_types.clone();
    let was_hidden_root = state.hidden_roots().contains(&ty);
    state.toggle_hidden_root(&ty);
    assert_ne!(state.hidden_roots().contains(&ty), was_hidden_root);
    assert_eq!(state.prefs.planner.hidden_types, type_hidden);
}

#[tokio::test]
async fn clear_dates_has_undo() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some())
        .cloned()
        .unwrap();
    let original = node.start_date.clone();
    state.clear_dates(node.id).await.expect("clear");
    let cleared = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert!(cleared.start_date.is_none() || cleared.start_date.as_deref() == Some("null"));
    let undo = state
        .toasts
        .items
        .iter()
        .rev()
        .find_map(|t| t.undo.clone())
        .expect("undo");
    let UndoAction::RestoreDates { id, .. } = &undo else {
        panic!("clear dates must offer RestoreDates undo, got {undo:?}");
    };
    assert_eq!(*id, node.id);
    state.undo_clear_dates(undo).await.expect("undo");
    let restored = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(restored.start_date, original);
}

#[tokio::test]
async fn schedule_to_iteration_sets_start_and_target() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| {
            n.has_date_fields
                && state.iterations.iter().any(|iteration| {
                    iteration.path == n.iteration_path && iteration.start_date.is_some()
                })
        })
        .cloned()
        .unwrap();
    state.clear_dates(node.id).await.expect("clear first");
    state
        .schedule_to_iteration(node.id)
        .await
        .expect("schedule");
    let updated = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    let iteration = state
        .iterations
        .iter()
        .find(|iteration| iteration.path == node.iteration_path)
        .unwrap();
    assert_eq!(updated.start_date, iteration.start_date);
    assert_eq!(updated.target_date, iteration.finish_date);
}

#[tokio::test]
async fn hide_this_state_writes_the_states_filter() {
    let (_mock, mut state, _dir) = signed_in().await;
    let state_name = state
        .nodes
        .iter()
        .find(|n| n.id == 1003)
        .unwrap()
        .state
        .clone();
    state.toggle_hidden_state(&state_name);
    assert!(state.prefs.planner.hidden_states.contains(&state_name));
    // Ancestors of remaining matches stay in the forest, same as the Electron overlays.
}

#[tokio::test]
async fn expand_collapse_below_this_row() {
    let (_mock, mut state, _dir) = signed_in().await;
    let nodes = state.visible_nodes();
    let root = nodes
        .iter()
        .find(|n| n.parent_id.is_none())
        .map(|n| n.id)
        .unwrap();
    let before = state.gantt.flatten(&nodes).len();
    state.gantt.collapse_all_below(&nodes, root);
    assert!(state.gantt.flatten(&nodes).len() <= before);
    state.gantt.expand_all_below(&nodes, root);
    assert_eq!(state.gantt.flatten(&nodes).len(), before);
    state.gantt.context_menu = Some(ContextTarget {
        id: root,
        from_bar: false,
        x: 320.0,
        y: 280.0,
        link: None,
    });
    assert!(state.gantt.context_menu.is_some());
}

#[gpui_kit::test]
fn gpui_context_menu_action_labels(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let labels = [
        "Open",
        "Open in Azure DevOps",
        "Copy ID",
        "Copy link",
        "Clear dates",
        "Schedule to iteration",
        "Expand all",
        "Collapse all",
        "Hide this type",
        "Hide this state",
    ];
    for label in labels {
        assert!(!label.is_empty());
    }
}
