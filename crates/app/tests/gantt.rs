use ado_core::parse_iso_date;
use ado_planner::gantt::{GanttScale, GanttView, GestureKind, NudgeAxis};
use ado_planner::paths::AppPaths;
use ado_planner::state::{PlannerState, fake_hierarchy};
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
    gantt.zoom_at(1.5, 40.0, 0.0);
    assert!(gantt.zoom > 1.0);
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
async fn save_title_patches_system_title() {
    let (_mock, mut state, _dir) = signed_in().await;
    state
        .save_title(1003, "Inline title".into())
        .await
        .expect("title");
    let node = state.nodes.iter().find(|n| n.id == 1003).unwrap();
    assert_eq!(node.title, "Inline title");
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
