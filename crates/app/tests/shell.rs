//! Ports sign-in, scope, filter and persistence cases from `e2e/app.spec.ts`.

use ado_planner::paths::AppPaths;
use ado_planner::state::PlannerState;
use ado_session::{EncryptedStore, MemoryStore, PlannerSession};
use gpui_kit::AppContext as _;

async fn signed_in_state() -> (ado_mock::AdoMock, PlannerState, tempfile::TempDir) {
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
async fn seeded_session_shows_signed_in_chrome_and_scope_picker() {
    let (_mock, state, _dir) = signed_in_state().await;
    assert!(state.session_info.signed_in);
    assert_eq!(
        state.session_info.display_name.as_deref(),
        Some("Ada Lovelace")
    );
    assert_eq!(state.prefs.planner.org, "contoso");
    assert_eq!(state.prefs.planner.project, "Shop");
    assert_eq!(state.prefs.planner.team, "Platform");
    assert!(state.prefs.planner.iteration_path.is_empty());
    assert_eq!(state.prefs.planner.assignee, "anyone");
}

#[tokio::test]
async fn scope_controls_lock_while_switching_organizations() {
    let (mock, mut state, _dir) = signed_in_state().await;
    assert!(state.visible_nodes().iter().any(|n| n.id == 1001));
    mock.delay_next("/fabrikam/_apis/projects", 50);
    state.scope_locked = true;
    state.set_org("fabrikam".into()).await.expect("switch org");
    assert!(!state.scope_locked);
    assert_eq!(state.prefs.planner.project, "Roadmap");
    assert_eq!(state.prefs.planner.team, "Delivery");
}

#[tokio::test]
async fn scope_errors_show_a_copyable_toast() {
    let (mock, mut state, _dir) = signed_in_state().await;
    mock.fail_next("/fabrikam/_apis/projects", 503, "Projects unavailable");
    let _ = state.set_org("fabrikam".into()).await;
    let message = state
        .toasts
        .last_error_message()
        .unwrap_or_default()
        .to_string();
    assert!(message.contains("Projects unavailable") || message.contains("503"));
    let id = state.toasts.items.last().unwrap().id;
    assert!(state.toasts.items.last().unwrap().copyable);
    state.copy_toast(id);
    assert_eq!(state.copied.as_deref(), Some(message.as_str()));
}

#[tokio::test]
async fn loads_hierarchy_including_an_unparented_root() {
    let (_mock, state, _dir) = signed_in_state().await;
    let visible = state.visible_nodes();
    assert!(visible.iter().any(|n| n.id == 1001));
    assert!(
        visible
            .iter()
            .any(|n| n.id == 1003 && n.title.contains("Persist cart"))
    );
    assert!(!visible.iter().any(|n| n.id == 1012));
}

#[tokio::test]
async fn roots_hide_unparented_work_items_that_are_not_a_root_type() {
    let (_mock, mut state, _dir) = signed_in_state().await;
    assert!(!state.visible_nodes().iter().any(|n| n.id == 1012));
    assert!(state.hidden_root_count() >= 1);
    state.show_all_roots();
    assert!(
        state
            .visible_nodes()
            .iter()
            .any(|n| n.id == 1012 && n.title.contains("Unparented spike"))
    );
    state.hide_all_roots();
    assert!(state.visible_nodes().is_empty());
}

#[tokio::test]
async fn assignee_menu_lists_team_members_and_filters() {
    let (_mock, mut state, _dir) = signed_in_state().await;
    let options = state.assignee_options();
    assert!(options.iter().any(|(_, l)| l == "Ada Lovelace"));
    assert!(options.iter().any(|(_, l)| l == "Grace Hopper"));
    let grace = options
        .into_iter()
        .find(|(_, l)| l == "Grace Hopper")
        .unwrap()
        .0;
    state.set_assignee(grace);
    assert!(!state.visible_nodes().iter().any(|n| n.id == 1003));
    state.set_assignee("anyone".into());
    assert!(state.visible_nodes().iter().any(|n| n.id == 1003));
}

#[tokio::test]
async fn scope_team_search_filters_options() {
    let (_mock, state, _dir) = signed_in_state().await;
    let names: Vec<_> = state.teams.iter().map(|t| t.name.clone()).collect();
    let filtered = state.filter_options(&names, "Plat");
    assert_eq!(filtered, vec!["Platform".to_string()]);
}

#[tokio::test]
async fn scope_and_filters_persist_across_reload() {
    let (mock, mut state, dir) = signed_in_state().await;
    state.set_org("fabrikam".into()).await.expect("org");
    state.toggle_hidden_type("Task");
    assert!(state.prefs.planner.hidden_types.contains(&"Task".into()));
    drop(state);

    let paths = AppPaths::new(dir.path().to_path_buf());
    let store = EncryptedStore::new(Box::new(MemoryStore::new(true)), Some([3u8; 32]));
    let session = PlannerSession::pat(store).with_bases(&mock.url, &mock.url);
    let mut restored = PlannerState::new(
        paths,
        session,
        Some(mock.url.clone()),
        Some(mock.url.clone()),
    );
    restored.sign_in_org = "https://dev.azure.com/fabrikam".into();
    restored.sign_in_pat = "pat-secret-value".into();
    restored.login_pat().await.expect("login again");
    assert_eq!(restored.prefs.planner.org, "fabrikam");
    assert_eq!(restored.prefs.planner.project, "Roadmap");
    assert_eq!(restored.prefs.planner.team, "Delivery");
    assert!(restored.prefs.planner.hidden_types.contains(&"Task".into()));
}

#[tokio::test]
async fn type_filter_search_narrows_the_list() {
    let (_mock, state, _dir) = signed_in_state().await;
    let filtered = state.filter_options(&state.types(), "Task");
    assert!(filtered.iter().any(|t| t == "Task"));
    assert!(!filtered.iter().any(|t| t == "Epic"));
}

#[tokio::test]
async fn type_filter_hides_a_leaf_and_can_show_it_again() {
    let (_mock, mut state, _dir) = signed_in_state().await;
    assert!(state.visible_nodes().iter().any(|n| n.id == 1005));
    state.toggle_hidden_type("Task");
    assert!(!state.visible_nodes().iter().any(|n| n.id == 1005));
    state.toggle_hidden_type("Task");
    assert!(state.visible_nodes().iter().any(|n| n.id == 1005));
}

#[tokio::test]
async fn type_filter_hide_all_clears_the_hierarchy() {
    let (_mock, mut state, _dir) = signed_in_state().await;
    state.hide_all_types();
    assert!(state.visible_nodes().is_empty());
    state.show_all_types();
    assert!(state.visible_nodes().iter().any(|n| n.id == 1001));
}

#[gpui_kit::test]
fn gpui_sign_in_shell_renders(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(dir.path().to_path_buf());
    let store = EncryptedStore::new(Box::new(MemoryStore::new(false)), None);
    let session = PlannerSession::pat(store);
    cx.update(|cx| {
        gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| ado_planner::ui::PlannerView::new(window, cx, session, paths, None, None))
        })
        .expect("window");
    });
}
