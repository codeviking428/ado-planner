//! Ports sign-in, scope, filter and persistence cases from `e2e/app.spec.ts`.

use ado_planner::paths::AppPaths;
use ado_planner::state::PlannerState;
use ado_planner::ui::PlannerView;
use ado_session::{EncryptedStore, MemoryStore, OauthEndpoints, PlannerSession};
use gpui_kit::px;

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
async fn expired_pat_shows_a_sign_in_error_and_retry_clears_it() {
    let mock = ado_mock::start().await;
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(dir.path().to_path_buf());
    let store = EncryptedStore::new(Box::new(MemoryStore::new(false)), None);
    let session = PlannerSession::pat(store).with_bases(&mock.url, &mock.url);
    let mut state = PlannerState::new(
        paths,
        session,
        Some(mock.url.clone()),
        Some(mock.url.clone()),
    );
    state.sign_in_org = "https://dev.azure.com/contoso".into();
    state.sign_in_pat = "expired-pat-value".into();
    mock.fail_next(
        "/contoso/_apis/projects",
        401,
        "<html>TF400813: Personal access token has expired</html>",
    );
    assert!(state.login_pat().await.is_err());
    assert!(!state.session_info.signed_in);
    let inline = state.sign_in_error.clone().expect("inline sign-in error");
    assert!(inline.contains("invalid, expired"), "{inline}");
    assert!(!inline.contains("<html>") && !inline.contains("expired-pat-value"));

    state.sign_in_pat = "pat-secret-value".into();
    state.login_pat().await.expect("retry");
    assert!(state.session_info.signed_in);
    assert!(state.sign_in_error.is_none());
    assert!(
        state.toasts.last_error_message().is_none(),
        "a rejected attempt must not leave an error toast for the signed-in shell"
    );
    assert_eq!(mock.mutating_writes(), Vec::<(String, String)>::new());
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
    assert!(
        visible.iter().any(|n| n.id == 1012),
        "startup Roots default is every loaded type, including Task root #1012"
    );
}

#[tokio::test]
async fn roots_hide_unparented_work_items_that_are_not_a_root_type() {
    let (_mock, mut state, _dir) = signed_in_state().await;
    assert!(
        state.visible_nodes().iter().any(|n| n.id == 1012),
        "Task root #1012 is visible under the startup Roots default"
    );
    assert_eq!(state.hidden_root_count(), 0);
    state.toggle_hidden_root("Task");
    assert!(!state.visible_nodes().iter().any(|n| n.id == 1012));
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
async fn roots_count_is_the_selected_types_and_team_switch_matches_first_load() {
    let (_mock, mut state, _dir) = signed_in_state().await;
    let first_hidden = state.visible_nodes().iter().any(|n| n.id == 1012);
    let first_selected = state.selected_root_count();
    assert!(
        first_hidden,
        "first load should show every root type, including Task root #1012"
    );
    assert_eq!(
        first_selected,
        state.types().len(),
        "Roots selected count should be every loaded type, got {first_selected} of {}",
        state.types().len()
    );
    assert_eq!(state.hidden_root_count(), 0);
    state.set_team("Growth".into()).await.expect("Growth");
    state
        .set_team("Platform".into())
        .await
        .expect("back to Platform");
    assert!(
        state.visible_nodes().iter().any(|n| n.id == 1012),
        "Platform→Growth→Platform must keep Task root #1012, matching the startup default"
    );
    assert_eq!(
        state.selected_root_count(),
        first_selected,
        "Roots selected count after a team switch must match the initial default"
    );
    assert_eq!(state.hidden_root_count(), 0);
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

#[tokio::test]
async fn types_fall_back_to_loaded_nodes_when_hierarchy_types_are_empty() {
    let (_mock, mut state, _dir) = signed_in_state().await;
    if let Some(hierarchy) = state.hierarchy.as_mut() {
        hierarchy.types.clear();
    }
    let types = state.types();
    assert!(types.iter().any(|ty| ty == "Epic" || ty == "User Story"));
    assert!(
        !state.filter_options(&types, "user").is_empty() || types.iter().any(|ty| ty == "Epic")
    );
}

#[gpui_kit::test]
fn gpui_sign_in_shell_renders(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(dir.path().to_path_buf());
    let store = EncryptedStore::new(Box::new(MemoryStore::new(false)), None);
    let session = PlannerSession::pat(store);
    let mut state = PlannerState::new(paths, session, None, None);
    // `PlannerView::new` bootstraps on the `ado-client` runtime. Completing
    // that oneshot while the GPUI test scheduler is ticking panics as
    // non-deterministic (Windows CI flake). Paint the idle PAT shell instead.
    state.session_loading = false;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(
        cx.debug_bounds("sign-in-shell").is_some(),
        "the PAT sign-in shell must paint without spawning ADO I/O"
    );
    assert!(
        cx.debug_bounds("app-version").is_some(),
        "the sign-in card shows the embedded version"
    );
}

/// The PAT validation ping. Initial loading lists projects with `$top=200`.
const LOGIN_PING: &str = "/contoso/_apis/projects?$top=1&";

fn signed_out_with_mock() -> (
    tokio::runtime::Runtime,
    ado_mock::AdoMock,
    PlannerState,
    tempfile::TempDir,
) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("ado-mock-sign-in")
        .build()
        .expect("ado-mock sign-in runtime");
    let (mock, state, dir) = runtime.block_on(async {
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
        state.session_loading = false;
        (mock, state, dir)
    });
    (runtime, mock, state, dir)
}

fn draw(cx: &mut gpui_kit::VisualTestContext) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

/// Real HTTP to the mock completes on the `ado-client` runtime; the GPUI test
/// scheduler must be allowed to park while it waits.
fn wait_for_idle(view: &gpui_kit::Entity<PlannerView>, cx: &mut gpui_kit::VisualTestContext) {
    cx.background_executor.allow_parking();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        cx.run_until_parked();
        draw(cx);
        if !view.read_with(cx, |view, _| view.is_busy()) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("sign-in did not finish within 10s");
}

#[gpui_kit::test]
fn continue_shows_busy_state_and_ignores_resubmits_until_sign_in_loads(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (_runtime, mock, state, _dir) = signed_out_with_mock();
    mock.delay_next("/contoso/_apis/projects", 400);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(cx.debug_bounds("sign-in-status").is_none());

    let started = view.update_in(cx, |view, window, cx| {
        view.submit_pat_for_test(
            "https://dev.azure.com/contoso",
            "pat-secret-value",
            window,
            cx,
        )
    });
    assert!(started, "Continue must start a sign-in");
    draw(cx);
    assert!(
        cx.debug_bounds("sign-in-status").is_some(),
        "busy feedback must paint on the first frame after Continue"
    );
    assert!(
        cx.debug_bounds("sign-in-shell").is_some(),
        "the sign-in card stays up while signing in"
    );
    for _ in 0..3 {
        let again = view.update(cx, |view, cx| view.resubmit_pat_for_test(cx));
        assert!(
            !again,
            "Continue or Enter while signing in must not resubmit"
        );
    }

    wait_for_idle(&view, cx);
    assert_eq!(
        mock.request_count(LOGIN_PING),
        1,
        "repeat submits during sign-in must not send another login"
    );
    view.read_with(cx, |view, _| {
        assert!(view.state.session_info.signed_in);
        assert!(
            !view.state.nodes.is_empty(),
            "busy state covers the initial hierarchy load"
        );
    });
    assert!(cx.debug_bounds("sign-in-status").is_none());
    assert!(cx.debug_bounds("sign-in-shell").is_none());
}

#[gpui_kit::test]
fn failed_sign_in_clears_busy_state_and_allows_retry(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (_runtime, mock, state, _dir) = signed_out_with_mock();
    mock.fail_next("/contoso/_apis/projects", 401, "Unauthorized");
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);

    let started = view.update_in(cx, |view, window, cx| {
        view.submit_pat_for_test("https://dev.azure.com/contoso", "expired-pat", window, cx)
    });
    assert!(started);
    draw(cx);
    assert!(cx.debug_bounds("sign-in-status").is_some());

    wait_for_idle(&view, cx);
    view.read_with(cx, |view, _| {
        assert!(!view.state.session_info.signed_in);
    });
    assert!(
        cx.debug_bounds("sign-in-status").is_none(),
        "busy feedback must clear when sign-in fails"
    );
    assert!(cx.debug_bounds("sign-in-shell").is_some());
    assert!(
        cx.debug_bounds("sign-in-error").is_some(),
        "a rejected PAT must show an inline error once busy clears"
    );

    let retried = view.update_in(cx, |view, window, cx| {
        view.submit_pat_for_test(
            "https://dev.azure.com/contoso",
            "pat-secret-value",
            window,
            cx,
        )
    });
    assert!(retried, "Continue must work again after a failed sign-in");
    draw(cx);
    assert!(
        cx.debug_bounds("sign-in-error").is_none(),
        "the previous error must not show beside the busy state"
    );
    wait_for_idle(&view, cx);
    assert_eq!(mock.request_count(LOGIN_PING), 2);
    view.read_with(cx, |view, _| {
        assert!(view.state.session_info.signed_in);
    });
    assert!(cx.debug_bounds("sign-in-shell").is_none());
}

/// An Entra Session restored from disk whose access token has expired.
fn expired_entra_state() -> (
    tokio::runtime::Runtime,
    ado_mock::AdoMock,
    PlannerState,
    tempfile::TempDir,
) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("ado-mock-entra")
        .build()
        .expect("ado-mock Entra runtime");
    let (mock, state, dir) = runtime.block_on(async {
        let mock = ado_mock::start().await;
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().to_path_buf());
        let store = EncryptedStore::new(Box::new(MemoryStore::new(true)), Some([3u8; 32]));
        store
            .write_plain(
                &serde_json::json!({
                    "access_token": "stale-access",
                    "refresh_token": "stale-refresh",
                    "expires_at": 1,
                    "redirect": "http://127.0.0.1:9/",
                    "display_name": "Ada Lovelace",
                    "username": "ada@contoso.com"
                })
                .to_string(),
            )
            .unwrap();
        let session = PlannerSession::oauth(store, "11111111-2222-3333-4444-555555555555".into())
            .with_oauth_endpoints(OauthEndpoints {
                auth_url: format!("{}/oauth2/v2.0/authorize", mock.url),
                token_url: format!("{}{}", mock.url, ado_mock::TOKEN_PATH),
            })
            .with_bases(&mock.url, &mock.url)
            .without_browser();
        let mut state = PlannerState::new(
            paths,
            session,
            Some(mock.url.clone()),
            Some(mock.url.clone()),
        );
        state.session_loading = false;
        (mock, state, dir)
    });
    (runtime, mock, state, dir)
}

#[gpui_kit::test]
fn rejected_entra_refresh_returns_to_the_sign_in_card(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (_runtime, mock, state, _dir) = expired_entra_state();
    mock.reject_token_requests();
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));

    assert!(view.update(cx, |view, cx| view.bootstrap_for_test(cx)));
    wait_for_idle(&view, cx);

    assert_eq!(mock.request_count(ado_mock::TOKEN_PATH), 1);
    assert_eq!(
        mock.request_count("/_apis/"),
        0,
        "no Azure DevOps request goes out with the expired access token"
    );
    view.read_with(cx, |view, _| {
        assert!(!view.state.session_info.signed_in);
        assert_eq!(view.state.session_info.auth_mode, ado_core::AuthMode::Entra);
        let message = view.state.sign_in_error.clone().expect("sign-in reason");
        assert!(message.contains("Sign in again"), "{message}");
        assert!(!message.contains("stale-refresh") && !message.contains("stale-access"));
        assert!(
            view.state.toasts.last_error_message().is_none(),
            "the failed loads must not leave error toasts behind"
        );
        assert!(view.state.orgs.is_empty() && view.state.nodes.is_empty());
    });
    assert!(cx.debug_bounds("sign-in-shell").is_some());
    assert!(
        cx.debug_bounds("sign-in-error").is_some(),
        "the Entra sign-in card explains why it is back"
    );
    assert!(cx.debug_bounds("signed-in-chrome").is_none());
}

#[test]
fn entra_refresh_through_the_mock_token_endpoint_loads_the_planner() {
    let (runtime, mock, mut state, _dir) = expired_entra_state();
    runtime
        .block_on(state.bootstrap())
        .expect("refreshed bootstrap");
    assert_eq!(mock.request_count(ado_mock::TOKEN_PATH), 1);
    assert!(state.session_info.signed_in);
    assert!(state.sign_in_error.is_none());
    assert!(state.visible_nodes().iter().any(|n| n.id == 1001));
}
