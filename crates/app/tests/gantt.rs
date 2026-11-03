use ado_core::{START_DATE_FIELD, TARGET_DATE_FIELD, parse_iso_date};
use ado_planner::gantt::{GanttScale, GanttView, GestureKind, NudgeAxis};
use ado_planner::paths::AppPaths;
use ado_planner::state::{DateSaveRole, PlannerState, fake_hierarchy};
use ado_planner::toasts::{HistoryEntry, UndoAction};
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
        gantt.zoom_percent_label(),
        format!("{:.0}%", gantt.zoom * 100.0)
    );
    assert_eq!(gantt.scale.button_label(gantt.zoom), "Week");
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
            .contains("Moved")
            && state
                .toasts
                .items
                .last()
                .is_some_and(|toast| toast.undo.is_some())
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
async fn cancelling_a_link_drag_does_not_unschedule_a_start_only_item() {
    let (_mock, mut state, _dir) = signed_in().await;
    let id = 1005;
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == id) {
        node.start_date = Some("2026-07-06T00:00:00Z".into());
        node.target_date = None;
        node.has_date_fields = true;
    }
    let original = state
        .nodes
        .iter()
        .find(|node| node.id == id)
        .and_then(|node| node.start_date.clone());
    state
        .begin_bar_gesture(id, GestureKind::Link, 400.0)
        .expect("right-handle link");
    assert!(
        state
            .gantt
            .gesture
            .as_ref()
            .is_some_and(|gesture| gesture.kind == GestureKind::Link && gesture.had_start),
        "link drag must remember the start-only side"
    );
    state.cancel_gesture();
    let live = state.nodes.iter().find(|node| node.id == id).unwrap();
    assert_eq!(
        live.start_date, original,
        "cancelling a right-handle link must not silently unschedule a start-only bar"
    );
    assert!(live.target_date.is_none());
    assert!(
        state.toasts.items.is_empty(),
        "silent unschedule had no toast; the dates must stay put instead, got {:?}",
        state
            .toasts
            .items
            .iter()
            .map(|toast| toast.message.as_str())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn empty_picker_enter_clears_dates_and_start_only_is_exact() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let start = Date::new(2026, 8, 4).unwrap();
    let pending = state
        .commit_range_dates_now(node.id, Some(start), None)
        .expect("start-only");
    let live = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(parse_iso_date(live.start_date.as_deref()), Some(start));
    assert!(
        live.target_date.is_none(),
        "start-only commit must clear Target Date, got {:?}",
        live.target_date
    );
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist start-only");
    let pending = state
        .commit_range_dates_now(node.id, None, None)
        .expect("clear");
    let live = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert!(live.start_date.is_none() && live.target_date.is_none());
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist clear");
    let message = state.toasts.last_message().unwrap_or_default();
    assert!(
        message.contains("Cleared dates") && message.contains('"'),
        "clear must be undoable and quote the name, got {message}"
    );
    assert!(
        state
            .toasts
            .items
            .last()
            .is_some_and(|toast| toast.undo.is_some()),
        "Clear dates must offer Undo"
    );
}

#[tokio::test]
async fn clear_dates_clears_the_open_form_fields() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    state.open_form(node.id).await.expect("open form");
    state.clear_dates(node.id).await.expect("clear");
    let form = state.form.as_ref().expect("form stays open");
    let live = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(
        form.model.rev, live.rev,
        "Clear dates must refresh the cached form rev so Save does not 412"
    );
    assert!(
        form.values
            .get(START_DATE_FIELD)
            .is_none_or(|value| value.is_null()),
        "Clear dates must wipe the dialog Start Date, got {:?}",
        form.values.get(START_DATE_FIELD)
    );
    assert!(
        form.values
            .get(TARGET_DATE_FIELD)
            .is_none_or(|value| value.is_null()),
        "Clear dates must wipe the dialog Target Date, got {:?}",
        form.values.get(TARGET_DATE_FIELD)
    );
    let message = state.toasts.last_message().unwrap_or_default();
    assert!(
        message.starts_with("Cleared dates on \""),
        "clear toast must quote the name, got {message}"
    );
}

#[tokio::test]
async fn failed_clear_dates_keeps_the_open_form_fields() {
    let (mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let start = node.start_date.clone();
    let target = node.target_date.clone();
    state.open_form(node.id).await.expect("open form");
    let form_start = state
        .form
        .as_ref()
        .and_then(|form| form.values.get(START_DATE_FIELD).cloned());
    let form_target = state
        .form
        .as_ref()
        .and_then(|form| form.values.get(TARGET_DATE_FIELD).cloned());
    mock.fail_next(&format!("/workitems/{}", node.id), 503, "clear failed");
    assert!(state.clear_dates(node.id).await.is_err());
    let live = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(
        live.start_date, start,
        "failed clear must restore the Gantt row"
    );
    assert_eq!(live.target_date, target);
    let form = state.form.as_ref().expect("form stays open");
    assert_eq!(
        form.values.get(START_DATE_FIELD),
        form_start.as_ref(),
        "failed clear must not leave the dialog empty and dirty"
    );
    assert_eq!(form.values.get(TARGET_DATE_FIELD), form_target.as_ref());
}

#[tokio::test]
async fn undo_after_clear_dates_keeps_form_rev_in_sync() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    state.open_form(node.id).await.expect("open form");
    let opened_rev = state.form.as_ref().unwrap().model.rev;
    state.clear_dates(node.id).await.expect("clear");
    let after_clear = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(
        state.form.as_ref().unwrap().model.rev,
        after_clear.rev,
        "clear must bump the cached form rev"
    );
    assert_ne!(after_clear.rev, opened_rev);
    let pending = state.undo_dates_now().expect("undo clear");
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist undo");
    let live = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    let form = state.form.as_mut().expect("form stays open");
    assert_eq!(
        form.model.rev, live.rev,
        "Undo must refresh the cached form rev so Save does not 412"
    );
    form.set_string("System.Title", "After undo save".into());
    state
        .save_form()
        .await
        .expect("save after undo must not 412");
}

#[tokio::test]
async fn undo_after_picker_resyncs_open_form_dates() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let original_start = node.start_date.clone();
    let original_target = node.target_date.clone();
    state.open_form(node.id).await.expect("open form");
    let start = Date::new(2026, 8, 4).unwrap();
    let target = Date::new(2026, 8, 21).unwrap();
    let pending = state
        .commit_range_dates_now(node.id, Some(start), Some(target))
        .expect("picker save");
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist picker");
    let after_pick = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(
        state.form.as_ref().unwrap().model.rev,
        after_pick.rev,
        "picker persist must refresh the cached form rev"
    );
    let pending_undo = state.undo_dates_now().expect("undo picker");
    let form = state.form.as_ref().expect("form stays open");
    assert_eq!(
        form.values
            .get(START_DATE_FIELD)
            .and_then(serde_json::Value::as_str),
        original_start.as_deref(),
        "undo must resync the open dialog Start Date"
    );
    assert_eq!(
        form.values
            .get(TARGET_DATE_FIELD)
            .and_then(serde_json::Value::as_str),
        original_target.as_deref(),
        "undo must resync the open dialog Target Date"
    );
    state
        .persist_pending_dates(pending_undo)
        .await
        .expect("persist undo");
    let live = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    let form = state.form.as_mut().expect("form stays open");
    assert_eq!(form.model.rev, live.rev);
    form.set_string("System.Title", "Picker undo save".into());
    state
        .save_form()
        .await
        .expect("save after picker undo must not 412");
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
async fn queued_undo_persist_skips_when_live_dates_moved() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let original_start = node.start_date.clone();
    let step = state.gantt.scale.pixels_per_day(state.gantt.zoom);
    let _ = state.begin_bar_gesture(node.id, GestureKind::Move, 0.0);
    state.update_bar_gesture(step * 2.0);
    let pending = state.commit_date_gesture_now().expect("drag");
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist drag");
    let pending_undo = state.undo_dates_now().expect("undo");
    if let Some(live) = state.nodes.iter_mut().find(|n| n.id == node.id) {
        live.start_date = Some("2026-09-01T00:00:00Z".into());
        live.target_date = Some("2026-09-10T00:00:00Z".into());
    }
    state
        .persist_pending_dates(pending_undo)
        .await
        .expect("stale undo persist is a no-op");
    let live = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(
        live.start_date.as_deref(),
        Some("2026-09-01T00:00:00Z"),
        "a queued undo must not overwrite a newer local edit"
    );
    assert_ne!(live.start_date, original_start);
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
    let origin = state.gantt.origin;
    let scroll = state.gantt.scroll_x;
    let _ = state.begin_bar_gesture(node.id, GestureKind::ResizeEnd, 0.0);
    state.update_bar_gesture(state.gantt.scale.pixels_per_day(state.gantt.zoom) * 4.0);
    state.commit_gesture().await.expect("commit resize");
    let moved = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(moved.start_date, original_start);
    assert_ne!(moved.target_date, original_target);
    assert_eq!(
        state.gantt.origin, origin,
        "resizing a bar end must not shift the camera origin"
    );
    assert_eq!(
        state.gantt.scroll_x, scroll,
        "resizing a bar end must not pan the viewport"
    );
    state.undo().await.expect("undo resize");
    let restored = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(restored.start_date, original_start);
    assert_eq!(restored.target_date, original_target);
    assert_eq!(
        state.gantt.scroll_x, scroll,
        "Undo after an end-handle drag must keep the viewport"
    );
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

#[test]
fn follow_up_date_save_rolls_back_to_the_persisted_node() {
    let persisted = fake_hierarchy(1).remove(0);
    let mut live = persisted.clone();
    live.start_date = Some("2026-09-01T00:00:00Z".into());
    live.target_date = Some("2026-09-10T00:00:00Z".into());
    live.rev = persisted.rev + 3;
    let pending = PlannerState::follow_up_date_save(&live, &persisted);
    assert_eq!(
        pending.previous.start_date, persisted.start_date,
        "a follow-up persist must roll back to the just-saved server dates, not the newer live node"
    );
    assert_eq!(pending.previous.rev, persisted.rev);
    assert_eq!(pending.after_start, live.start_date);
    assert_eq!(pending.rev, persisted.rev);
}

#[tokio::test]
async fn picker_range_save_records_undo_before_persist() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let start = Date::new(2026, 8, 4).unwrap();
    let target = Date::new(2026, 8, 21).unwrap();
    let pending = state
        .commit_range_dates_now(node.id, Some(start), Some(target))
        .expect("picker save");
    assert!(
        !state.history.undo.is_empty(),
        "Ctrl+Z must see the picker save before PATCH returns"
    );
    let live = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(parse_iso_date(live.start_date.as_deref()), Some(start));
    assert_eq!(parse_iso_date(live.target_date.as_deref()), Some(target));
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist picker dates");
}

#[tokio::test]
async fn toast_undo_after_picker_dates_restores_them() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    let before_start = node.start_date.clone();
    let before_target = node.target_date.clone();
    let start = Date::new(2026, 8, 4).unwrap();
    let target = Date::new(2026, 8, 21).unwrap();
    let pending = state
        .commit_range_dates_now(node.id, Some(start), Some(target))
        .expect("picker save");
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist picker dates");
    let action = state
        .toasts
        .items
        .iter()
        .rev()
        .find_map(|toast| toast.undo.clone())
        .expect("picker save must offer Undo");
    let pending = state
        .toast_restore_or_undo_dates(action)
        .expect("toast undo");
    assert_eq!(pending.role, DateSaveRole::Undo);
    let live = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(live.start_date, before_start);
    assert_eq!(live.target_date, before_target);
    let message = state.toasts.last_message().unwrap_or_default();
    assert!(
        message.starts_with("Undone moved"),
        "toast undo must describe the restored dates, got {message}"
    );
    assert!(
        !message.contains("Aug\u{00a0}4") && !message.contains("Aug 4"),
        "undo toast must not keep the forward dates, got {message}"
    );
}

#[tokio::test]
async fn scheduling_an_unscheduled_item_says_scheduled() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_none() && n.target_date.is_none())
        .cloned()
        .expect("unscheduled item");
    let start = Date::new(2026, 11, 10).unwrap();
    let target = Date::new(2026, 11, 12).unwrap();
    let pending = state
        .commit_range_dates_now(node.id, Some(start), Some(target))
        .expect("schedule");
    state
        .persist_pending_dates(pending)
        .await
        .expect("persist schedule");
    let message = state.toasts.last_message().unwrap_or_default();
    assert!(
        message.starts_with("Scheduled ") && message.contains('→'),
        "scheduling an unscheduled item must say Scheduled, got {message}"
    );
    let action = state
        .toasts
        .items
        .iter()
        .rev()
        .find_map(|toast| toast.undo.clone())
        .expect("schedule toast undo");
    let _ = state
        .toast_restore_or_undo_dates(action)
        .expect("undo schedule");
    let undone = state.toasts.last_message().unwrap_or_default();
    assert!(
        undone.to_lowercase().starts_with("undone scheduled"),
        "undoing a schedule must not say Cleared dates, got {undone}"
    );
    assert!(
        !undone.to_lowercase().contains("cleared dates"),
        "undoing a schedule must not say Cleared dates, got {undone}"
    );
}

#[tokio::test]
async fn picker_dates_do_not_jump_the_camera() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scroll_x = 120.0;
    let origin = state.gantt.origin;
    let scroll = state.gantt.scroll_x;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some())
        .cloned()
        .unwrap();
    let _ = state.commit_range_dates_now(
        node.id,
        Some(Date::new(2026, 11, 10).unwrap()),
        Some(Date::new(2026, 11, 12).unwrap()),
    );
    assert_eq!(
        state.gantt.origin, origin,
        "scheduling from the picker must not move the origin"
    );
    assert_eq!(
        state.gantt.scroll_x, scroll,
        "scheduling from the picker must not pan the camera"
    );
}

#[tokio::test]
async fn failed_toast_restore_keeps_undo_history() {
    let (mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields && n.start_date.is_some() && n.target_date.is_some())
        .cloned()
        .unwrap();
    state.clear_dates(node.id).await.expect("clear");
    let undo = state
        .toasts
        .items
        .iter()
        .rev()
        .find_map(|t| t.undo.clone())
        .expect("toast undo");
    let history_len = state.history.undo.len();
    mock.fail_next(&format!("/workitems/{}", node.id), 503, "save failed");
    let pending = state.apply_toast_restore_dates(undo).expect("restore");
    assert_eq!(state.history.undo.len(), history_len);
    let _ = state.persist_pending_dates(pending).await;
    let live = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert!(
        live.start_date.is_none() && live.target_date.is_none(),
        "a failed restore persist must roll dates back to the cleared node"
    );
    assert_eq!(
        state.history.undo.len(),
        history_len,
        "a failed restore persist must keep the Dates undo entry"
    );
    assert!(
        state.toasts.items.iter().any(|toast| toast.undo.is_some()),
        "a failed restore must re-offer toast Undo"
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
async fn schedule_span_uses_the_dragged_range() {
    let (_mock, mut state, _dir) = signed_in().await;
    let node = state
        .nodes
        .iter()
        .find(|n| n.has_date_fields)
        .cloned()
        .unwrap();
    state.clear_dates(node.id).await.expect("clear");
    let start = Date::new(2026, 8, 3).unwrap();
    let target = Date::new(2026, 8, 12).unwrap();
    state
        .schedule_span(node.id, start, target)
        .await
        .expect("schedule span");
    let updated = state.nodes.iter().find(|n| n.id == node.id).unwrap();
    assert_eq!(
        parse_iso_date(updated.start_date.as_deref()),
        Some(start),
        "drag start must become Start Date"
    );
    assert_eq!(
        parse_iso_date(updated.target_date.as_deref()),
        Some(target),
        "drag end must become Target Date, not a default-length bar"
    );
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
}

#[tokio::test]
async fn switching_teams_through_a_spawn_clone_restores_scroll() {
    let (_mock, mut state, _dir) = signed_in().await;
    let home = state.prefs.planner.team.clone();
    state.gantt.viewport_h = 120.0;
    state.gantt.scroll_y = 80.0;
    state.gantt.scroll_x = 48.0;
    state.begin_team_switch("Growth".into());
    let mut working = state.clone();
    working
        .set_team("Growth".into())
        .await
        .expect("switch to Growth");
    state.adopt_spawned(working);
    state.begin_team_switch(home.clone());
    let mut working = state.clone();
    working.set_team(home).await.expect("switch home");
    state.adopt_spawned(working);
    assert_eq!(state.gantt.scroll_y, 80.0);
}

#[tokio::test]
async fn drag_save_then_team_switch_refetches_grid_dates() {
    let (_mock, mut state, _dir) = signed_in().await;
    let id = 1012;
    let original = state
        .nodes
        .iter()
        .find(|node| node.id == id)
        .cloned()
        .expect("1012");
    let step = state.gantt.scale.pixels_per_day(state.gantt.zoom);
    let _ = state.begin_bar_gesture(id, GestureKind::Move, 0.0);
    state.update_bar_gesture(step * 8.0);
    let pending = state.commit_date_gesture_now().expect("drag");
    let mut working = state.clone();
    working
        .persist_pending_dates(pending)
        .await
        .expect("persist drag");
    if let Some(rev) = working
        .nodes
        .iter()
        .find(|node| node.id == id)
        .map(|node| node.rev)
        && let Some(live) = state.nodes.iter_mut().find(|node| node.id == id)
    {
        live.rev = rev;
    }
    let saved = state
        .nodes
        .iter()
        .find(|node| node.id == id)
        .cloned()
        .expect("saved 1012");
    assert_ne!(
        saved.target_date, original.target_date,
        "drag must move Target Date off the fixture 08-04"
    );

    state.begin_team_switch("Empty".into());
    let mut working = state.clone();
    working.set_team("Empty".into()).await.expect("Empty");
    state.adopt_spawned(working);
    state.begin_team_switch("Platform".into());
    let mut working = state.clone();
    working.set_team("Platform".into()).await.expect("Platform");
    state.adopt_spawned(working);

    let grid = state
        .nodes
        .iter()
        .find(|node| node.id == id)
        .cloned()
        .expect("1012 after switch");
    assert_eq!(
        grid.start_date, saved.start_date,
        "grid Start Date must match the saved server dates after Platform→Empty→Platform"
    );
    assert_eq!(
        grid.target_date, saved.target_date,
        "grid Target Date must not snap back to the cached 08-04"
    );
    let grid_start = grid.start_date.as_deref().and_then(|value| value.get(..10));
    let grid_target = grid
        .target_date
        .as_deref()
        .and_then(|value| value.get(..10));

    state.open_form(id).await.expect("details");
    let form = state.form.as_ref().expect("form");
    let form_start = form
        .values
        .get(START_DATE_FIELD)
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.get(..10));
    let form_target = form
        .values
        .get(TARGET_DATE_FIELD)
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.get(..10));
    assert_eq!(form_start, grid_start);
    assert_eq!(form_target, grid_target);
}

#[tokio::test]
async fn undo_dates_pushes_a_named_toast() {
    let (_mock, mut state, _dir) = signed_in().await;
    let id = 1012;
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == id) {
        node.title = "Cart UI".into();
    }
    let step = state.gantt.scale.pixels_per_day(state.gantt.zoom);
    let _ = state.begin_bar_gesture(id, GestureKind::Move, 0.0);
    state.update_bar_gesture(step * 2.0);
    let pending = state.commit_date_gesture_now().expect("drag");
    state.persist_pending_dates(pending).await.expect("persist");
    let _ = state.undo_dates_now().expect("undo");
    let undid = state.toasts.last_message().unwrap_or_default();
    assert!(
        undid.starts_with("Undone moved \"Cart UI\""),
        "undo must name the restored dates, got {undid}"
    );
    let _ = state.redo_dates_now().expect("redo");
    let redid = state.toasts.last_message().unwrap_or_default();
    assert!(
        redid.starts_with("Redid: Moved \"Cart UI\""),
        "redo must repeat the original action, got {redid}"
    );
}

#[test]
fn assignee_search_matches_display_name() {
    let options = vec![
        ("anyone".into(), "Anyone".into()),
        ("alan@contoso.com".into(), "Alan Turing".into()),
        ("ada@contoso.com".into(), "Ada Lovelace".into()),
    ];
    let hits = PlannerState::filter_scope_options(&options, "Alan");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].1, "Alan Turing");
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
async fn switching_to_growth_keeps_the_camera() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.gantt.timeline_width = 800.0;
    let origin = Date::new(2026, 9, 1).unwrap();
    state.gantt.origin = origin;
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 0.6;
    state.begin_team_switch("Growth".into());
    state
        .set_team("Growth".into())
        .await
        .expect("switch to Growth");
    assert!(
        (state.gantt.zoom - 0.6).abs() < 0.01,
        "team switch must not change zoom unasked, got {}",
        state.gantt.zoom
    );
    assert_eq!(
        state.gantt.scale,
        GanttScale::Month,
        "team switch must not change the scale unasked"
    );
    assert_eq!(
        state.gantt.origin, origin,
        "team switch must not pan the range unasked"
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
async fn load_camera_ignores_hidden_scheduled_items() {
    let (_mock, mut state, _dir) = signed_in().await;
    let hidden: Vec<String> = state
        .types()
        .into_iter()
        .filter(|ty| ty != "Task")
        .collect();
    state.set_hidden_roots(hidden);
    let scheduled: Vec<i64> = state
        .visible_nodes()
        .iter()
        .filter(|node| node.start_date.is_some() || node.target_date.is_some())
        .map(|node| node.id)
        .collect();
    for id in scheduled {
        state.clear_dates(id).await.expect("clear visible dates");
    }
    assert!(
        state
            .visible_nodes()
            .iter()
            .all(|node| node.start_date.is_none() && node.target_date.is_none()),
        "visible rows must be unscheduled diamonds"
    );
    assert!(
        state.nodes.iter().any(|node| node.start_date.is_some()),
        "hidden scheduled items must still be in the full node list"
    );
    state.gantt.origin = Date::new(2025, 1, 1).unwrap();
    state.gantt.timeline_width = 800.0;
    state.load_hierarchy().await.expect("reload");
    let today = jiff::Zoned::now().date();
    assert_eq!(
        state.gantt.origin, today,
        "load must not Auto Fit to hidden July bars, origin={}",
        state.gantt.origin
    );
    let visible = state.visible_nodes();
    let rows = state.gantt.flatten(&visible);
    let bars = state
        .gantt
        .layout_bars(&visible, &rows, &state.iterations, 720.0);
    assert!(
        bars.iter()
            .any(|bar| bar.marker && bar.x < 800.0 && bar.x + bar.width > 0.0),
        "filtered load must keep Today diamonds on screen, origin={} bars={bars:?}",
        state.gantt.origin
    );
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
async fn toast_restore_link_action_puts_the_predecessor_back() {
    let (_mock, mut state, _dir) = signed_in().await;
    state
        .add_dependency(1005, 1003)
        .await
        .expect("link 1003 → 1005");
    let added = state.toasts.last_message().unwrap_or_default();
    assert!(
        added.starts_with("Added link:")
            && added.contains('→')
            && added.contains('"')
            && !added.contains('#'),
        "link toasts must use 'Added link: \"A\" → \"B\"', got {added}"
    );
    state
        .remove_dependency(1005, 1003)
        .await
        .expect("remove link");
    let removed = state.toasts.last_message().unwrap_or_default();
    assert!(
        removed.starts_with("Removed link:")
            && removed.contains('→')
            && removed.contains('"')
            && !removed.contains('#'),
        "link toasts must quote item titles and use →, got {removed}"
    );
    let action = state
        .toasts
        .items
        .iter()
        .rev()
        .find_map(|toast| toast.undo.clone())
        .expect("Removed link toast must offer Undo");
    assert!(
        matches!(
            action,
            UndoAction::RestoreLink {
                successor_id: 1005,
                predecessor_id: 1003
            }
        ),
        "toast Undo must restore the dropped link, got {action:?}"
    );
    state.apply_undo(action).await.expect("toast undo");
    assert!(
        state
            .nodes
            .iter()
            .find(|node| node.id == 1005)
            .unwrap()
            .predecessors
            .contains(&1003),
        "toast Undo must put predecessor #1003 back on #1005"
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
    assert!(
        message.starts_with("Moved "),
        "end-only save must name the item, got {message}"
    );
    assert!(
        state
            .toasts
            .items
            .last()
            .is_some_and(|toast| toast.undo.is_some()),
        "date save toasts must offer Undo"
    );
}

#[tokio::test]
async fn add_item_creates_a_work_item_on_the_team() {
    let (_mock, mut state, _dir) = signed_in().await;
    let before = state.nodes.len();
    state.add_item().await.expect("create");
    assert_eq!(state.nodes.len(), before + 1);
    let created = created_item(&state);
    assert_eq!(created.title, "New work item");
    assert_eq!(
        created.work_item_type, "Epic",
        "a root Add item should be an Epic when that type is available, got {}",
        created.work_item_type
    );
    assert_eq!(state.gantt.selected_id, Some(created.id));
    let rows = state.gantt.flatten(&state.nodes);
    assert_eq!(
        rows.last().map(|row| row.id),
        Some(created.id),
        "a new root must appear at the end of the list, not the top"
    );
    let message = state.toasts.last_message().unwrap_or_default();
    assert!(
        message.contains("Created Epic")
            && message.contains(&format!("\"{}\"", created.title))
            && message.contains("at the root"),
        "create toast must name the type, quoted title, and placement, got {message}"
    );
    assert!(
        state
            .toasts
            .items
            .last()
            .is_some_and(|toast| toast.undo.is_some()),
        "create toast must offer Undo"
    );
}

#[tokio::test]
async fn add_item_keeps_the_selected_parent_after_reload() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.gantt.selected_id = Some(1005);
    state.add_item().await.expect("create");
    let created = created_item(&state);
    assert_eq!(
        created.parent_id,
        Some(1005),
        "new items must nest under the selected work item"
    );
    state.load_hierarchy().await.expect("reload");
    let reloaded = state
        .nodes
        .iter()
        .find(|node| node.id == created.id)
        .expect("created item survives reload");
    assert_eq!(
        reloaded.parent_id,
        Some(1005),
        "Hierarchy-Reverse must persist System.Parent after refetch"
    );
}

#[tokio::test]
async fn undo_created_item_deletes_it() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.add_item().await.expect("create");
    let created = created_item(&state);
    let undo = state
        .toasts
        .items
        .last()
        .and_then(|toast| toast.undo.clone())
        .expect("create undo");
    state.apply_undo(undo).await.expect("delete created");
    assert!(
        state.nodes.iter().all(|node| node.id != created.id),
        "Undo on Created must delete the new work item"
    );
}

#[tokio::test]
async fn ctrl_z_undoes_create_from_the_history_stack() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.add_item().await.expect("create");
    let created = created_item(&state);
    assert!(
        matches!(
            state.history.undo.last(),
            Some(HistoryEntry::Created { id, .. }) if *id == created.id
        ),
        "create must land on the Ctrl+Z stack, not only the toast"
    );
    state.undo().await.expect("undo create");
    assert!(
        state.nodes.iter().all(|node| node.id != created.id),
        "Ctrl+Z must delete the created work item after the toast expires"
    );
}

#[tokio::test]
async fn failed_delete_surfaces_an_error_toast() {
    let (mock, mut state, _dir) = signed_in().await;
    let id = state.nodes[0].id;
    mock.fail_next(&format!("/workitems/{id}"), 503, "delete failed");
    let err = state.delete_item(id).await;
    assert!(err.is_err(), "DELETE 503 must fail the action");
    assert!(
        state.nodes.iter().any(|node| node.id == id),
        "a failed delete must leave the work item in the list"
    );
    assert!(
        state
            .toasts
            .last_error_message()
            .is_some_and(|message| !message.is_empty()),
        "a failed delete must toast an error instead of failing silently"
    );
}

#[tokio::test]
async fn delete_item_dismisses_create_undo_toast() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.add_item().await.expect("create");
    let created = created_item(&state);
    assert!(
        state.toasts.items.iter().any(|toast| {
            matches!(
                toast.undo,
                Some(UndoAction::DeleteCreated { id, .. }) if id == created.id
            )
        }),
        "creating a work item must offer Undo"
    );
    state.delete_item(created.id).await.expect("delete");
    assert!(
        state.toasts.items.iter().all(|toast| {
            !matches!(
                toast.undo,
                Some(UndoAction::DeleteCreated { id, .. }) if id == created.id
            )
        }),
        "deleting the item must drop its create-Undo toast"
    );
    assert!(
        state.nodes.iter().all(|node| node.id != created.id),
        "the deleted work item must leave the tree"
    );
}

#[tokio::test]
async fn undo_create_drops_date_history_for_that_item() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.add_item().await.expect("create");
    let created = created_item(&state);
    state.history.undo.push(HistoryEntry::Dates {
        id: created.id,
        before_start: None,
        before_target: None,
        after_start: Some("2026-07-06".into()),
        after_target: Some("2026-07-20".into()),
    });
    state.history.redo.push(HistoryEntry::Dates {
        id: created.id,
        before_start: None,
        before_target: None,
        after_start: Some("2026-07-06".into()),
        after_target: Some("2026-07-20".into()),
    });
    let undo = UndoAction::DeleteCreated {
        id: created.id,
        title: created.title.clone(),
        work_item_type: created.work_item_type.clone(),
        placement: "at the root".into(),
    };
    state.apply_undo(undo).await.expect("delete created");
    let leftover = |entry: &HistoryEntry| match entry {
        HistoryEntry::Created { id, .. }
        | HistoryEntry::Dates { id, .. }
        | HistoryEntry::Fields { id, .. } => *id == created.id,
        HistoryEntry::Link {
            successor_id,
            predecessor_id,
            ..
        } => *successor_id == created.id || *predecessor_id == created.id,
    };
    assert!(
        state.history.undo.iter().all(|entry| !leftover(entry)),
        "undoing a create must drop date/link history for that id"
    );
    assert!(
        state.history.redo.iter().all(|entry| !leftover(entry)),
        "undoing a create must drop redo history for that id"
    );
}

#[tokio::test]
async fn add_item_on_a_user_story_creates_a_child_task() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.gantt.selected_id = Some(1003);
    state.add_item().await.expect("create");
    let created = created_item(&state);
    assert_eq!(
        created.parent_id,
        Some(1003),
        "new items must nest under the selected User Story"
    );
    assert_eq!(
        created.work_item_type, "Task",
        "a child of a User Story must be a Task"
    );
    let rows: Vec<i64> = state
        .gantt
        .flatten(&state.nodes)
        .into_iter()
        .map(|row| row.id)
        .collect();
    let parent_at = rows.iter().position(|id| *id == 1003).expect("story");
    let created_at = rows
        .iter()
        .position(|id| *id == created.id)
        .expect("created child");
    assert!(
        created_at > parent_at,
        "a new child must appear under its parent, got {rows:?}"
    );
    let message = state.toasts.last_message().unwrap_or_default();
    assert!(
        message.contains("Created Task")
            && message.contains(&format!("\"{}\"", created.title))
            && message.contains("under"),
        "create toast must state type and placement, got {message}"
    );
}

#[tokio::test]
async fn add_item_on_an_epic_creates_a_child_feature() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.gantt.selected_id = Some(1001);
    state.add_item().await.expect("create");
    let created = created_item(&state);
    assert_eq!(created.parent_id, Some(1001));
    assert_eq!(created.work_item_type, "Feature");
    let rows: Vec<i64> = state
        .gantt
        .flatten(&state.nodes)
        .into_iter()
        .map(|row| row.id)
        .collect();
    let epic_at = rows.iter().position(|id| *id == 1001).expect("epic");
    let created_at = rows
        .iter()
        .position(|id| *id == created.id)
        .expect("created child");
    assert!(
        created_at > epic_at,
        "a child of Epic 1001 must land under it, got {rows:?}"
    );
    let visible_rows = state.gantt.flatten(&state.visible_nodes());
    let created_row = visible_rows
        .iter()
        .find(|row| row.id == created.id)
        .expect("created child is visible");
    let sibling = visible_rows
        .iter()
        .find(|row| row.id == 1013)
        .expect("existing Feature under the Epic");
    assert!(
        created_row.depth > 0,
        "a child of an Epic must not appear at root, got {created_row:?}"
    );
    assert_eq!(
        created_row.depth, sibling.depth,
        "new Feature must share tree depth with Feature #1013, got {created_row:?} vs {sibling:?}"
    );
    let visible_ids: Vec<i64> = visible_rows.iter().map(|row| row.id).collect();
    let sibling_at = visible_ids.iter().position(|id| *id == 1013).expect("1013");
    let created_at_vis = visible_ids
        .iter()
        .position(|id| *id == created.id)
        .expect("created");
    let root_at = visible_ids.iter().position(|id| *id == 1012);
    assert!(
        created_at_vis > sibling_at,
        "new Feature must paint after the Epic's last existing child, got {visible_ids:?}"
    );
    if let Some(root_at) = root_at {
        assert!(
            created_at_vis < root_at,
            "new Feature must stay under the Epic, before the next root, got {visible_ids:?}"
        );
    }
    assert_eq!(
        state
            .visible_nodes()
            .iter()
            .find(|node| node.id == created.id)
            .map(|node| node.parent_id),
        Some(Some(1001)),
        "visible tree must keep the Epic parent, not strip it to root"
    );
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
    assert!(
        matches!(
            state.history.undo.last(),
            Some(HistoryEntry::Fields { id: 1003, .. })
        ),
        "title edits must land on the undo stack, got {:?}",
        state.history.undo.last()
    );
    assert!(
        state
            .toasts
            .items
            .last()
            .is_some_and(|toast| toast.undo.is_some()),
        "Saved title must offer Undo"
    );
    state.undo().await.expect("undo title");
    let message = state.toasts.last_message().unwrap_or_default();
    assert!(
        message.starts_with("Undone renamed"),
        "field undo toast must repeat the rename, got {message}"
    );
    let node = state.nodes.iter().find(|n| n.id == 1003).unwrap();
    assert_ne!(
        node.title, "Inline title",
        "Ctrl+Z must restore the previous title"
    );
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

fn created_item(state: &PlannerState) -> ado_core::WorkItemNode {
    let id = state.gantt.selected_id.expect("selected created item");
    state
        .nodes
        .iter()
        .find(|node| node.id == id)
        .cloned()
        .expect("created")
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
