//! Rendered `PlannerView` coverage for popups, the Gantt header, and the form.

use ado_core::SessionInfo;
use ado_planner::form::FormDraft;
use ado_planner::gantt::GanttView;
use ado_planner::paths::AppPaths;
use ado_planner::state::{PlannerState, fake_hierarchy};
use ado_planner::ui::PlannerView;
use ado_session::{EncryptedStore, MemoryStore, PlannerSession};
use gpui_kit::{KeyDownEvent, Keystroke, VisualTestContext};
use jiff::civil::Date;

fn idle_state(dir: &tempfile::TempDir) -> PlannerState {
    let paths = AppPaths::new(dir.path().to_path_buf());
    let store = EncryptedStore::new(Box::new(MemoryStore::new(false)), None);
    let session = PlannerSession::pat(store);
    let mut state = PlannerState::new(paths, session, None, None);
    state.session_loading = false;
    state.session_info = SessionInfo {
        signed_in: true,
        display_name: Some("Ada Lovelace".into()),
        username: Some("ada@contoso.com".into()),
        auth_mode: ado_core::AuthMode::Pat,
    };
    state.prefs.planner.org = "contoso".into();
    state.prefs.planner.project = "Shop".into();
    state.prefs.planner.team = "Platform".into();
    state.nodes = fake_hierarchy(12);
    state.gantt = GanttView {
        origin: Date::new(2026, 7, 1).unwrap(),
        viewport_h: 720.0,
        ..GanttView::default()
    };
    state
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

#[gpui_kit::test]
fn rendered_gantt_shows_scale_header_and_bars(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    view.update(cx, |view, _| {
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars = view
            .state
            .gantt
            .layout_bars(&view.state.nodes, &rows, &[], 720.0);
        assert!(
            !bars.is_empty(),
            "scheduled fake hierarchy rows should produce Gantt bars"
        );
        assert!(!view.state.gantt.timeline_ticks(800.0).is_empty());
    });
    let header = cx.debug_bounds("gantt-scale-header");
    assert!(
        header.is_some_and(|bounds| bounds.size.height > gpui_kit::px(0.)),
        "timeline scale header should be painted"
    );
    assert!(
        cx.debug_bounds("gantt-tree-header")
            .is_some_and(|bounds| bounds.size.height > gpui_kit::px(0.)),
        "tree header should align with the timeline header"
    );
    assert!(
        cx.debug_bounds("gantt-today").is_some(),
        "today marker should be painted"
    );
    assert!(
        cx.debug_bounds("gantt-bar").is_some(),
        "at least one Gantt bar should be painted"
    );
}

#[gpui_kit::test]
fn rendered_unscheduled_chip_and_form_header(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.first_mut() {
        node.start_date = None;
        node.target_date = None;
        node.has_date_fields = true;
    }
    let mut values = serde_json::Map::new();
    values.insert(
        "System.Title".into(),
        serde_json::Value::String("Persist cart".into()),
    );
    state.form = Some(FormDraft::from_model(ado_core::WorkItemFormModel {
        id: 1003,
        rev: 1,
        work_item_type: "User Story".into(),
        system_controls: Vec::new(),
        pages: Vec::new(),
        values,
    }));
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("unscheduled-chip").is_some(),
        "unscheduled dates should render as a chip, not a clipped string"
    );
    assert!(
        cx.debug_bounds("work-item-form-header")
            .is_some_and(|bounds| bounds.size.height > gpui_kit::px(24.)),
        "item panel should paint a crisp header"
    );
}

#[gpui_kit::test]
fn escape_closes_filter_popup_and_form(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.form = Some(FormDraft::from_model(ado_core::WorkItemFormModel {
        id: 1003,
        rev: 1,
        work_item_type: "User Story".into(),
        system_controls: Vec::new(),
        pages: Vec::new(),
        values: serde_json::Map::new(),
    }));
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(cx.debug_bounds("work-item-form").is_some());

    let escape = KeyDownEvent {
        keystroke: Keystroke::parse("escape").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&escape, cx);
        assert!(view.state.form.is_none());
        view.set_open_filter(Some("type-filter"));
        assert!(view.popup_is_open());
        view.handle_key(&escape, cx);
        assert!(!view.popup_is_open());
    });
    draw(cx);
    view.update(cx, |view, cx| {
        view.set_open_filter(Some("type-filter"));
        cx.notify();
    });
    draw(cx);
    assert!(
        cx.debug_bounds("overlay-popup-panel")
            .is_some_and(|bounds| bounds.size.width > gpui_kit::px(100.)),
        "filter popup should render as a full panel, not a clipped sliver"
    );
}

#[gpui_kit::test]
fn sign_in_shell_still_renders(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(dir.path().to_path_buf());
    let store = EncryptedStore::new(Box::new(MemoryStore::new(false)), None);
    let session = PlannerSession::pat(store);
    let mut state = PlannerState::new(paths, session, None, None);
    state.session_loading = false;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
}
