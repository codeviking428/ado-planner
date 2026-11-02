use ado_core::parse_iso_date;
use ado_planner::gantt::{GanttScale, GanttView, GestureKind, NudgeAxis};
use ado_planner::paths::AppPaths;
use ado_planner::state::{DateSaveRole, PlannerState, fake_hierarchy};
use ado_session::{EncryptedStore, MemoryStore, PlannerSession};
use jiff::ToSpan;
use jiff::civil::Date;

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

#[test]
fn expand_collapse_and_scale_change() {
    let nodes = fake_hierarchy(40);
    let mut gantt = GanttView::default();
    let rows = gantt.flatten(&nodes);
    assert!(rows.len() > 1);
    let root = rows[0].id;
    gantt.toggle_expand(root);
    assert!(gantt.flatten(&nodes).len() < rows.len());
    gantt.toggle_expand(root);
    assert_eq!(gantt.flatten(&nodes).len(), rows.len());
    gantt.scale = GanttScale::Week;
    assert_eq!(gantt.scale.label(), "Week");
    assert_eq!(gantt.scale.button_label(1.0), "Week");
    gantt.zoom_at(1.5, 40.0, 0.0);
    assert!(gantt.zoom > 1.0);
    assert_eq!(
        gantt.scale.button_label(gantt.zoom),
        format!("Week ({:.0}%)", gantt.zoom * 100.0)
    );
}

#[test]
fn five_thousand_rows_render_virtualized() {
    let nodes = fake_hierarchy(5_000);
    let gantt = GanttView::default();
    let rows = gantt.flatten(&nodes);
    assert!(rows.len() >= 5_000 / 20);
    let window = gantt.visible_window(&rows, 720.0);
    assert!(window.len() < 50);
    let bars = gantt.layout_bars(&nodes, &rows, &[], 720.0);
    assert!(bars.len() < 50);
}

#[tokio::test]
async fn drag_patches_start_and_target() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .visible_nodes()
        .into_iter()
        .find(|n| n.start_date.is_some() && n.target_date.is_some() && n.has_date_fields)
        .expect("scheduled");
    let start = parse_iso_date(node.start_date.as_deref()).unwrap();
    let target = parse_iso_date(node.target_date.as_deref()).unwrap();
    let next_start = start.saturating_add(2.days());
    let next_target = target.saturating_add(2.days());
    state
        .save_dates(node.id, next_start, next_target)
        .await
        .expect("save");
    assert!(
        state
            .toasts
            .last_message()
            .unwrap_or_default()
            .contains("Saved Start Date and Target Date")
    );
}

#[tokio::test]
async fn selecting_a_bar_without_moving_does_not_save() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some())
        .cloned()
        .unwrap();
    let before = state.toasts.items.len();
    let _ = state.begin_bar_gesture(node.id, GestureKind::Move, 0.0);
    state.commit_gesture().await.expect("commit click");
    assert_eq!(state.toasts.items.len(), before);
    assert_eq!(state.gantt.selected_id, Some(node.id));
}

#[tokio::test]
async fn escape_cancels_a_gesture() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some())
        .cloned()
        .unwrap();
    let original = node.start_date.clone();
    let _ = state.begin_bar_gesture(node.id, GestureKind::Move, 0.0);
    state.update_bar_gesture(80.0);
    state.cancel_gesture();
    let restored = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(restored.start_date, original);
}

#[tokio::test]
async fn nudge_coalesces_five_presses_into_one_patch() {
    let (mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some())
        .cloned()
        .unwrap();
    state.gantt.selected_id = Some(node.id);
    let before = mock.patch_count();
    for _ in 0..5 {
        state.nudge(NudgeAxis::Both, 1).expect("nudge");
    }
    assert_eq!(mock.patch_count(), before);
    state.flush_nudge().await.expect("flush");
    assert_eq!(mock.patch_count(), before + 1);
}

#[tokio::test]
async fn resize_end_patches_target_only() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let original_start = node.start_date.clone();
    let original_target = parse_iso_date(node.target_date.as_deref()).unwrap();
    let _ = state.begin_bar_gesture(node.id, GestureKind::ResizeEnd, 0.0);
    state.update_bar_gesture(state.gantt.scale.pixels_per_day(state.gantt.zoom) * 3.0);
    state.commit_gesture().await.expect("commit resize");
    let updated = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(updated.start_date, original_start);
    let next_target = parse_iso_date(updated.target_date.as_deref()).unwrap();
    assert_eq!(next_target, original_target.saturating_add(3.days()));
}

#[tokio::test]
async fn undo_after_a_bar_drag_restores_original_dates() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let original_start = node.start_date.clone();
    let original_target = node.target_date.clone();
    let _ = state.begin_bar_gesture(node.id, GestureKind::Move, 0.0);
    state.update_bar_gesture(state.gantt.scale.pixels_per_day(state.gantt.zoom) * 3.0);
    state.commit_gesture().await.expect("commit drag");
    let moved = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_ne!(
        moved.start_date, original_start,
        "drag must save a new Start Date so undo has work to do"
    );
    assert!(
        !state.history.undo.is_empty(),
        "a bar drag must push an undo entry"
    );
    state.undo().await.expect("undo drag");
    let restored = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(restored.start_date, original_start);
    assert_eq!(restored.target_date, original_target);
}

#[tokio::test]
async fn two_drags_then_two_undos_revert_both() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let original_start = node.start_date.clone();
    let original_target = node.target_date.clone();
    let step = state.gantt.scale.pixels_per_day(state.gantt.zoom);

    let _ = state.begin_bar_gesture(node.id, GestureKind::Move, 0.0);
    state.update_bar_gesture(step * 2.0);
    state.commit_gesture().await.expect("drag 1");
    let after_first = state
        .nodes
        .iter()
        .find(|n| n.id == node.id)
        .cloned()
        .unwrap();
    assert_ne!(after_first.start_date, original_start);

    let _ = state.begin_bar_gesture(node.id, GestureKind::Move, 0.0);
    state.update_bar_gesture(step * 3.0);
    state.commit_gesture().await.expect("drag 2");
    let after_second = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_ne!(after_second.start_date, after_first.start_date);
    assert_eq!(
        state.history.undo.len(),
        2,
        "two drags must leave two undo entries, got {}",
        state.history.undo.len()
    );

    state.undo().await.expect("undo 2");
    let undone_once = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(undone_once.start_date, after_first.start_date);
    assert_eq!(undone_once.target_date, after_first.target_date);

    state.undo().await.expect("undo 1");
    let restored = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(restored.start_date, original_start);
    assert_eq!(restored.target_date, original_target);
}

#[tokio::test]
async fn two_optimistic_undos_revert_both_drags() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let original_start = node.start_date.clone();
    let original_target = node.target_date.clone();
    let step = state.gantt.scale.pixels_per_day(state.gantt.zoom);

    let _ = state.begin_bar_gesture(node.id, GestureKind::Move, 0.0);
    state.update_bar_gesture(step * 2.0);
    let pending1 = state.commit_date_gesture_now().expect("drag 1");
    state
        .persist_pending_dates(pending1)
        .await
        .expect("persist 1");
    let after_first = state
        .nodes
        .iter()
        .find(|n| n.id == node.id)
        .cloned()
        .unwrap();
    let _ = state.begin_bar_gesture(node.id, GestureKind::Move, 0.0);
    state.update_bar_gesture(step * 3.0);
    let pending2 = state.commit_date_gesture_now().expect("drag 2");
    state
        .persist_pending_dates(pending2)
        .await
        .expect("persist 2");
    assert_eq!(state.history.undo.len(), 2);

    let _ = state.undo_dates_now().expect("optimistic undo 2");
    let undone_once = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(undone_once.start_date, after_first.start_date);
    let _ = state.undo_dates_now().expect("optimistic undo 1");
    let restored = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(restored.start_date, original_start);
    assert_eq!(restored.target_date, original_target);
}

#[tokio::test]
async fn failed_undo_revert_keeps_the_sibling_undo_entry() {
    let (mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let step = state.gantt.scale.pixels_per_day(state.gantt.zoom);
    let _ = state.begin_bar_gesture(node.id, GestureKind::Move, 0.0);
    state.update_bar_gesture(step * 2.0);
    let pending1 = state.commit_date_gesture_now().expect("drag 1");
    state
        .persist_pending_dates(pending1)
        .await
        .expect("persist 1");
    let _ = state.begin_bar_gesture(node.id, GestureKind::Move, 0.0);
    state.update_bar_gesture(step * 3.0);
    let pending2 = state.commit_date_gesture_now().expect("drag 2");
    state
        .persist_pending_dates(pending2)
        .await
        .expect("persist 2");
    assert_eq!(state.history.undo.len(), 2);
    mock.fail_next(&format!("/workitems/{}", node.id), 503, "save failed");
    let pending_undo = state.undo_dates_now().expect("undo last");
    assert_eq!(pending_undo.role, DateSaveRole::Undo);
    assert_eq!(state.history.undo.len(), 1);
    let _ = state.persist_pending_dates(pending_undo).await;
    assert_eq!(
        state.history.undo.len(),
        2,
        "a failed undo persist must put the Dates entry back, not drop a sibling"
    );
}

#[tokio::test]
async fn end_handle_drop_records_undo_before_the_patch_returns() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let original_target = node.target_date.clone();
    let _ = state.begin_bar_gesture(node.id, GestureKind::ResizeEnd, 0.0);
    state.update_bar_gesture(state.gantt.scale.pixels_per_day(state.gantt.zoom) * 4.0);
    let pending = state
        .commit_date_gesture_now()
        .expect("drop must produce a pending save");
    assert!(
        !state.history.undo.is_empty(),
        "Ctrl+Z must see the drag on the undo stack before PATCH returns"
    );
    let moved = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_ne!(moved.target_date, original_target);
    state.persist_pending_dates(pending).await.expect("persist");
    state.undo().await.expect("undo");
    let restored = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(restored.target_date, original_target);
}

#[tokio::test]
async fn undo_after_an_end_handle_drag_restores_original_target() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let original_start = node.start_date.clone();
    let original_target = node.target_date.clone();
    let _ = state.begin_bar_gesture(node.id, GestureKind::ResizeEnd, 0.0);
    state.update_bar_gesture(state.gantt.scale.pixels_per_day(state.gantt.zoom) * 4.0);
    state.commit_gesture().await.expect("commit resize");
    let moved = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(moved.start_date, original_start);
    assert_ne!(moved.target_date, original_target);
    state.undo().await.expect("undo resize");
    let restored = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(restored.start_date, original_start);
    assert_eq!(restored.target_date, original_target);
}

#[tokio::test]
async fn failed_persist_pending_dates_pushes_an_error_toast() {
    let (mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    mock.fail_next(&format!("/workitems/{}", node.id), 503, "save failed");
    let _ = state.begin_bar_gesture(node.id, GestureKind::Move, 0.0);
    state.update_bar_gesture(state.gantt.scale.pixels_per_day(state.gantt.zoom) * 2.0);
    let pending = state.commit_date_gesture_now().expect("pending");
    let _ = state.persist_pending_dates(pending).await;
    assert!(
        state.toasts.last_error_message().is_some(),
        "a failed date persist must surface an error toast on the live state"
    );
}

#[tokio::test]
async fn persist_pending_dates_retries_a_stale_rev() {
    let (mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let _ = state.begin_bar_gesture(node.id, GestureKind::Move, 0.0);
    state.update_bar_gesture(state.gantt.scale.pixels_per_day(state.gantt.zoom) * 2.0);
    let mut pending = state.commit_date_gesture_now().expect("pending");
    pending.rev = 0;
    state
        .persist_pending_dates(pending)
        .await
        .expect("stale rev must refetch and retry");
    let live = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    let server = mock.work_item(node.id).await.expect("server item");
    let fields = server.get("fields").cloned().unwrap_or_default();
    assert_eq!(
        parse_iso_date(live.start_date.as_deref()),
        parse_iso_date(
            fields
                .get(ado_core::START_DATE_FIELD)
                .and_then(|value| value.as_str())
        )
    );
}

#[tokio::test]
async fn undo_restores_a_cleared_target_date() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let original_target = node.target_date.clone();
    let start = parse_iso_date(node.start_date.as_deref()).unwrap();
    state
        .save_exact_dates(node.id, Some(start), None)
        .await
        .expect("clear target");
    let cleared = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert!(cleared.target_date.is_none() || cleared.target_date.as_deref() == Some("null"));
    state.undo().await.expect("undo");
    let restored = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(restored.target_date, original_target);
}

#[tokio::test]
async fn rollback_on_injected_failure() {
    let (mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some())
        .cloned()
        .unwrap();
    let original = node.start_date.clone();
    mock.fail_next(&format!("/workitems/{}", node.id), 503, "save failed");
    let start = parse_iso_date(node.start_date.as_deref()).unwrap();
    let target = parse_iso_date(node.target_date.as_deref()).unwrap();
    let _ = state
        .save_dates(
            node.id,
            start.saturating_add(3.days()),
            target.saturating_add(3.days()),
        )
        .await;
    let restored = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(restored.start_date, original);
    assert!(state.toasts.last_error_message().is_some());
}

#[tokio::test]
async fn schedule_at_writes_start_and_target_for_an_unscheduled_item() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields)
        .cloned()
        .unwrap();
    state.clear_dates(node.id).await.expect("clear");
    let start = parse_iso_date(Some("2026-08-03T00:00:00Z")).unwrap();
    state.schedule_at(node.id, start).await.expect("schedule");
    let updated = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(
        parse_iso_date(updated.start_date.as_deref()).unwrap(),
        start
    );
    assert!(parse_iso_date(updated.target_date.as_deref()).unwrap() > start);
}

#[tokio::test]
async fn changing_scope_reveals_timeline_content() {
    let (_mock, mut state, _dir) = signed_in().await;
    assert!(!state.nodes.is_empty());
    state.gantt.origin = Date::new(2025, 1, 1).unwrap();
    let team = state.prefs.planner.team.clone();
    state.set_team(team).await.expect("reload team");
    assert_ne!(
        state.gantt.origin,
        Date::new(2025, 1, 1).unwrap(),
        "reloading a team should reveal the new work on the timeline"
    );
}

#[tokio::test]
async fn switching_teams_restores_gantt_scroll() {
    let (_mock, mut state, _dir) = signed_in().await;
    let home = state.prefs.planner.team.clone();
    state.gantt.viewport_h = 120.0;
    state.gantt.scroll_y = 80.0;
    state.gantt.scroll_x = 48.0;
    let origin = state.gantt.origin;
    state.begin_team_switch("Growth".into());
    state
        .set_team("Growth".into())
        .await
        .expect("switch to Growth");
    assert_eq!(
        state.gantt.scroll_y, 0.0,
        "a team without a saved view should start at the top"
    );
    state.begin_team_switch(home.clone());
    state.set_team(home).await.expect("switch home");
    assert_eq!(state.gantt.scroll_y, 80.0);
    assert_eq!(state.gantt.scroll_x, 48.0);
    assert_eq!(state.gantt.origin, origin);
}

#[tokio::test]
async fn restored_team_scroll_is_clamped_to_the_new_rows() {
    let (_mock, mut state, _dir) = signed_in().await;
    let home = state.prefs.planner.team.clone();
    state
        .set_team("Growth".into())
        .await
        .expect("switch to Growth");
    state.gantt.scroll_y = 10_000.0;
    state.begin_team_switch(home.clone());
    state.set_team(home).await.expect("switch home");
    state.begin_team_switch("Growth".into());
    state
        .set_team("Growth".into())
        .await
        .expect("return to Growth");
    let rows = state.visible_rows().len();
    let content_h = (state.gantt.viewport_h - ado_planner::gantt::TIMELINE_HEADER_H).max(0.0);
    let max_y = (rows as f32 * ado_planner::gantt::ROW_HEIGHT - content_h).max(0.0);
    assert!(
        state.gantt.scroll_y <= max_y + 0.5,
        "restored scroll_y {} must fit {rows} rows (max {max_y})",
        state.gantt.scroll_y
    );
}

#[tokio::test]
async fn growth_team_loads_its_epic_and_empty_team_has_no_items() {
    let (_mock, mut state, _dir) = signed_in().await;
    state
        .set_team("Growth".into())
        .await
        .expect("switch to Growth");
    assert!(
        state.visible_nodes().iter().any(|node| node.id == 1020),
        "Growth's Epic must stay visible under the default Roots filter, got {:?}",
        state
            .visible_nodes()
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>()
    );
    state
        .set_team("Empty".into())
        .await
        .expect("switch to Empty");
    assert!(
        state.visible_nodes().is_empty(),
        "Empty team should have no work items"
    );
}

#[tokio::test]
async fn switching_teams_reseeds_roots_so_growth_epic_stays_visible() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.prefs.planner.root_types = Some(Some(vec!["Feature".into()]));
    state
        .set_team("Growth".into())
        .await
        .expect("switch to Growth");
    assert!(
        state.visible_nodes().iter().any(|node| node.id == 1020),
        "Roots must not carry Feature over from Platform and hide Growth's only Epic"
    );
}

#[tokio::test]
async fn filters_that_hide_every_item_are_counted() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.hide_all_roots();
    assert!(
        state.filters_hide_work_items(),
        "hiding every root type should leave the Gantt empty because of filters, not because the team has no work"
    );
    assert_eq!(state.hidden_by_filters_count(), state.nodes.len());
}

#[tokio::test]
async fn removing_a_dependency_clears_the_predecessor() {
    let (_mock, mut state, _dir) = signed_in().await;
    state
        .add_dependency(1005, 1003)
        .await
        .expect("link 1003 → 1005");
    let linked = state.nodes.iter().find(|node| node.id == 1005).unwrap();
    assert!(
        linked.predecessors.contains(&1003),
        "adding a dependency must record the predecessor, got {:?}",
        linked.predecessors
    );
    state
        .remove_dependency(1005, 1003)
        .await
        .expect("remove link");
    let cleared = state.nodes.iter().find(|node| node.id == 1005).unwrap();
    assert!(
        !cleared.predecessors.contains(&1003),
        "removing a link must drop the predecessor, got {:?}",
        cleared.predecessors
    );
    let message = state.toasts.last_message().unwrap_or_default();
    assert!(
        message.to_lowercase().contains("removed link"),
        "delete should toast the removed link, got {message}"
    );
}

#[tokio::test]
async fn undo_and_redo_restore_dates_and_links() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.id == 1003)
        .cloned()
        .expect("1003");
    let original_start = node.start_date.clone();
    let original_target = node.target_date.clone();
    let start = parse_iso_date(original_start.as_deref()).unwrap();
    let target = parse_iso_date(original_target.as_deref()).unwrap();
    let next_start = start.saturating_add(3.days());
    let next_target = target.saturating_add(3.days());
    state
        .save_dates(node.id, next_start, next_target)
        .await
        .expect("save dates");
    let moved = state.nodes.iter().find(|n| n.id == 1003).unwrap();
    assert_ne!(moved.start_date, original_start);
    state.undo().await.expect("undo dates");
    let restored = state.nodes.iter().find(|n| n.id == 1003).unwrap();
    assert_eq!(restored.start_date, original_start);
    assert_eq!(restored.target_date, original_target);
    state.redo().await.expect("redo dates");
    let redone = state.nodes.iter().find(|n| n.id == 1003).unwrap();
    assert_eq!(
        parse_iso_date(redone.start_date.as_deref()).unwrap(),
        next_start
    );
    state.undo().await.expect("undo redo");

    state
        .add_dependency(1005, 1003)
        .await
        .expect("link 1003 → 1005");
    assert!(
        state
            .nodes
            .iter()
            .find(|n| n.id == 1005)
            .unwrap()
            .predecessors
            .contains(&1003)
    );
    state
        .remove_dependency(1005, 1003)
        .await
        .expect("remove link");
    assert!(
        !state
            .nodes
            .iter()
            .find(|n| n.id == 1005)
            .unwrap()
            .predecessors
            .contains(&1003)
    );
    state.undo().await.expect("undo remove link");
    assert!(
        state
            .nodes
            .iter()
            .find(|n| n.id == 1005)
            .unwrap()
            .predecessors
            .contains(&1003),
        "first Ctrl+Z after removing a link must restore it"
    );
    state.undo().await.expect("undo add link");
    assert!(
        !state
            .nodes
            .iter()
            .find(|n| n.id == 1005)
            .unwrap()
            .predecessors
            .contains(&1003),
        "second Ctrl+Z must undo the original add, not remove the restored link again"
    );
}

#[tokio::test]
async fn resize_end_toasts_only_target_date() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.id == 1003 && n.target_date.is_some())
        .cloned()
        .expect("1003");
    let target = parse_iso_date(node.target_date.as_deref()).unwrap();
    state
        .save_partial_dates(node.id, None, Some(target.saturating_add(2.days())))
        .await
        .expect("save target");
    let message = state.toasts.last_message().unwrap_or_default();
    assert_eq!(
        message, "Saved Target Date",
        "end-only save must toast Target Date, got {message}"
    );
    assert!(
        !message.to_lowercase().contains("start"),
        "end-only toast must not mention Start Date: {message}"
    );
}

#[tokio::test]
async fn add_item_creates_a_work_item_on_the_team() {
    let (_mock, mut state, _dir) = signed_in().await;
    let before = state.nodes.len();
    state.add_item().await.expect("create");
    assert_eq!(state.nodes.len(), before + 1);
    let created = state.nodes.last().expect("created");
    assert_eq!(created.title, "New work item");
    assert_eq!(state.gantt.selected_id, Some(created.id));
}

#[tokio::test]
async fn save_title_patches_system_title() {
    let (_mock, mut state, _dir) = signed_in().await;
    state
        .save_title(1003, "Inline title".into())
        .await
        .expect("title");
    let node = state.nodes.iter().find(|n| n.id == 1003).unwrap();
    assert_eq!(node.title, "Inline title");
}

async fn server_dates(mock: &ado_mock::AdoMock, id: i64) -> (Option<Date>, Option<Date>) {
    let item = mock.work_item(id).await.expect("server work item");
    let fields = item.get("fields").cloned().unwrap_or_default();
    (
        parse_iso_date(
            fields
                .get(ado_core::START_DATE_FIELD)
                .and_then(|value| value.as_str()),
        ),
        parse_iso_date(
            fields
                .get(ado_core::TARGET_DATE_FIELD)
                .and_then(|value| value.as_str()),
        ),
    )
}

fn live_dates(state: &PlannerState, id: i64) -> (Option<Date>, Option<Date>) {
    let node = state.nodes.iter().find(|node| node.id == id).expect("live");
    (
        parse_iso_date(node.start_date.as_deref()),
        parse_iso_date(node.target_date.as_deref()),
    )
}

#[tokio::test]
async fn three_gestures_three_undos_match_the_server_after_each() {
    let (mock, mut state, _dir) = signed_in().await;
    let id = 1003;
    let original = live_dates(&state, id);
    let step = state.gantt.scale.pixels_per_day(state.gantt.zoom);

    let _ = state.begin_bar_gesture(id, GestureKind::ResizeEnd, 0.0);
    state.update_bar_gesture(step * 2.0);
    let pending = state.commit_date_gesture_now().expect("A");
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist A");
    let after_a = live_dates(&state, id);
    assert_ne!(after_a, original);
    assert_eq!(after_a, server_dates(&mock, id).await);

    let _ = state.begin_bar_gesture(id, GestureKind::ResizeEnd, 0.0);
    state.update_bar_gesture(step * 3.0);
    let pending = state.commit_date_gesture_now().expect("B");
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist B");
    let after_b = live_dates(&state, id);
    assert_ne!(after_b, after_a);
    assert_eq!(after_b, server_dates(&mock, id).await);

    let _ = state.begin_bar_gesture(id, GestureKind::Move, 0.0);
    state.update_bar_gesture(step * 2.0);
    let pending = state.commit_date_gesture_now().expect("C");
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist C");
    let after_c = live_dates(&state, id);
    assert_ne!(after_c, after_b);
    assert_eq!(after_c, server_dates(&mock, id).await);
    assert_eq!(state.history.undo.len(), 3);

    let pending = state.undo_dates_now().expect("undo C");
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist undo C");
    assert_eq!(live_dates(&state, id), after_b);
    assert_eq!(
        server_dates(&mock, id).await,
        after_b,
        "first undo must PATCH C back to B on the server"
    );

    let pending = state.undo_dates_now().expect("undo B");
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist undo B");
    assert_eq!(live_dates(&state, id), after_a);
    assert_eq!(
        server_dates(&mock, id).await,
        after_a,
        "second undo must PATCH B back to A on the server"
    );

    let pending = state.undo_dates_now().expect("undo A");
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist undo A");
    assert_eq!(live_dates(&state, id), original);
    assert_eq!(
        server_dates(&mock, id).await,
        original,
        "third undo must PATCH A back to the original dates on the server"
    );
}

#[tokio::test]
async fn remove_link_after_undo_succeeds() {
    let (mock, mut state, _dir) = signed_in().await;
    state
        .add_dependency(1005, 1003)
        .await
        .expect("link 1003 → 1005");
    let id = 1005;
    let step = state.gantt.scale.pixels_per_day(state.gantt.zoom);
    let _ = state.begin_bar_gesture(id, GestureKind::Move, 0.0);
    state.update_bar_gesture(step * 4.0);
    let pending = state.commit_date_gesture_now().expect("move");
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist move");
    let pending = state.undo_dates_now().expect("undo move");
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist undo");
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == id) {
        node.rev = 1;
    }
    state
        .remove_dependency(1005, 1003)
        .await
        .expect("remove link after undo must refetch rev instead of 412");
    let cleared = state.nodes.iter().find(|node| node.id == 1005).unwrap();
    assert!(
        !cleared.predecessors.contains(&1003),
        "the predecessor must be dropped locally"
    );
    let server = mock.work_item(1005).await.expect("server 1005");
    let relations = server
        .get("relations")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        !relations.iter().any(|rel| rel
            .get("url")
            .and_then(|value| value.as_str())
            .is_some_and(|url| url.ends_with("/1003"))),
        "the server must no longer hold the dependency after Remove link"
    );
}

#[gpui_kit::test]
fn gpui_gantt_scale_and_zoom(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let mut gantt = GanttView {
        scale: GanttScale::Day,
        ..Default::default()
    };
    gantt.zoom_at(2.0, 10.0, 0.0);
    assert_eq!(gantt.scale, GanttScale::Day);
    assert!((gantt.zoom - 2.0).abs() < f32::EPSILON);
}
