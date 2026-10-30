use ado_planner::form::FormDraft;
use ado_planner::paths::AppPaths;
use ado_planner::state::PlannerState;
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
async fn open_and_save_work_item_form() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.open_form(1003).await.expect("open");
    let form = state.form.as_mut().expect("form");
    assert!(form.title().contains("#1003"));
    form.set_string("System.Title", "Persist cart (saved)".into());
    state.save_form().await.expect("save");
    assert!(
        state
            .toasts
            .last_message()
            .unwrap_or_default()
            .contains("Saved title")
    );
    let node = state.nodes.iter().find(|n| n.id == 1003).unwrap();
    assert_eq!(node.title, "Persist cart (saved)");
}

#[tokio::test]
async fn save_toast_uses_display_names_and_skips_unchanged_fields() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.open_form(1003).await.expect("open");
    state.save_form().await.expect("save with no edits");
    let message = state.toasts.last_message().unwrap_or_default();
    assert!(
        !message.to_lowercase().contains("storypoints"),
        "unchanged Story Points must not appear as a raw key: {message}"
    );
    assert!(
        !message.to_lowercase().contains("remainingwork"),
        "unchanged Remaining Work must not appear as a raw key: {message}"
    );
    let form_open = {
        state.open_form(1003).await.expect("open again");
        true
    };
    assert!(form_open);
    let form = state.form.as_mut().expect("form");
    form.set_string("System.Title", "Persist cart (named)".into());
    state.save_form().await.expect("save title");
    let message = state.toasts.last_message().unwrap_or_default();
    assert!(
        message.to_lowercase().contains("title"),
        "toast should use the display name, got {message}"
    );
    assert!(
        !message.to_lowercase().contains("storypoints"),
        "unchanged fields must stay out of the toast: {message}"
    );
}

#[tokio::test]
async fn save_toast_does_not_report_description_when_only_story_points_changed() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.open_form(1003).await.expect("open");
    let form = state.form.as_mut().expect("form");
    let markdown = form
        .html_markdown
        .get("System.Description")
        .cloned()
        .or_else(|| {
            form.values
                .get("System.Description")
                .and_then(serde_json::Value::as_str)
                .and_then(ado_core::html_to_markdown)
        })
        .expect("description markdown");
    form.set_markdown("System.Description", markdown);
    form.values.insert(
        "Microsoft.VSTS.Scheduling.StoryPoints".into(),
        serde_json::json!(8),
    );
    state.save_form().await.expect("save");
    let message = state
        .toasts
        .last_message()
        .unwrap_or_default()
        .to_lowercase();
    assert!(
        message.contains("story points"),
        "toast should mention story points, got {message}"
    );
    assert!(
        !message.contains("description"),
        "rewriting description HTML through markdown must not count as a change: {message}"
    );
}

#[tokio::test]
async fn non_plain_html_field_is_read_only() {
    let html = r#"<p>Hello <span style="color:red">Ada</span></p>"#;
    assert!(!ado_core::is_plain_html_field(html));
    let mut values = serde_json::Map::new();
    values.insert(
        "System.Description".into(),
        serde_json::Value::String(html.into()),
    );
    let model = ado_core::WorkItemFormModel {
        id: 9,
        rev: 1,
        work_item_type: "Bug".into(),
        system_controls: Vec::new(),
        pages: vec![ado_core::FormPage {
            id: "p".into(),
            label: "Details".into(),
            groups: vec![ado_core::FormGroup {
                id: "g".into(),
                label: "Description".into(),
                controls: vec![ado_core::FormControl {
                    id: "System.Description".into(),
                    reference_name: "System.Description".into(),
                    label: "Description".into(),
                    kind: ado_core::FormControlKind::Html,
                    required: false,
                    read_only: false,
                    visible: true,
                    options: None,
                }],
            }],
        }],
        values,
    };
    let draft = FormDraft::from_model(model);
    assert!(!draft.is_plain_html("System.Description"));
}

#[tokio::test]
async fn identity_search_returns_team_members() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.open_form(1003).await.expect("open");
    state.search_identities("Ada".into()).await.expect("search");
    let form = state.form.as_ref().expect("form");
    assert!(
        form.identity_results
            .iter()
            .any(|identity| identity.display_name.contains("Ada"))
    );
}

#[tokio::test]
async fn planning_section_includes_date_number_and_boolean_fields() {
    let (_mock, mut state, _dir) = signed_in().await;
    state.open_form(1003).await.expect("open");
    let form = state.form.as_ref().expect("form");
    let planning = form
        .model
        .pages
        .iter()
        .flat_map(|page| page.groups.iter())
        .find(|group| group.label == "Planning")
        .expect("planning group");
    let kinds: Vec<_> = planning.controls.iter().map(|c| c.kind).collect();
    assert!(kinds.contains(&ado_core::FormControlKind::DateTime));
    assert!(kinds.contains(&ado_core::FormControlKind::Double));
    assert!(kinds.contains(&ado_core::FormControlKind::Boolean));
}

#[gpui_kit::test]
fn gpui_form_title_renders(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let model = ado_core::WorkItemFormModel {
        id: 1003,
        rev: 1,
        work_item_type: "User Story".into(),
        system_controls: Vec::new(),
        pages: Vec::new(),
        values: serde_json::Map::new(),
    };
    let draft = FormDraft::from_model(model);
    assert!(draft.title().contains("#1003"));
    assert!(draft.title().contains("User Story"));
}
