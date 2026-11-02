//! Rendered `PlannerView` coverage for popups, the Gantt header, and the form.

use ado_core::{IdentityValue, SessionInfo};
use ado_planner::form::FormDraft;
use ado_planner::gantt::{
    ContextTarget, GanttScale, GanttView, GestureKind, HANDLE_WIDTH, MIN_TIMELINE_WIDTH, ROW_HEIGHT,
};
use ado_planner::paths::AppPaths;
use ado_planner::state::{PlannerState, fake_hierarchy};
use ado_planner::toasts::HistoryEntry;
use ado_planner::ui::PlannerView;
use ado_planner::ui::{dates_from_range_picker, looks_like_pat};
use ado_session::{EncryptedStore, MemoryStore, PlannerSession};
use gpui_kit::component::calendar::Date as PickerDate;
use gpui_kit::{
    KeyDownEvent, Keystroke, Modifiers, MouseButton, ScrollDelta, ScrollEvent, VisualTestContext,
    point, px,
};
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
    let band1 = cx
        .debug_bounds("gantt-row-band-1")
        .expect("first timeline band should paint");
    let band2 = cx
        .debug_bounds("gantt-row-band-2")
        .expect("second timeline band should paint");
    let tree_stride = f32::from(row2.origin.y) - f32::from(row1.origin.y);
    let band_stride = f32::from(band2.origin.y) - f32::from(band1.origin.y);
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
        (band_stride - ROW_HEIGHT).abs() < 2.0,
        "timeline band stride {band_stride} should be ROW_HEIGHT {ROW_HEIGHT}, not the 24/44 mismatch"
    );
    let bar_mid = f32::from(bar1.origin.y) + f32::from(bar1.size.height) / 2.0;
    let row_mid = f32::from(row1.origin.y) + f32::from(row1.size.height) / 2.0;
    assert!(
        (bar_mid - row_mid).abs() < 8.0,
        "bar for row 1 sits at {bar_mid}, tree row at {row_mid}"
    );
}

#[gpui_kit::test]
fn painted_rows_are_monday_height_at_1280x800(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let row1 = cx.debug_bounds("gantt-tree-row-1").expect("row");
    let height = f32::from(row1.size.height);
    assert!(
        (height - ROW_HEIGHT).abs() < 2.0 && height >= 36.0,
        "tree row painted {height}px; expected ~{ROW_HEIGHT}"
    );
    let label = cx
        .debug_bounds("gantt-bar-label-1")
        .or_else(|| cx.debug_bounds("gantt-bar-label"));
    if let Some(label) = label {
        let label_h = f32::from(label.size.height);
        assert!(
            label_h >= 12.0,
            "bar labels must be unclipped, painted {label_h}px"
        );
    }
}

#[gpui_kit::test]
fn painted_700x500_window_keeps_a_240px_timeline(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(700.),
        height: px(500.),
    });
    draw(cx);
    view.update(cx, |view, cx| {
        view.state
            .gantt
            .set_tree_width_for_pane(view.state.gantt.tree_width, 700.0);
        cx.notify();
    });
    draw(cx);
    let tree = cx.debug_bounds("gantt-tree").expect("tree");
    let timeline = cx.debug_bounds("gantt-timeline").expect("timeline");
    assert!(
        f32::from(timeline.size.width) >= MIN_TIMELINE_WIDTH - 1.0,
        "700×500 timeline painted {}px (tree {}px); MIN_TIMELINE_WIDTH is {MIN_TIMELINE_WIDTH}",
        f32::from(timeline.size.width),
        f32::from(tree.size.width)
    );
}

#[gpui_kit::test]
fn clicking_the_dates_cell_opens_a_range_picker(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let cell = cx
        .debug_bounds("gantt-dates-cell")
        .expect("dates cell should paint at default width");
    cx.simulate_click(cell.center(), gpui_kit::Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-dates").is_some(),
        "clicking Dates must open the range picker, not only a tooltip"
    );
}

#[test]
fn looks_like_a_pat_masks_long_tokens() {
    assert!(looks_like_pat("abcdefghijklmnopqrstuv123"));
    assert!(!looks_like_pat("contoso"));
    assert!(!looks_like_pat("https://dev.azure.com/contoso"));
}

#[test]
fn range_picker_start_only_keeps_target_none() {
    let start = chrono::NaiveDate::from_ymd_opt(2026, 8, 4).unwrap();
    let picked = dates_from_range_picker(PickerDate::Range(Some(start), None), "").unwrap();
    assert_eq!(
        picked.0,
        Some(Date::new(2026, 8, 4).unwrap()),
        "a start-only pick must produce a Start Date"
    );
    assert!(
        picked.1.is_none(),
        "a start-only pick must leave Target Date unset so save keeps it"
    );
    let typed = dates_from_range_picker(PickerDate::Range(None, None), "2026-08-12").unwrap();
    assert_eq!(typed.0, Some(Date::new(2026, 8, 12).unwrap()));
    assert!(typed.1.is_none());
    let both =
        dates_from_range_picker(PickerDate::Range(None, None), "2026-08-04 → 2026-08-21").unwrap();
    assert_eq!(both.0, Some(Date::new(2026, 8, 4).unwrap()));
    assert_eq!(both.1, Some(Date::new(2026, 8, 21).unwrap()));
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
        link: None,
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
    cx.update(|window, cx| {
        window.blur(cx);
    });
    cx.simulate_keystrokes("escape");
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.form.is_none(),
            "Escape after double-clicking a row (form open, nothing focused) should close it"
        );
    });
}

#[gpui_kit::test]
fn escape_closes_the_context_menu_when_nothing_is_focused(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.context_menu = Some(ContextTarget {
        id: 1,
        from_bar: false,
        x: 320.0,
        y: 280.0,
        link: None,
    });
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(cx.debug_bounds("gantt-context-menu").is_some());
    cx.update(|window, cx| {
        window.blur(cx);
    });
    cx.simulate_keystrokes("escape");
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.context_menu.is_none(),
            "Escape on an unfocused right-click menu should close it"
        );
    });
}

#[gpui_kit::test]
fn painted_parent_summary_bar_is_shorter_than_a_leaf_and_shows_fill(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(child) = state.nodes.iter_mut().find(|node| node.id == 2) {
        child.state = "Active".into();
        child.parent_id = Some(1);
    }
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    let parent = cx
        .debug_bounds("gantt-bar-1")
        .expect("parent bar should paint");
    let leaf = cx
        .debug_bounds("gantt-bar-2")
        .expect("leaf bar should paint");
    assert!(
        f32::from(parent.size.height) + 4.0 < f32::from(leaf.size.height),
        "parent summary bar height {} should be shorter than leaf {}",
        f32::from(parent.size.height),
        f32::from(leaf.size.height)
    );
    let fill = cx
        .debug_bounds("gantt-bar-progress-1")
        .expect("parent progress fill must paint — Cart/Persist cart must not look like leaves");
    assert!(
        f32::from(fill.size.width) > 2.0,
        "progress fill was {}px — Active children should show a visible fill",
        f32::from(fill.size.width)
    );
}

#[gpui_kit::test]
fn painted_dependency_arrow_joins_predecessor_to_successor(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.predecessors = vec![1];
        node.start_date = Some("2026-07-21T00:00:00Z".into());
        node.target_date = Some("2026-08-04T00:00:00Z".into());
    }
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-dep-arrow-1-2").is_some(),
        "dependency arrows must paint once System.LinkTypes.Dependency predecessors are loaded"
    );
}

#[gpui_kit::test]
fn inline_dates_use_a_range_picker(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_inline_dates(1, window, cx);
        });
    });
    draw(cx);
    let editor = cx
        .debug_bounds("gantt-inline-dates")
        .expect("inline dates should paint a range picker, not a 99px text field");
    assert!(
        f32::from(editor.size.width) >= 180.0,
        "inline date editor was only {}px wide",
        f32::from(editor.size.width)
    );
}

#[gpui_kit::test]
fn inline_assignee_opens_an_editor(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_inline_assignee(1, window, cx);
        });
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-assignee").is_some(),
        "clicking assignee must open an editor, not only a tooltip"
    );
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
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("planner-toast").is_some(),
        "toasts should paint"
    );
    assert!(
        cx.debug_bounds("gantt-today").is_some(),
        "today marker should paint"
    );
    let toast = cx.debug_bounds("planner-toast").expect("toast");
    let nav = cx.debug_bounds("gantt-nav").expect("toolbar");
    let toast_top = f32::from(toast.origin.y);
    let nav_bottom = f32::from(nav.origin.y) + f32::from(nav.size.height);
    assert!(
        toast_top >= nav_bottom - 2.0,
        "toasts at y={toast_top} must sit below the toolbar bottom {nav_bottom}"
    );
}

#[gpui_kit::test]
fn clicking_today_keeps_calendar_month_labels_and_oct_30_marker(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let today_btn = cx.debug_bounds("gantt-today-btn").expect("Today button");
    cx.simulate_click(today_btn.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let labels: Vec<String> = view
            .state
            .gantt
            .header_tick_labels(view.state.gantt.timeline_width.max(400.0))
            .into_iter()
            .map(|(_, label)| label)
            .collect();
        for pair in labels.windows(2) {
            assert_ne!(
                pair[0], pair[1],
                "month header labels must not repeat after Today: {labels:?}"
            );
        }
        let title = view.state.gantt.viewport_range_title();
        let centre = view.state.gantt.viewport_center_date();
        assert_eq!(
            title,
            view.state.gantt.scale.range_title(centre),
            "range title must follow the viewport centre ({centre}), not the first tick ({labels:?})"
        );
        let today_x = view.state.gantt.today_x(jiff::Zoned::now().date());
        let expected = view.state.gantt.timeline_width.max(1.0) / 3.0;
        assert!(
            (today_x - expected).abs() < 8.0,
            "Today line at {today_x} should sit about a third in, expected {expected}"
        );
    });
}

#[gpui_kit::test]
fn day_zoom_shows_today_label_and_year_in_the_header(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.set_scale(GanttScale::Day);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let today_btn = cx.debug_bounds("gantt-today-btn").expect("Today button");
    cx.simulate_click(today_btn.center(), Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-today-label").is_some(),
        "Day zoom must paint the Today label instead of clipping it at the left edge"
    );
    view.update(cx, |view, _| {
        let title = view.state.gantt.viewport_range_title();
        assert!(
            title.contains("2026"),
            "Day header must include the year, got {title:?}"
        );
        let today = jiff::Zoned::now().date();
        let today_x = view.state.gantt.today_x(today);
        let expected = view.state.gantt.timeline_width.max(1.0) / 3.0;
        assert!(
            (today_x - expected).abs() < 8.0,
            "Day zoom Today line at {today_x} should sit about a third in, expected {expected}"
        );
    });
}

#[gpui_kit::test]
fn horizontal_scroll_left_reveals_earlier_month_labels(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.origin = Date::new(2026, 8, 1).unwrap();
    state.gantt.scroll_x = 0.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let before = view.update(cx, |view, _| {
        view.state.gantt.x_for_date(Date::new(2026, 8, 1).unwrap())
    });
    let timeline = cx.debug_bounds("gantt-timeline").expect("timeline");
    cx.simulate_event(ScrollEvent {
        position: timeline.center(),
        delta: ScrollDelta::Pixels(point(px(120.), px(0.))),
        ..Default::default()
    });
    draw(cx);
    view.update(cx, |view, _| {
        let after = view
            .state
            .gantt
            .x_for_date(Date::new(2026, 8, 1).unwrap());
        assert!(
            after > before,
            "scroll-left (positive GPUI delta.x) must move Aug 1 right, revealing earlier months: {before} -> {after}"
        );
        let labels: Vec<_> = view
            .state
            .gantt
            .header_tick_labels(view.state.gantt.timeline_width.max(400.0))
            .into_iter()
            .map(|(_, label)| label)
            .collect();
        assert!(
            labels.iter().any(|label| label == "Jul" || label == "Aug"),
            "header labels after a left scroll should be calendar months, got {labels:?}"
        );
    });
}

#[gpui_kit::test]
fn dragging_a_link_dot_starts_a_link_not_a_resize(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(1);
    state.gantt.hovered_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let handle = cx
        .debug_bounds("gantt-link-handle-1-link-end")
        .expect("selected bar must show a link dot outside the bar");
    let bar = cx.debug_bounds("gantt-bar-1").expect("bar");
    assert!(
        f32::from(handle.origin.x) + 1.0 >= f32::from(bar.origin.x) + f32::from(bar.size.width),
        "link dot must sit outside the bar end so a drag is not a resize"
    );
    cx.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view.state.gantt.gesture.as_ref().expect("link gesture");
        assert_eq!(gesture.kind, GestureKind::Link);
        assert_eq!(gesture.id, 1);
    });
    let other = cx.debug_bounds("gantt-bar-2").expect("successor bar");
    cx.simulate_mouse_move(
        other.center(),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view.state.gantt.gesture.as_ref().expect("still linking");
        assert_eq!(gesture.kind, GestureKind::Link);
        assert_eq!(
            gesture.link_to,
            Some(2),
            "a simple press, move and hover over the target must arm the link"
        );
        let start = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 1)
            .and_then(|node| node.start_date.clone());
        assert_eq!(
            start.as_deref(),
            Some("2026-07-06T00:00:00Z"),
            "link drag must not rewrite dates"
        );
    });
}

#[gpui_kit::test]
fn dragging_a_start_only_bar_moves_its_start_date(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.target_date = None;
        node.title = "Cart UI".into();
    }
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-2")
        .expect("start-only bar should paint");
    let start = bar.center();
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view.state.gantt.gesture.as_ref().expect("move");
        assert_eq!(gesture.kind, GestureKind::Move);
        assert!(gesture.had_start);
        assert!(!gesture.had_target);
    });
    let ppd = view.update(cx, |view, _| {
        view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom)
    });
    cx.simulate_mouse_move(
        point(start.x + px(ppd * 4.0), start.y),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view.state.gantt.gesture.clone().expect("live move");
        let (start_date, target_date) = GanttView::gesture_patch_dates(&gesture);
        assert_eq!(
            start_date,
            Some(Date::new(2026, 7, 10).unwrap()),
            "dragging 4 days from Jul 6 must land on Jul 10, got {start_date:?}"
        );
        assert!(
            target_date.is_none(),
            "start-only drag must not invent a target, got {target_date:?}"
        );
    });
}

#[gpui_kit::test]
fn first_click_on_dates_opens_the_calendar(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let cell = cx.debug_bounds("gantt-dates-cell").expect("dates cell");
    cx.simulate_mouse_down(cell.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-dates").is_some(),
        "the first click must open the calendar, not convert the cell to a closed input"
    );
}

#[gpui_kit::test]
fn form_range_field_is_labelled_dates(cx: &mut gpui_kit::TestAppContext) {
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
        ado_core::TARGET_DATE_FIELD.into(),
        serde_json::json!("2026-07-20T00:00:00Z"),
    );
    state.form = Some(FormDraft::from_model(ado_core::WorkItemFormModel {
        id: 1003,
        rev: 1,
        work_item_type: "User Story".into(),
        system_controls: Vec::new(),
        pages: vec![ado_core::FormPage {
            id: "details".into(),
            label: "Details".into(),
            groups: vec![ado_core::FormGroup {
                id: "planning".into(),
                label: "Planning".into(),
                controls: vec![
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
                        id: ado_core::TARGET_DATE_FIELD.into(),
                        reference_name: ado_core::TARGET_DATE_FIELD.into(),
                        label: "Target Date".into(),
                        kind: ado_core::FormControlKind::DateTime,
                        required: false,
                        read_only: false,
                        visible: true,
                        options: None,
                    },
                    ado_core::FormControl {
                        id: "Custom.NeedsReview".into(),
                        reference_name: "Custom.NeedsReview".into(),
                        label: "Needs Review".into(),
                        kind: ado_core::FormControlKind::Boolean,
                        required: false,
                        read_only: false,
                        visible: true,
                        options: None,
                    },
                ],
            }],
        }],
        values,
    }));
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("form-dates-label").is_some(),
        "the form range field must be labelled Dates, not only Start Date"
    );
    assert!(
        cx.debug_bounds("form-boolean").is_some(),
        "boolean should paint as a labeled checkbox, not a ghost box plus Yes/No text"
    );
    assert!(
        cx.debug_bounds("form-boolean-box").is_none(),
        "Needs Review must not draw a ghost box beside the real checkbox"
    );
}

#[gpui_kit::test]
fn placeholder_drop_on_august_keeps_august_dates(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.start_date = None;
        node.target_date = None;
        node.title = "Backlog grooming".into();
        node.iteration_path = "Shop\\FY26\\Sprint 12".into();
    }
    state.iterations = vec![ado_core::IterationNode {
        path: "Shop\\FY26\\Sprint 12".into(),
        name: "Sprint 12".into(),
        start_date: Some("2026-08-10T00:00:00Z".into()),
        finish_date: Some("2026-08-21T00:00:00Z".into()),
        children: Vec::new(),
    }];
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scroll_x = 0.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-2")
        .expect("unscheduled placeholder should paint from the sprint hint");
    cx.simulate_mouse_down(bar.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view.state.gantt.gesture.clone().expect("placeholder move");
        assert_eq!(gesture.kind, GestureKind::Move);
        let (start, _) = GanttView::gesture_patch_dates(&gesture);
        let start = start.expect("start");
        assert_eq!(
            start.month(),
            8,
            "placeholder must use the painted sprint dates, not the July origin, got {start}"
        );
        assert_ne!(
            start,
            Date::new(2026, 7, 1).unwrap(),
            "gesture start must not fall back to the timeline origin"
        );
    });
    assert!(
        cx.debug_bounds("gantt-drag-preview").is_some(),
        "dragging a bar or placeholder must show a live date preview"
    );
}

#[gpui_kit::test]
fn empty_team_keeps_the_gantt_toolbar(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes.clear();
    state.prefs.planner.team = "Empty".into();
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-nav").is_some(),
        "an empty team must keep Org/Project/Team/Today controls"
    );
    assert!(
        cx.debug_bounds("empty-gantt").is_some(),
        "empty team should show a friendly empty state, not only a missing gantt"
    );
    assert!(
        cx.debug_bounds("gantt-timeline-body").is_some(),
        "empty-state body must still draw the timeline grid"
    );
    assert!(
        cx.debug_bounds("gantt-today").is_some(),
        "empty-state body must still draw the Today line"
    );
    let today = cx.debug_bounds("gantt-today").expect("today line");
    let body = cx
        .debug_bounds("gantt-timeline-body")
        .expect("timeline body");
    let today_x = f32::from(today.origin.x);
    let left = f32::from(body.origin.x);
    let right = left + f32::from(body.size.width);
    assert!(
        today_x >= left - 2.0 && today_x <= right,
        "empty-state Today line at {today_x} must sit inside the grid {left}..{right}"
    );
}

#[gpui_kit::test]
fn loading_overlay_paints_skeleton_rows(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.hierarchy_loading = true;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-loading").is_some(),
        "loading the hierarchy must show an overlay"
    );
    assert!(
        cx.debug_bounds("gantt-loading-skeleton").is_some(),
        "loading overlay should include skeleton rows so it is visible"
    );
}

#[gpui_kit::test]
fn clicking_a_dependency_arrow_selects_the_link(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.predecessors = vec![1];
        node.start_date = Some("2026-07-21T00:00:00Z".into());
        node.target_date = Some("2026-08-04T00:00:00Z".into());
    }
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let arrow = cx
        .debug_bounds("gantt-dep-arrow-1-2")
        .expect("dependency arrow hit target");
    cx.simulate_mouse_down(arrow.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.selected_link,
            Some((1, 2)),
            "clicking the arrow must select the dependency so Delete can remove it"
        );
    });
}

#[gpui_kit::test]
fn filtered_empty_gantt_offers_show_all(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.hide_all_roots();
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("empty-gantt").is_some(),
        "filters that hide every item must not use the 'team has no work' empty state"
    );
    assert!(
        cx.debug_bounds("empty-show-all").is_some(),
        "filtered empty state should offer Show all"
    );
    assert!(
        cx.debug_bounds("gantt-add-item").is_some(),
        "+ Add item must stay visible next to Today and Auto Fit"
    );
}

#[gpui_kit::test]
fn short_bar_paints_its_title_outside(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.title = "Unparented spike".into();
        node.start_date = Some("2026-07-06T00:00:00Z".into());
        node.target_date = Some("2026-07-07T00:00:00Z".into());
        node.parent_id = None;
    }
    state.gantt.scale = ado_planner::gantt::GanttScale::Year;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-bar-label-2").is_some()
            || cx.debug_bounds("gantt-bar-label").is_some(),
        "a title that does not fit the bar must paint outside instead of truncating"
    );
}

#[gpui_kit::test]
fn dates_calendar_floats_above_the_cell(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let cell = cx.debug_bounds("gantt-dates-cell").expect("dates cell");
    let cell_w = f32::from(cell.size.width);
    cx.simulate_mouse_down(cell.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    let popover = cx
        .debug_bounds("gantt-inline-dates")
        .expect("calendar popover");
    assert!(
        f32::from(popover.size.width) >= 180.0,
        "calendar popover was only {}px wide",
        f32::from(popover.size.width)
    );
    let cell_after = cx.debug_bounds("gantt-dates-cell").expect("cell stays");
    assert!(
        (f32::from(cell_after.size.width) - cell_w).abs() < 8.0,
        "opening Dates must not stretch the tree column; cell grew from {cell_w} to {}",
        f32::from(cell_after.size.width)
    );
}

#[gpui_kit::test]
fn context_menu_remove_link_does_not_schedule_the_bar(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.start_date = None;
        node.target_date = None;
        node.predecessors = vec![1];
    }
    state.gantt.context_menu = Some(ContextTarget {
        id: 2,
        from_bar: false,
        x: 360.0,
        y: 280.0,
        link: Some((1, 2)),
    });
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let item = cx
        .debug_bounds("gantt-menu-Remove link")
        .expect("Remove link must paint");
    cx.simulate_click(item.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.schedule_drag.is_none(),
            "menu clicks must not fall through and arm schedule_drag"
        );
        assert!(
            view.state.gantt.gesture.is_none(),
            "menu clicks must not start a bar gesture"
        );
        assert!(
            view.state.gantt.context_menu.is_none(),
            "choosing Remove link should close the menu"
        );
    });
}

#[gpui_kit::test]
fn mouse_leave_clears_row_hover_and_link_handles(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.hovered_id = Some(1);
    state.gantt.hovered_link = Some((1, 2));
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let band = cx
        .debug_bounds("gantt-row-band-1")
        .expect("timeline row band");
    cx.simulate_mouse_move(band.center(), None, Modifiers::default());
    draw(cx);
    let nav = cx
        .debug_bounds("gantt-nav")
        .expect("nav sits above the canvas");
    cx.simulate_mouse_move(nav.center(), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.hovered_id, None,
            "leaving the canvas must clear the row highlight"
        );
        assert_eq!(
            view.state.gantt.hovered_link, None,
            "leaving the canvas must hide link handles"
        );
    });
}

#[gpui_kit::test]
fn drag_end_clears_row_hover_and_link_handles(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.hovered_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1").expect("bar");
    cx.simulate_mouse_down(bar.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    let moved = gpui_kit::point(bar.center().x + px(36.), bar.center().y);
    cx.simulate_mouse_move(moved, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_some(),
            "mouse down + move must start a drag"
        );
        assert_eq!(
            view.state.gantt.hovered_id,
            Some(1),
            "hover stays during the drag so the bar can follow the pointer"
        );
    });
    cx.simulate_mouse_up(moved, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.hovered_id, None,
            "drag end must clear the row highlight"
        );
        assert_eq!(
            view.state.gantt.hovered_link, None,
            "drag end must hide link handles"
        );
    });
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.selected_id,
            Some(1),
            "a finished drag leaves the bar selected so link handles can stay"
        );
    });
    let nav = cx.debug_bounds("gantt-nav").expect("toolbar");
    cx.simulate_mouse_move(nav.center(), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.hovered_id, None,
            "moving to the toolbar must keep the row highlight cleared"
        );
    });
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.hovered_id, None,
            "moving off the canvas must not restore hover"
        );
    });
}

#[gpui_kit::test]
fn closing_a_dialog_clears_canvas_hover(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.hovered_id = Some(1);
    state.gantt.hovered_link = Some((1, 2));
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    view.update(cx, |view, _| {
        view.state.close_form();
        assert_eq!(view.state.gantt.hovered_id, None);
        assert_eq!(view.state.gantt.hovered_link, None);
    });
}

#[gpui_kit::test]
fn ctrl_z_and_ctrl_shift_z_are_wired_to_history(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    let undo = KeyDownEvent {
        keystroke: Keystroke::parse("ctrl-z").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    let redo = KeyDownEvent {
        keystroke: Keystroke::parse("ctrl-shift-z").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&undo, cx);
        view.handle_key(&redo, cx);
        assert!(
            view.state.history.undo.is_empty() && view.state.history.redo.is_empty(),
            "empty stacks must stay empty"
        );
    });
}

#[gpui_kit::test]
fn filtered_empty_keeps_the_timeline_header_and_grid(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.hide_all_roots();
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-scale-header").is_some(),
        "filters that hide every item must keep the timeline header"
    );
    assert!(
        cx.debug_bounds("gantt-timeline-body").is_some(),
        "filters that hide every item must keep the timeline grid"
    );
    assert!(
        cx.debug_bounds("gantt-today").is_some(),
        "filtered empty body must still draw the Today line"
    );
    let today = cx.debug_bounds("gantt-today").expect("today line");
    let body = cx
        .debug_bounds("gantt-timeline-body")
        .expect("timeline body");
    let today_x = f32::from(today.origin.x);
    let left = f32::from(body.origin.x);
    let right = left + f32::from(body.size.width);
    assert!(
        today_x >= left - 2.0 && today_x <= right,
        "filtered empty Today line at {today_x} must sit inside the grid {left}..{right}"
    );
    assert!(
        cx.debug_bounds("empty-show-all").is_some(),
        "filtered empty overlay should still offer Show all"
    );
    assert!(
        cx.debug_bounds("empty-gantt-card").is_some(),
        "filter-hidden notice must paint a solid card above the Today line"
    );
}

#[gpui_kit::test]
fn zoom_buttons_and_ctrl_scroll_update_the_range_title(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let before = view.update(cx, |view, _| view.state.gantt.zoom);
    let zoom_in = cx.debug_bounds("gantt-zoom-in").expect("zoom in");
    cx.simulate_click(zoom_in.center(), Modifiers::default());
    draw(cx);
    let after_in = view.update(cx, |view, _| view.state.gantt.zoom);
    assert!(after_in > before, "+ must zoom in, {before} -> {after_in}");
    let zoom_out = cx.debug_bounds("gantt-zoom-out").expect("zoom out");
    cx.simulate_click(zoom_out.center(), Modifiers::default());
    draw(cx);
    let after_out = view.update(cx, |view, _| view.state.gantt.zoom);
    assert!(
        after_out < after_in,
        "− must zoom out, {after_in} -> {after_out}"
    );
    let title_before = view.update(cx, |view, _| view.state.gantt.viewport_range_title());
    let timeline = cx.debug_bounds("gantt-timeline").expect("timeline");
    cx.simulate_event(ScrollEvent {
        position: timeline.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(80.))),
        modifiers: Modifiers {
            control: true,
            ..Default::default()
        },
        ..Default::default()
    });
    draw(cx);
    view.update(cx, |view, _| {
        assert_ne!(
            view.state.gantt.zoom, after_out,
            "Ctrl+scroll must change zoom"
        );
        let title_after = view.state.gantt.viewport_range_title();
        assert_eq!(
            title_after,
            view.state
                .gantt
                .scale
                .range_title(view.state.gantt.viewport_center_date()),
            "range title is derived from the viewport centre"
        );
        let _ = title_before;
    });
    assert!(
        cx.debug_bounds("gantt-range-title").is_some(),
        "range title should paint so Ctrl+scroll can update it"
    );
}

#[gpui_kit::test]
fn start_only_details_dialog_shows_the_start_date(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    let mut values = serde_json::Map::new();
    values.insert("System.Title".into(), serde_json::json!("Persist cart"));
    values.insert(
        ado_core::START_DATE_FIELD.into(),
        serde_json::json!("2026-07-06T00:00:00Z"),
    );
    state.form = Some(FormDraft::from_model(ado_core::WorkItemFormModel {
        id: 1003,
        rev: 1,
        work_item_type: "User Story".into(),
        system_controls: Vec::new(),
        pages: vec![ado_core::FormPage {
            id: "details".into(),
            label: "Details".into(),
            groups: vec![ado_core::FormGroup {
                id: "planning".into(),
                label: "Planning".into(),
                controls: vec![
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
                        id: ado_core::TARGET_DATE_FIELD.into(),
                        reference_name: ado_core::TARGET_DATE_FIELD.into(),
                        label: "Target Date".into(),
                        kind: ado_core::FormControlKind::DateTime,
                        required: false,
                        read_only: false,
                        visible: true,
                        options: None,
                    },
                ],
            }],
        }],
        values,
    }));
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("form-date-picker").is_some(),
        "start-only items must still paint a date picker, not an empty Select date control"
    );
}

#[gpui_kit::test]
fn link_handle_hit_area_is_at_least_fourteen_pixels(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.hovered_id = Some(1);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let handle = cx
        .debug_bounds("gantt-link-handle-1-link-end")
        .expect("hovered bar must show a link handle");
    assert!(
        f32::from(handle.size.width) >= 14.0 && f32::from(handle.size.height) >= 14.0,
        "link handle hit area was {}×{}",
        f32::from(handle.size.width),
        f32::from(handle.size.height)
    );
}

#[gpui_kit::test]
fn zoom_label_uses_a_fixed_width_so_plus_minus_do_not_shift(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let minus_before = cx.debug_bounds("gantt-zoom-out").expect("zoom out");
    let plus_before = cx.debug_bounds("gantt-zoom-in").expect("zoom in");
    let x_minus = f32::from(minus_before.origin.x);
    let x_plus = f32::from(plus_before.origin.x);
    let zoom_in = cx.debug_bounds("gantt-zoom-in").expect("zoom in");
    cx.simulate_click(zoom_in.center(), Modifiers::default());
    draw(cx);
    let minus_after = cx.debug_bounds("gantt-zoom-out").expect("zoom out");
    let plus_after = cx.debug_bounds("gantt-zoom-in").expect("zoom in");
    assert!(
        (f32::from(minus_after.origin.x) - x_minus).abs() < 2.0,
        "Month (125%) must not shift −; before {x_minus} after {}",
        f32::from(minus_after.origin.x)
    );
    assert!(
        (f32::from(plus_after.origin.x) - x_plus).abs() < 2.0,
        "Month (125%) must not shift +; before {x_plus} after {}",
        f32::from(plus_after.origin.x)
    );
}

#[gpui_kit::test]
fn single_click_selects_a_bar_and_enter_opens_details(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1").expect("bar");
    cx.simulate_mouse_down(bar.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(bar.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.selected_id,
            Some(1),
            "a single click on a bar must select it"
        );
        assert!(
            view.state.form.is_none(),
            "a single click must not open details"
        );
    });
    let enter = KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&enter, cx);
    });
}

#[gpui_kit::test]
fn mouse_up_clears_hover_even_when_the_pointer_stays_on_the_bar(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.hovered_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1").expect("bar");
    cx.simulate_mouse_down(bar.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(bar.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(view.state.gantt.hovered_id, None);
        assert!(view.state.gantt.hover_locked);
    });
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.selected_id,
            Some(1),
            "a click must still select the bar"
        );
    });
    cx.simulate_mouse_move(bar.center(), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.hovered_id,
            Some(1),
            "moving on the bar after mouse-up must restore hover so link handles return"
        );
    });
    assert!(
        cx.debug_bounds("gantt-link-handle-1-link-end").is_some(),
        "a hovered bar must show a link handle"
    );
}

#[gpui_kit::test]
fn empty_platform_team_shows_an_empty_state(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes.clear();
    state.prefs.planner.team = "Platform".into();
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("empty-gantt").is_some(),
        "an empty Platform team must show an empty state, not a blank pane"
    );
}

#[gpui_kit::test]
fn loading_hides_the_empty_state(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes.clear();
    state.hierarchy_loading = true;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-loading-skeleton").is_some(),
        "loading should show the skeleton"
    );
    assert!(
        cx.debug_bounds("empty-gantt").is_none(),
        "empty text must not overlap the loading skeleton"
    );
}

#[gpui_kit::test]
fn switching_team_clears_rows_and_updates_the_label(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    view.update(cx, |view, _| {
        assert!(!view.state.nodes.is_empty());
        view.state.begin_team_switch("Shop".into());
        assert_eq!(view.state.prefs.planner.team, "Shop");
        assert!(view.state.nodes.is_empty());
        assert!(view.state.hierarchy_loading);
    });
    draw(cx);
    assert!(
        cx.debug_bounds("team-trigger").is_some(),
        "team picker should still paint after the switch"
    );
}

#[gpui_kit::test]
fn form_hides_the_team_settings_banner(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.team_settings_error = Some("Team \"Platform\" has invalid backlog settings.".into());
    let mut values = serde_json::Map::new();
    values.insert("System.Title".into(), serde_json::json!("Persist cart"));
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
        cx.debug_bounds("work-item-form").is_some(),
        "details dialog should paint"
    );
    assert!(
        cx.debug_bounds("team-settings-error").is_none(),
        "the banner must not show through the modal"
    );
}

#[gpui_kit::test]
fn few_rows_fill_the_timeline_with_grid_shading(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes.truncate(2);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-grid-fill").is_some(),
        "few rows must fill the rest of the pane with shaded grid rows"
    );
    let body = cx
        .debug_bounds("gantt-timeline-body")
        .expect("timeline body");
    assert!(
        f32::from(body.size.height) > 200.0,
        "timeline body should fill the pane, got {}",
        f32::from(body.size.height)
    );
}

#[gpui_kit::test]
fn splitter_has_no_white_tick_on_the_divider(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-splitter").is_some(),
        "the tree splitter hit target must still paint"
    );
    assert!(
        cx.debug_bounds("gantt-splitter-grip").is_none(),
        "the list/timeline divider must not draw a white tick"
    );
}

#[gpui_kit::test]
fn current_team_shows_a_checkmark_in_the_picker(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.teams = vec![
        ado_core::Team {
            id: "1".into(),
            name: "Platform".into(),
        },
        ado_core::Team {
            id: "2".into(),
            name: "Shop".into(),
        },
    ];
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    let trigger = cx.debug_bounds("team-trigger").expect("team trigger");
    cx.simulate_click(trigger.center(), Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("scope-current").is_some(),
        "the current team must show a checkmark in the picker"
    );
}

#[gpui_kit::test]
fn one_ctrl_z_reverts_exactly_one_dates_entry(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    for after in [
        "2026-07-08T00:00:00Z",
        "2026-07-10T00:00:00Z",
        "2026-07-12T00:00:00Z",
    ] {
        state.history.undo.push(HistoryEntry::Dates {
            id: 1,
            before_start: Some("2026-07-06T00:00:00Z".into()),
            before_target: Some("2026-07-20T00:00:00Z".into()),
            after_start: Some(after.into()),
            after_target: Some("2026-07-20T00:00:00Z".into()),
        });
    }
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    let undo = KeyDownEvent {
        keystroke: Keystroke::parse("ctrl-z").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&undo, cx);
        view.handle_key(&undo, cx);
        assert_eq!(
            view.state.history.undo.len(),
            2,
            "intercept+capture of one Ctrl+Z must pop exactly one Dates entry, got {}",
            view.state.history.undo.len()
        );
        assert_eq!(view.state.history.redo.len(), 1);
    });
}

#[gpui_kit::test]
fn held_ctrl_z_does_not_repeat_undo(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.history.undo.push(HistoryEntry::Dates {
        id: 1,
        before_start: Some("2026-07-06T00:00:00Z".into()),
        before_target: Some("2026-07-20T00:00:00Z".into()),
        after_start: Some("2026-07-08T00:00:00Z".into()),
        after_target: Some("2026-07-20T00:00:00Z".into()),
    });
    state.history.undo.push(HistoryEntry::Dates {
        id: 1,
        before_start: Some("2026-07-08T00:00:00Z".into()),
        before_target: Some("2026-07-20T00:00:00Z".into()),
        after_start: Some("2026-07-10T00:00:00Z".into()),
        after_target: Some("2026-07-20T00:00:00Z".into()),
    });
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    let held = KeyDownEvent {
        keystroke: Keystroke::parse("ctrl-z").unwrap(),
        is_held: true,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&held, cx);
        assert_eq!(
            view.state.history.undo.len(),
            2,
            "key-repeat Ctrl+Z must not pop a second undo entry"
        );
    });
}

#[gpui_kit::test]
fn hovered_bar_shows_round_link_handles_not_resize_bars(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(2);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-link-handle-2-link-end").is_none(),
        "selection alone must not show link handles"
    );
    view.update(cx, |view, cx| {
        view.state.gantt.hovered_id = Some(2);
        cx.notify();
    });
    draw(cx);
    let link = cx
        .debug_bounds("gantt-link-handle-2-link-end")
        .expect("hovered bar must show a link handle");
    let resize = cx
        .debug_bounds("gantt-resize-handle-2-end")
        .expect("hovered leaf bar must show a resize handle");
    assert!(
        (f32::from(link.size.width) - f32::from(link.size.height)).abs() < 0.5,
        "link handle hit area must be square/circular, got {}×{}",
        f32::from(link.size.width),
        f32::from(link.size.height)
    );
    assert!(
        (f32::from(resize.size.width) - HANDLE_WIDTH).abs() < 0.5,
        "resize handle must stay a {}px end cap, got {}",
        HANDLE_WIDTH,
        f32::from(resize.size.width)
    );
    assert!(
        (f32::from(link.size.width) - f32::from(resize.size.width)).abs() > 4.0,
        "link handles must not share the resize-handle size"
    );
    let bar = cx.debug_bounds("gantt-bar-2").expect("hovered bar");
    let start = cx
        .debug_bounds("gantt-resize-handle-2-start")
        .expect("hovered leaf bar must show a start resize handle");
    assert!(
        f32::from(start.origin.x) < f32::from(bar.origin.x) - 1.0,
        "start resize grip must extend outside the bar, handle x {} vs bar x {}",
        f32::from(start.origin.x),
        f32::from(bar.origin.x)
    );
    assert!(
        f32::from(resize.origin.x) + 1.0
            >= f32::from(bar.origin.x) + f32::from(bar.size.width) - 1.0,
        "end resize grip must sit on or outside the bar's right edge"
    );
}

#[gpui_kit::test]
fn offscreen_bar_title_stays_on_the_timeline(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 1) {
        node.title = "Checkout rewrite".into();
        node.start_date = Some("2026-07-01T00:00:00Z".into());
        node.target_date = Some("2026-09-30T00:00:00Z".into());
    }
    state.gantt.scale = GanttScale::Month;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, _| {
        view.state.gantt.go_today(Date::new(2026, 11, 2).unwrap());
    });
    draw(cx);
    let body = cx
        .debug_bounds("gantt-timeline-body")
        .expect("timeline body");
    let left = f32::from(body.origin.x);
    if let Some(label) = cx
        .debug_bounds("gantt-bar-label-1")
        .or_else(|| cx.debug_bounds("gantt-bar-label"))
    {
        let label_x = f32::from(label.origin.x);
        assert!(
            label_x + 1.0 >= left,
            "outside title must not clip from the left (got {label_x}, timeline {left})"
        );
    }
}

#[gpui_kit::test]
fn assignee_search_filters_members_by_name(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.members = vec![
        IdentityValue {
            display_name: "Alan Turing".into(),
            unique_name: "alan@contoso.com".into(),
            id: None,
        },
        IdentityValue {
            display_name: "Ada Lovelace".into(),
            unique_name: "ada@contoso.com".into(),
            id: None,
        },
    ];
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    let trigger = cx
        .debug_bounds("assignee-trigger")
        .expect("assignee trigger");
    cx.simulate_click(trigger.center(), Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("assignee-option-alan@contoso.com")
            .is_some(),
        "Alan should list before searching"
    );
    assert!(
        cx.debug_bounds("assignee-option-ada@contoso.com").is_some(),
        "Ada should list before searching"
    );
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_scope_search("Alan", window, cx);
        });
    });
    draw(cx);
    assert!(
        cx.debug_bounds("assignee-option-alan@contoso.com")
            .is_some(),
        "searching Alan must keep Alan Turing"
    );
    assert!(
        cx.debug_bounds("assignee-option-ada@contoso.com").is_none(),
        "searching Alan must hide Ada Lovelace"
    );
}

#[gpui_kit::test]
fn team_switch_loading_does_not_reset_the_timeline(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.origin = Date::new(2026, 5, 1).unwrap();
    state.gantt.scroll_x = 96.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    view.update(cx, |view, _| {
        view.state.begin_team_switch("Shop".into());
    });
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.origin,
            Date::new(2026, 5, 1).unwrap(),
            "an in-flight team switch must not recentre the timeline"
        );
        assert_eq!(view.state.gantt.scroll_x, 96.0);
    });
}

#[gpui_kit::test]
fn dates_picker_has_month_arrows_and_typed_input(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let cell = cx
        .debug_bounds("gantt-dates-cell")
        .expect("dates cell should paint");
    cx.simulate_click(cell.center(), gpui_kit::Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-dates-prev-month").is_some(),
        "range picker must show a previous-month arrow"
    );
    assert!(
        cx.debug_bounds("gantt-dates-next-month").is_some(),
        "range picker must show a next-month arrow"
    );
    assert!(
        cx.debug_bounds("gantt-dates-typed").is_some(),
        "range picker must allow typed dates"
    );
    let before = view.update(cx, |view, cx| view.inline_calendar_month(cx));
    let prev = cx
        .debug_bounds("gantt-dates-prev-month")
        .expect("prev month");
    cx.simulate_click(prev.center(), gpui_kit::Modifiers::default());
    draw(cx);
    let after = view.update(cx, |view, cx| view.inline_calendar_month(cx));
    assert_ne!(
        before, after,
        "clicking ‹ must change the visible month, {before:?} -> {after:?}"
    );
}

#[gpui_kit::test]
fn parent_summary_bar_shows_link_handles_without_resize(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(1);
    state.gantt.hovered_id = Some(1);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-link-handle-1-link-end").is_some(),
        "a hovered parent summary bar must show a link handle"
    );
    assert!(
        cx.debug_bounds("gantt-resize-handle-1-end").is_none(),
        "a parent summary bar must not grow resize handles"
    );
}

#[gpui_kit::test]
fn resize_starts_on_mouse_down_without_hover(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-resize-handle-2-end").is_none(),
        "resize grips stay hidden until hover"
    );
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    let grip = point(
        bar.origin.x + bar.size.width + px(4.),
        bar.origin.y + bar.size.height / 2.0,
    );
    cx.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("mouse-down on the end grip must start a resize");
        assert_eq!(
            gesture.kind,
            GestureKind::ResizeEnd,
            "hit-testing on mouse-down must pick the end handle without a prior hover, got {:?}",
            gesture.kind
        );
        assert_eq!(gesture.id, 2);
    });
}
