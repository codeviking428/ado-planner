mod support;

use ado_core::{START_DATE_FIELD, TARGET_DATE_FIELD};
use ado_planner::gantt::ContextTarget;
use ado_planner::paths::AppPaths;
use ado_planner::state::DateSaveRole;
use ado_planner::state::PlannerState;
use ado_planner::toasts::{HistoryEntry, UndoAction};
use ado_planner::ui::PlannerView;
use ado_session::{EncryptedStore, MemoryStore, PlannerSession};
use gpui_kit::{Entity, VisualTestContext};
use support::{MockApp, click, date_only, node_dates, right_click, settle};

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

const PERSIST_CART: i64 = 1003;

fn planner<'a>(
    cx: &'a mut gpui_kit::TestAppContext,
    app: &MockApp,
) -> (Entity<PlannerView>, &'a mut VisualTestContext) {
    support::open(cx, app.signed_in())
}

fn menu_is_closed(view: &Entity<PlannerView>, cx: &mut VisualTestContext) {
    view.read_with(cx, |view, _| {
        assert!(
            view.state.gantt.context_menu.is_none(),
            "choosing an action must close the menu"
        );
    });
    assert!(cx.debug_bounds("gantt-context-menu").is_none());
}

#[gpui_kit::test]
fn bar_menu_open_shows_the_work_item_form(cx: &mut gpui_kit::TestAppContext) {
    let app = MockApp::start();
    let (view, cx) = planner(cx, &app);
    right_click(cx, "gantt-bar-1003");
    click(cx, "gantt-menu-Open");
    settle(&view, cx, "the form to load", |view| {
        view.state.form.is_some() && !view.form_is_loading()
    });
    menu_is_closed(&view, cx);
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.state.form.as_ref().map(|form| form.model.id),
            Some(PERSIST_CART)
        );
    });
    assert!(cx.debug_bounds("work-item-form-dialog").is_some());
}

#[gpui_kit::test]
fn bar_menu_open_in_azure_devops_opens_the_edit_url(cx: &mut gpui_kit::TestAppContext) {
    let app = MockApp::start();
    let (view, cx) = planner(cx, &app);
    right_click(cx, "gantt-bar-1003");
    click(cx, "gantt-menu-Open in Azure DevOps");
    menu_is_closed(&view, cx);
    let url = cx
        .opened_url()
        .expect("Open in Azure DevOps must open a URL");
    assert!(
        url.contains("/contoso/Shop/_workitems/edit/1003"),
        "unexpected URL {url}"
    );
}

#[gpui_kit::test]
fn bar_menu_copy_id_and_copy_link_write_the_clipboard(cx: &mut gpui_kit::TestAppContext) {
    let app = MockApp::start();
    let (view, cx) = planner(cx, &app);
    right_click(cx, "gantt-bar-1003");
    click(cx, "gantt-menu-Copy ID");
    menu_is_closed(&view, cx);
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some("1003"));
    view.read_with(cx, |view, _| {
        assert_eq!(view.state.toasts.last_message(), Some("Copied #1003"));
    });

    right_click(cx, "gantt-bar-1003");
    click(cx, "gantt-menu-Copy link");
    menu_is_closed(&view, cx);
    let link = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap_or_default();
    assert!(
        link.contains("/contoso/Shop/_workitems/edit/1003"),
        "{link}"
    );
    view.read_with(cx, |view, _| {
        assert_eq!(view.state.toasts.last_message(), Some("Copied link"));
    });
}

#[gpui_kit::test]
fn bar_menu_clear_dates_then_toast_undo_restores_them(cx: &mut gpui_kit::TestAppContext) {
    let app = MockApp::start();
    let (view, cx) = planner(cx, &app);
    let original = view.read_with(cx, |view, _| node_dates(view, PERSIST_CART));
    right_click(cx, "gantt-bar-1003");
    click(cx, "gantt-menu-Clear dates");
    settle(&view, cx, "Clear dates", |view| {
        node_dates(view, PERSIST_CART) == (None, None)
    });
    menu_is_closed(&view, cx);
    assert_eq!(app.mock_field(PERSIST_CART, START_DATE_FIELD), None);
    assert_eq!(app.mock_field(PERSIST_CART, TARGET_DATE_FIELD), None);

    click(cx, "toast-undo");
    settle(&view, cx, "the Clear dates undo", |view| {
        node_dates(view, PERSIST_CART) == original
    });
    assert_eq!(
        date_only(app.mock_field(PERSIST_CART, START_DATE_FIELD).as_deref()),
        Some("2026-07-06"),
        "Undo must write Start back to Azure DevOps"
    );
    assert_eq!(
        date_only(app.mock_field(PERSIST_CART, TARGET_DATE_FIELD).as_deref()),
        Some("2026-07-20")
    );
    assert!(cx.debug_bounds("gantt-bar-1003").is_some());
}

#[gpui_kit::test]
fn bar_menu_schedule_to_iteration_uses_the_sprint_dates(cx: &mut gpui_kit::TestAppContext) {
    let app = MockApp::start();
    let (view, cx) = planner(cx, &app);
    right_click(cx, "gantt-bar-1003");
    click(cx, "gantt-menu-Schedule to iteration");
    settle(&view, cx, "Schedule to iteration", |view| {
        let (start, _) = node_dates(view, PERSIST_CART);
        date_only(start.as_deref()) == Some("2026-07-13")
    });
    menu_is_closed(&view, cx);
    assert_eq!(
        date_only(app.mock_field(PERSIST_CART, START_DATE_FIELD).as_deref()),
        Some("2026-07-13"),
        "Sprint 10 starts Jul 13"
    );
    assert_eq!(
        date_only(app.mock_field(PERSIST_CART, TARGET_DATE_FIELD).as_deref()),
        Some("2026-07-24"),
        "Sprint 10 finishes Jul 24"
    );
}

#[gpui_kit::test]
fn row_menu_collapse_all_then_expand_all_below_this_row(cx: &mut gpui_kit::TestAppContext) {
    let app = MockApp::start();
    let (_view, cx) = planner(cx, &app);
    for id in [
        "gantt-tree-row-1002",
        "gantt-tree-row-1003",
        "gantt-tree-row-1005",
    ] {
        assert!(cx.debug_bounds(id).is_some(), "{id} starts expanded");
    }
    right_click(cx, "gantt-tree-row-1001");
    click(cx, "gantt-menu-Collapse all");
    assert!(cx.debug_bounds("gantt-tree-row-1001").is_some());
    for id in [
        "gantt-tree-row-1002",
        "gantt-tree-row-1003",
        "gantt-tree-row-1005",
    ] {
        assert!(
            cx.debug_bounds(id).is_none(),
            "{id} must collapse under the Epic"
        );
    }
    assert!(
        cx.debug_bounds("gantt-tree-row-1012").is_some(),
        "rows outside the subtree stay put"
    );
    right_click(cx, "gantt-tree-row-1001");
    click(cx, "gantt-menu-Expand all");
    for id in [
        "gantt-tree-row-1002",
        "gantt-tree-row-1003",
        "gantt-tree-row-1005",
    ] {
        assert!(cx.debug_bounds(id).is_some(), "{id} must expand again");
    }
}

#[gpui_kit::test]
fn row_menu_hide_this_type_writes_the_types_filter(cx: &mut gpui_kit::TestAppContext) {
    let app = MockApp::start();
    let (view, cx) = planner(cx, &app);
    right_click(cx, "gantt-tree-row-1005");
    click(cx, "gantt-menu-Hide this type");
    menu_is_closed(&view, cx);
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.state.prefs.planner.hidden_types,
            vec!["Task".to_string()]
        );
    });
    assert!(cx.debug_bounds("gantt-tree-row-1005").is_none());
    assert!(cx.debug_bounds("gantt-tree-row-1012").is_none());
    assert!(cx.debug_bounds("gantt-tree-row-1003").is_some());
    assert!(
        cx.debug_bounds("filter-caption-Types: 1 hidden").is_some(),
        "the Types button must count the hidden type"
    );
}

#[gpui_kit::test]
fn row_menu_hide_this_state_writes_the_states_filter(cx: &mut gpui_kit::TestAppContext) {
    let app = MockApp::start();
    let (view, cx) = planner(cx, &app);
    right_click(cx, "gantt-tree-row-1005");
    click(cx, "gantt-menu-Hide this state");
    menu_is_closed(&view, cx);
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.state.prefs.planner.hidden_states,
            vec!["New".to_string()]
        );
    });
    assert!(
        cx.debug_bounds("gantt-tree-row-1005").is_none(),
        "Cart UI is New"
    );
    assert!(
        cx.debug_bounds("gantt-tree-row-1003").is_some(),
        "Persist cart is Active"
    );
    assert!(cx.debug_bounds("filter-caption-States: 1 hidden").is_some());
}

#[gpui_kit::test]
fn row_menu_fits_inside_the_window_near_the_bottom(cx: &mut gpui_kit::TestAppContext) {
    let app = MockApp::start();
    let (_view, cx) = planner(cx, &app);
    right_click(cx, "gantt-tree-row-1014");
    let menu = support::bounds(cx, "gantt-context-menu");
    let last = support::bounds(cx, "gantt-menu-Hide this state");
    let bottom = f32::from(menu.origin.y + menu.size.height);
    assert!(
        bottom <= 800.0,
        "a menu opened low in an 800px window must move up to fit, bottom={bottom}"
    );
    assert!(f32::from(last.origin.y + last.size.height) <= 800.0);
}

#[gpui_kit::test]
fn bar_menu_has_no_hide_filters(cx: &mut gpui_kit::TestAppContext) {
    let app = MockApp::start();
    let (_view, cx) = planner(cx, &app);
    right_click(cx, "gantt-bar-1003");
    assert!(cx.debug_bounds("gantt-menu-Open").is_some());
    assert!(cx.debug_bounds("gantt-menu-Hide this type").is_none());
    assert!(cx.debug_bounds("gantt-menu-Hide this state").is_none());
}

#[gpui_kit::test]
fn date_actions_on_an_item_without_date_fields_toast_and_skip_the_patch(
    cx: &mut gpui_kit::TestAppContext,
) {
    let app = MockApp::start();
    let mut state = app.signed_in();
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == PERSIST_CART) {
        node.has_date_fields = false;
    }
    let (view, cx) = support::open(cx, state);
    let before = app.mock.patch_count();
    for action in ["gantt-menu-Clear dates", "gantt-menu-Schedule to iteration"] {
        right_click(cx, "gantt-tree-row-1003");
        click(cx, action);
        settle(&view, cx, action, |_| true);
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.state.toasts.last_error_message(),
                Some("This Work Item has no Start Date and Target Date fields."),
                "{action} must use the drag error toast"
            );
        });
    }
    assert_eq!(app.mock.patch_count(), before);
}
