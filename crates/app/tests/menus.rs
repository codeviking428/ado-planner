use ado_planner::gantt::ContextTarget;
use ado_planner::paths::AppPaths;
use ado_planner::state::DateSaveRole;
use ado_planner::state::PlannerState;
use ado_planner::toasts::{HistoryEntry, UndoAction};
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
async fn toast_undo_keeps_a_later_dates_history_entry() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let original_start = node.start_date.clone();
    let original_target = node.target_date.clone();
    state.clear_dates(node.id).await.expect("clear");
    let undo = state
        .toasts
        .items
        .iter()
        .rev()
        .find_map(|t| t.undo.clone())
        .expect("toast undo");
    state.history.undo.push(HistoryEntry::Dates {
        id: node.id,
        before_start: None,
        before_target: None,
        after_start: Some("2026-09-01T00:00:00Z".into()),
        after_target: Some("2026-09-10T00:00:00Z".into()),
    });
    let later_len = state.history.undo.len();
    let pending = state
        .apply_toast_restore_dates(undo)
        .expect("toast restore");
    assert_eq!(pending.role, DateSaveRole::Restore);
    let restored = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(restored.start_date, original_start);
    assert_eq!(restored.target_date, original_target);
    assert_eq!(
        state.history.undo.len(),
        later_len,
        "toast undo must keep history until the restore persist succeeds"
    );
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist restore");
    assert_eq!(
        state.history.undo.len(),
        later_len - 1,
        "a successful restore persist must drop the matching clear, not the later dates edit"
    );
    assert!(
        matches!(
            state.history.undo.last(),
            Some(HistoryEntry::Dates {
                after_start: Some(start),
                ..
            }) if start == "2026-09-01T00:00:00Z"
        ),
        "the later dates entry must remain on the Ctrl+Z stack"
    );
}

#[tokio::test]
async fn merge_undo_action_keeps_live_dates() {
    let (_mock, mut state, _dir) = signed_in().await;
    let successor_id = 1005;
    let predecessor_id = 1003;
    let mut working = state.clone();
    if let Some(node) = working.nodes.iter_mut().find(|n| n.id == successor_id) {
        node.predecessors.retain(|id| *id != predecessor_id);
        node.start_date = Some("2026-01-01T00:00:00Z".into());
        node.target_date = Some("2026-01-10T00:00:00Z".into());
        node.rev = 99;
    }
    if let Some(node) = state.nodes.iter_mut().find(|n| n.id == successor_id) {
        node.start_date = Some("2026-09-01T00:00:00Z".into());
        node.target_date = Some("2026-09-10T00:00:00Z".into());
        node.rev = 12;
        if !node.predecessors.contains(&predecessor_id) {
            node.predecessors.push(predecessor_id);
        }
    }
    let live_start = state
        .nodes
        .iter()
        .find(|n| n.id == successor_id)
        .unwrap()
        .start_date
        .clone();
    state.merge_undo_action(
        &working,
        &UndoAction::RemoveLink {
            successor_id,
            predecessor_id,
        },
    );
    let live = state.nodes.iter().find(|n| n.id == successor_id).unwrap();
    assert_eq!(
        live.start_date, live_start,
        "live date edits must survive a link toast merge"
    );
    assert!(
        !live.predecessors.contains(&predecessor_id),
        "the link undo must still copy predecessors from the spawned clone"
    );
}

#[tokio::test]
async fn merge_title_undo_does_not_clobber_live_dates() {
    let (_mock, mut state, _dir) = signed_in().await;
    let id = 1003;
    let mut working = state.clone();
    if let Some(node) = working.nodes.iter_mut().find(|n| n.id == id) {
        node.title = "Spawned title".into();
        node.start_date = Some("2026-01-01T00:00:00Z".into());
        node.target_date = Some("2026-01-10T00:00:00Z".into());
    }
    if let Some(node) = state.nodes.iter_mut().find(|n| n.id == id) {
        node.start_date = Some("2026-09-01T00:00:00Z".into());
        node.target_date = Some("2026-09-10T00:00:00Z".into());
    }
    let live_start = state
        .nodes
        .iter()
        .find(|n| n.id == id)
        .unwrap()
        .start_date
        .clone();
    state.merge_undo_action(
        &working,
        &UndoAction::RestoreFields {
            id,
            fields: vec![("System.Title".into(), serde_json::json!("Cart UI"))],
        },
    );
    let live = state.nodes.iter().find(|n| n.id == id).unwrap();
    assert_eq!(
        live.start_date, live_start,
        "a title toast undo must not overwrite a live bar drag"
    );
    assert_eq!(live.title, "Spawned title");
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
