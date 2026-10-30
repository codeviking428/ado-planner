//! Rendered `PlannerView` coverage for popups, the Gantt header, and the form.

use ado_core::SessionInfo;
use ado_planner::form::FormDraft;
use ado_planner::gantt::{ContextTarget, GanttView, MIN_TIMELINE_WIDTH, ROW_HEIGHT};
use ado_planner::paths::AppPaths;
use ado_planner::state::{PlannerState, fake_hierarchy};
use ado_planner::ui::PlannerView;
use ado_session::{EncryptedStore, MemoryStore, PlannerSession};
use gpui_kit::{KeyDownEvent, Keystroke, VisualTestContext, px};
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
        cx.debug_bounds("gantt-bar-1").is_some(),
        "at least one Gantt bar should be painted"
    );
}

#[gpui_kit::test]
fn painted_tree_rows_and_bars_share_row_height(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    let row1 = cx
        .debug_bounds("gantt-tree-row-1")
        .expect("first tree row should paint");
    let row2 = cx
        .debug_bounds("gantt-tree-row-2")
        .expect("second tree row should paint");
    let bar1 = cx
        .debug_bounds("gantt-bar-1")
        .expect("first bar should paint");
    let bar2 = cx
        .debug_bounds("gantt-bar-2")
        .expect("second bar should paint");
    let tree_stride = f32::from(row2.origin.y) - f32::from(row1.origin.y);
    let bar_stride = f32::from(bar2.origin.y) - f32::from(bar1.origin.y);
    assert!(
        (f32::from(row1.size.height) - ROW_HEIGHT).abs() < 2.0,
        "tree row height {} should be ROW_HEIGHT {ROW_HEIGHT}",
        f32::from(row1.size.height)
    );
    assert!(
        (tree_stride - ROW_HEIGHT).abs() < 2.0,
        "tree row stride {tree_stride} should be ROW_HEIGHT {ROW_HEIGHT}"
    );
    assert!(
        (bar_stride - ROW_HEIGHT).abs() < 2.0,
        "bar stride {bar_stride} should be ROW_HEIGHT {ROW_HEIGHT}, not the 24/44 mismatch"
    );
    let bar_mid = f32::from(bar1.origin.y) + f32::from(bar1.size.height) / 2.0;
    let row_mid = f32::from(row1.origin.y) + f32::from(row1.size.height) / 2.0;
    assert!(
        (bar_mid - row_mid).abs() < 8.0,
        "bar for row 1 sits at {bar_mid}, tree row at {row_mid}"
    );
}

#[gpui_kit::test]
fn painted_splitter_keeps_a_minimum_timeline(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1024.),
        height: px(720.),
    });
    draw(cx);
    view.update(cx, |view, cx| {
        view.state.gantt.set_tree_width_for_pane(10_000.0, 1024.0);
        cx.notify();
    });
    draw(cx);
    let tree = cx.debug_bounds("gantt-tree").expect("tree");
    let timeline = cx.debug_bounds("gantt-timeline").expect("timeline");
    assert!(
        f32::from(timeline.size.width) >= MIN_TIMELINE_WIDTH - 1.0,
        "timeline painted {}px while tree painted {}px; MIN_TIMELINE_WIDTH is {MIN_TIMELINE_WIDTH}",
        f32::from(timeline.size.width),
        f32::from(tree.size.width)
    );
    assert!(
        f32::from(tree.size.width) <= 1024.0 - MIN_TIMELINE_WIDTH + 8.0,
        "tree painted {}px and ate the timeline",
        f32::from(tree.size.width)
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
        view.set_open_filter(Some("type-filter"));
        assert!(view.popup_is_open());
        view.handle_key(&escape, cx);
        assert!(view.state.form.is_none(), "escape should close the form");
        assert!(
            !view.popup_is_open(),
            "escape should close stacked popups together"
        );
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
fn pagedown_moves_the_selection(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    let page = KeyDownEvent {
        keystroke: Keystroke::parse("pagedown").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        let before = view.state.gantt.selected_id;
        view.handle_key(&page, cx);
        let after = view.state.gantt.selected_id;
        assert_ne!(after, before, "PageDown should move the selection");
        let rows = view.state.visible_rows();
        let before_idx = rows
            .iter()
            .position(|row| Some(row.id) == before)
            .unwrap_or(0);
        let after_idx = rows
            .iter()
            .position(|row| Some(row.id) == after)
            .unwrap_or(0);
        assert!(
            after_idx > before_idx + 1 || after_idx + 1 == rows.len(),
            "PageDown should jump by a page of rows, not a single Down press"
        );
    });
}

#[gpui_kit::test]
fn down_arrow_moves_the_selection_by_one_row(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    let down = KeyDownEvent {
        keystroke: Keystroke::parse("down").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&down, cx);
        assert_ne!(view.state.gantt.selected_id, Some(1));
    });
}

#[gpui_kit::test]
fn unscheduled_only_grid_shows_a_drop_hint(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    for node in &mut state.nodes {
        node.start_date = None;
        node.target_date = None;
        node.has_date_fields = true;
    }
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("unscheduled-empty-hint").is_some(),
        "unscheduled-only teams should get a timeline drop hint"
    );
}

#[gpui_kit::test]
fn team_settings_error_renders_inline(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.team_settings_error =
        Some("Team \"Amego Stakeholders\" has invalid backlog settings (TF400497).".into());
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("team-settings-error")
            .is_some_and(|bounds| bounds.size.width > gpui_kit::px(200.)),
        "team-settings errors should be an inline readable banner"
    );
}

#[gpui_kit::test]
fn fractional_scroll_clips_rows_under_frozen_headers(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.scroll_y = 50.0;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-tree-body")
            .is_some_and(|bounds| bounds.size.height > gpui_kit::px(0.)),
        "tree body should clip rows that would paint over the frozen header"
    );
    assert!(
        cx.debug_bounds("gantt-timeline-body")
            .is_some_and(|bounds| bounds.size.height > gpui_kit::px(0.)),
        "timeline body should clip bars that would paint over the scale header"
    );
    let header = cx.debug_bounds("gantt-scale-header").expect("scale header");
    let tree_header = cx.debug_bounds("gantt-tree-header").expect("tree header");
    let body = cx
        .debug_bounds("gantt-timeline-body")
        .expect("timeline body");
    assert!(
        body.origin.y >= header.origin.y + header.size.height - gpui_kit::px(1.),
        "clipped timeline body must start at or below the frozen header"
    );
    assert!(
        tree_header.size.height > gpui_kit::px(0.),
        "tree header stays frozen above the clipped body"
    );
}

#[gpui_kit::test]
fn context_menu_opens_at_the_click_not_the_window_origin(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.context_menu = Some(ContextTarget {
        id: 1,
        from_bar: false,
        x: 320.0,
        y: 280.0,
    });
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    let menu = cx
        .debug_bounds("gantt-context-menu")
        .expect("context menu should paint");
    assert!(
        menu.origin.x > gpui_kit::px(100.) && menu.origin.y > gpui_kit::px(100.),
        "context menu should open near the click, not over the Org/Project header"
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

#[gpui_kit::test]
fn escape_closes_a_freshly_opened_form_without_a_focused_field(cx: &mut gpui_kit::TestAppContext) {
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
    cx.simulate_keystrokes("escape");
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.form.is_none(),
            "Escape on a freshly opened form with no field focused should close it"
        );
    });
}

#[gpui_kit::test]
fn inline_title_editor_is_wide_enough_to_show_the_title(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_inline_title(1, window, cx);
        });
    });
    draw(cx);
    let editor = cx
        .debug_bounds("gantt-inline-title")
        .expect("inline title editor should paint");
    assert!(
        f32::from(editor.size.width) >= 160.0,
        "inline editor was only {}px wide",
        f32::from(editor.size.width)
    );
}

#[gpui_kit::test]
fn form_uses_date_picker_avatar_chip_and_checkbox(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    let mut values = serde_json::Map::new();
    values.insert("System.Title".into(), serde_json::json!("Persist cart"));
    values.insert(
        ado_core::START_DATE_FIELD.into(),
        serde_json::json!("2026-07-06T00:00:00Z"),
    );
    values.insert(
        "Microsoft.VSTS.Scheduling.StoryPoints".into(),
        serde_json::json!(13.0),
    );
    values.insert(
        "System.AssignedTo".into(),
        serde_json::json!({"displayName": "Ada Lovelace", "uniqueName": "ada@contoso.com"}),
    );
    values.insert("Custom.Blocked".into(), serde_json::json!(true));
    state.form = Some(FormDraft::from_model(ado_core::WorkItemFormModel {
        id: 1003,
        rev: 1,
        work_item_type: "User Story".into(),
        system_controls: vec![
            ado_core::FormControl {
                id: "System.Title".into(),
                reference_name: "System.Title".into(),
                label: "Title".into(),
                kind: ado_core::FormControlKind::String,
                required: true,
                read_only: false,
                visible: true,
                options: None,
            },
            ado_core::FormControl {
                id: ado_core::START_DATE_FIELD.into(),
                reference_name: ado_core::START_DATE_FIELD.into(),
                label: "Start Date".into(),
                kind: ado_core::FormControlKind::DateTime,
                required: false,
                read_only: false,
                visible: true,
                options: None,
            },
            ado_core::FormControl {
                id: "Microsoft.VSTS.Scheduling.StoryPoints".into(),
                reference_name: "Microsoft.VSTS.Scheduling.StoryPoints".into(),
                label: "Story Points".into(),
                kind: ado_core::FormControlKind::Integer,
                required: false,
                read_only: false,
                visible: true,
                options: None,
            },
            ado_core::FormControl {
                id: "System.AssignedTo".into(),
                reference_name: "System.AssignedTo".into(),
                label: "Assigned To".into(),
                kind: ado_core::FormControlKind::Identity,
                required: false,
                read_only: false,
                visible: true,
                options: None,
            },
            ado_core::FormControl {
                id: "Custom.Blocked".into(),
                reference_name: "Custom.Blocked".into(),
                label: "Blocked".into(),
                kind: ado_core::FormControlKind::Boolean,
                required: false,
                read_only: false,
                visible: true,
                options: None,
            },
        ],
        pages: Vec::new(),
        values,
    }));
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("form-date-picker").is_some(),
        "date fields should render a date picker, not a raw ISO string"
    );
    assert!(
        cx.debug_bounds("form-assignee-chip").is_some(),
        "Assigned To should render as an avatar chip"
    );
    assert!(
        cx.debug_bounds("form-boolean").is_some(),
        "booleans should render as a labeled checkbox, not a flat control"
    );
    view.update(cx, |view, cx| {
        let points = view
            .form_input_value("Microsoft.VSTS.Scheduling.StoryPoints", cx)
            .unwrap_or_default();
        assert_eq!(
            points, "13",
            "integers must not render as 13.0, got {points}"
        );
    });
}

#[gpui_kit::test]
fn narrow_window_toolbar_wraps_instead_of_clipping(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(700.),
        height: px(500.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("header-log-out").is_some(),
        "Log out should remain painted in a 700px window"
    );
    assert!(
        cx.debug_bounds("gantt-nav")
            .is_some_and(|bounds| bounds.size.width > px(0.)),
        "gantt toolbar should wrap rather than vanish"
    );
}

#[gpui_kit::test]
fn lighter_toasts_and_today_label_paint(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.toasts.push(
        ado_planner::toasts::ToastKind::Success,
        "Copied work item ID",
    );
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("planner-toast").is_some(),
        "toasts should paint"
    );
    assert!(
        cx.debug_bounds("gantt-today").is_some(),
        "today marker should paint"
    );
}
