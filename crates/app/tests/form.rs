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
            .contains("Saved Work Item #1003")
    );
    let node = state.nodes.iter().find(|n| n.id == 1003).unwrap();
    assert_eq!(node.title, "Persist cart (saved)");
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
