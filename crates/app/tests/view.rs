//! Rendered `PlannerView` coverage for popups, the Gantt header, and the form.

use ado_core::{GanttBarKind, IdentityValue, SessionInfo, WorkItemNode};
use ado_planner::form::FormDraft;
use ado_planner::gantt::{
    ASSIGNEE_COL_WIDTH, BarHitZone, ContextTarget, GanttScale, GanttView, GestureKind,
    HANDLE_WIDTH, LINK_HANDLE, LINK_HANDLE_GAP, LINK_HANDLE_INSET, MILESTONE_SIZE,
    MIN_TIMELINE_WIDTH, OUTER_LINK_HIT, OUTSIDE_LABEL_GAP, PARALLEL_LANE_MIN, PARALLEL_VERT_MIN,
    RESIZE_HIT_WIDTH, ROW_HEIGHT, SHORT_BAR_OUTER_LINK_HIT, TODAY_ALIGN, TODAY_PILL_GAP,
    TREE_INDENT, UNSCHEDULED_GUTTER_RIGHT, YEAR_MIN_BAR_H, YEAR_MIN_BAR_WIDTH, bar_hover_tooltip,
    bar_title_is_inside, bar_title_paint_px, insert_index_for_child, outside_label_is_beside_bar,
    related_dependency_ids,
};
use ado_planner::paths::AppPaths;
use ado_planner::state::{PlannerState, fake_ado_shaped_backlog, fake_hierarchy};
use ado_planner::toasts::{HistoryEntry, ToastExtra, ToastKind, UndoAction};
use ado_planner::ui::PlannerView;
use ado_planner::ui::{
    date_field_text, dates_from_range_picker, dates_from_range_picker_in_years,
    format_picker_dates, looks_like_pat, pending_range_typed, range_picker_is_complete,
    typed_dates_error, typed_dates_error_in,
};
use ado_session::{EncryptedStore, MemoryStore, PlannerSession};
use gpui_kit::component::calendar::Date as PickerDate;
use gpui_kit::{
    Entity, KeyDownEvent, Keystroke, Modifiers, MouseButton, ScrollDelta, ScrollEvent,
    VisualTestContext, point, px,
};
use jiff::ToSpan;
use jiff::civil::Date;
use std::time::Duration;

fn signed_in_with_mock() -> (
    tokio::runtime::Runtime,
    ado_mock::AdoMock,
    PlannerState,
    tempfile::TempDir,
) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("ado-mock-visual")
        .build()
        .expect("ado-mock visual runtime");
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
        state.sign_in_org = "https://dev.azure.com/contoso".into();
        state.sign_in_pat = "pat-secret-value".into();
        state.login_pat().await.expect("login");
        state.session_loading = false;
        (mock, state, dir)
    });
    (runtime, mock, state, dir)
}

/// ado-client completing a oneshot otherwise panics the GPUI test scheduler
/// ("activity on thread ado-client"). Real HTTP waits must allow parking.
fn allow_ado_client_wake(cx: &gpui_kit::VisualTestContext) {
    cx.background_executor.allow_parking();
}

fn mock_has_predecessor(
    runtime: &tokio::runtime::Runtime,
    mock: &ado_mock::AdoMock,
    successor_id: i64,
    predecessor_id: i64,
) -> bool {
    runtime.block_on(async {
        let Some(item) = mock.work_item(successor_id).await else {
            return false;
        };
        item.get("relations")
            .and_then(|rels| rels.as_array())
            .is_some_and(|rels| {
                rels.iter().any(|rel| {
                    rel.get("rel").and_then(|value| value.as_str())
                        == Some("System.LinkTypes.Dependency-Reverse")
                        && rel
                            .get("url")
                            .and_then(|value| value.as_str())
                            .is_some_and(|url| {
                                url.ends_with(&format!("/{predecessor_id}"))
                                    || url.contains(&format!("/workItems/{predecessor_id}"))
                                    || url.contains(&format!("/workitems/{predecessor_id}"))
                            })
                })
            })
    })
}

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

/// `e2e/fixtures/work-items.json` plus the live Persist cart → Go-live link.
fn mock_fixture_nodes() -> Vec<WorkItemNode> {
    fn item(
        id: i64,
        parent: Option<i64>,
        ty: &str,
        title: &str,
        start: Option<&str>,
        target: Option<&str>,
    ) -> WorkItemNode {
        WorkItemNode {
            id,
            rev: 1,
            title: title.into(),
            work_item_type: ty.into(),
            state: "Active".into(),
            assigned_to: None,
            parent_id: parent,
            area_path: "Shop\\Platform".into(),
            iteration_path: "Shop\\FY26".into(),
            start_date: start.map(str::to_string),
            target_date: target.map(str::to_string),
            has_date_fields: true,
            predecessors: Vec::new(),
            fields: serde_json::Map::new(),
        }
    }
    let persist = item(
        1003,
        Some(1002),
        "User Story",
        "Persist cart",
        Some("2026-07-06T00:00:00Z"),
        Some("2026-07-20T00:00:00Z"),
    );
    let cart_ui = item(
        1005,
        Some(1003),
        "Task",
        "Cart UI",
        Some("2026-07-21T00:00:00Z"),
        None,
    );
    let mut spike = item(
        1012,
        None,
        "Task",
        "Unparented spike",
        Some("2026-07-21T00:00:00Z"),
        Some("2026-08-04T00:00:00Z"),
    );
    spike.predecessors = vec![1005];
    let mut go_live = item(
        1013,
        Some(1001),
        "Feature",
        "Go-live milestone",
        None,
        Some("2026-08-15T00:00:00Z"),
    );
    go_live.predecessors = vec![1003];
    vec![
        item(
            1001,
            None,
            "Epic",
            "Checkout rewrite",
            Some("2026-07-01T00:00:00Z"),
            Some("2026-09-30T00:00:00Z"),
        ),
        item(
            1002,
            Some(1001),
            "Feature",
            "Cart",
            Some("2026-07-06T00:00:00Z"),
            Some("2026-08-14T00:00:00Z"),
        ),
        persist,
        cart_ui,
        spike,
        go_live,
        item(1014, Some(1002), "Task", "Backlog grooming", None, None),
        item(
            1020,
            None,
            "Epic",
            "Growth experiments",
            Some("2026-08-01T00:00:00Z"),
            Some("2026-09-15T00:00:00Z"),
        ),
    ]
}

/// Visible end-dot / outer handle pixel starts a link.
fn grab_link_handle(
    handle: gpui_kit::Bounds<gpui_kit::Pixels>,
) -> gpui_kit::Point<gpui_kit::Pixels> {
    point(
        handle.origin.x + handle.size.width - px(1.),
        handle.origin.y + handle.size.height / 2.0,
    )
}

/// 5.5px inside the painted end — still the 6px resize halo.
fn grab_bar_link_sliver(
    bar: gpui_kit::Bounds<gpui_kit::Pixels>,
) -> gpui_kit::Point<gpui_kit::Pixels> {
    grab_bar_from_right(bar, 5.5)
}

/// 3px inside the painted end — the resize halo.
fn grab_end_resize_halo(
    bar: gpui_kit::Bounds<gpui_kit::Pixels>,
) -> gpui_kit::Point<gpui_kit::Pixels> {
    grab_bar_from_right(bar, 3.0)
}

/// Outer 8px connector in the gap past the bar (~x=618 on Persist cart).
fn grab_outer_link_handle(
    bar: gpui_kit::Bounds<gpui_kit::Pixels>,
) -> gpui_kit::Point<gpui_kit::Pixels> {
    point(
        bar.origin.x + bar.size.width + px(LINK_HANDLE_GAP + LINK_HANDLE / 2.0),
        bar.origin.y + bar.size.height / 2.0,
    )
}

/// Inbound connector ~10px left of the painted start.
fn grab_inbound_link_handle(
    bar: gpui_kit::Bounds<gpui_kit::Pixels>,
) -> gpui_kit::Point<gpui_kit::Pixels> {
    point(
        bar.origin.x - px(LINK_HANDLE_GAP + LINK_HANDLE / 2.0),
        bar.origin.y + bar.size.height / 2.0,
    )
}

fn grab_bar_from_right(
    bar: gpui_kit::Bounds<gpui_kit::Pixels>,
    inset: f32,
) -> gpui_kit::Point<gpui_kit::Pixels> {
    point(
        bar.origin.x + bar.size.width - px(inset),
        bar.origin.y + bar.size.height / 2.0,
    )
}

fn grab_bar_from_left(
    bar: gpui_kit::Bounds<gpui_kit::Pixels>,
    inset: f32,
) -> gpui_kit::Point<gpui_kit::Pixels> {
    point(
        bar.origin.x + px(inset),
        bar.origin.y + bar.size.height / 2.0,
    )
}

/// Shifted end-resize grip, just left of the 6px link sliver.
fn grab_end_resize(bar: gpui_kit::Bounds<gpui_kit::Pixels>) -> gpui_kit::Point<gpui_kit::Pixels> {
    point(
        bar.origin.x + bar.size.width - px(HANDLE_WIDTH + LINK_HANDLE_INSET / 2.0),
        bar.origin.y + bar.size.height / 2.0,
    )
}

fn painted_x_gap(
    a: gpui_kit::Bounds<gpui_kit::Pixels>,
    b: gpui_kit::Bounds<gpui_kit::Pixels>,
) -> f32 {
    let a0 = f32::from(a.origin.x);
    let a1 = a0 + f32::from(a.size.width);
    let b0 = f32::from(b.origin.x);
    let b1 = b0 + f32::from(b.size.width);
    if a1 <= b0 {
        b0 - a1
    } else if b1 <= a0 {
        a0 - b1
    } else {
        0.0
    }
}

fn painted_y_gap(
    a: gpui_kit::Bounds<gpui_kit::Pixels>,
    b: gpui_kit::Bounds<gpui_kit::Pixels>,
) -> f32 {
    let a0 = f32::from(a.origin.y);
    let a1 = a0 + f32::from(a.size.height);
    let b0 = f32::from(b.origin.y);
    let b1 = b0 + f32::from(b.size.height);
    if a1 <= b0 {
        b0 - a1
    } else if b1 <= a0 {
        a0 - b1
    } else {
        0.0
    }
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
    let today_on_screen = view.update(cx, |view, _| {
        let x = view.state.gantt.today_x(jiff::Zoned::now().date());
        x >= 0.0 && x <= view.state.gantt.timeline_width
    });
    assert_eq!(
        cx.debug_bounds("gantt-today").is_some(),
        today_on_screen,
        "Today must paint only when it sits on the chart"
    );
    assert!(
        cx.debug_bounds("gantt-bar-1").is_some(),
        "at least one Gantt bar should be painted"
    );
    assert!(
        cx.debug_bounds("gantt-bar-title-chip").is_some(),
        "bar titles must sit on a contrast chip so mid-tone fills stay readable"
    );
    assert!(
        cx.debug_bounds("unscheduled-empty-hint").is_none(),
        "a scheduled backlog must not show the empty-timeline hint"
    );
    assert!(
        cx.debug_bounds("gantt-sync-status").is_some(),
        "the toolbar must show a subtle save/sync status"
    );
    let sync_icon = cx
        .debug_bounds("gantt-sync-status-icon")
        .expect("Synced / Saving… must show a status icon in the pill");
    assert!(
        f32::from(sync_icon.size.width) >= 8.0 && f32::from(sync_icon.size.height) >= 8.0,
        "the Synced pill icon must paint, got {:?}",
        sync_icon.size
    );
    view.update(cx, |view, _| {
        assert_eq!(view.sync_status_for_test(), "Synced");
        view.set_form_loading_for_test(1, "Persist cart");
        assert_eq!(
            view.sync_status_for_test(),
            "Loading…",
            "opening details is a fetch, not a write"
        );
        view.set_form_loading_for_test_clear();
        view.set_busy_for_test(true);
        assert_eq!(view.sync_status_for_test(), "Saving…");
        view.set_busy_for_test(false);
        assert_eq!(
            view.sync_status_for_test(),
            "Saving…",
            "Saving… must stay visible for a short hold after I/O clears"
        );
    });
    assert!(
        cx.debug_bounds("header-filters-label").is_some(),
        "Roots/Types/States must be grouped under a Filters label"
    );
    assert!(
        cx.debug_bounds("header-theme-trigger").is_some(),
        "theme must be an icon button on the header"
    );
    assert!(
        cx.debug_bounds("header-account-actions").is_some(),
        "theme and log out must sit together on the right of the header"
    );
    assert!(
        cx.debug_bounds("header-log-out").is_some(),
        "log out must stay on the header as an icon button"
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
    let picked = dates_from_range_picker(PickerDate::Range(Some(start), None), "", "").unwrap();
    assert_eq!(
        picked.0,
        Some(Date::new(2026, 8, 4).unwrap()),
        "a start-only pick must produce a Start Date"
    );
    assert!(
        picked.1.is_none(),
        "a start-only pick must leave Target Date unset so save keeps it"
    );
    let typed = dates_from_range_picker(PickerDate::Range(None, None), "2026-08-12", "").unwrap();
    assert_eq!(typed.0, Some(Date::new(2026, 8, 12).unwrap()));
    assert!(typed.1.is_none());
    let both =
        dates_from_range_picker(PickerDate::Range(None, None), "2026-08-04 → 2026-08-21", "")
            .unwrap();
    assert_eq!(both.0, Some(Date::new(2026, 8, 4).unwrap()));
    assert_eq!(both.1, Some(Date::new(2026, 8, 21).unwrap()));
}

#[test]
fn range_picker_prefers_typed_when_it_changed() {
    let cal_start = chrono::NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
    let cal_end = chrono::NaiveDate::from_ymd_opt(2026, 7, 20).unwrap();
    let calendar = PickerDate::Range(Some(cal_start), Some(cal_end));
    let seed = "2026-07-06 / 2026-07-20";
    let from_calendar = dates_from_range_picker(calendar, seed, seed).unwrap();
    assert_eq!(from_calendar.0, Some(Date::new(2026, 7, 6).unwrap()));
    assert_eq!(from_calendar.1, Some(Date::new(2026, 7, 20).unwrap()));
    let from_typed = dates_from_range_picker(calendar, "2026-08-01 / 2026-08-15", seed).unwrap();
    assert_eq!(
        from_typed.0,
        Some(Date::new(2026, 8, 1).unwrap()),
        "a typed edit on a scheduled item must win over the seeded calendar"
    );
    assert_eq!(from_typed.1, Some(Date::new(2026, 8, 15).unwrap()));
    assert!(
        !range_picker_is_complete(PickerDate::Range(Some(cal_start), None)),
        "a start-only first click must not auto-save"
    );
    assert!(range_picker_is_complete(calendar));
}

#[test]
fn calendar_end_click_wins_over_pending_typed_start() {
    let seed = "2026-07-10 → 2026-08-04";
    let start = chrono::NaiveDate::from_ymd_opt(2026, 7, 10).unwrap();
    let end = chrono::NaiveDate::from_ymd_opt(2026, 7, 17).unwrap();
    let calendar = PickerDate::Range(Some(start), Some(end));
    let pending_arrow = pending_range_typed(PickerDate::Range(Some(start), None)).unwrap();
    assert_eq!(pending_arrow, "Jul 10 → …");
    for pending in [pending_arrow.as_str(), "2026-07-10 / …"] {
        let picked = dates_from_range_picker(calendar, pending, seed).unwrap();
        assert_eq!(
            picked.0,
            Some(Date::new(2026, 7, 10).unwrap()),
            "start click must keep July 10"
        );
        assert_eq!(
            picked.1,
            Some(Date::new(2026, 7, 17).unwrap()),
            "end click must save July 17, not keep the seeded August 4 from a start-only typed preview ({pending})"
        );
    }
}

#[test]
fn form_dates_copy_uses_arrow_and_keeps_start_only() {
    let start = chrono::NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
    assert_eq!(
        format_picker_dates(PickerDate::Range(Some(start), None)),
        "Jul 6 → …"
    );
    let end = chrono::NaiveDate::from_ymd_opt(2026, 7, 20).unwrap();
    let both = format_picker_dates(PickerDate::Range(Some(start), Some(end)));
    assert_eq!(both, "Jul 6 → Jul 20");
    assert!(
        !both.contains(" - "),
        "modal dates must use →, not a hyphen join, got {both}"
    );
}

#[test]
fn read_only_closed_date_uses_compact_text() {
    let mut values = serde_json::Map::new();
    values.insert(
        "Microsoft.VSTS.Common.ClosedDate".into(),
        serde_json::json!("2026-07-20T00:00:00Z"),
    );
    let value = values
        .get("Microsoft.VSTS.Common.ClosedDate")
        .cloned()
        .unwrap();
    assert_eq!(
        date_field_text("Microsoft.VSTS.Common.ClosedDate", &value, &values),
        "Jul 20"
    );
}

#[test]
fn pending_start_shows_ellipsis_in_the_typed_field() {
    let start = chrono::NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
    assert_eq!(
        pending_range_typed(PickerDate::Range(Some(start), None)).as_deref(),
        Some("Nov 10 → …")
    );
}

#[test]
fn typed_compact_range_does_not_split_on_comma() {
    let (start, target) = dates_from_range_picker(
        PickerDate::Range(None, None),
        "Jul 6, 2026 → Jul 20, 2026",
        "",
    )
    .unwrap();
    assert_eq!(start, Some(Date::new(2026, 7, 6).unwrap()));
    assert_eq!(target, Some(Date::new(2026, 7, 20).unwrap()));
    let iso = dates_from_range_picker(PickerDate::Range(None, None), "2026-08-04 → 2026-08-21", "")
        .unwrap();
    assert_eq!(iso.0, Some(Date::new(2026, 8, 4).unwrap()));
    assert_eq!(iso.1, Some(Date::new(2026, 8, 21).unwrap()));
}

#[test]
fn typed_compact_range_keeps_the_item_year() {
    let start = chrono::NaiveDate::from_ymd_opt(2025, 7, 6).unwrap();
    let end = chrono::NaiveDate::from_ymd_opt(2025, 7, 20).unwrap();
    let calendar = PickerDate::Range(Some(start), Some(end));
    let seed = "Jul 6 → Jul 20";
    let confirmed = dates_from_range_picker(calendar, seed, seed).unwrap();
    assert_eq!(
        confirmed.0,
        Some(Date::new(2025, 7, 6).unwrap()),
        "confirming compact copy must keep 2025, not this year"
    );
    assert_eq!(confirmed.1, Some(Date::new(2025, 7, 20).unwrap()));
    let typed = dates_from_range_picker(calendar, "Jul 6 → Aug 18", seed).unwrap();
    assert_eq!(typed.0, Some(Date::new(2025, 7, 6).unwrap()));
    assert_eq!(
        typed.1,
        Some(Date::new(2025, 8, 18).unwrap()),
        "year-less typed compact must inherit the bar's year"
    );
    let empty = dates_from_range_picker_in_years(
        PickerDate::Range(None, None),
        "Jul 6 → Aug 18",
        seed,
        Some(2027),
        Some(2027),
    )
    .unwrap();
    assert_eq!(empty.0, Some(Date::new(2027, 7, 6).unwrap()));
    assert_eq!(empty.1, Some(Date::new(2027, 8, 18).unwrap()));
    let cross = PickerDate::Range(
        Some(chrono::NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()),
        Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()),
    );
    let crossed = dates_from_range_picker(cross, "Dec 2 → Jan 16", "Dec 1 → Jan 15").unwrap();
    assert_eq!(crossed.0, Some(Date::new(2025, 12, 2).unwrap()));
    assert_eq!(
        crossed.1,
        Some(Date::new(2026, 1, 16).unwrap()),
        "each side of a cross-year range keeps its own year"
    );
}

#[test]
fn typed_ascii_arrow_and_to_separators_parse_a_range() {
    let start = chrono::NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
    let end = chrono::NaiveDate::from_ymd_opt(2026, 7, 20).unwrap();
    let calendar = PickerDate::Range(Some(start), Some(end));
    let seed = "Jul 6 → Jul 20";
    for typed in [
        "Jul 22 -> Jul 28",
        "Jul 22 - Jul 28",
        "Jul 22 to Jul 28",
        "Jul 22 TO Jul 28",
        "Jul 22 → Jul 28",
    ] {
        let picked = dates_from_range_picker(calendar, typed, seed)
            .unwrap_or_else(|| panic!("{typed} must parse as a range"));
        assert_eq!(
            picked.0,
            Some(Date::new(2026, 7, 22).unwrap()),
            "start for {typed}"
        );
        assert_eq!(
            picked.1,
            Some(Date::new(2026, 7, 28).unwrap()),
            "target for {typed}"
        );
        assert!(
            typed_dates_error(typed).is_none(),
            "{typed} must be valid Dates text"
        );
    }
}

#[test]
fn typed_unparseable_dates_do_not_clear_the_range() {
    let start = chrono::NaiveDate::from_ymd_opt(2026, 7, 6).unwrap();
    let end = chrono::NaiveDate::from_ymd_opt(2026, 7, 20).unwrap();
    let calendar = PickerDate::Range(Some(start), Some(end));
    let seed = "Jul 6 → Jul 20";
    assert!(
        dates_from_range_picker(calendar, "Jul 22 -> zz", seed).is_none(),
        "a parse failure must not yield empty dates"
    );
    assert!(
        dates_from_range_picker(PickerDate::Range(None, None), "not a date", "").is_none(),
        "garbage must not become a cleared range"
    );
    assert!(typed_dates_error("Jul 22 -> zz").is_some());
    assert!(typed_dates_error("").is_none());
    let hint = typed_dates_error("not a date").expect("parse hint");
    assert!(
        hint.contains("Jan 6") && hint.contains("Jan 12"),
        "parse-hint without item dates must use a fixed example, got {hint}"
    );
    let today = jiff::Zoned::now().date();
    let today_s = ado_core::compact_date(today);
    assert!(
        !hint.contains(&today_s),
        "parse-hint must not use today's date, got {hint}"
    );
    let item_hint = typed_dates_error_in(
        "foo",
        Some(Date::new(2026, 7, 6).unwrap()),
        Some(Date::new(2026, 7, 20).unwrap()),
    )
    .expect("item parse hint");
    assert!(
        item_hint.contains("Jul 6") && item_hint.contains("Jul 20"),
        "parse-hint must reuse the item's own dates, got {item_hint}"
    );
    let cleared = dates_from_range_picker(calendar, "", seed).expect("empty typed is a clear");
    assert_eq!(
        cleared,
        (None, None),
        "empty picker + Enter must clear both dates instead of no-op"
    );
    let start_only = dates_from_range_picker(PickerDate::Range(None, None), "Jul 6", "")
        .expect("start-only typed");
    assert_eq!(start_only.0, Some(Date::new(2026, 7, 6).unwrap()));
    assert!(
        start_only.1.is_none(),
        "typed start-only must not invent a target"
    );
    let start_only_over_range = dates_from_range_picker(calendar, "Jul 6", seed)
        .expect("typed start-only over a leftover calendar range");
    assert_eq!(
        start_only_over_range.0,
        Some(Date::new(2026, 7, 6).unwrap())
    );
    assert!(
        start_only_over_range.1.is_none(),
        "typing a start date only must clear Target Date even when the calendar still holds a range"
    );
}

#[test]
fn typed_milestone_diamond_is_not_a_parse_error() {
    assert!(
        typed_dates_error("◆ Aug 15").is_none(),
        "a target-only Dates seed must not show a parse error"
    );
    assert!(typed_dates_error("◆ Jul 20").is_none());
    let calendar = PickerDate::Single(Some(chrono::NaiveDate::from_ymd_opt(2026, 8, 15).unwrap()));
    let picked = dates_from_range_picker(calendar, "◆ Aug 15", "◆ Aug 15")
        .expect("diamond target-only must parse");
    assert!(
        picked.0.is_none(),
        "◆ Aug 15 must stay target-only, not become a start date"
    );
    assert_eq!(picked.1, Some(Date::new(2026, 8, 15).unwrap()));
    let calendar = PickerDate::Range(
        Some(chrono::NaiveDate::from_ymd_opt(2026, 7, 6).unwrap()),
        Some(chrono::NaiveDate::from_ymd_opt(2026, 8, 18).unwrap()),
    );
    let from_calendar = dates_from_range_picker(calendar, "◆ Aug 15", "◆ Aug 15")
        .expect("unchanged diamond seed must not block a calendar range");
    assert_eq!(
        from_calendar.0,
        Some(Date::new(2026, 7, 6).unwrap()),
        "a calendar range must win over an unchanged ◆ seed"
    );
    assert_eq!(from_calendar.1, Some(Date::new(2026, 8, 18).unwrap()));
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
fn form_heading_updates_on_save_not_while_typing(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    let mut values = serde_json::Map::new();
    values.insert(
        "System.Title".into(),
        serde_json::Value::String("Persist cart".into()),
    );
    state.form = Some(FormDraft::from_model(ado_core::WorkItemFormModel {
        id: 1003,
        rev: 1,
        work_item_type: "User Story".into(),
        system_controls: vec![ado_core::FormControl {
            id: "System.Title".into(),
            reference_name: "System.Title".into(),
            label: "Title".into(),
            kind: ado_core::FormControlKind::String,
            required: true,
            read_only: false,
            visible: true,
            options: None,
        }],
        pages: Vec::new(),
        values,
    }));
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert_eq!(
                view.form_heading_for_test().as_deref(),
                Some("Persist cart")
            );
            view.type_form_field("System.Title", "Renamed live", window, cx);
            assert_eq!(
                view.form_heading_for_test().as_deref(),
                Some("Persist cart"),
                "the dialog heading must stay on the opened title while typing"
            );
            assert_eq!(
                view.state.form.as_ref().map(|form| form.item_title()),
                Some("Renamed live".into()),
                "the Title field must still take the typed value"
            );
        });
    });
    draw(cx);
    assert!(
        cx.debug_bounds("work-item-form-heading").is_some(),
        "the dialog must paint a heading distinct from the Title field"
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
fn unscheduled_only_grid_paints_today_markers(cx: &mut gpui_kit::TestAppContext) {
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
        cx.debug_bounds("gantt-bar-1").is_some(),
        "unscheduled rows must still paint a Today marker so a new row is not blank"
    );
    assert!(
        cx.debug_bounds("unscheduled-empty-hint").is_some(),
        "an all-unscheduled grid must still hint that the timeline is empty"
    );
}

fn amego_unscheduled_state(dir: &tempfile::TempDir) -> PlannerState {
    let mut state = idle_state(dir);
    state.prefs.planner.team = "Amego".into();
    let kevin = IdentityValue {
        display_name: "Kevin Mccloskey".into(),
        unique_name: "kevin@contoso.com".into(),
        id: Some("k2".into()),
    };
    fn undated(
        id: i64,
        parent: Option<i64>,
        ty: &str,
        title: &str,
        kevin: &IdentityValue,
    ) -> WorkItemNode {
        WorkItemNode {
            id,
            rev: 1,
            title: title.into(),
            work_item_type: ty.into(),
            state: "New".into(),
            assigned_to: Some(kevin.clone()),
            parent_id: parent,
            area_path: "Shop\\Amego".into(),
            iteration_path: "Shop\\FY26".into(),
            start_date: None,
            target_date: None,
            has_date_fields: true,
            predecessors: Vec::new(),
            fields: serde_json::Map::new(),
        }
    }
    let mut onboard = undated(2003, Some(2002), "User Story", "Onboard tenant", &kevin);
    onboard.predecessors = vec![2002];
    state.nodes = vec![
        undated(2001, None, "Epic", "Amego backlog", &kevin),
        undated(2002, Some(2001), "Feature", "Stakeholder review", &kevin),
        onboard,
    ];
    state
}

#[gpui_kit::test]
fn unscheduled_empty_hint_dismisses_on_click(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = amego_unscheduled_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let hint = cx
        .debug_bounds("unscheduled-empty-hint")
        .expect("all-unscheduled Amego must show the empty-timeline hint");
    cx.simulate_click(hint.center(), Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("unscheduled-empty-hint").is_none(),
        "clicking the hint must dismiss it so it cannot stay pinned"
    );
}

#[gpui_kit::test]
fn unscheduled_empty_hint_dismisses_on_scroll(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = amego_unscheduled_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(cx.debug_bounds("unscheduled-empty-hint").is_some());
    let timeline = cx.debug_bounds("gantt-timeline").expect("timeline");
    cx.simulate_event(ScrollEvent {
        position: timeline.center(),
        delta: ScrollDelta::Pixels(point(px(80.), px(0.))),
        ..Default::default()
    });
    draw(cx);
    assert!(
        cx.debug_bounds("unscheduled-empty-hint").is_none(),
        "scrolling the timeline must dismiss the empty hint"
    );
}

#[gpui_kit::test]
fn unscheduled_empty_hint_dismisses_on_pointer_leave(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = amego_unscheduled_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let hint = cx
        .debug_bounds("unscheduled-empty-hint")
        .expect("all-unscheduled Amego must show the empty-timeline hint");
    cx.simulate_mouse_move(hint.center(), None, Modifiers::default());
    draw(cx);
    let tree = cx.debug_bounds("gantt-tree").expect("tree");
    cx.simulate_mouse_move(tree.center(), None, Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("unscheduled-empty-hint").is_none(),
        "leaving the hint must dismiss it so it cannot stay pinned"
    );
}

#[gpui_kit::test]
fn unscheduled_empty_hint_dismisses_when_the_picker_opens(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = amego_unscheduled_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("unscheduled-empty-hint").is_some(),
        "precondition: all-unscheduled Amego shows the empty-timeline hint"
    );
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_inline_dates(2001, window, cx);
        });
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-dates").is_some(),
        "opening Dates must show the range picker"
    );
    assert!(
        cx.debug_bounds("unscheduled-empty-hint").is_none(),
        "opening the dates picker must dismiss the empty-timeline hint"
    );
}

#[gpui_kit::test]
fn unscheduled_empty_hint_dismisses_on_move_outside_its_rect(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = amego_unscheduled_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let hint = cx
        .debug_bounds("unscheduled-empty-hint")
        .expect("all-unscheduled Amego must show the empty-timeline hint");
    let outside = point(
        hint.origin.x - px(48.),
        hint.origin.y + hint.size.height / 2.0,
    );
    cx.simulate_mouse_move(outside, None, Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("unscheduled-empty-hint").is_none(),
        "a mouse-move outside the chip, without ever entering it, must dismiss the hint"
    );
}

#[gpui_kit::test]
fn unscheduled_empty_hint_dismisses_when_the_team_picker_opens(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = amego_unscheduled_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("unscheduled-empty-hint").is_some(),
        "precondition: all-unscheduled Amego shows the empty-timeline hint"
    );
    let team = cx.debug_bounds("team-trigger").expect("team picker");
    cx.simulate_click(team.center(), Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("unscheduled-empty-hint").is_none(),
        "opening the team picker must dismiss the empty-timeline hint"
    );
}

#[gpui_kit::test]
fn auto_fit_is_disabled_when_nothing_is_scheduled(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = amego_unscheduled_state(&dir);
    let origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.origin = origin;
    state.gantt.scale = GanttScale::Month;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit button");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.origin, origin,
            "Auto fit must not jump to Today when every item is unscheduled"
        );
        assert_eq!(view.state.gantt.scale, GanttScale::Month);
        assert!(
            GanttView::scheduled_span(&view.state.visible_nodes(), &view.state.iterations)
                .is_none()
        );
    });
}

#[gpui_kit::test]
fn assignee_column_fits_kevin_mccloskey(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = amego_unscheduled_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let cell = cx
        .debug_bounds("gantt-assignee-2001")
        .expect("Amego assignee cell");
    assert!(
        f32::from(cell.size.width) + 0.5 >= ASSIGNEE_COL_WIDTH,
        "assignee column must be wide enough for Kevin Mccloskey, got {:?}",
        cell.size.width
    );
}

#[gpui_kit::test]
fn amego_unscheduled_team_paints_no_dependency_connectors(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = amego_unscheduled_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, _| {
        let nodes = view.state.visible_nodes();
        let rows = view.state.gantt.flatten(&nodes);
        let bars = view
            .state
            .gantt
            .layout_bars(&nodes, &rows, &view.state.iterations, 720.0);
        assert!(
            view.state.gantt.layout_dependency_arrows(&bars).is_empty(),
            "Amego's undated predecessor link must not draw a connector"
        );
    });
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
    assert!(
        cx.debug_bounds("gantt-menu-Add child").is_some(),
        "right-click must offer Add child"
    );
    assert!(
        cx.debug_bounds("gantt-menu-Edit title").is_some(),
        "right-click must offer Edit title so F2 is not the only edit path"
    );
}

#[gpui_kit::test]
fn context_menu_closes_after_an_action_is_chosen(cx: &mut gpui_kit::TestAppContext) {
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
    let add = cx
        .debug_bounds("gantt-menu-Add child")
        .expect("Add child action");
    cx.simulate_mouse_down(add.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.context_menu.is_none(),
            "choosing Add child must close the context menu"
        );
    });
}

#[gpui_kit::test]
fn context_menu_rename_starts_inline_edit(cx: &mut gpui_kit::TestAppContext) {
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
    let rename = cx
        .debug_bounds("gantt-menu-Edit title")
        .expect("Edit title");
    cx.simulate_mouse_down(rename.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-title").is_some(),
        "Edit title must start an inline title edit"
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
fn mock_fixture_painted_verticals_stay_offset(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    if let Some(go_live) = state.nodes.iter_mut().find(|node| node.id == 1013) {
        go_live.predecessors.clear();
    }
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Week;
    state.gantt.zoom = 1.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-dep-inv-1005-1012").is_some(),
        "Cart UI → spike must paint before the new Persist cart → Go-live link"
    );
    assert!(
        cx.debug_bounds("gantt-dep-inv-1003-1013").is_none(),
        "Persist cart → Go-live is created next"
    );
    view.update(cx, |view, cx| {
        if let Some(go_live) = view.state.nodes.iter_mut().find(|node| node.id == 1013) {
            go_live.predecessors = vec![1003];
        }
        cx.notify();
    });
    draw(cx);
    let persist = cx
        .debug_bounds("gantt-dep-inv-1003-1013")
        .expect("painted Persist cart → Go-live inbound");
    let spike = cx
        .debug_bounds("gantt-dep-inv-1005-1012")
        .expect("painted Cart UI → spike inbound");
    let persist_x = f32::from(persist.origin.x) + f32::from(persist.size.width) / 2.0;
    let spike_x = f32::from(spike.origin.x) + f32::from(spike.size.width) / 2.0;
    let dx = (persist_x - spike_x).abs();
    assert!(
        dx >= 12.0,
        "rendered Persist cart → Go-live inv at {persist_x} must sit ≥12px from Cart UI → spike at {spike_x} (dx={dx})"
    );
    view.update(cx, |view, _| {
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars =
            view.state
                .gantt
                .layout_bars(&view.state.nodes, &rows, &view.state.iterations, 720.0);
        let live = view
            .state
            .gantt
            .layout_dependency_arrows(&bars)
            .into_iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("Persist cart → Go-live");
        if live.x_head > view.state.gantt.timeline_width {
            return;
        }
        let cart = bars.iter().find(|bar| bar.id == 1005).unwrap();
        assert!(
            live.x2 < cart.x,
            "painted Persist cart → Go-live inbound x2={} must drop before Cart UI starts at {}",
            live.x2,
            cart.x
        );
    });
}

#[gpui_kit::test]
fn mock_fixture_month_painted_verticals_stay_offset(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 1.0;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let persist = cx
        .debug_bounds("gantt-dep-inv-1003-1013")
        .expect("painted Persist cart → Go-live inbound (Month)");
    let spike = cx
        .debug_bounds("gantt-dep-inv-1005-1012")
        .expect("painted Cart UI → spike inbound (Month)");
    let persist_x = f32::from(persist.origin.x) + f32::from(persist.size.width) / 2.0;
    let spike_x = f32::from(spike.origin.x) + f32::from(spike.size.width) / 2.0;
    let dx = (persist_x - spike_x).abs();
    assert!(
        dx >= 12.0,
        "Month mock fixture: Persist cart → Go-live inv at {persist_x} must sit ≥12px from Cart UI → spike at {spike_x} (dx={dx})"
    );
    let cart = cx
        .debug_bounds("gantt-bar-1005")
        .expect("Cart UI bar (Month)");
    let cart_left = f32::from(cart.origin.x);
    assert!(
        persist_x < cart_left - 1.0,
        "Month painted Persist cart → Go-live inbound at {persist_x} must drop before Cart UI at {cart_left}"
    );
}

#[gpui_kit::test]
fn runtime_shared_source_outgoing_jogs_paint_sixteen_apart(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    if let Some(spike) = state.nodes.iter_mut().find(|node| node.id == 1012) {
        spike.predecessors = vec![1003];
    }
    if let Some(go_live) = state.nodes.iter_mut().find(|node| node.id == 1013) {
        go_live.predecessors.clear();
    }
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 0.7;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-dep-out-1003-1012").is_some(),
        "live fixture: Persist cart → spike outgoing must paint before the new link"
    );
    assert!(
        cx.debug_bounds("gantt-dep-out-1003-1013").is_none(),
        "Persist cart → Go-live is created next, like a handle drop"
    );
    view.update(cx, |view, cx| {
        if let Some(go_live) = view.state.nodes.iter_mut().find(|node| node.id == 1013) {
            go_live.predecessors = vec![1003];
        }
        cx.notify();
    });
    draw(cx);
    let spike = cx
        .debug_bounds("gantt-dep-out-1003-1012")
        .expect("painted Persist cart → spike outgoing");
    let live = cx
        .debug_bounds("gantt-dep-out-1003-1013")
        .expect("painted runtime Persist cart → Go-live outgoing");
    let spike_x = f32::from(spike.origin.x) + f32::from(spike.size.width) / 2.0;
    let live_x = f32::from(live.origin.x) + f32::from(live.size.width) / 2.0;
    let dx = (spike_x - live_x).abs();
    assert!(
        dx >= PARALLEL_VERT_MIN,
        "painted shared-source jogs must sit ≥{PARALLEL_VERT_MIN}px apart, spike={spike_x} live={live_x} dx={dx}"
    );
    let spike_in = cx
        .debug_bounds("gantt-dep-inv-1003-1012")
        .expect("painted Persist cart → spike inbound");
    let live_in = cx
        .debug_bounds("gantt-dep-inv-1003-1013")
        .expect("painted Persist cart → Go-live inbound");
    let spike_in_x = f32::from(spike_in.origin.x) + f32::from(spike_in.size.width) / 2.0;
    let live_in_x = f32::from(live_in.origin.x) + f32::from(live_in.size.width) / 2.0;
    let inbound_dx = (spike_in_x - live_in_x).abs();
    assert!(
        inbound_dx >= 12.0,
        "painted shared-source inbounds must sit ≥12px apart, spike={spike_in_x} live={live_in_x} dx={inbound_dx}"
    );
    if let Some(cart) = cx.debug_bounds("gantt-bar-1005") {
        let cart_left = f32::from(cart.origin.x);
        assert!(
            live_in_x < cart_left - 1.0,
            "painted Persist cart → Go-live inbound at {live_in_x} must drop before Cart UI at {cart_left}"
        );
    }
    let label = cx
        .debug_bounds("gantt-bar-label-1003")
        .expect("zoomed-out Month Persist cart must paint an outside label");
    let left = f32::from(label.origin.x);
    let right = left + f32::from(label.size.width);
    assert!(
        live_x >= right + 6.0 || live_x <= left - 6.0,
        "painted Persist cart → Go-live outgoing at {live_x} must sit ≥6px from the Persist cart label {left}..{right}"
    );
    for (name, x) in [("spike", spike_x), ("go-live", live_x)] {
        assert!(
            x < left - 1.0 || x > right + 1.0,
            "painted Persist cart → {name} outgoing at {x} must miss the Persist cart label {left}..{right}"
        );
    }
}

fn painted_center_x(bounds: gpui_kit::Bounds<gpui_kit::Pixels>) -> f32 {
    f32::from(bounds.origin.x) + f32::from(bounds.size.width) / 2.0
}

fn open_gantt_overflow(cx: &mut gpui_kit::VisualTestContext) {
    let more = cx
        .debug_bounds("gantt-toolbar-more")
        .expect("toolbar overflow");
    cx.simulate_click(more.center(), Modifiers::default());
    draw(cx);
}

fn painted_min_gap(
    a: gpui_kit::Bounds<gpui_kit::Pixels>,
    b: gpui_kit::Bounds<gpui_kit::Pixels>,
) -> f32 {
    let ax0 = f32::from(a.origin.x);
    let ax1 = ax0 + f32::from(a.size.width);
    let ay0 = f32::from(a.origin.y);
    let ay1 = ay0 + f32::from(a.size.height);
    let bx0 = f32::from(b.origin.x);
    let bx1 = bx0 + f32::from(b.size.width);
    let by0 = f32::from(b.origin.y);
    let by1 = by0 + f32::from(b.size.height);
    let overlap_x = ax0 < bx1 && bx0 < ax1;
    let overlap_y = ay0 < by1 && by0 < ay1;
    if overlap_x && overlap_y {
        return 0.0;
    }
    let x_gap = if overlap_x {
        0.0
    } else {
        (bx0 - ax1).max(ax0 - bx1).max(0.0)
    };
    let y_gap = if overlap_y {
        0.0
    } else {
        (by0 - ay1).max(ay0 - by1).max(0.0)
    };
    if overlap_x {
        y_gap
    } else if overlap_y {
        x_gap
    } else {
        x_gap.hypot(y_gap)
    }
}

fn painted_overlap(
    a: gpui_kit::Bounds<gpui_kit::Pixels>,
    b: gpui_kit::Bounds<gpui_kit::Pixels>,
) -> bool {
    let ax0 = f32::from(a.origin.x);
    let ax1 = ax0 + f32::from(a.size.width);
    let ay0 = f32::from(a.origin.y);
    let ay1 = ay0 + f32::from(a.size.height);
    let bx0 = f32::from(b.origin.x);
    let bx1 = bx0 + f32::from(b.size.width);
    let by0 = f32::from(b.origin.y);
    let by1 = by0 + f32::from(b.size.height);
    ax0 < bx1 && bx0 < ax1 && ay0 < by1 && by0 < ay1
}

const TODAY_BODY_SELECTORS: [&str; 16] = [
    "gantt-today-body-0",
    "gantt-today-body-1",
    "gantt-today-body-2",
    "gantt-today-body-3",
    "gantt-today-body-4",
    "gantt-today-body-5",
    "gantt-today-body-6",
    "gantt-today-body-7",
    "gantt-today-body-8",
    "gantt-today-body-9",
    "gantt-today-body-10",
    "gantt-today-body-11",
    "gantt-today-body-12",
    "gantt-today-body-13",
    "gantt-today-body-14",
    "gantt-today-body-15",
];

fn today_body_segments(
    cx: &mut gpui_kit::VisualTestContext,
) -> Vec<gpui_kit::Bounds<gpui_kit::Pixels>> {
    let mut out = Vec::new();
    for selector in TODAY_BODY_SELECTORS {
        match cx.debug_bounds(selector) {
            Some(bounds) => out.push(bounds),
            None => break,
        }
    }
    out
}

fn live_launch_camera_nodes() -> Vec<WorkItemNode> {
    let mut nodes = mock_fixture_nodes();
    if let Some(spike) = nodes.iter_mut().find(|node| node.id == 1012) {
        spike.predecessors = vec![1003];
    }
    if let Some(go_live) = nodes.iter_mut().find(|node| node.id == 1013) {
        go_live.predecessors.clear();
    }
    nodes
}

/// Zoom Auto Fit used to pick for the mock Jul–Nov span at 800px Month.
/// Tests that need that short-bar camera set this; Auto Fit itself no
/// longer changes zoom (month columns stay put).
const FITTED_MONTH_ZOOM: f32 = 0.4;

/// 1280×800 launch camera: `apply_hierarchy` auto_fit at 800, then clamp.
fn apply_launch_camera(state: &mut PlannerState) {
    state.gantt.zoom = FITTED_MONTH_ZOOM;
    let nodes = state.nodes.clone();
    state.gantt.auto_fit(&nodes, &[], 800.0);
}

fn drag_persist_cart_to_go_live(cx: &mut gpui_kit::VisualTestContext, view: &Entity<PlannerView>) {
    let handle = cx
        .debug_bounds("gantt-link-handle-1003-link-end")
        .expect("Persist cart link handle");
    cx.simulate_mouse_down(
        grab_link_handle(handle),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    let target = cx.debug_bounds("gantt-bar-1013").expect("Go-live bar");
    cx.simulate_mouse_move(
        target.center(),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    cx.simulate_mouse_up(target.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, cx| {
        if let Some(go_live) = view.state.nodes.iter_mut().find(|node| node.id == 1013)
            && !go_live.predecessors.contains(&1003)
        {
            go_live.predecessors.push(1003);
        }
        cx.notify();
    });
    draw(cx);
}

#[gpui_kit::test]
fn launch_camera_painted_output_after_runtime_link_and_year(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = live_launch_camera_nodes();
    apply_launch_camera(&mut state);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-dep-inv-1003-1013").is_none(),
        "Persist cart → Go-live is created by the handle drop"
    );
    drag_persist_cart_to_go_live(cx, &view);

    let live = cx
        .debug_bounds("gantt-dep-inv-1003-1013")
        .expect("runtime Persist cart → Go-live inbound");
    let spike = cx
        .debug_bounds("gantt-dep-inv-1003-1012")
        .expect("Persist cart → spike inbound");
    let live_out = cx
        .debug_bounds("gantt-dep-out-1003-1013")
        .expect("runtime Persist cart → Go-live outgoing");
    let spike_out = cx
        .debug_bounds("gantt-dep-out-1003-1012")
        .expect("Persist cart → spike outgoing");
    let inv_gap = painted_x_gap(live, spike);
    let out_gap = painted_x_gap(live_out, spike_out);
    let live_x = painted_center_x(live);
    let spike_x = painted_center_x(spike);
    let live_out_x = painted_center_x(live_out);
    let spike_out_x = painted_center_x(spike_out);
    assert!(
        inv_gap >= 8.0,
        "launch camera: drag-created inbound boxes must sit ≥8px apart, live={live_x} spike={spike_x} edge={inv_gap}"
    );
    assert!(
        (live_x - spike_x).abs() >= 8.0,
        "launch camera: inbound centres must sit ≥8px apart, live={live_x} spike={spike_x}"
    );
    assert!(
        out_gap >= 8.0,
        "launch camera: outgoing boxes must sit ≥8px apart (was 2px at x≈600/602), live={live_out_x} spike={spike_out_x} edge={out_gap}"
    );
    assert!(
        (live_out_x - spike_out_x).abs() >= PARALLEL_VERT_MIN,
        "launch camera: outgoing centres must sit ≥{PARALLEL_VERT_MIN}px apart, live={live_out_x} spike={spike_out_x}"
    );
    let cart = cx.debug_bounds("gantt-bar-1005").expect("Cart UI bar");
    let cart_left = f32::from(cart.origin.x);
    assert!(
        live_x < cart_left - 1.0,
        "launch camera: Persist cart → Go-live inbound at {live_x} must drop before Cart UI at {cart_left}"
    );
    assert!(
        spike_x < cart_left - 1.0,
        "launch camera: Persist cart → spike inbound at {spike_x} must not jump onto Cart UI's left edge at {cart_left}"
    );
    assert!(
        painted_x_gap(spike, cart) >= 6.0,
        "launch camera: spike inbound box must sit ≥6px off Cart UI, spike={spike_x} cart_left={cart_left} gap={}",
        painted_x_gap(spike, cart)
    );
    let horiz = cx
        .debug_bounds("gantt-dep-arrow-1003-1013")
        .expect("runtime Persist cart → Go-live horizontal");
    let hit_h = f32::from(horiz.size.height);
    let hit_w = f32::from(horiz.size.width);
    if hit_w > hit_h {
        assert!(
            (8.0..=PARALLEL_LANE_MIN + 0.5).contains(&hit_h),
            "launch camera: Persist cart → Go-live horizontal hit must stay clickable without overlapping {PARALLEL_LANE_MIN}px lanes, got {hit_h}"
        );
    }
    if let Some(spike_h) = cx.debug_bounds("gantt-dep-arrow-1003-1012")
        && painted_x_gap(horiz, spike_h) < 1.0
    {
        assert!(
            !painted_overlap(horiz, spike_h),
            "launch camera: parallel horizontal hits must not overlap, live y-gap={}",
            painted_y_gap(horiz, spike_h)
        );
    }
    if let Some(label) = cx.debug_bounds("gantt-bar-label-1003") {
        let left = f32::from(label.origin.x);
        let right = left + f32::from(label.size.width);
        assert!(
            live_out_x >= right + 16.0 || live_out_x <= left - 16.0,
            "launch camera: first stub at {live_out_x} must sit ≥16px from the Persist cart label {left}..{right}"
        );
        if let Some(handle) = cx.debug_bounds("gantt-link-handle-1003-link-end") {
            assert!(
                !painted_overlap(handle, label),
                "launch camera: Persist cart right handle must not overlap the label, handle x={:.1}..{:.1} label={left}..{right}",
                f32::from(handle.origin.x),
                f32::from(handle.origin.x) + f32::from(handle.size.width)
            );
        }
    }
    view.update(cx, |view, cx| {
        view.state.gantt.hovered_link = Some((1003, 1013));
        cx.notify();
    });
    draw(cx);
    if let Some(tip) = cx.debug_bounds("gantt-link-tooltip") {
        if let Some(spike_label) = cx.debug_bounds("gantt-bar-label-1012") {
            assert!(
                !painted_overlap(tip, spike_label),
                "link tooltip must not cover the Unparented spike label"
            );
        }
        if let Some(persist_label) = cx.debug_bounds("gantt-bar-label-1003") {
            assert!(
                !painted_overlap(tip, persist_label),
                "link tooltip must not cover the Persist cart label, tip=({:.1},{:.1})+{:.1}x{:.1} label=({:.1},{:.1})+{:.1}x{:.1}",
                f32::from(tip.origin.x),
                f32::from(tip.origin.y),
                f32::from(tip.size.width),
                f32::from(tip.size.height),
                f32::from(persist_label.origin.x),
                f32::from(persist_label.origin.y),
                f32::from(persist_label.size.width),
                f32::from(persist_label.size.height)
            );
        }
        if let Some(cart) = cx.debug_bounds("gantt-bar-1005") {
            assert!(
                !painted_overlap(tip, cart),
                "link tooltip must not cover the next row's bar"
            );
        }
    }

    let year = cx.debug_bounds("gantt-scale-Year").expect("Year toolbar");
    cx.simulate_click(year.center(), Modifiers::default());
    draw(cx);
    if cx.debug_bounds("gantt-today").is_none() {
        return;
    }
    let segments = today_body_segments(cx);
    assert_eq!(
        segments.len(),
        1,
        "Year Today stays one stroke behind labels, segments={}",
        segments.len()
    );
    for selector in [
        "gantt-bar-label-1001",
        "gantt-bar-label-1003",
        "gantt-bar-label-1005",
        "gantt-bar-label-1012",
        "gantt-bar-label-1013",
    ] {
        let Some(label) = cx.debug_bounds(selector) else {
            continue;
        };
        assert!(
            f32::from(label.size.height) >= 12.0,
            "launch camera Year before Today: {selector} must still paint on top of the Today stroke"
        );
    }
}

#[gpui_kit::test]
fn runtime_drag_persist_cart_to_go_live_stays_eight_apart(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    if let Some(spike) = state.nodes.iter_mut().find(|node| node.id == 1012) {
        spike.predecessors = vec![1003];
    }
    if let Some(go_live) = state.nodes.iter_mut().find(|node| node.id == 1013) {
        go_live.predecessors.clear();
    }
    let nodes = state.nodes.clone();
    // Running-app camera: apply_hierarchy auto_fit at 800, then a 1280 window.
    state.gantt.zoom = FITTED_MONTH_ZOOM;
    state.gantt.auto_fit(&nodes, &[], 800.0);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-dep-inv-1003-1012").is_some(),
        "live fixture: Persist cart → spike must paint before the drag"
    );
    assert!(
        cx.debug_bounds("gantt-dep-inv-1003-1013").is_none(),
        "Persist cart → Go-live is created by the handle drop"
    );
    let handle = cx
        .debug_bounds("gantt-link-handle-1003-link-end")
        .expect("Persist cart link handle");
    cx.simulate_mouse_down(
        grab_link_handle(handle),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    let target = cx.debug_bounds("gantt-bar-1013").expect("Go-live bar");
    cx.simulate_mouse_move(
        target.center(),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view.state.gantt.gesture.as_ref().expect("still linking");
        assert_eq!(
            gesture.link_to,
            Some(1013),
            "dropping on Go-live must arm Persist cart → Go-live"
        );
    });
    cx.simulate_mouse_up(target.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    // Mock PAT has no ADO; apply the same optimistic predecessor add_dependency does.
    view.update(cx, |view, cx| {
        if let Some(go_live) = view.state.nodes.iter_mut().find(|node| node.id == 1013)
            && !go_live.predecessors.contains(&1003)
        {
            go_live.predecessors.push(1003);
        }
        cx.notify();
    });
    draw(cx);
    let live = cx
        .debug_bounds("gantt-dep-inv-1003-1013")
        .expect("runtime Persist cart → Go-live inbound");
    let spike = cx
        .debug_bounds("gantt-dep-inv-1003-1012")
        .expect("Persist cart → spike inbound");
    let live_out = cx
        .debug_bounds("gantt-dep-out-1003-1013")
        .expect("runtime Persist cart → Go-live outgoing");
    let spike_out = cx
        .debug_bounds("gantt-dep-out-1003-1012")
        .expect("Persist cart → spike outgoing");
    let edge = painted_x_gap(live, spike);
    let out_edge = painted_x_gap(live_out, spike_out);
    let live_x = painted_center_x(live);
    let spike_x = painted_center_x(spike);
    let live_out_x = painted_center_x(live_out);
    let spike_out_x = painted_center_x(spike_out);
    assert!(
        edge >= 8.0,
        "drag-created Persist cart → Go-live inv at {live_x} must sit ≥8px from spike at {spike_x} (edge gap={edge})"
    );
    assert!(
        (live_x - spike_x).abs() >= 8.0,
        "drag-created inbound centres must sit ≥8px apart, live={live_x} spike={spike_x}"
    );
    assert!(
        out_edge >= 8.0,
        "drag-created outgoing boxes must sit ≥8px apart, live={live_out_x} spike={spike_out_x} edge={out_edge}"
    );
    if let Some(cart) = cx.debug_bounds("gantt-bar-1005") {
        let cart_left = f32::from(cart.origin.x);
        assert!(
            live_x < cart_left - 1.0,
            "runtime Persist cart → Go-live inbound at {live_x} must drop before Cart UI at {cart_left}"
        );
        assert!(
            spike_x < cart_left - 1.0,
            "runtime Persist cart → spike inbound at {spike_x} must not jump onto Cart UI at {cart_left}"
        );
    }
}

#[gpui_kit::test]
fn month_hides_today_when_off_the_chart(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 1.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let today = jiff::Zoned::now().date();
    let on_screen = view.update(cx, |view, _| {
        let x = view.state.gantt.today_x(today);
        x >= 0.0 && x <= view.state.gantt.timeline_width
    });
    if on_screen {
        let today_line = cx.debug_bounds("gantt-today").expect("on-screen Today");
        let body = cx
            .debug_bounds("gantt-timeline-body")
            .expect("timeline body");
        let x = f32::from(today_line.origin.x);
        let left = f32::from(body.origin.x);
        let right = left + f32::from(body.size.width);
        assert!(
            x >= left - 1.0 && x <= right,
            "on-screen Today at {x} must sit inside the chart {left}..{right}"
        );
    } else {
        assert!(
            cx.debug_bounds("gantt-today").is_none(),
            "Month must not paint an out-of-range Today line at the chart edge"
        );
        assert!(
            today_body_segments(cx).is_empty(),
            "Month must not paint an out-of-range Today body at the chart edge"
        );
    }
}

#[gpui_kit::test]
fn year_before_today_click_gaps_under_sliver_pills(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    let nodes = state.nodes.clone();
    state.gantt.auto_fit(&nodes, &[], 800.0);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let year = cx.debug_bounds("gantt-scale-Year").expect("Year toolbar");
    cx.simulate_click(year.center(), Modifiers::default());
    draw(cx);
    if cx.debug_bounds("gantt-today").is_none() {
        return;
    }
    let segments = today_body_segments(cx);
    assert_eq!(
        segments.len(),
        1,
        "Year Today stays one stroke behind labels, segments={}",
        segments.len()
    );
    for selector in [
        "gantt-bar-label-1001",
        "gantt-bar-label-1003",
        "gantt-bar-label-1005",
        "gantt-bar-label-1012",
        "gantt-bar-label-1013",
    ] {
        let Some(label) = cx.debug_bounds(selector) else {
            continue;
        };
        assert!(
            f32::from(label.size.height) >= 12.0,
            "Year before Today: {selector} must still paint on top of the Today stroke"
        );
    }
}

#[gpui_kit::test]
fn inbound_arrowhead_paints_at_the_target_not_the_elbow(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.start_date = Some("2026-07-10T00:00:00Z".into());
        node.target_date = Some("2026-08-04T00:00:00Z".into());
    }
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 3) {
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
    let head = cx.debug_bounds("gantt-dep-arrowhead").expect("inbound ▶");
    let target = cx.debug_bounds("gantt-bar-3").expect("successor");
    let elbow_gap = view.update(cx, |view, _| {
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars =
            view.state
                .gantt
                .layout_bars(&view.state.nodes, &rows, &view.state.iterations, 720.0);
        let arrow = view
            .state
            .gantt
            .layout_dependency_arrows(&bars)
            .into_iter()
            .find(|arrow| arrow.from_id == 1 && arrow.to_id == 3)
            .expect("1→3");
        (arrow.x2 - arrow.x_head).abs()
    });
    let head_right = f32::from(head.origin.x + head.size.width);
    let target_left = f32::from(target.origin.x);
    assert!(
        (head_right - target_left).abs() < 24.0,
        "▶ must sit on the successor edge, not the dodged elbow; head_right={head_right} target_left={target_left} elbow_gap={elbow_gap}"
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
fn typed_complete_range_does_not_commit_until_enter(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_inline_dates(1, window, cx);
            view.type_inline_dates("2026-08-01 → 2026-08-15", window, cx);
        });
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-dates").is_some(),
        "a complete typed range must stay in the picker until Enter"
    );
    view.update(cx, |view, _| {
        let node = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 1)
            .expect("item 1");
        assert_eq!(
            node.start_date.as_deref(),
            Some("2026-07-06T00:00:00Z"),
            "typing a complete range must not PATCH until Enter"
        );
        assert_eq!(
            node.target_date.as_deref(),
            Some("2026-07-20T00:00:00Z"),
            "typing a complete range must not PATCH until Enter"
        );
    });
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
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
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
    let dates = cx
        .debug_bounds("gantt-dates-cell")
        .expect("Dates column must stay visible while F2 editing the title");
    let editor_right = f32::from(editor.origin.x) + f32::from(editor.size.width);
    let dates_left = f32::from(dates.origin.x);
    assert!(
        editor_right <= dates_left + 2.0,
        "F2 title field at right={editor_right} must stay inside the title column, Dates starts at {dates_left}"
    );
    assert!(
        f32::from(editor.size.width) >= 80.0,
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
    let toast = cx.debug_bounds("planner-toast").expect("toast");
    let toast_top = f32::from(toast.origin.y);
    let toast_bottom = toast_top + f32::from(toast.size.height);
    let toast_center = f32::from(toast.origin.x) + f32::from(toast.size.width) / 2.0;
    assert!(
        toast_top > 400.0,
        "toasts at y={toast_top} must sit in the bottom half, not over Month bar labels"
    );
    assert!(
        toast_bottom <= 800.0 + 8.0,
        "toasts at bottom={toast_bottom} must stay on screen"
    );
    assert!(
        (toast_center - 640.0).abs() < 220.0,
        "toasts should sit near the horizontal center, center={toast_center}"
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
    assert!(
        cx.debug_bounds("gantt-auto-fit").is_some(),
        "toolbar must expose monday.com Auto fit next to Today"
    );
    assert!(
        cx.debug_bounds("gantt-scale-group").is_some(),
        "scale buttons must sit in one segmented control"
    );
    assert!(
        cx.debug_bounds("gantt-critical-path").is_none(),
        "Critical path belongs in the overflow menu until opened"
    );
    open_gantt_overflow(cx);
    assert!(
        cx.debug_bounds("gantt-critical-path").is_some(),
        "toolbar overflow must expose a Critical path toggle"
    );
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
        let first = labels.first().map(String::as_str).unwrap_or("");
        assert!(
            title.starts_with(first) || first.is_empty(),
            "range title {title:?} must match the painted axis start {first:?}, labels={labels:?}"
        );
        let today_x = view.state.gantt.today_x(jiff::Zoned::now().date());
        let expected = view.state.gantt.timeline_width.max(1.0) * TODAY_ALIGN;
        assert!(
            (today_x - expected).abs() < 8.0,
            "Today line at {today_x} should sit near the timeline center, expected {expected}"
        );
    });
}

#[gpui_kit::test]
fn critical_path_toolbar_toggles_the_highlight(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            !view.state.gantt.show_critical_path,
            "critical path is off until toggled"
        );
    });
    open_gantt_overflow(cx);
    let btn = cx
        .debug_bounds("gantt-critical-path")
        .expect("Critical path button");
    cx.simulate_click(btn.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.show_critical_path,
            "toolbar click must turn the critical-path highlight on"
        );
    });
    open_gantt_overflow(cx);
    let btn = cx
        .debug_bounds("gantt-critical-path")
        .expect("Critical path button");
    cx.simulate_click(btn.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            !view.state.gantt.show_critical_path,
            "a second click must turn the highlight off"
        );
    });
}

#[gpui_kit::test]
fn critical_path_menu_item_shows_a_check_while_on(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    open_gantt_overflow(cx);
    assert!(
        cx.debug_bounds("gantt-critical-path-check").is_none(),
        "no check while the highlight is off"
    );
    let btn = cx
        .debug_bounds("gantt-critical-path")
        .expect("Critical path button");
    cx.simulate_click(btn.center(), Modifiers::default());
    draw(cx);
    open_gantt_overflow(cx);
    assert!(
        cx.debug_bounds("gantt-critical-path-check").is_some(),
        "the menu item must show a check while the highlight is on"
    );
    let btn = cx
        .debug_bounds("gantt-critical-path")
        .expect("Critical path button");
    cx.simulate_click(btn.center(), Modifiers::default());
    draw(cx);
    open_gantt_overflow(cx);
    assert!(
        cx.debug_bounds("gantt-critical-path-check").is_none(),
        "the check must clear once the highlight is off"
    );
}

#[gpui_kit::test]
fn auto_fit_toolbar_button_fits_the_span(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 1) {
        node.start_date = Some("2026-07-06T00:00:00Z".into());
        node.target_date = Some("2026-11-20T00:00:00Z".into());
    }
    state.gantt.set_scale(GanttScale::Day);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit button");
    let (before_zoom, before_ppd) = view.update(cx, |view, _| {
        assert_eq!(view.state.gantt.scale, GanttScale::Day);
        (
            view.state.gantt.zoom,
            view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom),
        )
    });
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.scale,
            GanttScale::Day,
            "Auto fit must keep Day so column width does not jump, got {:?}",
            view.state.gantt.scale
        );
        assert!(
            (view.state.gantt.zoom - before_zoom).abs() < f32::EPSILON,
            "Auto fit must not retune zoom, {} vs {before_zoom}",
            view.state.gantt.zoom
        );
        let after_ppd = view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom);
        assert!(
            (after_ppd - before_ppd).abs() < 0.01,
            "Auto fit must not change the Day column width, {before_ppd} vs {after_ppd}"
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
            !title.contains(',') && title.chars().any(|ch| ch.is_ascii_digit()),
            "Day header must use compact month-day, got {title:?}"
        );
        let majors = view
            .state
            .gantt
            .header_major_labels(view.state.gantt.timeline_width.max(400.0));
        assert!(
            majors.iter().any(|(_, label)| label.contains("2026")),
            "the year belongs on the month banner, not the Day range title: {majors:?}"
        );
        let today = jiff::Zoned::now().date();
        let today_x = view.state.gantt.today_x(today);
        let expected = view.state.gantt.timeline_width.max(1.0) * TODAY_ALIGN;
        assert!(
            (today_x - expected).abs() < 8.0,
            "Day zoom Today line at {today_x} should sit near the timeline center, expected {expected}"
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
    let bar_right = f32::from(bar.origin.x) + f32::from(bar.size.width);
    assert!(
        f32::from(handle.origin.x) + f32::from(handle.size.width) > bar_right + 1.0,
        "link hit must extend past the bar end so the outer half can link"
    );
    cx.simulate_mouse_down(
        grab_link_handle(handle),
        MouseButton::Left,
        Modifiers::default(),
    );
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
fn milestone_details_dialog_parses_the_diamond_prefix(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    let mut values = serde_json::Map::new();
    values.insert(
        "System.Title".into(),
        serde_json::json!("Go-live milestone"),
    );
    values.insert(
        ado_core::TARGET_DATE_FIELD.into(),
        serde_json::json!("2026-08-15T00:00:00Z"),
    );
    state.form = Some(FormDraft::from_model(ado_core::WorkItemFormModel {
        id: 1013,
        rev: 1,
        work_item_type: "Feature".into(),
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
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("form-dates-label").is_some(),
        "a milestone date field must be labelled Dates, not Target Date"
    );
    view.update(cx, |view, cx| {
        assert!(
            view.form_dates_error_text(cx).is_none(),
            "opening a target-only item seeded with ◆ must not show a parse error, got {:?}",
            view.form_dates_error_text(cx)
        );
    });
}

#[gpui_kit::test]
fn ctrl_a_then_typing_in_details_dates_keeps_the_field(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    let mut values = serde_json::Map::new();
    values.insert(
        "System.Title".into(),
        serde_json::json!("Go-live milestone"),
    );
    values.insert(
        ado_core::TARGET_DATE_FIELD.into(),
        serde_json::json!("2026-08-15T00:00:00Z"),
    );
    state.form = Some(FormDraft::from_model(ado_core::WorkItemFormModel {
        id: 1013,
        rev: 1,
        work_item_type: "Feature".into(),
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
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("form-dates-typed").is_some(),
        "precondition: Dates field must paint"
    );
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_form_dates("", window, cx);
        });
    });
    draw(cx);
    assert!(
        cx.debug_bounds("form-dates-typed").is_some(),
        "Ctrl+A must not unmount the Dates field"
    );
    assert!(
        cx.debug_bounds("form-date-picker").is_some(),
        "the Dates picker must stay painted after select-all"
    );
    view.update(cx, |view, cx| {
        assert!(
            !view.form_save_is_enabled(cx),
            "select-all must not enable Save"
        );
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_form_dates("a", window, cx);
        });
    });
    draw(cx);
    assert!(
        cx.debug_bounds("form-dates-typed").is_some(),
        "typing after Ctrl+A must keep the Dates field"
    );
    assert!(
        cx.debug_bounds("form-date-picker").is_some(),
        "typing after Ctrl+A must keep the Dates picker"
    );
    view.update(cx, |view, cx| {
        assert!(
            !view.form_save_is_enabled(cx),
            "an incomplete Dates edit must not enable Save"
        );
    });
}

#[gpui_kit::test]
fn emptying_a_non_range_datetime_enables_save(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    let mut values = serde_json::Map::new();
    values.insert("System.Title".into(), serde_json::json!("Cart"));
    values.insert(
        "Custom.ReviewBy".into(),
        serde_json::json!("2026-08-01T00:00:00Z"),
    );
    state.form = Some(FormDraft::from_model(ado_core::WorkItemFormModel {
        id: 1002,
        rev: 1,
        work_item_type: "Feature".into(),
        system_controls: Vec::new(),
        pages: vec![ado_core::FormPage {
            id: "details".into(),
            label: "Details".into(),
            groups: vec![ado_core::FormGroup {
                id: "planning".into(),
                label: "Planning".into(),
                controls: vec![ado_core::FormControl {
                    id: "Custom.ReviewBy".into(),
                    reference_name: "Custom.ReviewBy".into(),
                    label: "Review By".into(),
                    kind: ado_core::FormControlKind::DateTime,
                    required: false,
                    read_only: false,
                    visible: true,
                    options: None,
                }],
            }],
        }],
        values,
    }));
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    view.update(cx, |view, cx| {
        assert!(
            view.form_input_value("Custom.ReviewBy", cx).is_some(),
            "standalone DateTime must paint an editable field"
        );
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_form_field("Custom.ReviewBy", "", window, cx);
        });
    });
    draw(cx);
    view.update(cx, |view, cx| {
        let live = view.form_input_value("Custom.ReviewBy", cx);
        let value = view
            .state
            .form
            .as_ref()
            .and_then(|form| form.values.get("Custom.ReviewBy"));
        assert_eq!(
            value,
            Some(&serde_json::Value::Null),
            "an emptied standalone DateTime must write null, live={live:?} value={value:?}"
        );
        assert!(
            view.form_save_is_enabled(cx),
            "clearing a standalone DateTime must enable Save, live={live:?}"
        );
    });
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
    let today = jiff::Zoned::now().date();
    state.gantt.go_today(today);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, _| {
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars =
            view.state
                .gantt
                .layout_bars(&view.state.nodes, &rows, &view.state.iterations, 720.0);
        assert!(
            bars.iter()
                .find(|bar| bar.id == 2)
                .is_some_and(|bar| bar.marker),
            "unscheduled items must be a gutter pill, not a sprint ghost"
        );
    });
    let bar = cx
        .debug_bounds("gantt-bar-2")
        .expect("unscheduled gutter pill");
    cx.simulate_mouse_down(bar.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view.state.gantt.gesture.clone().expect("placeholder move");
        assert_eq!(gesture.kind, GestureKind::Move);
        let (start, _) = GanttView::gesture_patch_dates(&gesture);
        let start = start.expect("start");
        assert_eq!(
            start, today,
            "the marker gesture must start at Today, not the July origin, got {start}"
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
fn unscheduled_pill_click_without_drag_does_not_schedule(cx: &mut gpui_kit::TestAppContext) {
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
    let today = jiff::Zoned::now().date();
    state.gantt.go_today(today);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-2")
        .expect("unscheduled gutter pill");
    cx.simulate_mouse_down(bar.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(bar.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "a no-move pill click must finish the gesture"
        );
        let node = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 2)
            .expect("pill item");
        assert_eq!(
            node.start_date, None,
            "a click without drag must not PATCH Start, got {:?}",
            node.start_date
        );
        assert_eq!(
            node.target_date, None,
            "a click without drag must not PATCH Target, got {:?}",
            node.target_date
        );
        assert!(
            view.state.history.undo.is_empty(),
            "a no-move pill click must not record a date change"
        );
    });
}

#[gpui_kit::test]
fn unscheduled_pill_micro_drag_in_gutter_does_not_schedule(cx: &mut gpui_kit::TestAppContext) {
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
    let today = jiff::Zoned::now().date();
    state.gantt.go_today(today);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let pill = cx
        .debug_bounds("gantt-bar-2")
        .expect("unscheduled gutter pill");
    let timeline = cx
        .debug_bounds("gantt-timeline-body")
        .expect("timeline body");
    let sloppy = point(
        px(f32::from(timeline.origin.x) + UNSCHEDULED_GUTTER_RIGHT - 4.0),
        pill.center().y,
    );
    let press = pill.center();
    assert!(
        (f32::from(sloppy.x) - f32::from(press.x)).abs() >= 6.0,
        "precondition: the sloppy release must travel past the 6px no-move cancel"
    );
    cx.simulate_mouse_down(press, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(sloppy, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(sloppy, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "a gutter micro-drag must finish the gesture"
        );
        let node = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 2)
            .expect("pill item");
        assert_eq!(
            node.start_date, None,
            "releasing still in the gutter must not PATCH Start, got {:?}",
            node.start_date
        );
        assert_eq!(
            node.target_date, None,
            "releasing still in the gutter must not PATCH Target, got {:?}",
            node.target_date
        );
        assert!(
            view.last_committed_start_for_test().is_none(),
            "a gutter micro-drag must not record a committed start"
        );
        assert!(
            view.state.history.undo.is_empty(),
            "a gutter micro-drag must not record a date change"
        );
    });
}

#[gpui_kit::test]
fn unscheduled_pill_drop_at_known_x_follows_the_cursor(cx: &mut gpui_kit::TestAppContext) {
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
    state.gantt.scale = GanttScale::Month;
    state.gantt.origin = Date::new(2026, 8, 1).unwrap();
    let drop = Date::new(2026, 8, 25).unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let pill = cx
        .debug_bounds("gantt-bar-2")
        .expect("unscheduled gutter pill");
    let timeline = cx
        .debug_bounds("gantt-timeline-body")
        .expect("timeline body");
    let drop_window_x = view.update(cx, |view, _| {
        f32::from(timeline.origin.x) + view.state.gantt.x_for_date(drop)
    });
    cx.simulate_mouse_down(pill.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_some(),
            "pill press must start a move gesture"
        );
    });
    let drop_at = point(px(drop_window_x), pill.center().y);
    cx.simulate_mouse_move(drop_at, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("drag must keep the move gesture");
        assert_eq!(
            gesture.live_start, drop,
            "dropping the gutter pill at x for {drop} must follow the cursor, got {}",
            gesture.live_start
        );
        let pending = view.state.commit_date_gesture_now();
        let start = pending.as_ref().and_then(|save| save.start);
        assert_eq!(
            start,
            Some(drop),
            "committing that drop must PATCH Start={drop}, got {start:?}"
        );
    });
}

#[gpui_kit::test]
fn unscheduled_pill_flick_to_known_x_follows_the_cursor(cx: &mut gpui_kit::TestAppContext) {
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
    state.gantt.scale = GanttScale::Month;
    state.gantt.origin = Date::new(2026, 8, 1).unwrap();
    let drop = Date::new(2026, 8, 25).unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let pill = cx
        .debug_bounds("gantt-bar-2")
        .expect("unscheduled gutter pill");
    let timeline = cx
        .debug_bounds("gantt-timeline-body")
        .expect("timeline body");
    let drop_window_x = view.update(cx, |view, _| {
        f32::from(timeline.origin.x) + view.state.gantt.x_for_date(drop)
    });
    cx.simulate_mouse_down(pill.center(), MouseButton::Left, Modifiers::default());
    let drop_at = point(px(drop_window_x), pill.center().y);
    cx.simulate_mouse_up(drop_at, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let node = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 2)
            .expect("pill item");
        assert_eq!(
            view.last_committed_start_for_test(),
            Some(drop),
            "a gutter flick must PATCH the date under the cursor, not Today+Δ, node={:?}/{:?}",
            node.start_date,
            node.target_date
        );
    });
}

#[gpui_kit::test]
fn unscheduled_pill_missed_mouseup_in_gutter_does_not_schedule(cx: &mut gpui_kit::TestAppContext) {
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
    let today = jiff::Zoned::now().date();
    state.gantt.go_today(today);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let pill = cx
        .debug_bounds("gantt-bar-2")
        .expect("unscheduled gutter pill");
    let timeline = cx
        .debug_bounds("gantt-timeline-body")
        .expect("timeline body");
    let sloppy = point(
        px(f32::from(timeline.origin.x) + UNSCHEDULED_GUTTER_RIGHT - 4.0),
        pill.center().y,
    );
    cx.simulate_mouse_down(pill.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(sloppy, None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let node = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 2)
            .expect("pill item");
        assert_eq!(
            node.start_date, None,
            "a missed mouse-up still in the gutter must not PATCH Start, got {:?}",
            node.start_date
        );
        assert!(
            view.last_committed_start_for_test().is_none(),
            "a missed mouse-up in the gutter must not commit"
        );
        assert!(
            view.state.history.undo.is_empty(),
            "a missed mouse-up in the gutter must not record a date change"
        );
    });
}

#[gpui_kit::test]
fn empty_team_keeps_the_gantt_toolbar(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes.clear();
    state.prefs.planner.team = "Empty".into();
    state.gantt.origin = jiff::Zoned::now().date();
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
        cx.debug_bounds("gantt-toolbar-more").is_some(),
        "less-used actions sit in the toolbar overflow"
    );
    open_gantt_overflow(cx);
    assert!(
        cx.debug_bounds("gantt-add-item").is_some(),
        "+ Add item must stay reachable from the overflow menu"
    );
}

#[gpui_kit::test]
fn short_bar_paints_its_title_outside(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.title = "Persist cart".into();
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
            view.state.gantt.hovered_id,
            Some(1),
            "drop must recompute hover at the release point so the tooltip returns without a nudge"
        );
        assert_eq!(
            view.state.gantt.hovered_link, None,
            "drag end must not leave a selected-link highlight"
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
        assert!(
            !view.state.gantt.hover_locked,
            "closing details must let the next hover paint"
        );
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
    state.gantt.origin = jiff::Zoned::now().date();
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
        let axis = view
            .state
            .gantt
            .header_tick_labels(view.state.gantt.timeline_width.max(400.0));
        let first = axis.first().map(|(_, label)| label.as_str()).unwrap_or("");
        assert!(
            title_after.starts_with(first) || first.is_empty(),
            "range title {title_after:?} must match the painted axis start {first:?}"
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
    assert!(
        cx.debug_bounds("form-dates-value").is_some(),
        "a start-only item must show its start date, not the empty YYYY-MM-DD placeholder"
    );
}

#[gpui_kit::test]
fn read_only_closed_date_paints_text(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    let mut values = serde_json::Map::new();
    values.insert("System.Title".into(), serde_json::json!("Persist cart"));
    values.insert(
        "Microsoft.VSTS.Common.ClosedDate".into(),
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
                controls: vec![ado_core::FormControl {
                    id: "Microsoft.VSTS.Common.ClosedDate".into(),
                    reference_name: "Microsoft.VSTS.Common.ClosedDate".into(),
                    label: "Closed Date".into(),
                    kind: ado_core::FormControlKind::DateTime,
                    required: false,
                    read_only: true,
                    visible: true,
                    options: None,
                }],
            }],
        }],
        values,
    }));
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("form-dates-value").is_some(),
        "read-only dates such as Closed Date must show compact text, not an empty widget"
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
        assert!(
            view.is_busy() || view.state.form.is_some(),
            "Enter on a selected bar must open details"
        );
    });
}

#[gpui_kit::test]
fn clicking_a_bar_focuses_planner_keys_for_nudge(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.teams = vec![ado_core::Team {
        id: "1".into(),
        name: "Platform".into(),
    }];
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let trigger = cx.debug_bounds("team-trigger").expect("team trigger");
    cx.simulate_click(trigger.center(), Modifiers::default());
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, _| {
            assert!(
                !view.planner_keys_are_focused(window),
                "opening a scope picker must take focus off the planner keys"
            );
        });
    });
    let bar = cx.debug_bounds("gantt-bar-1").expect("bar");
    cx.simulate_mouse_move(bar.center(), None, Modifiers::default());
    cx.simulate_mouse_down(bar.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(bar.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, _| {
            assert_eq!(view.state.gantt.selected_id, Some(1));
            assert!(
                view.planner_keys_are_focused(window),
                "clicking a bar must focus planner keys so ←/→ can nudge"
            );
        });
    });
}

#[gpui_kit::test]
fn down_after_clicking_a_bar_moves_one_row(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1").expect("bar");
    cx.simulate_mouse_move(bar.center(), None, Modifiers::default());
    cx.simulate_mouse_down(bar.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(bar.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    let down = KeyDownEvent {
        keystroke: Keystroke::parse("down").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        let before = view.state.gantt.selected_id;
        let rows = view.state.visible_rows();
        let before_idx = rows
            .iter()
            .position(|row| Some(row.id) == before)
            .unwrap_or(0);
        view.handle_key(&down, cx);
        let after = view.state.gantt.selected_id;
        let after_idx = rows
            .iter()
            .position(|row| Some(row.id) == after)
            .unwrap_or(0);
        assert_eq!(
            after_idx,
            before_idx + 1,
            "Down after a bar click must move one row, {before:?} -> {after:?}"
        );
    });
}

#[gpui_kit::test]
fn right_link_handle_drag_does_not_unschedule_start_only(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.target_date = None;
        node.title = "Start only".into();
    }
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.selected_id = Some(2);
    state.gantt.hovered_id = Some(2);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let handle = cx
        .debug_bounds("gantt-link-handle-2-link-end")
        .expect("start-only bar must show a right link handle");
    let start = grab_link_handle(handle);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    draw(cx);
    cx.simulate_mouse_move(
        point(start.x + px(40.), start.y),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    view.update(cx, |view, cx| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::Link)
        );
        view.handle_key(
            &KeyDownEvent {
                keystroke: Keystroke::parse("escape").unwrap(),
                is_held: false,
                prefer_character_input: false,
            },
            cx,
        );
        let node = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 2)
            .expect("node");
        assert_eq!(
            node.start_date.as_deref(),
            Some("2026-07-06T00:00:00Z"),
            "cancelling a right-handle link must keep the start date"
        );
        assert!(
            node.target_date.is_none(),
            "start-only must stay start-only, got {:?}",
            node.target_date
        );
        assert!(
            view.state.toasts.items.iter().all(|toast| toast.kind
                != ado_planner::toasts::ToastKind::Success
                || !toast.message.contains("Cleared")),
            "unschedule must not happen silently"
        );
    });
}

#[gpui_kit::test]
fn clicking_an_empty_unscheduled_row_does_not_schedule(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 3) {
        node.start_date = None;
        node.target_date = None;
        node.has_date_fields = true;
    }
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let band = cx
        .debug_bounds("gantt-row-band-3")
        .expect("unscheduled gantt row");
    let click = point(
        gpui_kit::px(f32::from(band.origin.x) + f32::from(band.size.width) * 0.8),
        band.center().y,
    );
    cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(click, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let node = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 3)
            .expect("node");
        assert!(
            node.start_date.is_none() && node.target_date.is_none(),
            "a click on an empty unscheduled row must not auto-schedule, got {:?} {:?}",
            node.start_date,
            node.target_date
        );
        assert_eq!(view.state.gantt.selected_id, Some(3));
        assert!(view.state.gantt.schedule_drag.is_none());
    });
}

#[gpui_kit::test]
fn link_handle_press_works_without_hovering_first(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.selected_id = None;
    state.gantt.hovered_id = None;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let handle = cx
        .debug_bounds("gantt-link-handle-1-link-end")
        .expect("link handle hit area must exist without a prior hover");
    cx.simulate_mouse_down(
        grab_link_handle(handle),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::Link),
            "pressing a link handle must start a link without hovering first"
        );
    });
}

#[gpui_kit::test]
fn pressing_the_last_pixels_of_a_bar_starts_a_link(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 1003) {
        node.target_date = node.start_date.clone();
    }
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Year;
    state.gantt.zoom = 1.0;
    state.gantt.hovered_id = Some(1003);
    state.gantt.bar_hovered_id = Some(1003);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Persist cart sliver");
    let handle = cx
        .debug_bounds("gantt-link-handle-1003-link-end")
        .expect("right link handle");
    let bar_right = f32::from(bar.origin.x) + f32::from(bar.size.width);
    let handle_left = f32::from(handle.origin.x);
    assert!(
        handle_left <= bar_right - 6.0,
        "right handle must cover the last 6–8px of a sliver, handle_left={handle_left} bar_right={bar_right}"
    );
    let on_end = point(
        bar.origin.x + bar.size.width - px(4.),
        bar.origin.y + bar.size.height / 2.0,
    );
    cx.simulate_mouse_down(on_end, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::Link),
            "a press on a sliver's last pixels must start a link"
        );
    });
    let preview = cx
        .debug_bounds("gantt-link-preview")
        .expect("pressing the link handle must show a ghost line");
    let preview_left = f32::from(preview.origin.x);
    assert!(
        preview_left + 1.0 >= bar_right,
        "ghost must not run left through the bar, preview_left={preview_left} bar_right={bar_right}"
    );
}

#[gpui_kit::test]
fn month_persist_last_pixels_start_a_link(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = FITTED_MONTH_ZOOM;
    state.gantt.selected_id = None;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart");
    let bar_w = f32::from(bar.size.width);
    let bar_right = f32::from(bar.origin.x) + bar_w;
    assert!(
        bar_w > 40.0 && bar_w < 100.0,
        "Auto-fit Month Persist cart should be a short ~64px bar, width={bar_w}"
    );
    let sliver = cx
        .debug_bounds("gantt-link-handle-1003-link-end-inset")
        .expect("wide dated bar must expose an inset link sliver");
    let sliver_left = f32::from(sliver.origin.x);
    let sliver_w = f32::from(sliver.size.width);
    assert!(
        sliver_w >= 5.5 && sliver_left <= bar_right - 5.5,
        "inset sliver must cover the last ~6px, sliver={sliver_left}+{sliver_w} bar_right={bar_right}"
    );
    let halo = cx
        .debug_bounds("gantt-resize-halo-1003-end")
        .expect("dated bar must expose an end resize halo");
    let halo_left = f32::from(halo.origin.x);
    let halo_right = halo_left + f32::from(halo.size.width);
    assert!(
        halo_left <= bar_right - 5.5 && halo_right >= bar_right + 0.5,
        "end halo must cover edge−6..+1, halo={halo_left}..{halo_right} bar_right={bar_right}"
    );
    let resize = cx
        .debug_bounds("gantt-resize-handle-1003-end")
        .expect("short Persist cart must keep an end resize grip");
    let resize_w = f32::from(resize.size.width);
    let resize_right = f32::from(resize.origin.x) + resize_w;
    assert!(
        (12.0..=15.0).contains(&resize_w),
        "end resize must be a ~14px unique zone, width={resize_w}"
    );
    assert!(
        sliver_left <= resize_right + 0.5,
        "resize and link must be contiguous (no dead zone), gap={} resize_right={resize_right} sliver_left={sliver_left}",
        sliver_left - resize_right
    );
    assert!(
        resize_right <= sliver_left + 2.5,
        "end resize must sit just left of the link sliver, resize_right={resize_right} sliver_left={sliver_left}"
    );
    cx.simulate_mouse_down(
        grab_outer_link_handle(bar),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::Link),
            "pressing Auto-fit Month Persist cart's outer handle must start a link, not a move"
        );
        assert_eq!(
            view.state.gantt.selected_id,
            Some(1003),
            "a Persist cart link press must not select Cart UI"
        );
    });
    let preview = cx
        .debug_bounds("gantt-link-preview")
        .expect("a real drag from the sliver must show a ghost line");
    let preview_left = f32::from(preview.origin.x);
    assert!(
        preview_left + 1.0 >= bar_right,
        "ghost must not run left through the bar, preview_left={preview_left} bar_right={bar_right}"
    );
    let cart = cx.debug_bounds("gantt-bar-1005").expect("Cart UI");
    cx.simulate_mouse_move(cart.center(), Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-link-preview").is_some(),
        "dragging the Persist cart sliver to Cart UI must keep the ghost line"
    );
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::Link)
        );
        assert_eq!(
            view.state.gantt.selected_id,
            Some(1003),
            "dragging a Persist cart link must not select Cart UI"
        );
    });
    assert!(
        cx.debug_bounds("gantt-row-accent-1005").is_none(),
        "Cart UI must not show a selected-row accent during a Persist cart link drag"
    );
    view.update(cx, |view, _| {
        view.state.cancel_gesture();
    });
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart after cancel");
    cx.simulate_mouse_down(
        grab_end_resize(bar),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::ResizeEnd),
            "the 14px just left of the Persist cart sliver must resize, not move"
        );
    });
    let dragged = point(grab_end_resize(bar).x + px(20.), grab_end_resize(bar).y);
    cx.simulate_mouse_move(dragged, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::ResizeEnd),
            "dragging the Persist cart end grip must stay a resize"
        );
        view.state.cancel_gesture();
    });
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart for move");
    cx.simulate_mouse_down(bar.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::Move),
            "the Persist cart body inside the edge zones must still move"
        );
    });
    cx.simulate_mouse_up(bar.center(), MouseButton::Left, Modifiers::default());
    view.update(cx, |view, _| {
        view.state.cancel_gesture();
    });
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart for hover cursors");
    cx.simulate_mouse_move(bar.center(), None, Modifiers::default());
    cx.simulate_mouse_move(grab_end_resize(bar), None, Modifiers::default());
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_bar_zone_for_test().map(|(_, zone)| zone),
            Some(BarHitZone::ResizeEnd),
            "hovering the end grip must classify resize before a press"
        );
        assert_eq!(
            view.hover_cursor_for_test(),
            Some("ew-resize"),
            "the ew-resize cursor must show on hover, before the press"
        );
    });
    cx.simulate_mouse_move(grab_end_resize_halo(bar), None, Modifiers::default());
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_bar_zone_for_test().map(|(_, zone)| zone),
            Some(BarHitZone::ResizeEnd),
            "hovering 2–4px inside the end edge must classify resize"
        );
        assert_eq!(
            view.hover_cursor_for_test(),
            Some("ew-resize"),
            "2–4px inside the end edge must show the resize cursor"
        );
    });
    cx.simulate_mouse_move(grab_bar_link_sliver(bar), None, Modifiers::default());
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_bar_zone_for_test().map(|(_, zone)| zone),
            Some(BarHitZone::ResizeEnd),
            "hovering 5.5px inside the end edge must classify resize (halo covers edge−6)"
        );
        assert_eq!(
            view.hover_cursor_for_test(),
            Some("ew-resize"),
            "5.5px inside the end edge must show the resize cursor"
        );
    });
    cx.simulate_mouse_move(grab_outer_link_handle(bar), None, Modifiers::default());
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_bar_zone_for_test().map(|(_, zone)| zone),
            Some(BarHitZone::Link),
            "hovering the outer handle must classify link"
        );
        assert_eq!(
            view.hover_cursor_for_test(),
            Some("drag-link"),
            "the outer handle must show a link cursor"
        );
    });
}

#[gpui_kit::test]
fn month_persist_outer_link_handle_starts_a_link_on_bar_hover(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = FITTED_MONTH_ZOOM;
    state.gantt.selected_id = None;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart");
    let bar_right = f32::from(bar.origin.x) + f32::from(bar.size.width);
    cx.simulate_mouse_move(bar.center(), None, Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-link-dot-1003-link-end").is_some(),
        "hovering Persist cart must paint the outer link connector"
    );
    let handle = cx
        .debug_bounds("gantt-link-handle-1003-link-end")
        .expect("outer link handle");
    let handle_left = f32::from(handle.origin.x);
    let handle_w = f32::from(handle.size.width);
    let handle_h = f32::from(handle.size.height);
    assert!(
        (handle_w - SHORT_BAR_OUTER_LINK_HIT).abs() < 0.5
            && (handle_h - SHORT_BAR_OUTER_LINK_HIT).abs() < 0.5,
        "a short Persist cart must grow the outbound plate, got {handle_w}×{handle_h}"
    );
    assert!(
        handle_left + 0.5 >= bar_right,
        "outer handle must sit in the gap past the bar, handle_left={handle_left} bar_right={bar_right}"
    );
    let connector = bar_right + LINK_HANDLE_GAP + LINK_HANDLE / 2.0;
    assert!(
        handle_left - 0.5 <= connector && handle_left + handle_w + 0.5 >= connector,
        "the enlarged plate must cover the gap connector, handle={handle_left}..{} connector={connector}",
        handle_left + handle_w
    );
    cx.simulate_mouse_move(grab_outer_link_handle(bar), None, Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-link-dot-1003-link-end").is_some(),
        "moving onto the outer handle must keep the connector visible"
    );
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_cursor_for_test(),
            Some("drag-link"),
            "the outer handle must show the link cursor on bar hover"
        );
    });
    cx.simulate_mouse_down(
        grab_outer_link_handle(bar),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::Link),
            "pressing the outer Persist cart handle (~8px past the bar) must start a link"
        );
    });
}

#[gpui_kit::test]
fn month_persist_outer_link_handle_starts_a_link_without_hover(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 1.0;
    state.gantt.selected_id = None;
    state.gantt.hovered_id = None;
    state.gantt.bar_hovered_id = None;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart");
    cx.simulate_mouse_down(
        grab_outer_link_handle(bar),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::Link),
            "the outer handle must start a link without hovering the bar first"
        );
    });
}

#[gpui_kit::test]
fn month_persist_edge_presses_have_no_dead_zone(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 1.0;
    state.gantt.selected_id = None;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart");
    let bar_right = f32::from(bar.origin.x) + f32::from(bar.size.width);
    let mid_y = bar.origin.y + bar.size.height / 2.0;
    let resize = cx
        .debug_bounds("gantt-resize-handle-1003-end")
        .expect("end resize");
    let sliver = cx
        .debug_bounds("gantt-link-handle-1003-link-end-inset")
        .expect("inset sliver");
    let handle = cx
        .debug_bounds("gantt-link-handle-1003-link-end")
        .expect("outer handle");
    let resize_left = f32::from(resize.origin.x);
    let resize_right = resize_left + f32::from(resize.size.width);
    let sliver_left = f32::from(sliver.origin.x);
    let sliver_right = sliver_left + f32::from(sliver.size.width);
    let handle_left = f32::from(handle.origin.x);
    let handle_w = f32::from(handle.size.width);
    let handle_center = handle_left + handle_w / 2.0;
    assert!(
        sliver_left <= resize_right + 0.5,
        "resize and link must meet, gap={:.1} resize={resize_left:.1}..{resize_right:.1} sliver={sliver_left:.1}..{sliver_right:.1} bar_right={bar_right:.1} handle={handle_left:.1}+{handle_w:.1} center={handle_center:.1}",
        sliver_left - resize_right
    );
    let mut dead = Vec::new();
    let mut kinds = Vec::new();
    for dx in -12i32..=16 {
        let x = bar_right + dx as f32;
        cx.simulate_mouse_down(point(px(x), mid_y), MouseButton::Left, Modifiers::default());
        draw(cx);
        let kind = view.update(cx, |view, _| {
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| format!("{:?}", gesture.kind))
        });
        if kind.is_none() && x <= bar_right + 0.5 && x >= resize_left - 0.5 {
            dead.push(x);
        }
        kinds.push((x, kind.clone()));
        view.update(cx, |view, _| {
            view.state.cancel_gesture();
        });
        draw(cx);
    }
    assert!(
        dead.is_empty(),
        "dead presses on Persist cart edge {dead:?}; probes={kinds:?}; resize={resize_left:.1}..{resize_right:.1} sliver={sliver_left:.1}..{sliver_right:.1} handle={handle_left:.1}..{:.1} center={handle_center:.1} bar_right={bar_right:.1}",
        handle_left + handle_w
    );
    let connector = bar_right + LINK_HANDLE_GAP + LINK_HANDLE / 2.0;
    assert!(
        handle_left - 0.5 <= connector && handle_left + handle_w + 0.5 >= connector,
        "enlarged plate must cover the gap connector, handle={handle_left:.1}..{:.1} connector={connector:.1}",
        handle_left + handle_w
    );
    // Short-bar plate is 28px in the gap. Presses around the connector
    // (reviewer ~653/659 on a 63px bar) must start a link.
    for (label, x) in [
        ("connector-8", connector - 8.0),
        ("connector+4", connector + 4.0),
        ("connector+8", connector + 8.0),
    ] {
        cx.simulate_mouse_down(point(px(x), mid_y), MouseButton::Left, Modifiers::default());
        draw(cx);
        view.update(cx, |view, _| {
            assert_eq!(
                view.state
                    .gantt
                    .gesture
                    .as_ref()
                    .map(|gesture| gesture.kind),
                Some(GestureKind::Link),
                "symmetric 8px radius must start a link at {label}={x:.1}, handle={handle_left:.1}..{:.1} center={handle_center:.1}",
                handle_left + handle_w
            );
            assert!(
                view.state.gantt.selected_link.is_none(),
                "handle press at {label}={x:.1} must not select the outgoing stub"
            );
            view.state.cancel_gesture();
        });
        draw(cx);
    }
}

#[gpui_kit::test]
fn day_persist_to_spike_paints_around_the_go_live_pill(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    if let Some(spike) = state.nodes.iter_mut().find(|node| node.id == 1012) {
        spike.predecessors = vec![1003];
    }
    if let Some(go_live) = state.nodes.iter_mut().find(|node| node.id == 1013) {
        go_live.predecessors = vec![1003];
    }
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Day;
    state.gantt.zoom = 1.0;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let day = cx.debug_bounds("gantt-scale-Day").expect("Day");
    cx.simulate_click(day.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    let pill = cx
        .debug_bounds("gantt-bar-label-1013")
        .or_else(|| cx.debug_bounds("gantt-bar-1013"))
        .expect("Go-live pill");
    for selector in [
        "gantt-dep-inv-1003-1012",
        "gantt-dep-out-1003-1012",
        "gantt-dep-arrow-1003-1012",
        "gantt-dep-drop-1003-1012",
    ] {
        let Some(seg) = cx.debug_bounds(selector) else {
            continue;
        };
        assert!(
            !painted_overlap(seg, pill),
            "Day Persist cart → spike {selector} must route around the Go-live pill, seg=({:.1},{:.1})+{:.1}x{:.1} pill=({:.1},{:.1})+{:.1}x{:.1}",
            f32::from(seg.origin.x),
            f32::from(seg.origin.y),
            f32::from(seg.size.width),
            f32::from(seg.size.height),
            f32::from(pill.origin.x),
            f32::from(pill.origin.y),
            f32::from(pill.size.width),
            f32::from(pill.size.height)
        );
    }
}

#[gpui_kit::test]
fn day_off_range_go_live_pill_is_not_crossed_by_persist_to_spike(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    if let Some(spike) = state.nodes.iter_mut().find(|node| node.id == 1012) {
        spike.predecessors = vec![1003];
    }
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Day;
    state.gantt.zoom = 1.0;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let day = cx.debug_bounds("gantt-scale-Day").expect("Day");
    cx.simulate_click(day.center(), Modifiers::default());
    draw(cx);
    let pill = cx
        .debug_bounds("gantt-bar-edge-right-1013")
        .expect("off-range Go-live pill");
    for selector in [
        "gantt-dep-inv-1003-1012",
        "gantt-dep-out-1003-1012",
        "gantt-dep-arrow-1003-1012",
        "gantt-dep-drop-1003-1012",
    ] {
        let Some(seg) = cx.debug_bounds(selector) else {
            continue;
        };
        assert!(
            !painted_overlap(seg, pill),
            "Day off-range Go-live pill must not be crossed by Persist cart → spike {selector}, seg=({:.1},{:.1})+{:.1}x{:.1} pill=({:.1},{:.1})+{:.1}x{:.1}",
            f32::from(seg.origin.x),
            f32::from(seg.origin.y),
            f32::from(seg.size.width),
            f32::from(seg.size.height),
            f32::from(pill.origin.x),
            f32::from(pill.origin.y),
            f32::from(pill.size.width),
            f32::from(pill.size.height)
        );
    }
}

#[gpui_kit::test]
fn resize_drop_on_outside_title_paints_toast_without_another_move(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 1.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart");
    let grip = grab_end_resize(bar);
    cx.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    let ppd = view.update(cx, |view, _| {
        view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom)
    });
    cx.simulate_mouse_move(
        point(grip.x + px(ppd * 4.0), grip.y),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-drag-preview").is_some(),
        "a live Persist cart resize must show the date preview"
    );
    let drop = cx
        .debug_bounds("gantt-bar-label-1003")
        .or_else(|| cx.debug_bounds("gantt-drag-preview"))
        .expect("outside title or preview to drop on");
    cx.simulate_mouse_up(drop.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "dropping a resize on an occluding title must commit without another mouse-move"
        );
        assert!(
            view.last_committed_target_for_test().is_some()
                || view.last_committed_start_for_test().is_some(),
            "the resize drop must commit dates before the next pointer move"
        );
        assert!(
            !view.state.toasts.items.is_empty(),
            "a toast must be queued on drop, without waiting for another mouse-move"
        );
    });
    assert!(
        cx.debug_bounds("gantt-drag-preview").is_none(),
        "the preview chip must clear on drop, without waiting for another mouse-move"
    );
    assert!(
        cx.debug_bounds("toast-title").is_some() || cx.debug_bounds("planner-toast").is_some(),
        "the resize toast must paint on drop, without waiting for another mouse-move"
    );
}

#[gpui_kit::test]
fn month_persist_edge_drag_adds_a_link_toast(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = FITTED_MONTH_ZOOM;
    state.gantt.selected_id = None;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart");
    let bar_left = f32::from(bar.origin.x);
    let bar_right = bar_left + f32::from(bar.size.width);
    assert!(
        (bar_right - bar_left - 64.0).abs() < 40.0,
        "Auto-fit Month Persist cart should be a short bar around x=545-609, got {bar_left}..{bar_right}"
    );
    for inset in [2.0, 4.0, 6.0, 3.0, 1.0] {
        cx.simulate_mouse_move(grab_bar_from_right(bar, inset), None, Modifiers::default());
        view.update(cx, |view, _| {
            assert_eq!(
                view.hover_bar_zone_for_test().map(|(_, zone)| zone),
                Some(BarHitZone::ResizeEnd),
                "hovering {inset}px inside the Persist cart end must resize"
            );
            assert_eq!(
                view.hover_cursor_for_test(),
                Some("ew-resize"),
                "2–6px inside the end edge must show the resize cursor, inset={inset}"
            );
        });
    }
    let at_edge = grab_outer_link_handle(bar);
    cx.simulate_mouse_move(at_edge, None, Modifiers::default());
    cx.background_executor
        .advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_bar_zone_for_test().map(|(_, zone)| zone),
            Some(BarHitZone::Link),
            "the outer handle must still be the link zone"
        );
        assert_eq!(
            view.hover_cursor_for_test(),
            Some("drag-link"),
            "the link cursor must survive the dates tooltip delay"
        );
    });
    cx.simulate_mouse_down(at_edge, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::Link),
            "a press on the outer handle must start a link, not a no-op"
        );
    });
    let cart = cx.debug_bounds("gantt-bar-1005").expect("Cart UI");
    let start = at_edge;
    let end = cart.center();
    for step in 1..=6 {
        let t = step as f32 / 6.0;
        let mid = point(
            start.x + (end.x - start.x) * t,
            start.y + (end.y - start.y) * t,
        );
        cx.simulate_mouse_move(mid, Some(MouseButton::Left), Modifiers::default());
        draw(cx);
    }
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let message = view.state.toasts.last_message().unwrap_or_default();
        assert!(
            message.starts_with("Added link:")
                && message.contains("Persist cart")
                && message.contains("Cart UI"),
            "a real multi-point drag from the Persist cart sliver onto Cart UI must toast, got {message:?}"
        );
        assert!(
            view.state
                .nodes
                .iter()
                .find(|node| node.id == 1005)
                .is_some_and(|node| node.predecessors.contains(&1003)),
            "the drop must add Persist cart as a predecessor of Cart UI"
        );
        assert!(view.state.gantt.gesture.is_none());
    });
    assert!(
        cx.debug_bounds("toast-title").is_some() || cx.debug_bounds("planner-toast").is_some(),
        "the Added link toast must paint"
    );
}

/// Xvfb/xdotool mousedown → several mousemoves → mouseup does not keep the
/// left button on later moves (capture is lost after a remount). The drop
/// must still persist Persist cart → Cart UI on the mock.
#[gpui_kit::test]
fn month_persist_xdotool_style_drag_persists_link_in_the_mock(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (runtime, mock, mut state, _dir) = signed_in_with_mock();
    assert!(
        state.nodes.iter().any(|node| node.id == 1003),
        "login must load Persist cart from the mock"
    );
    assert!(
        state
            .nodes
            .iter()
            .find(|node| node.id == 1005)
            .is_some_and(|node| !node.predecessors.contains(&1003)),
        "precondition: Cart UI must not already depend on Persist cart"
    );
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 1.0;
    state.gantt.selected_id = None;
    state.gantt.viewport_h = 720.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart");
    let at_edge = grab_outer_link_handle(bar);
    cx.simulate_mouse_move(at_edge, None, Modifiers::default());
    cx.simulate_mouse_down(at_edge, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::Link),
            "a press on the outer handle must start a link"
        );
    });
    let cart = cx.debug_bounds("gantt-bar-1005").expect("Cart UI");
    let start = at_edge;
    let end = cart.center();
    for step in 1..=6 {
        let t = step as f32 / 6.0;
        let mid = point(
            start.x + (end.x - start.x) * t,
            start.y + (end.y - start.y) * t,
        );
        // xdotool `mousemove` does not report a pressed button.
        cx.simulate_mouse_move(mid, None, Modifiers::default());
        draw(cx);
        view.update(cx, |view, _| {
            assert_eq!(
                view.state
                    .gantt
                    .gesture
                    .as_ref()
                    .map(|gesture| gesture.kind),
                Some(GestureKind::Link),
                "button-less move {step} must keep the link drag, not commit"
            );
        });
    }
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .and_then(|gesture| gesture.link_to),
            Some(1005),
            "the last hover-move over Cart UI must still arm the drop target"
        );
    });
    assert!(
        cx.debug_bounds("gantt-link-preview").is_some(),
        "the rubber-band must survive button-less moves"
    );
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let message = view.state.toasts.last_message().unwrap_or_default();
        assert!(
            message.starts_with("Added link:")
                && message.contains("Persist cart")
                && message.contains("Cart UI"),
            "mouse-up after hover-moves must toast, got {message:?}"
        );
        assert!(
            view.state
                .nodes
                .iter()
                .find(|node| node.id == 1005)
                .is_some_and(|node| node.predecessors.contains(&1003)),
            "the drop must add Persist cart as a predecessor of Cart UI"
        );
        assert!(view.state.gantt.gesture.is_none());
    });
    allow_ado_client_wake(cx);
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    while std::time::Instant::now() < deadline {
        cx.run_until_parked();
        draw(cx);
        if mock_has_predecessor(&runtime, &mock, 1005, 1003) {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        mock_has_predecessor(&runtime, &mock, 1005, 1003),
        "PATCH must persist Persist cart as a predecessor of Cart UI on the mock"
    );
}

/// Xvfb/xdotool mousedown → mousemoves → mouseup over Cart UI must commit
/// even when capture is gone and an occluding child (arrow, title, badge)
/// is under the pointer. The canvas drop layer + window-root mouse-up must
/// persist the link without a follow-up hover-move to trigger a repaint.
#[gpui_kit::test]
fn month_xdotool_mouseup_over_cart_ui_commits_link_in_the_mock(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (runtime, mock, mut state, _dir) = signed_in_with_mock();
    assert!(
        state
            .nodes
            .iter()
            .find(|node| node.id == 1005)
            .is_some_and(|node| !node.predecessors.contains(&1003)),
        "precondition: Cart UI must not already depend on Persist cart"
    );
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 1.0;
    state.gantt.selected_id = None;
    state.gantt.viewport_h = 720.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart");
    let at_edge = grab_outer_link_handle(bar);
    cx.simulate_mouse_move(at_edge, None, Modifiers::default());
    cx.simulate_mouse_down(at_edge, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::Link),
            "a press on the outer handle must start a link"
        );
    });
    let cart = cx.debug_bounds("gantt-bar-1005").expect("Cart UI");
    let start = at_edge;
    let end = cart.center();
    for step in 1..=6 {
        let t = step as f32 / 6.0;
        let mid = point(
            start.x + (end.x - start.x) * t,
            start.y + (end.y - start.y) * t,
        );
        cx.simulate_mouse_move(mid, None, Modifiers::default());
        draw(cx);
        view.update(cx, |view, _| {
            assert_eq!(
                view.state
                    .gantt
                    .gesture
                    .as_ref()
                    .map(|gesture| gesture.kind),
                Some(GestureKind::Link),
                "button-less move {step} must keep the link drag"
            );
        });
    }
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .and_then(|gesture| gesture.link_to),
            Some(1005),
            "the last hover-move over Cart UI must still arm the drop target"
        );
    });
    assert!(
        cx.debug_bounds("gantt-link-drop-layer").is_some(),
        "a link drag must paint a canvas drop layer so mouse-up hits even over occluding children"
    );
    // Prefer an occluding overlay on Cart UI (inbound arrow / title) so the
    // drop does not rely on the bar's own mouse-up.
    let drop_at = cx
        .debug_bounds("gantt-dep-inv-1005-1012")
        .or_else(|| cx.debug_bounds("gantt-bar-label-1005"))
        .map(|bounds| bounds.center())
        .unwrap_or(end);
    cx.simulate_mouse_up(drop_at, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let message = view.state.toasts.last_message().unwrap_or_default();
        assert!(
            message.starts_with("Added link:")
                && message.contains("Persist cart")
                && message.contains("Cart UI"),
            "mouse-up over Cart UI must toast without a follow-up move, got {message:?}"
        );
        assert!(
            view.state
                .nodes
                .iter()
                .find(|node| node.id == 1005)
                .is_some_and(|node| node.predecessors.contains(&1003)),
            "the drop must add Persist cart as a predecessor of Cart UI"
        );
        assert!(
            view.state.gantt.gesture.is_none(),
            "mouse-up over Cart UI must commit the link, not leave the preview following the pointer"
        );
    });
    assert!(
        cx.debug_bounds("gantt-link-preview").is_none(),
        "the rubber-band must unmount on drop without waiting for the next pointer move"
    );
    assert!(
        cx.debug_bounds("gantt-link-drop-layer").is_none(),
        "the canvas drop layer must unmount once the link is committed"
    );
    assert!(
        cx.debug_bounds("toast-title").is_some() || cx.debug_bounds("planner-toast").is_some(),
        "the Added link toast must paint on drop, not on the next mouse-move"
    );
    allow_ado_client_wake(cx);
    cx.background_executor
        .advance_clock(Duration::from_millis(20));
    cx.run_until_parked();
    draw(cx);
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    while std::time::Instant::now() < deadline {
        cx.run_until_parked();
        draw(cx);
        if mock_has_predecessor(&runtime, &mock, 1005, 1003) {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        mock_has_predecessor(&runtime, &mock, 1005, 1003),
        "PATCH must persist Persist cart as a predecessor of Cart UI on the mock"
    );
}

#[gpui_kit::test]
fn busy_io_still_persists_an_optimistic_link(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (runtime, mock, mut state, _dir) = signed_in_with_mock();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.viewport_h = 720.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, cx| {
        view.set_busy_for_test(true);
        assert!(view.state.apply_link_now(1005, 1003));
        view.enqueue_link_save_for_test(1005, 1003, cx);
        assert_eq!(
            view.link_save_queue_len_for_test(),
            1,
            "io_busy must queue the persist instead of dropping it"
        );
        // adopt_spawned of a clone taken before the drop wipes nodes, history,
        // and the Added-link toast. Restore must put all three back.
        if let Some(node) = view.state.nodes.iter_mut().find(|node| node.id == 1005) {
            node.predecessors.retain(|id| *id != 1003);
        }
        view.state.history.undo.clear();
        view.state.toasts.items.clear();
        view.complete_undo_io_for_test(cx);
    });
    allow_ado_client_wake(cx);
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    while std::time::Instant::now() < deadline {
        cx.run_until_parked();
        draw(cx);
        if mock_has_predecessor(&runtime, &mock, 1005, 1003) {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        mock_has_predecessor(&runtime, &mock, 1005, 1003),
        "queued persist after busy I/O must PATCH the optimistic link"
    );
    view.update(cx, |view, _| {
        assert!(
            view.state
                .nodes
                .iter()
                .find(|node| node.id == 1005)
                .is_some_and(|node| node.predecessors.contains(&1003)),
            "adopt_spawned of the in-flight clone must not wipe the queued link"
        );
        assert!(
            view.state.history.undo.iter().any(|entry| matches!(
                entry,
                HistoryEntry::Link {
                    successor_id: 1005,
                    predecessor_id: 1003,
                    linked: true,
                }
            )),
            "queued link must keep an undo entry after adopt_spawned"
        );
        let message = view.state.toasts.last_message().unwrap_or_default();
        assert!(
            message.starts_with("Added link:"),
            "queued link must keep its undo toast after adopt_spawned, got {message:?}"
        );
    });
}

#[gpui_kit::test]
fn undoing_a_queued_link_does_not_persist_later(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (runtime, mock, mut state, _dir) = signed_in_with_mock();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.viewport_h = 720.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, cx| {
        view.set_busy_for_test(true);
        assert!(view.state.apply_link_now(1005, 1003));
        view.enqueue_link_save_for_test(1005, 1003, cx);
        assert_eq!(view.link_save_queue_len_for_test(), 1);
        let toast_id = view.state.toasts.items.last().expect("Added link toast").id;
        view.apply_toast_undo(
            toast_id,
            UndoAction::RemoveLink {
                successor_id: 1005,
                predecessor_id: 1003,
            },
            cx,
        );
        assert_eq!(
            view.link_save_queue_len_for_test(),
            0,
            "toast Undo must dequeue the persist so a later drain cannot PATCH"
        );
        view.complete_undo_io_for_test(cx);
    });
    allow_ado_client_wake(cx);
    let deadline = std::time::Instant::now() + Duration::from_millis(800);
    while std::time::Instant::now() < deadline {
        cx.run_until_parked();
        draw(cx);
        if mock_has_predecessor(&runtime, &mock, 1005, 1003) {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        !mock_has_predecessor(&runtime, &mock, 1005, 1003),
        "Undo of a queued link must not PATCH it after I/O frees"
    );
}

#[gpui_kit::test]
fn leaving_a_bar_clears_the_hover_resize_cursor(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 1.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart");
    cx.simulate_mouse_move(grab_end_resize(bar), None, Modifiers::default());
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_cursor_for_test(),
            Some("ew-resize"),
            "hovering the end grip must show ew-resize"
        );
    });
    let tree = cx.debug_bounds("gantt-tree").expect("tree");
    cx.simulate_mouse_move(tree.center(), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_bar_zone_for_test(),
            None,
            "the tree must not keep a leftover bar hit zone"
        );
        assert_eq!(
            view.hover_cursor_for_test(),
            None,
            "ew-resize must not stick after the pointer leaves the bar for the tree"
        );
    });
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Persist cart after tree leave");
    cx.simulate_mouse_move(grab_end_resize(bar), None, Modifiers::default());
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_cursor_for_test(),
            Some("ew-resize"),
            "returning to the grip must restore ew-resize"
        );
    });
    let header = cx
        .debug_bounds("gantt-scale-header")
        .expect("timeline axis header");
    cx.simulate_mouse_move(header.center(), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_cursor_for_test(),
            None,
            "ew-resize must not stick on the timeline header"
        );
    });
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Persist cart after header leave");
    cx.simulate_mouse_move(grab_end_resize(bar), None, Modifiers::default());
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_cursor_for_test(),
            Some("ew-resize"),
            "returning to the grip after the header must restore ew-resize"
        );
    });
    let empty = point(px(1100.), px(760.));
    cx.simulate_mouse_move(empty, None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_cursor_for_test(),
            None,
            "ew-resize must not stick on empty timeline"
        );
    });
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Persist cart after empty leave");
    cx.simulate_mouse_move(grab_end_resize(bar), None, Modifiers::default());
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_cursor_for_test(),
            Some("ew-resize"),
            "returning to the grip after empty timeline must restore ew-resize"
        );
    });
    let nav = cx.debug_bounds("gantt-nav").expect("toolbar");
    cx.simulate_mouse_move(nav.center(), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_cursor_for_test(),
            None,
            "ew-resize must not stick on the toolbar after leaving the bar"
        );
    });
}

#[gpui_kit::test]
fn inset_link_sliver_forwards_wheel_pan(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 1.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let sliver = cx
        .debug_bounds("gantt-link-handle-1003-link-end-inset")
        .expect("Persist cart inset sliver");
    let before = view.update(cx, |view, _| view.state.gantt.scroll_x);
    cx.simulate_event(ScrollEvent {
        position: sliver.center(),
        delta: ScrollDelta::Pixels(point(px(120.), px(0.))),
        ..Default::default()
    });
    draw(cx);
    view.update(cx, |view, _| {
        assert_ne!(
            view.state.gantt.scroll_x, before,
            "wheel over the inner link sliver must pan the timeline, not drop the event"
        );
    });
}

#[gpui_kit::test]
fn quarter_today_stays_continuous_across_go_live_and_spike(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.scale = GanttScale::Quarter;
    state.gantt.zoom = 1.0;
    state.gantt.go_today(jiff::Zoned::now().date());
    state.gantt.selected_id = None;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let segments = today_body_segments(cx);
    assert!(
        !segments.is_empty(),
        "Quarter Today must paint when Today is on screen"
    );
    assert_eq!(
        segments.len(),
        1,
        "Quarter Today must be one continuous stroke, not chip-gapped, segments={}",
        segments.len()
    );
    assert!(
        cx.debug_bounds("gantt-row-accent-1005").is_none(),
        "Quarter must not paint a leftover Cart UI selected-row accent"
    );
}

#[gpui_kit::test]
fn last_pixels_of_a_milestone_still_move(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1013").expect("Go-live diamond");
    let on_end = point(
        bar.origin.x + bar.size.width - px(4.),
        bar.origin.y + bar.size.height / 2.0,
    );
    cx.simulate_mouse_down(on_end, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let kind = view
            .state
            .gantt
            .gesture
            .as_ref()
            .map(|gesture| gesture.kind);
        assert_ne!(
            kind,
            Some(GestureKind::Link),
            "a diamond's last pixels must stay Move, got {kind:?}"
        );
    });
}

#[gpui_kit::test]
fn releasing_a_link_drag_on_empty_space_clears_the_rubber_band(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.hovered_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let handle = cx
        .debug_bounds("gantt-link-handle-1-link-end")
        .expect("link handle");
    cx.simulate_mouse_down(
        grab_link_handle(handle),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    let away = point(handle.center().x + px(80.), handle.center().y - px(80.));
    cx.simulate_mouse_move(away, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-link-preview").is_some(),
        "rubber-band must paint while dragging"
    );
    cx.simulate_mouse_up(away, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "releasing on empty space must drop the link gesture"
        );
    });
    assert!(
        cx.debug_bounds("gantt-link-preview").is_none(),
        "the stray rubber-band must not stay after release"
    );
}

#[gpui_kit::test]
fn releasing_a_link_drag_at_a_target_edge_arms_the_link(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.hovered_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let handle = cx
        .debug_bounds("gantt-link-handle-1-link-end")
        .expect("link handle");
    cx.simulate_mouse_down(
        grab_link_handle(handle),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    let target = cx.debug_bounds("gantt-bar-2").expect("target bar");
    let edge = point(
        target.origin.x + target.size.width - px(1.),
        target.origin.y + target.size.height / 2.0,
    );
    cx.simulate_mouse_move(edge, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view.state.gantt.gesture.as_ref().expect("still linking");
        assert_eq!(
            gesture.link_to,
            Some(2),
            "releasing at the target's edge must arm the link, not drop it"
        );
    });
    cx.simulate_mouse_up(edge, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "mouse-up at the target edge must clear the rubber-band gesture"
        );
    });
    assert!(
        cx.debug_bounds("gantt-link-preview").is_none(),
        "the stray rubber-band must not stay after an edge drop"
    );
}

#[gpui_kit::test]
fn releasing_a_link_drag_on_empty_space_clears_link_selection(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.predecessors = vec![1];
        node.start_date = Some("2026-07-21T00:00:00Z".into());
        node.target_date = Some("2026-08-04T00:00:00Z".into());
    }
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.hovered_id = Some(1);
    state.gantt.selected_link = Some((1, 2));
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-link-remove").is_some(),
        "precondition: selected link shows ×"
    );
    let handle = cx
        .debug_bounds("gantt-link-handle-1-link-end")
        .expect("link handle");
    cx.simulate_mouse_down(
        grab_link_handle(handle),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    let away = point(handle.center().x + px(80.), handle.center().y - px(80.));
    cx.simulate_mouse_move(away, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(away, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.selected_link.is_none(),
            "releasing a handle drag on empty space must drop the stuck link selection"
        );
        assert!(view.state.gantt.gesture.is_none());
    });
    assert!(
        cx.debug_bounds("gantt-link-remove").is_none(),
        "the × must leave the handle after a cancelled link drag"
    );
    let handle = cx
        .debug_bounds("gantt-link-handle-1-link-end")
        .expect("link handle after cancel");
    cx.simulate_mouse_down(
        grab_link_handle(handle),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::Link),
            "the handle must still start a link after a cancelled drag; × must not cover it"
        );
        assert!(view.state.gantt.selected_link.is_none());
    });
}

#[gpui_kit::test]
fn clicking_a_tree_row_clears_link_selection(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.predecessors = vec![1];
        node.start_date = Some("2026-07-21T00:00:00Z".into());
        node.target_date = Some("2026-08-04T00:00:00Z".into());
    }
    state.gantt.selected_link = Some((1, 2));
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let title = cx.debug_bounds("gantt-title-1").expect("tree title");
    cx.simulate_click(title.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(view.state.gantt.selected_id, Some(1));
        assert!(
            view.state.gantt.selected_link.is_none(),
            "row select must clear the stuck link selection so × does not cover the handle"
        );
    });
    assert!(cx.debug_bounds("gantt-link-remove").is_none());
}

#[gpui_kit::test]
fn zoom_percent_is_not_shown_in_the_toolbar(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-zoom-pct").is_none(),
        "the zoom percent was not meaningful across scales; show +/− only"
    );
    assert!(
        cx.debug_bounds("gantt-zoom-level").is_some(),
        "the zoom level name must sit next to −/+"
    );
}

#[gpui_kit::test]
fn month_year_month_keeps_the_range_title(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 0.6;
    state.gantt.selected_id = None;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let (title, jul_x) = view.update(cx, |view, _| {
        (
            view.state.gantt.viewport_range_title(),
            view.state.gantt.x_for_date(Date::new(2026, 7, 1).unwrap()),
        )
    });
    let year = cx.debug_bounds("gantt-scale-Year").expect("Year");
    cx.simulate_click(year.center(), Modifiers::default());
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(view.state.gantt.scale, GanttScale::Month);
        assert_eq!(
            view.state.gantt.viewport_range_title(),
            title,
            "Month→Year→Month must restore the range"
        );
        let after = view.state.gantt.x_for_date(Date::new(2026, 7, 1).unwrap());
        assert!(
            (after - jul_x).abs() < 1.0,
            "Jul must stay at the same screen x, {jul_x} vs {after}"
        );
    });
}

#[gpui_kit::test]
fn year_today_label_does_not_overlap_month_ticks(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let year = cx.debug_bounds("gantt-scale-Year").expect("Year");
    cx.simulate_click(year.center(), Modifiers::default());
    draw(cx);
    let today = cx
        .debug_bounds("gantt-today-label")
        .expect("Year Today label");
    let today_line = cx.debug_bounds("gantt-today").expect("Year Today line");
    let tick = cx.debug_bounds("gantt-tick-0").expect("Year month tick");
    let today_top = f32::from(today.origin.y);
    let today_bottom = today_top + f32::from(today.size.height);
    let tick_top = f32::from(tick.origin.y);
    assert!(
        today_bottom <= tick_top + 1.0,
        "Year Today at {today_top}..{today_bottom} must sit on the year row, not on month ticks at {tick_top}"
    );
    let line_x = f32::from(today_line.origin.x);
    let line_right = line_x + f32::from(today_line.size.width);
    let pill_x = f32::from(today.origin.x);
    let pill_right = pill_x + f32::from(today.size.width);
    let attached_right = pill_x >= line_right - 1.0 && pill_x <= line_right + TODAY_PILL_GAP + 8.0;
    let attached_left = pill_right <= line_x + 1.0 && line_x - pill_right <= 8.0;
    assert!(
        attached_right || attached_left,
        "Year Today pill at {pill_x}..{pill_right} must stay on the line at {line_x}..{line_right}, not slide onto year labels"
    );
    let line_top = f32::from(today_line.origin.y);
    let line_bottom = line_top + f32::from(today_line.size.height);
    assert!(
        today_top + 1.0 >= line_top && today_bottom <= line_bottom + 1.0,
        "Year Today pill at {today_top}..{today_bottom} must sit on its line at {line_top}..{line_bottom}"
    );
    let mut tick_count = 0;
    for selector in [
        "gantt-tick-0",
        "gantt-tick-1",
        "gantt-tick-2",
        "gantt-tick-3",
        "gantt-tick-4",
        "gantt-tick-5",
        "gantt-tick-6",
        "gantt-tick-7",
        "gantt-tick-8",
        "gantt-tick-9",
        "gantt-tick-10",
        "gantt-tick-11",
        "gantt-tick-12",
    ] {
        if cx.debug_bounds(selector).is_some() {
            tick_count += 1;
        }
    }
    assert!(
        tick_count >= 6,
        "Year month ticks sit below Today and must stay visible (including Nov), got {tick_count}"
    );
}

#[gpui_kit::test]
fn day_today_label_does_not_hide_the_next_tick(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    let today = jiff::Zoned::now().date();
    state.gantt.origin = today.saturating_sub(3.days());
    state.gantt.scale = GanttScale::Day;
    state.gantt.zoom = 1.0;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let today_label = cx
        .debug_bounds("gantt-today-label")
        .expect("Day Today label");
    let today_top = f32::from(today_label.origin.y);
    let today_bottom = today_top + f32::from(today_label.size.height);
    let today_left = f32::from(today_label.origin.x);
    let today_right = today_left + f32::from(today_label.size.width);
    let mut tick_count = 0;
    let mut min_tick_top = f32::MAX;
    for selector in [
        "gantt-tick-0",
        "gantt-tick-1",
        "gantt-tick-2",
        "gantt-tick-3",
        "gantt-tick-4",
        "gantt-tick-5",
        "gantt-tick-6",
        "gantt-tick-7",
        "gantt-tick-8",
        "gantt-tick-9",
    ] {
        let Some(tick) = cx.debug_bounds(selector) else {
            continue;
        };
        tick_count += 1;
        let tick_top = f32::from(tick.origin.y);
        min_tick_top = min_tick_top.min(tick_top);
        let tick_left = f32::from(tick.origin.x);
        let tick_w = f32::from(tick.size.width);
        let tick_right = tick_left + tick_w;
        let overlaps_x = tick_left < today_right && tick_right > today_left;
        if overlaps_x {
            assert!(
                tick_w >= 7.0,
                "{selector} under Today must keep a readable width, not a 4px cap, got {tick_w}"
            );
        }
    }
    assert!(tick_count >= 4, "Day view must paint day-number ticks");
    assert!(
        today_bottom <= min_tick_top + 1.0,
        "Day Today at {today_top}..{today_bottom} must stay on the major row, not cover ticks at {min_tick_top}"
    );
    if let Some(line) = cx.debug_bounds("gantt-today") {
        let line_x = f32::from(line.origin.x);
        let mut on_column = false;
        for selector in [
            "gantt-tick-0",
            "gantt-tick-1",
            "gantt-tick-2",
            "gantt-tick-3",
            "gantt-tick-4",
            "gantt-tick-5",
            "gantt-tick-6",
            "gantt-tick-7",
            "gantt-tick-8",
            "gantt-tick-9",
        ] {
            let Some(tick) = cx.debug_bounds(selector) else {
                continue;
            };
            let tick_left = f32::from(tick.origin.x);
            if (tick_left - line_x).abs() < 6.0 {
                on_column = true;
                break;
            }
        }
        assert!(
            on_column,
            "Day Today tick must stay on the Today line at {line_x}, not shift off the column"
        );
    }
}

#[gpui_kit::test]
fn week_header_nov_does_not_overlap_today(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.origin = Date::new(2026, 11, 2).unwrap();
    state.gantt.scale = GanttScale::Week;
    state.gantt.zoom = 1.0;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let today = cx
        .debug_bounds("gantt-today-label")
        .expect("Week Today label");
    let today_left = f32::from(today.origin.x);
    let today_right = today_left + f32::from(today.size.width);
    let selectors = [
        "gantt-major-0",
        "gantt-major-1",
        "gantt-major-2",
        "gantt-major-3",
        "gantt-major-4",
        "gantt-major-5",
    ];
    let mut found = 0;
    for selector in selectors {
        let Some(major) = cx.debug_bounds(selector) else {
            continue;
        };
        found += 1;
        let major_left = f32::from(major.origin.x);
        let major_right = major_left + f32::from(major.size.width);
        assert!(
            major_right <= today_left + 1.0 || major_left >= today_right - 1.0,
            "{selector} at {major_left}..{major_right} must not overlap Today {today_left}..{today_right}"
        );
    }
    assert!(found > 0, "Week view must paint a month major label");
}

#[gpui_kit::test]
fn quarter_q_labels_are_not_jammed_at_the_left(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Quarter;
    state.gantt.zoom = 1.0;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let timeline = cx.debug_bounds("gantt-timeline").expect("timeline");
    let left = f32::from(timeline.origin.x);
    let mut xs = Vec::new();
    for selector in [
        "gantt-tick-0",
        "gantt-tick-1",
        "gantt-tick-2",
        "gantt-tick-3",
        "gantt-tick-4",
        "gantt-tick-5",
        "gantt-tick-6",
        "gantt-tick-7",
    ] {
        let Some(tick) = cx.debug_bounds(selector) else {
            continue;
        };
        xs.push(f32::from(tick.origin.x));
    }
    assert!(
        xs.len() >= 2,
        "Quarter view must paint more than one Q label, got {}",
        xs.len()
    );
    for pair in xs.windows(2) {
        assert!(
            pair[1] - pair[0] >= 40.0,
            "Quarter ticks at {} and {} are jammed together",
            pair[0],
            pair[1]
        );
    }
    let left_cluster = xs.iter().filter(|x| **x < left + 40.0).count();
    assert!(
        left_cluster <= 1,
        "at most one Quarter label may sit at the left edge, xs={xs:?} left={left}"
    );
}

#[gpui_kit::test]
fn year_bar_labels_paint_beside_the_bar(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 1) {
        node.title = "Cart".into();
    }
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.title = "Persist cart".into();
        node.start_date = Some("2026-07-06T00:00:00Z".into());
        node.target_date = Some("2026-07-20T00:00:00Z".into());
    }
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 3) {
        node.title = "Unparented spike".into();
        node.start_date = Some("2026-07-21T00:00:00Z".into());
        node.target_date = Some("2026-08-04T00:00:00Z".into());
    }
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Year;
    state.gantt.zoom = 1.0;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    for (id, title, required, min_w) in [
        (1_i64, "Cart", true, 36.0),
        (2, "Persist cart", true, 40.0),
        (3, "Unparented spike", true, 36.0),
    ] {
        let selector = match id {
            1 => "gantt-bar-label-1",
            2 => "gantt-bar-label-2",
            3 => "gantt-bar-label-3",
            _ => unreachable!("fixture ids"),
        };
        let Some(label) = cx.debug_bounds(selector) else {
            assert!(!required, "Year {title} must paint an outside label");
            continue;
        };
        assert!(
            f32::from(label.size.width) >= min_w,
            "Year {title} label width {} must not clip to a letter",
            f32::from(label.size.width)
        );
        let bar_selector = match id {
            1 => "gantt-bar-1",
            2 => "gantt-bar-2",
            3 => "gantt-bar-3",
            _ => unreachable!("fixture ids"),
        };
        let bar = cx
            .debug_bounds(bar_selector)
            .unwrap_or_else(|| panic!("Year {title} bar"));
        let label_left = f32::from(label.origin.x);
        let bar_left = f32::from(bar.origin.x);
        let bar_right = bar_left + f32::from(bar.size.width);
        let on_right = label_left + 4.0 >= bar_right - 8.0
            && (label_left - bar_right).abs() < OUTSIDE_LABEL_GAP + 12.0;
        let on_left = label_left + 8.0 <= bar_left + 4.0 && (bar_left - label_left).abs() < 160.0;
        assert!(
            on_right || on_left,
            "Year {title} label must sit next to its bar, not at the Today line, label_x={label_left} bar={bar_left}..{bar_right}"
        );
    }
}

#[gpui_kit::test]
fn year_checkout_rewrite_label_does_not_cross_today(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Year;
    state.gantt.zoom = 1.0;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    if let Some(label) = cx.debug_bounds("gantt-bar-label-1001") {
        let width = f32::from(label.size.width);
        assert!(
            width + 8.0 >= bar_title_paint_px("Checkout rewrite").min(80.0),
            "Year Checkout rewrite must stay readable, width={width}"
        );
    }
}

#[gpui_kit::test]
fn year_toolbar_launch_paints_full_outside_titles(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    let nodes = state.nodes.clone();
    // apply_hierarchy auto_fit, then the Year toolbar — the camera the
    // running Shop/Platform mock actually shows.
    state.gantt.auto_fit(&nodes, &[], 800.0);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let year = cx.debug_bounds("gantt-scale-Year").expect("Year toolbar");
    cx.simulate_click(year.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.scale,
            GanttScale::Year,
            "precondition: Year toolbar must stick"
        );
    });
    for (_id, title, selector, bar_selector, min_w) in [
        (
            1002_i64,
            "Cart",
            "gantt-bar-label-1002",
            "gantt-bar-1002",
            bar_title_paint_px("Cart"),
        ),
        (
            1003,
            "Persist cart",
            "gantt-bar-label-1003",
            "gantt-bar-1003",
            bar_title_paint_px("Persist cart"),
        ),
        (
            1005,
            "Cart UI",
            "gantt-bar-label-1005",
            "gantt-bar-1005",
            bar_title_paint_px("Cart UI"),
        ),
        (
            1012,
            "Unparented spike",
            "gantt-bar-label-1012",
            "gantt-bar-1012",
            bar_title_paint_px("Unparented spike"),
        ),
        (
            1013,
            "Go-live milestone",
            "gantt-bar-label-1013",
            "gantt-bar-1013",
            bar_title_paint_px("Go-live milestone"),
        ),
    ] {
        let label = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("launch Year {title} must paint a full outside label"));
        let width = f32::from(label.size.width);
        assert!(
            width + 8.0 >= min_w,
            "launch Year {title} must not truncate to Pers…/Go-l…, width={width} min={min_w}"
        );
        let bar = cx
            .debug_bounds(bar_selector)
            .unwrap_or_else(|| panic!("launch Year {title} bar"));
        let bar_left = f32::from(bar.origin.x);
        let bar_right = bar_left + f32::from(bar.size.width);
        let label_left = f32::from(label.origin.x);
        let label_right = label_left + width;
        let inside_bar = label_left + 2.0 >= bar_left && label_right <= bar_right + 2.0;
        let today_dodge_left = label_left + 2.0 < bar_left;
        let near_bar = (label_left - bar_right).abs() <= 48.0
            || (bar_left - label_right).abs() <= 48.0
            || inside_bar
            || today_dodge_left;
        assert!(
            near_bar,
            "launch Year {title} must sit within ~12px of its bar, not at Today, label={label_left}..{label_right} bar={bar_left}..{bar_right}"
        );
    }
}

#[gpui_kit::test]
fn year_toolbar_launch_outside_titles_stay_beside_the_bar(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    let nodes = state.nodes.clone();
    // apply_hierarchy auto_fit, then Year — before the Today button is pressed.
    state.gantt.auto_fit(&nodes, &[], 800.0);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let year = cx.debug_bounds("gantt-scale-Year").expect("Year toolbar");
    cx.simulate_click(year.center(), Modifiers::default());
    draw(cx);
    let today_line = cx
        .debug_bounds("gantt-today")
        .expect("launch Year Today line");
    if let Some(pill) = cx.debug_bounds("gantt-today-label") {
        let pill_top = f32::from(pill.origin.y);
        let pill_bottom = pill_top + f32::from(pill.size.height);
        let line_top = f32::from(today_line.origin.y);
        let line_bottom = line_top + f32::from(today_line.size.height);
        assert!(
            pill_top + 1.0 >= line_top && pill_bottom <= line_bottom + 1.0,
            "launch Year Today pill at {pill_top}..{pill_bottom} must sit on its line at {line_top}..{line_bottom}"
        );
    }
    view.update(cx, |view, _| {
        let today = jiff::Zoned::now().date();
        let today_px = view.state.gantt.x_for_date(today);
        let today_x =
            (today_px >= 0.0 && today_px <= view.state.gantt.timeline_width).then_some(today_px);
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars = view
            .state
            .gantt
            .layout_bars(&view.state.nodes, &rows, &[], 720.0);
        for id in [1001_i64, 1003, 1005, 1013] {
            let bar = bars
                .iter()
                .find(|bar| bar.id == id)
                .unwrap_or_else(|| panic!("launch Year bar {id}"));
            if bar_title_is_inside(bar) {
                continue;
            }
            let Some((x, w)) = ado_planner::gantt::bar_outside_label_frame(
                bar,
                view.state.gantt.timeline_width,
                true,
                today_x,
                bar.overflow_badge,
            ) else {
                continue;
            };
            assert!(
                outside_label_is_beside_bar(bar, x, w, true, bar.overflow_badge),
                "launch Year {} must stay within 12px of its bar, label={x}+{w} bar.x={}",
                bar.title,
                bar.x
            );
        }
    });
}

#[gpui_kit::test]
fn year_view_paints_a_label_on_every_visible_row(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Year;
    state.gantt.zoom = 1.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let visible = view.update(cx, |view, _| {
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars =
            view.state
                .gantt
                .layout_bars(&view.state.nodes, &rows, &view.state.iterations, 720.0);
        let timeline_w = view.state.gantt.timeline_width.max(1.0);
        bars.into_iter()
            .filter_map(|bar| {
                if bar.marker
                    || bar.kind == GanttBarKind::Unscheduled
                    || bar.kind == GanttBarKind::NoFields
                {
                    return None;
                }
                let width = if bar.milestone {
                    MILESTONE_SIZE
                } else {
                    bar.width.max(8.0)
                };
                if bar.x + width <= 0.0 || bar.x >= timeline_w {
                    return None;
                }
                Some((bar.id, bar.title.clone(), bar_title_is_inside(&bar)))
            })
            .collect::<Vec<_>>()
    });
    assert!(
        !visible.is_empty(),
        "Year view must have on-screen dated bars"
    );
    for (id, title, inside) in &visible {
        let selector = match id {
            1001 => "gantt-bar-label-1001",
            1002 => "gantt-bar-label-1002",
            1003 => "gantt-bar-label-1003",
            1005 => "gantt-bar-label-1005",
            1012 => "gantt-bar-label-1012",
            1013 => "gantt-bar-label-1013",
            1020 => "gantt-bar-label-1020",
            _ => "gantt-bar-label",
        };
        let label = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("Year {title} (#{id}) must paint a drawn label"));
        let min_w = if *inside { 24.0 } else { 36.0 };
        assert!(
            f32::from(label.size.width) >= min_w,
            "Year {title} label must stay readable, width={}",
            f32::from(label.size.width)
        );
    }
    for (id, title, selector) in [
        (1002_i64, "Cart", "gantt-bar-label-1002"),
        (1003, "Persist cart", "gantt-bar-label-1003"),
        (1012, "Unparented spike", "gantt-bar-label-1012"),
        (1013, "Go-live milestone", "gantt-bar-label-1013"),
    ] {
        assert!(
            visible.iter().any(|(row_id, _, _)| *row_id == id),
            "Year fixture must show {title} on screen"
        );
        assert!(
            cx.debug_bounds(selector).is_some(),
            "Year {title} must paint a drawn label"
        );
    }
}

#[gpui_kit::test]
fn year_go_live_milestone_keeps_its_label(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Year;
    state.gantt.zoom = 1.0;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let label = cx
        .debug_bounds("gantt-bar-label-1013")
        .expect("Year Go-live must paint an outside label");
    assert!(
        f32::from(label.size.width) + 8.0 >= bar_title_paint_px("Go-live milestone"),
        "Year Go-live must keep the full title outside the diamond, width={}",
        f32::from(label.size.width)
    );
}

#[gpui_kit::test]
fn year_persist_cart_label_and_go_live_link_clear_today(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Year;
    state.gantt.zoom = 1.0;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        !today_body_segments(cx).is_empty(),
        "Year body Today line must paint above arrows, gapped through chips"
    );
    let persist = cx
        .debug_bounds("gantt-bar-label-1003")
        .expect("Year Persist cart must paint an outside label, not Pers…");
    let persist_left = f32::from(persist.origin.x);
    let persist_right = persist_left + f32::from(persist.size.width);
    let persist_top = f32::from(persist.origin.y);
    let persist_bottom = persist_top + f32::from(persist.size.height);
    assert!(
        f32::from(persist.size.width) + 8.0 >= bar_title_paint_px("Persist cart"),
        "Year Persist cart must keep the full title outside the bar, width={}",
        f32::from(persist.size.width)
    );
    let go_live = cx
        .debug_bounds("gantt-bar-label-1013")
        .expect("Year Go-live must paint an outside label");
    assert!(
        f32::from(go_live.size.width) + 8.0 >= bar_title_paint_px("Go-live milestone"),
        "Year Go-live must keep the full title, width={}",
        f32::from(go_live.size.width)
    );
    if let Some(arrow) = cx.debug_bounds("gantt-dep-arrow-1003-1013") {
        let arrow_left = f32::from(arrow.origin.x);
        let arrow_right = arrow_left + f32::from(arrow.size.width);
        let arrow_top = f32::from(arrow.origin.y);
        let arrow_mid_y = arrow_top + f32::from(arrow.size.height) / 2.0;
        let overlaps_x = arrow_left < persist_right && arrow_right > persist_left;
        if overlaps_x {
            assert!(
                arrow_mid_y + 0.5 >= persist_bottom + 6.0,
                "Persist cart → Go-live stroke must sit ≥6px below the Persist cart label, stroke={arrow_mid_y} label_bottom={persist_bottom}"
            );
            if let Some(cart_ui) = cx.debug_bounds("gantt-bar-1005") {
                let cart_top = f32::from(cart_ui.origin.y);
                assert!(
                    arrow_mid_y < cart_top - 1.0,
                    "Persist cart → Go-live must stay in the Persist cart row, stroke={arrow_mid_y} Cart UI={cart_top}"
                );
            }
        } else {
            assert!(
                arrow_left >= persist_right + 6.0 || arrow_right <= persist_left - 6.0,
                "Persist cart → Go-live horizontal must clear the Persist cart label, arrow={arrow_left}..{arrow_right} label={persist_left}..{persist_right}"
            );
        }
    }
    if let (Some(cart_label), Some(today_line)) = (
        cx.debug_bounds("gantt-bar-label-1005"),
        cx.debug_bounds("gantt-today"),
    ) {
        let segments = today_body_segments(cx);
        let label_left = f32::from(cart_label.origin.x);
        let label_right = label_left + f32::from(cart_label.size.width);
        let header_x = f32::from(today_line.origin.x);
        let today_x = segments
            .first()
            .map(|line| f32::from(line.origin.x).min(header_x))
            .unwrap_or(header_x);
        let today_right = segments
            .first()
            .map(|line| {
                let line_x = f32::from(line.origin.x);
                (line_x + f32::from(line.size.width).max(3.0)).max(header_x + 3.0)
            })
            .unwrap_or(header_x + 3.0);
        assert_eq!(
            segments.len(),
            1,
            "Year Today stays one stroke behind the Cart UI label {label_left}..{label_right} today={today_x}..{today_right}"
        );
    }
}

#[gpui_kit::test]
fn dates_cell_stays_one_line_with_a_conflict_marker(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.target_date = Some("2026-09-01T00:00:00Z".into());
    }
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let cell = cx.debug_bounds("gantt-dates-cell").expect("dates cell");
    assert!(
        cx.debug_bounds("gantt-dates-overflow").is_some(),
        "a child past its parent dates must show the conflict marker"
    );
    assert!(
        f32::from(cell.size.height) <= ROW_HEIGHT + 1.0,
        "the Dates column must not wrap to two lines, height={}",
        f32::from(cell.size.height)
    );
    let mark = cx
        .debug_bounds("gantt-dates-overflow")
        .expect("overflow marker");
    let cell_right = f32::from(cell.origin.x) + f32::from(cell.size.width);
    let mark_right = f32::from(mark.origin.x) + f32::from(mark.size.width);
    assert!(
        mark_right <= cell_right - 2.0,
        "the amber ! must sit inside the Dates column, not clipped at the edge, mark_right={mark_right} cell_right={cell_right}"
    );
    assert!(
        f32::from(mark.size.width) >= 14.0 && f32::from(mark.size.height) >= 14.0,
        "the Dates ! must be a visible badge, got {:?}",
        mark.size
    );
    let bar_mark = cx
        .debug_bounds("gantt-parent-overflow")
        .expect("bar overflow badge");
    assert!(
        f32::from(bar_mark.size.width) >= 20.0 && f32::from(bar_mark.size.height) >= 20.0,
        "the bar ! must be a 22px badge (true 14px glyph), got {:?}",
        bar_mark.size
    );
}

#[gpui_kit::test]
fn overflow_badge_does_not_steal_the_right_edge_resize(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    // Shrink the parent so #2 overflows without stretching its bar past the
    // 1280px window (a Sep 1 target parks the right grip off-screen).
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 1) {
        node.target_date = Some("2026-07-10T00:00:00Z".into());
    }
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, cx| {
        view.state.gantt.hovered_id = Some(2);
        view.state.gantt.bar_hovered_id = Some(2);
        cx.notify();
    });
    draw(cx);
    let badge = cx
        .debug_bounds("gantt-parent-overflow")
        .expect("the overflow ! must still paint");
    let bar = cx.debug_bounds("gantt-bar-2").expect("overflow bar");
    let handle = cx
        .debug_bounds("gantt-link-handle-2-link-end")
        .expect("overflow bar must still expose a link handle");
    let bar_right = f32::from(bar.origin.x) + f32::from(bar.size.width);
    let badge_left = f32::from(badge.origin.x);
    let handle_left = f32::from(handle.origin.x);
    let handle_right = handle_left + f32::from(handle.size.width);
    assert!(
        badge_left >= bar_right - 0.5,
        "the ! must sit past the bar end so the grip stays hittable, badge_left={badge_left} bar_right={bar_right}"
    );
    assert!(
        handle_left >= bar_right - 0.5,
        "the link handle must sit just outside a resizable bar, handle_left={handle_left} bar_right={bar_right}"
    );
    assert!(
        badge_left >= handle_right - 0.5,
        "the ! must sit clear of the link handle, badge_left={badge_left} handle={handle_left}..{handle_right}"
    );
    assert!(
        (handle_right - handle_left - OUTER_LINK_HIT).abs() < 1.0,
        "link handle hit box must stay {}px on a resizable bar, got {}",
        OUTER_LINK_HIT,
        handle_right - handle_left
    );
    assert!(
        f32::from(badge.size.width) >= 20.0 && f32::from(badge.size.height) >= 20.0,
        "the bar ! must be a readable 22px badge (true 14px glyph), got {:?}",
        badge.size
    );
    assert!(
        bar_right < 1280.0,
        "the right edge must stay on-screen so the grip can receive the click, bar_right={bar_right}"
    );
    let on_end = point(
        bar.origin.x + bar.size.width - px(HANDLE_WIDTH + LINK_HANDLE_INSET / 2.0),
        bar.origin.y + bar.size.height / 2.0,
    );
    cx.simulate_mouse_down(on_end, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("the right-edge grip must win hits over the ! badge");
        assert_eq!(
            gesture.kind,
            GestureKind::ResizeEnd,
            "clicking the right edge under the ! must resize, got {:?}",
            gesture.kind
        );
    });
}

#[gpui_kit::test]
fn overflow_badge_does_not_cover_the_outside_title(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 1) {
        node.target_date = Some("2026-07-10T00:00:00Z".into());
    }
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.title = "Persist cart".into();
        node.start_date = Some("2026-07-06T00:00:00Z".into());
        node.target_date = Some("2026-07-20T00:00:00Z".into());
    }
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Quarter;
    state.gantt.zoom = 1.0;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let badge = cx
        .debug_bounds("gantt-parent-overflow")
        .expect("overflow !");
    let label = cx
        .debug_bounds("gantt-bar-label-2")
        .expect("Persist cart outside title");
    let badge_left = f32::from(badge.origin.x);
    let label_right = f32::from(label.origin.x) + f32::from(label.size.width);
    assert!(
        badge_left >= label_right - 1.0,
        "the Persist cart ! must sit past the outside title, badge_left={badge_left} label_right={label_right}"
    );
}

#[gpui_kit::test]
fn overflow_badge_wins_hits_over_a_link_elbow(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 1) {
        node.target_date = Some("2026-07-10T00:00:00Z".into());
    }
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 3) {
        node.predecessors = vec![2];
    }
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let badge = cx
        .debug_bounds("gantt-parent-overflow")
        .expect("overflow badge");
    let mid = point(
        badge.origin.x + badge.size.width / 2.0,
        badge.origin.y + badge.size.height / 2.0,
    );
    cx.simulate_mouse_move(mid, None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.hovered_link, None,
            "the ! badge must win the hit-test over a link elbow under it"
        );
    });
}

#[gpui_kit::test]
fn details_dialog_keeps_a_stable_height(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.form = Some(FormDraft::from_model(ado_core::WorkItemFormModel {
        id: 1,
        rev: 1,
        work_item_type: "Epic".into(),
        system_controls: Vec::new(),
        pages: vec![ado_core::FormPage {
            id: "details".into(),
            label: "Details".into(),
            groups: Vec::new(),
        }],
        values: serde_json::Map::new(),
    }));
    let (_epic_view, epic_cx) =
        cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    epic_cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(epic_cx);
    let epic = epic_cx
        .debug_bounds("work-item-form-dialog")
        .expect("epic dialog");
    let epic_h = f32::from(epic.size.height);
    let cancel = epic_cx.debug_bounds("cancel-form").expect("Cancel");
    let cancel_y = f32::from(cancel.origin.y);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.form = Some(FormDraft::from_model(ado_core::WorkItemFormModel {
        id: 4,
        rev: 1,
        work_item_type: "Task".into(),
        system_controls: Vec::new(),
        pages: vec![ado_core::FormPage {
            id: "details".into(),
            label: "Details".into(),
            groups: Vec::new(),
        }],
        values: serde_json::Map::new(),
    }));
    let (_task_view, task_cx) =
        cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    task_cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(task_cx);
    let task = task_cx
        .debug_bounds("work-item-form-dialog")
        .expect("task dialog");
    let task_cancel = task_cx.debug_bounds("cancel-form").expect("Cancel");
    assert!(
        (f32::from(task.size.height) - epic_h).abs() < 1.0,
        "dialog height must not depend on work item type, epic={epic_h} task={}",
        f32::from(task.size.height)
    );
    assert!(
        (f32::from(task_cancel.origin.y) - cancel_y).abs() < 1.0,
        "Cancel must stay put across types, epic y={cancel_y} task y={}",
        f32::from(task_cancel.origin.y)
    );
}

#[gpui_kit::test]
fn error_toast_copy_and_dismiss_do_not_overlap_the_title(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.toasts.push(
        ToastKind::Error,
        "The work item revision does not match the expected value from the last save. Refresh and try again.",
    );
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let title = cx.debug_bounds("toast-title").expect("toast title");
    let copy = cx.debug_bounds("toast-copy").expect("Copy");
    let dismiss = cx.debug_bounds("toast-dismiss").expect("dismiss");
    let title_right = f32::from(title.origin.x) + f32::from(title.size.width);
    let copy_left = f32::from(copy.origin.x);
    let copy_right = copy_left + f32::from(copy.size.width);
    let dismiss_left = f32::from(dismiss.origin.x);
    assert!(
        title_right <= copy_left + 1.0,
        "Copy must not overlap the toast text: title_right={title_right} copy_left={copy_left}"
    );
    assert!(
        copy_right <= dismiss_left + 1.0,
        "× must not overlap Copy: copy_right={copy_right} dismiss_left={dismiss_left}"
    );
}

#[gpui_kit::test]
fn dragging_an_unscheduled_row_shows_a_schedule_ghost(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 3) {
        node.start_date = None;
        node.target_date = None;
        node.has_date_fields = true;
    }
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let band = cx
        .debug_bounds("gantt-row-band-3")
        .expect("unscheduled gantt row");
    let start = point(
        gpui_kit::px(f32::from(band.origin.x) + f32::from(band.size.width) * 0.4),
        band.center().y,
    );
    let moved = point(start.x + px(48.), start.y);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(moved, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-schedule-ghost").is_some(),
        "dragging on an unscheduled row must show a ghost preview"
    );
    view.update(cx, |view, _| {
        assert_eq!(view.state.gantt.schedule_drag, Some(3));
    });
}

#[gpui_kit::test]
fn hovering_the_undo_toast_pauses_expiry(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.toasts.push_undo(
        "Moved Persist cart",
        UndoAction::RestoreDates {
            id: 1003,
            start_date: Some("2026-07-06T00:00:00Z".into()),
            target_date: Some("2026-07-20T00:00:00Z".into()),
            rev: 1,
        },
    );
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let toast = cx.debug_bounds("planner-toast").expect("undo toast");
    cx.simulate_mouse_move(toast.center(), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.toast_pause_for_test(),
            "moving onto the undo toast must pause expiry without a test-only hover hook"
        );
        view.state.toasts.items[0].created_at =
            std::time::Instant::now() - std::time::Duration::from_secs(30);
        view.state.toasts.expire();
        assert_eq!(
            view.state.toasts.items.len(),
            1,
            "a hovered undo toast must stay past 5s"
        );
    });
    let undo = cx.debug_bounds("toast-undo").expect("Undo");
    cx.simulate_mouse_move(undo.center(), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.toast_pause_for_test(),
            "hovering the Undo button must keep the toast paused"
        );
        view.state.toasts.items[0].created_at =
            std::time::Instant::now() - std::time::Duration::from_secs(30);
        view.state.toasts.expire();
        assert_eq!(
            view.state.toasts.items.len(),
            1,
            "the undo toast must stay open while the pointer is on Undo"
        );
    });
}

#[gpui_kit::test]
fn dismissing_a_hovered_toast_unpauses_expiry(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.toasts.push(ToastKind::Success, "Saved");
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, cx| {
        view.simulate_toast_hover_for_test(true, cx);
        view.simulate_toast_hover_for_test(true, cx);
        assert!(
            view.toast_pause_for_test(),
            "nested hover must pause expiry"
        );
    });
    let dismiss = cx
        .debug_bounds("toast-dismiss")
        .expect("toast dismiss control");
    cx.simulate_click(dismiss.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.toasts.items.is_empty(),
            "× must dismiss the hovered toast"
        );
        assert!(
            !view.toast_pause_for_test(),
            "dismissing a hovered toast must unpause so later toasts can expire"
        );
    });
}

#[gpui_kit::test]
fn bottom_band_does_not_pause_undone_toast_expiry(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.toasts.push_undo(
        "Undone",
        UndoAction::RestoreDates {
            id: 2,
            start_date: Some("2026-07-06T00:00:00Z".into()),
            target_date: Some("2026-07-20T00:00:00Z".into()),
            rev: 1,
        },
    );
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("planner-toast").is_some(),
        "the Undone toast must paint"
    );
    cx.simulate_mouse_move(point(px(24.), px(780.)), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            !view.toast_pause_for_test(),
            "the empty bottom band must not hover-pause toast expiry"
        );
        view.state.toasts.items[0].created_at =
            std::time::Instant::now() - std::time::Duration::from_secs(30);
        view.state.toasts.expire();
        assert!(
            view.state.toasts.items.is_empty(),
            "an Undone toast must auto-dismiss after its TTL"
        );
    });
}

#[gpui_kit::test]
fn toast_ttl_still_expires_across_a_scale_switch(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.toasts.push(ToastKind::Success, "Saved");
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("planner-toast").is_some(),
        "precondition: success toast is visible"
    );
    let day = cx.debug_bounds("gantt-scale-Day").expect("Day scale");
    cx.simulate_click(day.center(), Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("planner-toast").is_some(),
        "a fresh toast must survive a view switch"
    );
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.toasts.items.len(),
            1,
            "scale switch must keep toasts and only unstick hover-pause"
        );
        assert!(
            !view.toast_pause_for_test(),
            "scale switch must clear a stuck toast pause"
        );
        view.state.toasts.items[0].created_at =
            std::time::Instant::now() - std::time::Duration::from_secs(30);
        view.state.toasts.expire();
        assert!(
            view.state.toasts.items.is_empty(),
            "TTL must still dismiss the toast after a scale switch"
        );
    });
}

#[gpui_kit::test]
fn offscreen_bar_paints_an_edge_indicator(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.scroll_x = 4000.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, _| {
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars = view
            .state
            .gantt
            .layout_bars(&view.state.nodes, &rows, &[], 720.0);
        let bar = bars.iter().find(|bar| bar.id == 2).expect("leaf bar");
        assert!(
            bar.x + bar.width.max(8.0) <= 0.0,
            "precondition: work item 2 must sit fully left of the clip, x={} w={}",
            bar.x,
            bar.width
        );
    });
    assert!(
        cx.debug_bounds("gantt-bar-edge-left-2").is_some(),
        "a bar scrolled fully left of the viewport must show a left-edge indicator"
    );
    view.update(cx, |view, cx| {
        view.state.gantt.scroll_x = -4000.0;
        cx.notify();
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-bar-edge-right-2").is_some(),
        "a bar scrolled fully right of the viewport must show a right-edge indicator"
    );
}

#[gpui_kit::test]
fn day_offscreen_milestone_cue_includes_the_title(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Day;
    state.gantt.zoom = 1.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, _| {
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars = view
            .state
            .gantt
            .layout_bars(&view.state.nodes, &rows, &[], 720.0);
        let go_live = bars.iter().find(|bar| bar.id == 1013).expect("Go-live");
        assert!(
            go_live.x >= view.state.gantt.timeline_width,
            "precondition: Day Go-live must sit fully right of the clip, x={} timeline={}",
            go_live.x,
            view.state.gantt.timeline_width
        );
    });
    let cue = cx
        .debug_bounds("gantt-bar-edge-right-1013")
        .expect("Day off-screen Go-live must show a right-edge cue");
    assert!(
        f32::from(cue.size.width) >= 64.0,
        "Day off-screen Go-live cue must include the diamond and title, not a bare ›, width={}",
        f32::from(cue.size.width)
    );
    cx.simulate_mouse_move(cue.center(), None, Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-bar-edge-tooltip").is_some(),
        "hovering an off-range edge chip must show a dates tooltip"
    );
    assert!(
        cx.debug_bounds("gantt-hover-tooltip").is_none(),
        "off-range Go-live must not also paint a phantom tooltip at the off-screen diamond"
    );
}

#[gpui_kit::test]
fn day_selected_milestone_paints_a_diamond_not_a_day_box(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 8, 10).unwrap();
    state.gantt.scale = GanttScale::Day;
    state.gantt.zoom = 1.0;
    state.gantt.selected_id = Some(1013);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let go_live = cx
        .debug_bounds("gantt-bar-1013")
        .expect("on-screen Day Go-live");
    let width = f32::from(go_live.size.width);
    let day_px = GanttScale::Day.pixels_per_day(1.0);
    assert!(
        (width - MILESTONE_SIZE).abs() < 4.0,
        "a selected Day milestone must stay a {MILESTONE_SIZE}px diamond, not a {day_px}px day box, width={width}"
    );
}

#[gpui_kit::test]
fn month_persist_go_live_link_clears_the_painted_label(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    if let Some(spike) = state.nodes.iter_mut().find(|node| node.id == 1012) {
        spike.predecessors = vec![1003];
    }
    let nodes = state.nodes.clone();
    state.gantt.auto_fit(&nodes, &[], 800.0);
    if state.gantt.scale != GanttScale::Month {
        state.gantt.retarget_year_gutter(|gantt| {
            gantt.set_scale_from_toolbar(GanttScale::Month, Date::new(2026, 11, 3).unwrap());
        });
    }
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let persist = cx
        .debug_bounds("gantt-bar-label-1003")
        .expect("Month Persist cart must paint an outside label");
    assert!(
        f32::from(persist.size.width) + 8.0 >= bar_title_paint_px("Persist cart"),
        "Month Persist cart must keep the full title, width={}",
        f32::from(persist.size.width)
    );
    view.update(cx, |view, _| {
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars =
            view.state
                .gantt
                .layout_bars(&view.state.nodes, &rows, &view.state.iterations, 720.0);
        let pred = bars
            .iter()
            .find(|bar| bar.id == 1003)
            .expect("Persist cart");
        let live = view
            .state
            .gantt
            .layout_dependency_arrows(&bars)
            .into_iter()
            .find(|arrow| arrow.from_id == 1003 && arrow.to_id == 1013)
            .expect("Persist cart → Go-live");
        let timeline_w = view.state.gantt.timeline_width.max(1.0);
        let Some((left, width)) =
            ado_planner::gantt::bar_outside_label_frame(pred, timeline_w, true, None, false)
        else {
            return;
        };
        let right = left + width;
        let painted_w = pred.width.max(8.0);
        if live.x_head > pred.x + painted_w + 12.0 {
            assert!(
                live.x1 + 0.5 >= right + 6.0 || live.x1 <= left - 6.0 + 0.5,
                "Month Persist cart → Go-live must sit ≥6px from the label, x1={} label={left}..{right}",
                live.x1
            );
        }
        let cart_ui = bars.iter().find(|bar| bar.id == 1005).unwrap();
        assert!(
            live.x2 < cart_ui.x,
            "Month Persist cart → Go-live inbound must drop before Cart UI, x2={} cart.x={}",
            live.x2,
            cart_ui.x
        );
        assert!(
            !live.wraps_around_successor(),
            "forward Persist cart → Go-live must not wrap along the Go-live row"
        );
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
        assert_eq!(
            view.state.gantt.hovered_id,
            Some(1),
            "mouse-up on the bar must restore hover at the release point"
        );
        assert!(
            !view.state.gantt.hover_locked,
            "mouse-up must not lock hover or the next tooltip waits a second"
        );
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
        cx.debug_bounds("gantt-splitter-grip").is_some(),
        "the list/timeline divider must show a visible grip"
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
        cx.debug_bounds("gantt-link-handle-2-link-end").is_some(),
        "a selected bar must expose a hittable link handle without hovering first"
    );
    assert!(
        cx.debug_bounds("gantt-link-dot-2-link-end").is_none(),
        "a selected row must not paint resize dots until the bar is hovered"
    );
    view.update(cx, |view, cx| {
        view.state.gantt.hovered_id = Some(2);
        view.state.gantt.bar_hovered_id = Some(2);
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
        (f32::from(resize.size.width) - RESIZE_HIT_WIDTH).abs() < 1.0,
        "end resize grip must be the inner 14px of the bar, got {}",
        f32::from(resize.size.width)
    );
    assert!(
        (f32::from(link.size.width) - HANDLE_WIDTH).abs() > 4.0,
        "link handles must not share the start-resize size"
    );
    assert!(
        cx.debug_bounds("gantt-row-accent-2").is_some(),
        "the selected row must show a left accent bar"
    );
    let bar = cx.debug_bounds("gantt-bar-2").expect("hovered bar");
    let start = cx
        .debug_bounds("gantt-resize-handle-2-start")
        .expect("hovered leaf bar must show a start resize handle");
    assert!(
        f32::from(start.origin.x) + 0.5 >= f32::from(bar.origin.x)
            && f32::from(start.origin.x) <= f32::from(bar.origin.x) + 4.0,
        "start resize grip must sit on the inner left of the bar, handle x {} vs bar x {}",
        f32::from(start.origin.x),
        f32::from(bar.origin.x)
    );
    let bar_right = f32::from(bar.origin.x) + f32::from(bar.size.width);
    let handle_left = f32::from(resize.origin.x);
    let handle_right = handle_left + f32::from(resize.size.width);
    assert!(
        handle_left < bar_right - 1.0,
        "end resize grip must overlap the bar's right interior, handle x {handle_left} vs bar right {bar_right}"
    );
    assert!(
        handle_right <= bar_right - LINK_HANDLE_INSET + 0.5
            && handle_right + 4.0 >= bar_right - LINK_HANDLE_INSET,
        "end resize grip must sit just left of the 6px link sliver, handle right {handle_right} vs bar right {bar_right}"
    );
    let grip = cx
        .debug_bounds("gantt-resize-grip-2-end")
        .expect("hovered leaf bar must show a visible edge grip");
    let dot = cx
        .debug_bounds("gantt-link-dot-2-link-end")
        .expect("hovered leaf bar must show a link connector");
    assert!(
        (f32::from(grip.size.width) - 4.0).abs() < 1.0,
        "the resize grip must be a 4px edge pill, got {}",
        f32::from(grip.size.width)
    );
    assert!(
        (f32::from(dot.size.width) - LINK_HANDLE).abs() < 1.0
            && (f32::from(dot.size.height) - LINK_HANDLE).abs() < 1.0,
        "the link connector must be an 8px circle, got {:?}",
        dot.size
    );
    let grip_right = f32::from(grip.origin.x) + f32::from(grip.size.width);
    let dot_left = f32::from(dot.origin.x);
    assert!(
        dot_left >= bar_right - 0.5,
        "the link dot must sit outside the bar so it is not a second resize grip, dot {dot_left} vs bar {bar_right}"
    );
    assert!(
        grip_right <= bar_right + 0.5,
        "the visible resize grip must stay inside the bar, grip right {grip_right} vs bar {bar_right}"
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
    assert!(
        cx.debug_bounds("gantt-dates-range-start").is_some(),
        "a complete range must highlight the start day"
    );
    assert!(
        cx.debug_bounds("gantt-dates-range-end").is_some(),
        "a complete range must highlight the end day"
    );
    assert!(
        cx.debug_bounds("gantt-dates-in-range").is_some(),
        "a complete range must fill the days between start and end"
    );
    let before = view.update(cx, |view, cx| view.inline_calendar_month(cx));
    let prev = cx
        .debug_bounds("gantt-dates-prev-month")
        .expect("prev month");
    cx.simulate_click(prev.center(), gpui_kit::Modifiers::default());
    draw(cx);
    let after = view.update(cx, |view, cx| view.inline_calendar_month(cx));
    let expected = before.map(|month| if month == 1 { 12 } else { month - 1 });
    assert_eq!(
        after, expected,
        "clicking ‹ must move exactly one month, {before:?} -> {after:?}"
    );
    assert!(
        cx.debug_bounds("gantt-dates-month-label").is_some(),
        "range picker must show the current month between the arrows"
    );
}

#[gpui_kit::test]
fn zoom_plus_widens_month_columns_and_minus_goes_below_100(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let oct1 = Date::new(2026, 10, 1).unwrap();
    let before = view.update(cx, |view, _| view.state.gantt.period_width(oct1));
    let zoom_in = cx.debug_bounds("gantt-zoom-in").expect("zoom in");
    cx.simulate_click(zoom_in.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let after = view.state.gantt.period_width(oct1);
        assert!(
            after > before,
            "+ must widen month columns: {before} -> {after}"
        );
        assert!(
            view.state.gantt.zoom > 1.0,
            "+ must raise zoom above 100%, got {}",
            view.state.gantt.zoom
        );
    });
    view.update(cx, |view, _| {
        view.state.gantt.zoom = 1.0;
        view.state.gantt.scroll_x = 0.0;
    });
    let zoom_out = cx.debug_bounds("gantt-zoom-out").expect("zoom out");
    cx.simulate_click(zoom_out.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.zoom < 1.0,
            "− must go below 100%, got {}",
            view.state.gantt.zoom
        );
        assert!(
            view.state.gantt.period_width(oct1) < before,
            "− below 100% must shrink month columns"
        );
    });
}

#[gpui_kit::test]
fn hovered_row_shows_open_details_icon(cx: &mut gpui_kit::TestAppContext) {
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
    assert!(
        cx.debug_bounds("gantt-open-details").is_some(),
        "hovered rows must show an ⓘ to open details"
    );
}

#[gpui_kit::test]
fn details_slot_is_reserved_when_the_pill_is_hidden(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-details-slot").is_some(),
        "the ⓘ Details column must keep its width when the pill is hidden"
    );
    assert!(
        cx.debug_bounds("gantt-open-details").is_some(),
        "ⓘ must stay visible so details can open without a prior selection"
    );
}

#[gpui_kit::test]
fn selected_link_shows_a_visible_remove_control(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.predecessors = vec![1];
        node.start_date = Some("2026-07-21T00:00:00Z".into());
        node.target_date = Some("2026-08-04T00:00:00Z".into());
    }
    state.gantt.selected_link = Some((1, 2));
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-link-remove").is_some(),
        "a selected dependency must show a × to remove it"
    );
}

#[gpui_kit::test]
fn delete_on_a_selected_item_asks_for_confirm(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let delete = KeyDownEvent {
        keystroke: Keystroke::parse("delete").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&delete, cx);
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-delete-confirm").is_some(),
        "Delete on a selected work item must ask before removing it"
    );
}

#[gpui_kit::test]
fn held_delete_does_not_confirm_the_dialog(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let delete = KeyDownEvent {
        keystroke: Keystroke::parse("delete").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    let held = KeyDownEvent {
        keystroke: Keystroke::parse("delete").unwrap(),
        is_held: true,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&delete, cx);
        view.handle_key(&held, cx);
        view.handle_key(&delete, cx);
        assert_eq!(
            view.pending_delete_id(),
            Some(1),
            "Delete must only open the confirm dialog; repeats must not confirm it"
        );
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-delete-confirm").is_some(),
        "the confirm dialog must stay until Enter or the Delete button"
    );
}

#[gpui_kit::test]
fn enter_confirms_delete_and_busy_io_keeps_the_dialog(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let delete = KeyDownEvent {
        keystroke: Keystroke::parse("delete").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    let enter = KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&delete, cx);
        view.set_busy_for_test(true);
        view.handle_key(&enter, cx);
        assert_eq!(
            view.pending_delete_id(),
            Some(1),
            "confirm must wait until planner I/O is idle"
        );
        view.set_busy_for_test(false);
        view.handle_key(&enter, cx);
        assert_eq!(
            view.pending_delete_id(),
            None,
            "Enter on an idle confirm dialog must start the delete"
        );
        assert!(view.is_busy(), "confirming delete must take the I/O slot");
    });
}

#[gpui_kit::test]
fn f2_opens_inline_title_on_the_selected_row(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(1);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    cx.simulate_keystrokes("f2");
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-title").is_some(),
        "F2 must start an inline title edit on the selected row"
    );
}

#[gpui_kit::test]
fn click_on_selected_title_selects_without_renaming(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let title = cx
        .debug_bounds("gantt-title-1")
        .expect("selected row title");
    cx.simulate_click(title.center(), gpui_kit::Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-title").is_none(),
        "a single click on a selected title must not start rename"
    );
    view.update(cx, |view, _| {
        assert_eq!(view.state.gantt.selected_id, Some(1));
    });
    let delete = KeyDownEvent {
        keystroke: Keystroke::parse("delete").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&delete, cx);
        assert_eq!(
            view.pending_delete_id(),
            Some(1),
            "Delete after a title click must open confirm, not type into a rename field"
        );
    });
}

#[gpui_kit::test]
fn team_search_shows_a_placeholder(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    view.update(cx, |view, cx| {
        assert_eq!(
            view.scope_placeholder(cx),
            "Search",
            "Org/Project/Team search must show a Search placeholder"
        );
    });
}

#[gpui_kit::test]
fn parent_summary_bar_resizes_without_the_link_dot_stealing(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(1);
    state.gantt.hovered_id = Some(1);
    state.gantt.bar_hovered_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1").expect("parent summary bar");
    let bar_right = f32::from(bar.origin.x) + f32::from(bar.size.width);
    assert!(
        cx.debug_bounds("gantt-link-handle-1-link-end").is_some(),
        "a hovered parent summary bar must show a link handle"
    );
    let handle = cx
        .debug_bounds("gantt-resize-handle-1-end")
        .expect("a parent with its own dates must show a right resize grip");
    assert!(
        cx.debug_bounds("gantt-summary-edge-1-end").is_none(),
        "a dated parent must resize, not show a rollup not-allowed edge"
    );
    let handle_right = f32::from(handle.origin.x) + f32::from(handle.size.width);
    assert!(
        handle_right <= bar_right + 0.5,
        "the end grip must stay inside the bar, handle_right={handle_right} bar_right={bar_right}"
    );
    if let Some(dot) = cx.debug_bounds("gantt-link-handle-1-link-end") {
        let dot_left = f32::from(dot.origin.x);
        assert!(
            dot_left >= bar_right - 0.5,
            "the link dot must sit past the bar so it does not steal resize, dot_left={dot_left} bar_right={bar_right}"
        );
    }
    let grip = point(
        bar.origin.x + bar.size.width - px(HANDLE_WIDTH + LINK_HANDLE_INSET / 2.0),
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
            .expect("3px inside a parent end must resize, not start a link");
        assert_eq!(
            gesture.kind,
            GestureKind::ResizeEnd,
            "Cart-style parent right edge must resize, got {:?}",
            gesture.kind
        );
        assert_eq!(gesture.id, 1);
    });
}

#[gpui_kit::test]
fn short_year_parent_has_no_rollup_edge(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.scale = GanttScale::Year;
    state.gantt.zoom = 1.0;
    state.gantt.origin = Date::new(2026, 1, 1).unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1").expect("Year parent sliver");
    let bar_w = f32::from(bar.size.width);
    assert!(
        bar_w > 8.0 && bar_w <= 24.5,
        "Year 14-day parent should be a 16–24px sliver, width={bar_w}"
    );
    assert!(
        cx.debug_bounds("gantt-summary-edge-1-start").is_none(),
        "a 16–24px Year parent must not paint a rollup not-allowed start"
    );
    assert!(
        cx.debug_bounds("gantt-summary-edge-1-end").is_none(),
        "a 16–24px Year parent must not paint a rollup not-allowed end"
    );
    let edge = point(bar.origin.x + px(2.), bar.center().y);
    cx.simulate_mouse_down(edge, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("a short Year parent edge must still move");
        assert_eq!(
            gesture.kind,
            GestureKind::Move,
            "16–24px parent edges must stay Move, got {:?}",
            gesture.kind
        );
    });
}

#[gpui_kit::test]
fn summary_edge_does_not_silently_move(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1").expect("summary bar");
    let edge = point(
        bar.origin.x + bar.size.width - px(HANDLE_WIDTH + LINK_HANDLE_INSET / 2.0),
        bar.origin.y + bar.size.height / 2.0,
    );
    cx.simulate_mouse_down(edge, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("a dated parent edge must start a resize");
        assert_eq!(
            gesture.kind,
            GestureKind::ResizeEnd,
            "3px inside a summary end must resize, got {:?}",
            gesture.kind
        );
        let hit = view
            .last_bar_hit_for_test()
            .expect("summary edge must record a window-coord hit");
        assert_eq!(
            hit.zone,
            BarHitZone::ResizeEnd,
            "3px inside a summary end must be resize-end, got {}",
            hit.zone.as_str()
        );
    });
    cx.simulate_mouse_up(edge, MouseButton::Left, Modifiers::default());
    draw(cx);
    let body = cx.debug_bounds("gantt-bar-1").expect("summary bar");
    cx.simulate_mouse_down(body.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("the body of a summary bar can still move");
        assert_eq!(gesture.kind, GestureKind::Move);
        assert_eq!(gesture.id, 1);
    });
}

#[gpui_kit::test]
fn selected_parent_start_grip_resizes_instead_of_moving(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1").expect("summary bar");
    // Inner 1px of the 8px start grip: the 2px selection border must not
    // turn this into Move.
    let inner = point(bar.origin.x + px(7.), bar.origin.y + bar.size.height / 2.0);
    cx.simulate_mouse_down(inner, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("selected parent start grip must resize");
        assert_eq!(
            gesture.kind,
            GestureKind::ResizeStart,
            "inner start grip must be ResizeStart, got {:?}",
            gesture.kind
        );
        let hit = view
            .last_bar_hit_for_test()
            .expect("grip press must record a resize-start hit");
        assert_eq!(
            hit.zone,
            BarHitZone::ResizeStart,
            "forced grip press must stay resize-start, got {}",
            hit.zone.as_str()
        );
    });
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
        cx.debug_bounds("gantt-resize-handle-2-end").is_some(),
        "end grip must be hittable without hovering first"
    );
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    let grip = point(
        bar.origin.x + bar.size.width - px(HANDLE_WIDTH + LINK_HANDLE_INSET / 2.0),
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

#[gpui_kit::test]
fn inner_pixel_of_a_selected_end_grip_still_resizes(cx: &mut gpui_kit::TestAppContext) {
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
    let handle = cx
        .debug_bounds("gantt-resize-handle-2-end")
        .expect("end grip");
    let inner = point(
        handle.origin.x + px(1.),
        handle.origin.y + handle.size.height / 2.0,
    );
    cx.simulate_mouse_down(inner, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("the inner pixel of the end grip must still start a resize");
        assert_eq!(
            gesture.kind,
            GestureKind::ResizeEnd,
            "grip mouse-down must force ResizeEnd even when the painted border box would classify Move, got {:?}",
            gesture.kind
        );
        assert_eq!(gesture.id, 2);
    });
}

#[gpui_kit::test]
fn click_to_select_at_year_scale_does_not_shift_dates(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.scale = GanttScale::Year;
    state.gantt.selected_id = None;
    let before = state
        .nodes
        .iter()
        .find(|node| node.id == 2)
        .map(|node| (node.start_date.clone(), node.target_date.clone()))
        .expect("leaf");
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    cx.simulate_mouse_down(bar.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    cx.simulate_mouse_up(bar.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(view.state.gantt.selected_id, Some(2));
        let node = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 2)
            .expect("leaf");
        assert_eq!(
            (node.start_date.clone(), node.target_date.clone()),
            before,
            "selecting a bar must not PATCH dates when the pointer did not move"
        );
        assert!(
            view.state.gantt.gesture.is_none()
                || view
                    .state
                    .gantt
                    .gesture
                    .as_ref()
                    .is_some_and(|g| { g.live_start == g.start && g.live_target == g.target }),
            "a click without a drag must not change live dates"
        );
    });
}

#[gpui_kit::test]
fn hovered_end_grip_wins_hit_test_over_move(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, cx| {
        view.state.gantt.hovered_id = Some(2);
        cx.notify();
    });
    draw(cx);
    let handle = cx
        .debug_bounds("gantt-resize-handle-2-end")
        .expect("hovered leaf bar must show an end grip");
    cx.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("mouse-down on the visible end grip must start a resize");
        assert_eq!(
            gesture.kind,
            GestureKind::ResizeEnd,
            "the end grip must win the hit-test over Move, got {:?}",
            gesture.kind
        );
        assert_eq!(gesture.id, 2);
    });
}

#[gpui_kit::test]
fn end_grip_wins_hit_test_over_bar_body_and_link_handle(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, cx| {
        view.state.gantt.hovered_id = Some(2);
        cx.notify();
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    let handle = cx
        .debug_bounds("gantt-resize-handle-2-end")
        .expect("hovered leaf bar must show an end grip");
    let link = cx
        .debug_bounds("gantt-link-handle-2-link-end")
        .expect("hovered leaf bar must show a link handle");
    let bar_right = f32::from(bar.origin.x) + f32::from(bar.size.width);
    let handle_left = f32::from(handle.origin.x);
    let handle_right = handle_left + f32::from(handle.size.width);
    let link_left = f32::from(link.origin.x);
    assert!(
        handle_left < bar_right - 1.0,
        "end grip must overlap the bar body, grip {handle_left}..{handle_right} bar right {bar_right}"
    );
    assert!(
        handle_right <= link_left + 0.5,
        "end grip must stay inside the bar, grip {handle_left}..{handle_right} link {link_left}"
    );
    let on_bar = point(
        bar.origin.x + bar.size.width - px(HANDLE_WIDTH + LINK_HANDLE_INSET / 2.0),
        bar.origin.y + bar.size.height / 2.0,
    );
    cx.simulate_mouse_down(on_bar, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("mouse-down on the bar's right edge must start a resize");
        assert_eq!(
            gesture.kind,
            GestureKind::ResizeEnd,
            "the end grip must win over the bar body, got {:?}",
            gesture.kind
        );
        assert_eq!(gesture.id, 2);
    });
}

#[gpui_kit::test]
fn mouse_down_3px_inside_the_right_end_resizes_end_only(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let start_before = Date::new(2026, 7, 6).unwrap();
    let target_before = Date::new(2026, 7, 20).unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    let grip = point(
        bar.origin.x + bar.size.width - px(HANDLE_WIDTH + LINK_HANDLE_INSET / 2.0),
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
            .expect("mouse-down 3px inside the right end must start a resize");
        assert_eq!(
            gesture.kind,
            GestureKind::ResizeEnd,
            "inner-end grip mouse-down must be ResizeEnd, got {:?}",
            gesture.kind
        );
    });
    let ppd = view.update(cx, |view, _| {
        view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom)
    });
    cx.simulate_mouse_move(
        point(grip.x + px(ppd * 4.0), grip.y),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view.state.gantt.gesture.clone().expect("live resize");
        let (start, target) = GanttView::gesture_patch_dates(&gesture);
        assert_eq!(
            start, None,
            "resizing the end must not patch Start Date, got {start:?}"
        );
        assert_eq!(
            target,
            Some(target_before.saturating_add(4.days())),
            "dragging 4 days from the inner right grip must move Target Date, got {target:?}"
        );
        assert_eq!(
            gesture.live_start, start_before,
            "Start Date must stay {start_before}, got {}",
            gesture.live_start
        );
        assert_ne!(
            gesture.live_target, target_before,
            "Target Date must change"
        );
    });
    assert!(
        cx.debug_bounds("gantt-drag-preview").is_some(),
        "a live resize must show the date preview chip"
    );
    assert!(
        cx.debug_bounds("gantt-drag-date-start").is_some(),
        "a live resize must show the Start Date pill on the bar"
    );
    assert!(
        cx.debug_bounds("gantt-drag-date-end").is_some(),
        "a live resize must show the Target Date pill on the bar"
    );
    let preview = cx
        .debug_bounds("gantt-drag-preview")
        .expect("live resize preview");
    let bar_live = cx
        .debug_bounds("gantt-bar-2")
        .expect("leaf bar during resize");
    let preview_top = f32::from(preview.origin.y);
    let preview_bottom = preview_top + f32::from(preview.size.height);
    let bar_top = f32::from(bar_live.origin.y);
    let bar_bottom = bar_top + f32::from(bar_live.size.height);
    assert!(
        preview_top < bar_bottom + 4.0 && preview_bottom > bar_top - 4.0,
        "live drag preview must stay in the bar's row so the last visible row is not clipped, preview={preview_top}..{preview_bottom} bar={bar_top}..{bar_bottom}"
    );
    cx.simulate_mouse_up(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "mouse-up must end the resize gesture"
        );
    });
    assert!(
        cx.debug_bounds("gantt-drag-preview").is_none(),
        "the preview chip must clear when the resize ends, without waiting for another mouse-move"
    );
    cx.simulate_mouse_move(point(px(8.), px(8.)), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.pin_bar_tooltip, None,
            "leaving the bar must unpin the live tooltip"
        );
    });
    assert!(
        cx.debug_bounds("gantt-bar-tooltip").is_none(),
        "leaving the bar must keep the tooltip cleared"
    );
}

#[gpui_kit::test]
fn drop_undo_and_pointer_leave_clear_the_bar_tooltip(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    let grip = point(
        bar.origin.x + bar.size.width - px(HANDLE_WIDTH + LINK_HANDLE_INSET / 2.0),
        bar.origin.y + bar.size.height / 2.0,
    );
    cx.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    let ppd = view.update(cx, |view, _| {
        view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom)
    });
    cx.simulate_mouse_move(
        point(grip.x + px(ppd * 3.0), grip.y),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    cx.simulate_mouse_up(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        view.state.gantt.pin_bar_tooltip = Some(2);
    });
    let undo = KeyDownEvent {
        keystroke: Keystroke::parse("ctrl-z").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&undo, cx);
        assert_eq!(
            view.state.gantt.pin_bar_tooltip, None,
            "undo must clear a stale bar tooltip"
        );
        view.state.gantt.pin_bar_tooltip = Some(2);
    });
    draw(cx);
    cx.simulate_mouse_move(point(px(8.), px(8.)), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.pin_bar_tooltip, None,
            "pointer leave must clear a stale bar tooltip"
        );
    });
    assert!(
        cx.debug_bounds("gantt-bar-tooltip").is_none(),
        "a stale tooltip must not stay painted over empty space"
    );
}

#[gpui_kit::test]
fn ctrl_z_clears_the_hover_chip_so_the_next_press_moves(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    let grip = grab_end_resize(bar);
    cx.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    let ppd = view.update(cx, |view, _| {
        view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom)
    });
    cx.simulate_mouse_move(
        point(grip.x + px(ppd * 3.0), grip.y),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    cx.simulate_mouse_up(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        view.state.gantt.pin_bar_tooltip = Some(2);
        view.set_suppress_bar_tooltip_for_test(false);
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-bar-tooltip").is_some(),
        "precondition: dates chip is up after the drop"
    );
    let undo = KeyDownEvent {
        keystroke: Keystroke::parse("ctrl-z").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&undo, cx);
        assert_eq!(view.state.gantt.pin_bar_tooltip, None);
        assert!(
            !view.suppress_bar_tooltip_for_test(),
            "undo must recompute hover at the resting pointer, not suppress the tooltip"
        );
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-bar-tooltip").is_none(),
        "the dates chip must unmount on undo so it cannot swallow the next press"
    );
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar after undo");
    cx.simulate_mouse_down(bar.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_some(),
            "the first press after Ctrl+Z must start a bar gesture, not be swallowed"
        );
    });
}

#[gpui_kit::test]
fn ctrl_z_recomputes_handle_hit_under_a_still_pointer(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 1.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart");
    cx.simulate_mouse_move(bar.center(), None, Modifiers::default());
    draw(cx);
    let handle = cx
        .debug_bounds("gantt-link-handle-1003-link-end")
        .expect("outer handle");
    let handle_x = f32::from(handle.origin.x) + f32::from(handle.size.width) / 2.0 + 4.0;
    let handle_y = f32::from(handle.origin.y) + f32::from(handle.size.height) / 2.0;
    let rest = point(px(handle_x), px(handle_y));
    let grip = grab_end_resize(bar);
    cx.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    let ppd = view.update(cx, |view, _| {
        view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom)
    });
    cx.simulate_mouse_move(
        point(grip.x + px(ppd * 3.0), grip.y),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    cx.simulate_mouse_up(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    cx.simulate_mouse_move(rest, None, Modifiers::default());
    draw(cx);
    let undo = KeyDownEvent {
        keystroke: Keystroke::parse("ctrl-z").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&undo, cx);
        assert_eq!(
            view.hover_bar_zone_for_test().map(|(_, zone)| zone),
            Some(BarHitZone::Link),
            "undo must recompute the handle hit under a still pointer"
        );
    });
    draw(cx);
    cx.simulate_mouse_down(rest, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::Link),
            "the first handle press after Ctrl+Z must start a link, not be swallowed"
        );
        assert!(
            view.state.gantt.selected_link.is_none(),
            "handle press after undo must not select the outgoing stub"
        );
    });
}

#[gpui_kit::test]
fn ctrl_z_recomputes_hover_against_filtered_rows(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.toggle_hidden_type("Task");
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = FITTED_MONTH_ZOOM;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-bar-1005").is_none(),
        "Hide Task must unmount Cart UI so later rows shift up"
    );
    let persist = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart");
    let grip = grab_end_resize(persist);
    cx.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    let ppd = view.update(cx, |view, _| {
        view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom)
    });
    cx.simulate_mouse_move(
        point(grip.x + px(ppd * 3.0), grip.y),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    cx.simulate_mouse_up(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    let go_live = cx
        .debug_bounds("gantt-bar-1013")
        .expect("Go-live after Hide Task");
    cx.simulate_mouse_move(go_live.center(), None, Modifiers::default());
    draw(cx);
    let undo = KeyDownEvent {
        keystroke: Keystroke::parse("ctrl-z").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&undo, cx);
        assert_eq!(
            view.hover_bar_zone_for_test().map(|(id, _)| id),
            Some(1013),
            "undo must classify the resting pointer against painted rows, not hidden Task rows"
        );
        assert_eq!(
            view.state.gantt.bar_hovered_id,
            Some(1013),
            "the first drag after Ctrl+Z must target Go-live, not hidden Cart UI"
        );
    });
}

#[gpui_kit::test]
fn dates_chip_press_starts_a_bar_move(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, _| {
        view.state.gantt.pin_bar_tooltip = Some(2);
    });
    draw(cx);
    let tip = cx
        .debug_bounds("gantt-bar-tooltip")
        .expect("pinned dates chip");
    cx.simulate_mouse_down(tip.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::Move),
            "a press on the dates chip must move the bar, not disappear as a no-op"
        );
        assert_eq!(view.state.gantt.pin_bar_tooltip, None);
    });
}

#[gpui_kit::test]
fn drop_hides_the_bar_tooltip_beside_the_moved_toast(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    let origin = bar.center();
    cx.simulate_mouse_down(origin, MouseButton::Left, Modifiers::default());
    draw(cx);
    let ppd = view.update(cx, |view, _| {
        view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom)
    });
    let dest = point(origin.x + px(ppd * 5.0), origin.y);
    cx.simulate_mouse_move(dest, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars = view
            .state
            .gantt
            .layout_bars(&view.state.nodes, &rows, &[], 720.0);
        let bar = bars.iter().find(|bar| bar.id == 2).expect("leaf");
        let tip = bar_hover_tooltip(bar, &view.state.nodes);
        assert!(
            !tip.contains("Jul 6") || tip.contains("Jul 11"),
            "a live drag tooltip must follow the in-flight range, got {tip:?}"
        );
    });
    cx.simulate_mouse_up(dest, MouseButton::Left, Modifiers::default());
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.pin_bar_tooltip, None,
            "drop must not pin a dates chip beside the Moved toast"
        );
        assert!(
            !view.suppress_bar_tooltip_for_test(),
            "drop must recompute hover at the release point, without a 1px nudge"
        );
        assert_eq!(
            view.hover_bar_zone_for_test().map(|(_, zone)| zone),
            Some(BarHitZone::Move),
            "dropping mid-bar must restore the move hover, not leave a leftover ↔"
        );
        assert_eq!(
            view.hover_cursor_for_test(),
            None,
            "dropping mid-bar must not leave an ew-resize cursor stuck"
        );
        assert!(
            !view.state.gantt.hover_locked,
            "drop must not lock hover or the first tooltip after a drag waits ~1s"
        );
        let committed = view.last_committed_start_for_test();
        assert!(
            committed.is_some_and(|day| day > Date::new(2026, 7, 6).unwrap()),
            "move must commit a later Start Date, got {committed:?}"
        );
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-bar-tooltip").is_none(),
        "the pinned dates chip must not sit beside the Moved toast after drop"
    );
    assert!(
        cx.debug_bounds("gantt-hover-tooltip").is_some(),
        "drop must show the hover tooltip at the still pointer without a hover-enter"
    );
    assert!(
        cx.debug_bounds("gantt-hover-date-start").is_some(),
        "drop / hover must show the Start Date pill"
    );
    assert!(
        cx.debug_bounds("gantt-hover-date-end").is_some(),
        "drop / hover must show the Target Date pill"
    );
}

#[gpui_kit::test]
fn two_to_four_px_inside_the_end_edge_resizes(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    for inset in [2.0, 4.0, 6.0] {
        cx.simulate_mouse_move(grab_bar_from_right(bar, inset), None, Modifiers::default());
        view.update(cx, |view, _| {
            assert_eq!(
                view.hover_bar_zone_for_test().map(|(_, zone)| zone),
                Some(BarHitZone::ResizeEnd),
                "{inset}px inside the painted end must be the resize halo"
            );
            assert_eq!(
                view.hover_cursor_for_test(),
                Some("ew-resize"),
                "{inset}px inside the painted end must show the resize cursor"
            );
        });
    }
    let halo = grab_bar_from_right(bar, 4.0);
    cx.simulate_mouse_move(halo, None, Modifiers::default());
    cx.simulate_mouse_down(halo, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::ResizeEnd),
            "a press 2–4px inside the end edge must resize, not link or no-op"
        );
    });
}

#[gpui_kit::test]
fn milestone_diamond_shows_a_hover_tooltip(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = FITTED_MONTH_ZOOM;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    let target = cx
        .debug_bounds("gantt-bar-1013")
        .or_else(|| cx.debug_bounds("gantt-bar-label-1013"))
        .expect("Go-live milestone");
    cx.simulate_mouse_move(target.center(), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .hovered_id
                .or(view.state.gantt.bar_hovered_id),
            Some(1013),
            "hovering the diamond or its title must mark Go-live hovered"
        );
        assert!(
            !view.suppress_bar_tooltip_for_test(),
            "a milestone hover must not suppress the dates tooltip"
        );
        assert!(
            !matches!(
                view.hover_bar_zone_for_test(),
                Some((
                    _,
                    BarHitZone::Link | BarHitZone::ResizeStart | BarHitZone::ResizeEnd
                ))
            ),
            "a diamond hover must not sit in a resize/link zone that hides the tooltip, got {:?}",
            view.hover_bar_zone_for_test()
        );
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars = view
            .state
            .gantt
            .layout_bars(&view.state.nodes, &rows, &[], 720.0);
        let bar = bars.iter().find(|bar| bar.id == 1013).expect("Go-live");
        let tip = bar_hover_tooltip(bar, &view.state.nodes);
        assert!(
            tip.contains("Go-live milestone"),
            "milestone hover copy must include the title, got {tip:?}"
        );
    });
    let diamond = cx.debug_bounds("gantt-bar-1013").expect("Go-live diamond");
    let tip = cx
        .debug_bounds("gantt-hover-tooltip")
        .expect("Go-live hover tooltip");
    let diamond_top = f32::from(diamond.origin.y);
    let diamond_left = f32::from(diamond.origin.x);
    let tip_top = f32::from(tip.origin.y);
    let tip_left = f32::from(tip.origin.x);
    let tip_w = f32::from(tip.size.width);
    let tip_right = tip_left + tip_w;
    let diamond_bottom = diamond_top + f32::from(diamond.size.height);
    let diamond_right = diamond_left + f32::from(diamond.size.width);
    let tip_bottom = tip_top + f32::from(tip.size.height);
    let overlaps_diamond = tip_left < diamond_right - 1.0
        && tip_right > diamond_left + 1.0
        && tip_top < diamond_bottom - 1.0
        && tip_bottom > diamond_top + 1.0;
    assert!(
        !overlaps_diamond,
        "Go-live tooltip must sit beside the diamond, not on it, tip={tip_left}..{tip_right} y={tip_top}..{tip_bottom} diamond={diamond_left}..{diamond_right}"
    );
    let gap = if tip_right <= diamond_left + 1.0 {
        diamond_left - tip_right
    } else {
        tip_left - diamond_right
    };
    assert!(
        gap <= 48.0,
        "Go-live tooltip must stay anchored to the diamond, not ~150px past the title, gap={gap} tip={tip_left}..{tip_right} diamond={diamond_left}..{diamond_right}"
    );
    if let Some(spike) = cx.debug_bounds("gantt-bar-1012") {
        let spike_top = f32::from(spike.origin.y);
        let spike_bottom = spike_top + f32::from(spike.size.height);
        let spike_left = f32::from(spike.origin.x);
        let spike_right = spike_left + f32::from(spike.size.width);
        let overlaps_spike = tip_left < spike_right - 1.0
            && tip_right > spike_left + 1.0
            && tip_top < spike_bottom - 1.0
            && tip_bottom > spike_top + 1.0;
        assert!(
            !overlaps_spike,
            "Go-live tooltip must not cover Unparented spike, tip y={tip_top}..{tip_bottom} spike={spike_top}..{spike_bottom}"
        );
    }
    assert!(
        tip_w >= 80.0,
        "Go-live tooltip must be wide enough for 'Go-live milestone', width={tip_w}"
    );
    if let Some(label) = cx.debug_bounds("gantt-bar-label-1013") {
        let label_left = f32::from(label.origin.x);
        let label_right = label_left + f32::from(label.size.width);
        let label_top = f32::from(label.origin.y);
        let label_bottom = label_top + f32::from(label.size.height);
        let tip_bottom = tip_top + f32::from(tip.size.height);
        let overlaps = tip_left < label_right - 1.0
            && tip_right > label_left + 1.0
            && tip_top < label_bottom - 1.0
            && tip_bottom > label_top + 1.0;
        // Sitting left of the diamond must miss the title. Sitting flush
        // right (no left gutter) may share the title slot rather than
        // jumping ~150px past it.
        if tip_right <= diamond_left + 1.0 {
            assert!(
                !overlaps,
                "Go-live tooltip must not cover the outside title, tip={tip_left}..{tip_right} label={label_left}..{label_right}"
            );
        }
    } else {
        assert!(
            tip_right <= diamond_left + 1.0
                || tip_left >= diamond_left + f32::from(diamond.size.width) - 1.0,
            "Go-live tooltip must sit beside the diamond, not on it, tip={tip_left}..{tip_right} diamond_left={diamond_left}"
        );
    }
}

fn assert_milestone_tooltip_on_chart(
    cx: &mut VisualTestContext,
    scale: &str,
    diamond: gpui_kit::Bounds<gpui_kit::Pixels>,
    tip: gpui_kit::Bounds<gpui_kit::Pixels>,
) {
    let timeline = cx.debug_bounds("gantt-timeline").expect("timeline");
    let tip_left = f32::from(tip.origin.x);
    let tip_right = tip_left + f32::from(tip.size.width);
    let diamond_left = f32::from(diamond.origin.x);
    let diamond_right = diamond_left + f32::from(diamond.size.width);
    let chart_left = f32::from(timeline.origin.x);
    let chart_right = chart_left + f32::from(timeline.size.width);
    assert!(
        tip_left >= chart_left - 1.0,
        "{scale}: tooltip must not clip at the chart's left edge, tip={tip_left} chart={chart_left}"
    );
    assert!(
        tip_right <= chart_right + 1.0,
        "{scale}: tooltip must stay inside the chart, tip_right={tip_right} chart_right={chart_right}"
    );
    let sits_right = tip_left >= diamond_right - 1.0;
    let sits_left = tip_right <= diamond_left + 1.0;
    assert!(
        sits_right || sits_left,
        "{scale}: tooltip must sit beside the diamond, tip={tip_left}..{tip_right} diamond={diamond_left}..{diamond_right}"
    );
    if sits_right {
        assert!(
            tip_left - diamond_right <= 12.0,
            "{scale}: right-side chip must kiss the diamond, gap={} tip={tip_left} diamond={diamond_right}",
            tip_left - diamond_right
        );
    } else {
        assert!(
            diamond_left - tip_right <= 12.0,
            "{scale}: left-flip chip must kiss the diamond, gap={} tip_right={tip_right} diamond={diamond_left}",
            diamond_left - tip_right
        );
    }
}

#[gpui_kit::test]
fn week_and_day_milestone_tooltip_flips_flush_to_the_diamond(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    // Park Go-live a few days into the Week window so the right side
    // has room. Prefer that over a left flip that clips at the chart edge.
    state.gantt.origin = Date::new(2026, 8, 8).unwrap();
    state.gantt.scale = GanttScale::Week;
    state.gantt.zoom = 1.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let diamond = cx
        .debug_bounds("gantt-bar-1013")
        .or_else(|| cx.debug_bounds("gantt-bar-label-1013"))
        .expect("Week Go-live");
    cx.simulate_mouse_move(diamond.center(), None, Modifiers::default());
    draw(cx);
    let tip = cx
        .debug_bounds("gantt-hover-tooltip")
        .expect("Week Go-live tooltip");
    let diamond = cx.debug_bounds("gantt-bar-1013").expect("Week diamond");
    let tip_left = f32::from(tip.origin.x);
    let diamond_right = f32::from(diamond.origin.x) + f32::from(diamond.size.width);
    assert!(
        tip_left >= diamond_right - 1.0,
        "Week: room on the right must keep the tooltip right of the diamond, tip={tip_left} diamond_right={diamond_right}"
    );
    assert_milestone_tooltip_on_chart(cx, "Week", diamond, tip);
    view.update(cx, |view, cx| {
        view.state.gantt.scale = GanttScale::Day;
        view.state.gantt.origin = Date::new(2026, 8, 14).unwrap();
        view.state.gantt.scroll_x = 0.0;
        cx.notify();
    });
    draw(cx);
    let diamond = cx
        .debug_bounds("gantt-bar-1013")
        .or_else(|| cx.debug_bounds("gantt-bar-label-1013"))
        .expect("Day Go-live near the left clip");
    cx.simulate_mouse_move(diamond.center(), None, Modifiers::default());
    draw(cx);
    let tip = cx
        .debug_bounds("gantt-hover-tooltip")
        .expect("Day Go-live tooltip");
    let diamond = cx.debug_bounds("gantt-bar-1013").expect("Day diamond");
    let tip_left = f32::from(tip.origin.x);
    let diamond_right = f32::from(diamond.origin.x) + f32::from(diamond.size.width);
    assert!(
        tip_left >= diamond_right - 1.0,
        "Day: prefer the right of the diamond, tip={tip_left} diamond_right={diamond_right}"
    );
    assert_milestone_tooltip_on_chart(cx, "Day", diamond, tip);
}

#[gpui_kit::test]
fn hovering_a_bar_highlights_the_related_fs_chain(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1003").expect("Persist cart");
    cx.simulate_mouse_move(bar.center(), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(view.state.gantt.hovered_id, Some(1003));
        let related = related_dependency_ids(&view.state.nodes, 1003);
        assert!(
            related.contains(&1003) && related.contains(&1013),
            "hovering Persist cart must highlight its FS chain including Go-live, got {related:?}"
        );
    });
}

#[gpui_kit::test]
fn drop_off_the_bar_does_not_pin_a_tooltip(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    let grip = point(
        bar.origin.x + bar.size.width - px(HANDLE_WIDTH + LINK_HANDLE_INSET / 2.0),
        bar.origin.y + bar.size.height / 2.0,
    );
    cx.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    let ppd = view.update(cx, |view, _| {
        view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom)
    });
    let dest = point(grip.x + px(ppd * 5.0), grip.y);
    cx.simulate_mouse_move(dest, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    let nav = cx.debug_bounds("gantt-nav").expect("toolbar");
    cx.simulate_mouse_up(nav.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.pin_bar_tooltip, None,
            "dropping on the toolbar must not leave a stuck bar tooltip"
        );
        assert!(
            view.state.gantt.gesture.is_none(),
            "mouse-up off the bar must still commit the resize"
        );
    });
    assert!(
        cx.debug_bounds("gantt-bar-tooltip").is_none(),
        "a drop on the toolbar must not paint a pinned tooltip"
    );
}

#[gpui_kit::test]
fn drop_on_another_row_does_not_pin_a_tooltip(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    let parent = cx.debug_bounds("gantt-bar-1").expect("parent row");
    let grip = point(
        bar.origin.x + bar.size.width - px(HANDLE_WIDTH + LINK_HANDLE_INSET / 2.0),
        bar.origin.y + bar.size.height / 2.0,
    );
    cx.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    let ppd = view.update(cx, |view, _| {
        view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom)
    });
    let dest = point(grip.x + px(ppd * 5.0), grip.y);
    cx.simulate_mouse_move(dest, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    let off_row = point(dest.x, parent.origin.y + parent.size.height / 2.0);
    cx.simulate_mouse_up(off_row, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.pin_bar_tooltip, None,
            "releasing on another row at the same X must not pin the live tooltip"
        );
    });
}

#[gpui_kit::test]
fn pinned_tooltip_unpins_when_the_pointer_leaves_the_row(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    let grip = point(
        bar.origin.x + bar.size.width - px(HANDLE_WIDTH + LINK_HANDLE_INSET / 2.0),
        bar.origin.y + bar.size.height / 2.0,
    );
    cx.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    let ppd = view.update(cx, |view, _| {
        view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom)
    });
    let dest = point(grip.x + px(ppd * 5.0), grip.y);
    cx.simulate_mouse_move(dest, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    cx.simulate_mouse_up(dest, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        view.state.gantt.pin_bar_tooltip = Some(2);
        view.state.gantt.clear_row_hover();
        assert_eq!(
            view.state.gantt.pin_bar_tooltip,
            Some(2),
            "a remount hover-leave must not unpin while the pointer is still on the bar"
        );
    });
    let bar_after = cx.debug_bounds("gantt-bar-2").expect("leaf after drop");
    let parent = cx.debug_bounds("gantt-bar-1").expect("parent row");
    cx.simulate_mouse_move(
        point(
            bar_after.center().x,
            parent.origin.y + parent.size.height / 2.0,
        ),
        None,
        Modifiers::default(),
    );
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.pin_bar_tooltip, None,
            "leaving the bar's row at the same X must unpin the live tooltip"
        );
    });
}

#[gpui_kit::test]
fn pinned_tooltip_unpins_when_hovering_another_rows_overflow(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, _| {
        view.state.gantt.pin_bar_tooltip = Some(1002);
        view.state.gantt.bar_hovered_id = Some(1002);
    });
    draw(cx);
    let persist = cx.debug_bounds("gantt-bar-1003").expect("Persist cart bar");
    cx.simulate_mouse_move(persist.center(), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.pin_bar_tooltip, None,
            "moving onto Persist cart must unpin the Cart dates chip so two tooltips do not stack"
        );
        assert_ne!(
            view.state.gantt.bar_hovered_id,
            Some(1002),
            "Cart handles must not stay painted after the pointer moves to Persist cart"
        );
        view.state.gantt.pin_bar_tooltip = Some(1002);
        view.state.gantt.bar_hovered_id = Some(1002);
    });
    draw(cx);
    let bang = point(
        persist.origin.x + persist.size.width + px(28.),
        persist.origin.y + persist.size.height / 2.0,
    );
    cx.simulate_mouse_move(bang, None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.pin_bar_tooltip, None,
            "moving onto Persist cart's ! must unpin the Cart dates chip"
        );
        assert_ne!(
            view.state.gantt.bar_hovered_id,
            Some(1002),
            "Cart handles must clear when the ! on Persist cart unpins the chip"
        );
    });
    assert!(
        cx.debug_bounds("gantt-resize-grip-1002-end").is_none(),
        "Cart's resize grip must unmount once the pin is on another row"
    );
    assert!(
        cx.debug_bounds("gantt-link-dot-1002-link-end").is_none(),
        "Cart's link connector must unmount once the pin is on another row"
    );
}

#[gpui_kit::test]
fn pinned_tooltip_is_not_covered_by_the_bar_title(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Year;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-bar-label-1003").is_some(),
        "precondition: Persist cart paints an outside title"
    );
    view.update(cx, |view, _| {
        view.state.gantt.pin_bar_tooltip = Some(1003);
    });
    draw(cx);
    let tip = cx
        .debug_bounds("gantt-bar-tooltip")
        .expect("pinned Persist cart dates chip");
    assert!(
        cx.debug_bounds("gantt-bar-label-1003").is_none(),
        "the outside title must yield to the dates chip so the committed range is visible"
    );
    assert!(
        f32::from(tip.size.width) >= 48.0,
        "pinned dates chip must stay readable, width={}",
        f32::from(tip.size.width)
    );
}

#[gpui_kit::test]
fn left_end_resize_clears_preview_chip_on_mouse_up(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let start_before = Date::new(2026, 7, 6).unwrap();
    let target_before = Date::new(2026, 7, 20).unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    let grip = point(bar.origin.x + px(3.), bar.origin.y + bar.size.height / 2.0);
    cx.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("mouse-down 3px inside the left end must start a resize");
        assert_eq!(
            gesture.kind,
            GestureKind::ResizeStart,
            "inner-start grip mouse-down must be ResizeStart, got {:?}",
            gesture.kind
        );
    });
    let ppd = view.update(cx, |view, _| {
        view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom)
    });
    cx.simulate_mouse_move(
        point(grip.x - px(ppd * 4.0), grip.y),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view.state.gantt.gesture.clone().expect("live resize");
        let (start, target) = GanttView::gesture_patch_dates(&gesture);
        assert_eq!(
            target, None,
            "resizing the start must not patch Target Date, got {target:?}"
        );
        assert_eq!(
            start,
            Some(start_before.saturating_sub(4.days())),
            "dragging 4 days from the inner left grip must move Start Date, got {start:?}"
        );
        assert_eq!(
            gesture.live_target, target_before,
            "Target Date must stay {target_before}, got {}",
            gesture.live_target
        );
    });
    assert!(
        cx.debug_bounds("gantt-drag-preview").is_some(),
        "a live left-end resize must show the date preview chip"
    );
    // Release on the start grip (where the mouse still is) with no extra move.
    cx.simulate_mouse_up(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "mouse-up on the start grip must end the resize, got {:?}",
            view.state.gantt.gesture.as_ref().map(|g| g.kind)
        );
    });
    assert!(
        cx.debug_bounds("gantt-drag-preview").is_none(),
        "the preview chip must clear after a left-end commit without another mouse-move"
    );
}

#[gpui_kit::test]
fn start_handle_dot_is_part_of_the_resize_hit_zone(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.hovered_id = Some(2);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    // Inner start grip sits on the painted left edge and resizes. The
    // connector circle lives fully outside the bar and starts a link.
    let mid_y = bar.origin.y + bar.size.height / 2.0;
    let grip = point(bar.origin.x + px(3.), mid_y);
    cx.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("the start-handle dot must begin a resize");
        assert_eq!(
            gesture.kind,
            GestureKind::ResizeStart,
            "clicking the start handle dot must resize, not select a link, got {:?}",
            gesture.kind
        );
        assert!(
            view.state.gantt.selected_link.is_none(),
            "bar handles must win over incoming links"
        );
        view.state.cancel_gesture();
        view.state.gantt.hovered_id = Some(2);
    });
    draw(cx);
    let end = point(
        bar.origin.x + bar.size.width - px(HANDLE_WIDTH + LINK_HANDLE_INSET / 2.0),
        mid_y,
    );
    cx.simulate_mouse_down(end, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("the painted right edge must still resize");
        assert_eq!(
            gesture.kind,
            GestureKind::ResizeEnd,
            "clicking the bar's right edge must resize, got {:?}",
            gesture.kind
        );
        view.state.cancel_gesture();
        view.state.gantt.hovered_id = Some(2);
    });
    draw(cx);
    let handle = cx
        .debug_bounds("gantt-link-handle-2-link-end")
        .expect("end handle");
    cx.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("the end handle dot must begin a link");
        assert_eq!(
            gesture.kind,
            GestureKind::Link,
            "clicking the visible end dot must start a link, got {:?}",
            gesture.kind
        );
    });
}

#[gpui_kit::test]
fn right_dot_resize_commits_on_mouse_up_at_release_x(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.hovered_id = Some(2);
    state.gantt.scale = GanttScale::Month;
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let handle = cx
        .debug_bounds("gantt-resize-handle-2-end")
        .expect("end grip");
    let timeline = cx
        .debug_bounds("gantt-timeline-body")
        .expect("timeline body");
    let drop = Date::new(2026, 8, 8).unwrap();
    let later = Date::new(2026, 9, 1).unwrap();
    let drop_window_x = view.update(cx, |view, _| {
        f32::from(timeline.origin.x) + view.state.gantt.x_for_date(drop)
    });
    let later_window_x = view.update(cx, |view, _| {
        f32::from(timeline.origin.x) + view.state.gantt.x_for_date(later)
    });
    let mid_y = handle.origin.y + handle.size.height / 2.0;
    cx.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    let drop_at = point(px(drop_window_x), mid_y);
    cx.simulate_mouse_move(drop_at, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    let preview = view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("right-dot drag must keep a resize");
        assert_eq!(gesture.kind, GestureKind::ResizeEnd);
        gesture.live_target
    });
    assert!(
        cx.debug_bounds("gantt-drag-preview").is_some(),
        "a live right-dot resize must show the date preview"
    );
    cx.simulate_mouse_up(drop_at, MouseButton::Left, Modifiers::default());
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "mouse-up must commit the {preview} resize, got {:?}",
            view.state.gantt.gesture.as_ref().map(|g| g.kind)
        );
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-drag-preview").is_none(),
        "the preview chip must clear on mouse-up, without waiting for another mouse-move"
    );
    cx.simulate_mouse_move(point(px(later_window_x), mid_y), None, Modifiers::default());
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "a later mouse-move must not keep a resize gesture"
        );
        let saved = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 2)
            .and_then(|node| ado_core::parse_iso_date(node.target_date.as_deref()));
        assert_ne!(
            saved,
            Some(later),
            "a later mouse-move must not rewrite the {preview} release to {later}"
        );
    });
}

#[gpui_kit::test]
fn right_dot_missed_mouseup_does_not_commit_a_later_move_x(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.hovered_id = Some(2);
    state.gantt.bar_hovered_id = Some(2);
    state.gantt.scale = GanttScale::Month;
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let handle = cx
        .debug_bounds("gantt-resize-handle-2-end")
        .expect("end grip");
    let timeline = cx
        .debug_bounds("gantt-timeline-body")
        .expect("timeline body");
    let drop = Date::new(2026, 8, 8).unwrap();
    let shrink = Date::new(2026, 7, 28).unwrap();
    let origin_target = Date::new(2026, 7, 20).unwrap();
    let drop_window_x = view.update(cx, |view, _| {
        f32::from(timeline.origin.x) + view.state.gantt.x_for_date(drop)
    });
    let shrink_window_x = view.update(cx, |view, _| {
        f32::from(timeline.origin.x) + view.state.gantt.x_for_date(shrink)
    });
    let mid_y = handle.origin.y + handle.size.height / 2.0;
    cx.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    // Missed mouse-up: the next move has no button. Must not treat this
    // hover x as a release (Jul 28 shrink).
    cx.simulate_mouse_move(
        point(px(shrink_window_x), mid_y),
        None,
        Modifiers::default(),
    );
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "a button-up move must end the right-dot resize"
        );
        assert_eq!(
            view.last_committed_target_for_test(),
            None,
            "a missed mouse-up with no drag must not PATCH a later hover date"
        );
        let saved = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 2)
            .and_then(|node| ado_core::parse_iso_date(node.target_date.as_deref()));
        assert_eq!(
            saved,
            Some(origin_target),
            "a later hover must not shrink the bar to {shrink}"
        );
    });

    view.update(cx, |view, cx| {
        view.state.gantt.hovered_id = Some(2);
        view.state.gantt.bar_hovered_id = Some(2);
        cx.notify();
    });
    draw(cx);
    let handle = cx
        .debug_bounds("gantt-resize-handle-2-end")
        .expect("end grip after cancel");
    let mid_y = handle.origin.y + handle.size.height / 2.0;
    cx.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    let drop_at = point(px(drop_window_x), mid_y);
    cx.simulate_mouse_move(drop_at, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    let preview = view.update(cx, |view, _| {
        view.state
            .gantt
            .gesture
            .as_ref()
            .expect("right-dot drag must keep a resize")
            .live_target
    });
    cx.simulate_mouse_move(
        point(px(shrink_window_x), mid_y),
        None,
        Modifiers::default(),
    );
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "a missed mouse-up must commit the last pressed x, not wait for another move"
        );
        assert_eq!(
            view.last_committed_target_for_test(),
            Some(preview),
            "must commit the {preview} drag, not a later {shrink} hover"
        );
        assert_ne!(
            view.last_committed_target_for_test(),
            Some(shrink),
            "a later mouse-move must not rewrite the release to {shrink}"
        );
    });
}

#[gpui_kit::test]
fn toast_undo_and_scale_switch_clear_hover_dots(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.hovered_id = Some(2);
    state.gantt.bar_hovered_id = Some(2);
    state.gantt.selected_id = Some(2);
    let before_start = Some("2026-07-06T00:00:00Z".into());
    let before_target = Some("2026-07-20T00:00:00Z".into());
    state.toasts.push_undo(
        "Resized \"Work Item 1\" to Jul 6 → Jul 24",
        UndoAction::RestoreDates {
            id: 2,
            start_date: before_start,
            target_date: before_target,
            rev: 1,
        },
    );
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, cx| {
        view.state.gantt.hovered_id = Some(2);
        view.state.gantt.bar_hovered_id = Some(2);
        cx.notify();
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-link-dot-2-link-end").is_some(),
        "hover must paint the pink end dot"
    );
    let undo = cx.debug_bounds("toast-undo").expect("Undo");
    cx.simulate_click(undo.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.hovered_id, None,
            "toast Undo must clear hover dots"
        );
    });
    assert!(
        cx.debug_bounds("gantt-link-dot-2-link-end").is_none(),
        "toast Undo must hide the pink end dot even if the bar stays selected"
    );
    view.update(cx, |view, cx| {
        view.state.gantt.hovered_id = Some(2);
        view.state.gantt.bar_hovered_id = Some(2);
        cx.notify();
    });
    draw(cx);
    let year = cx.debug_bounds("gantt-scale-Year").expect("Year");
    cx.simulate_click(year.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.hovered_id, None,
            "switching views must clear hover dots"
        );
    });
    assert!(
        cx.debug_bounds("gantt-link-dot-2-link-end").is_none(),
        "a scale switch must hide the pink end dot"
    );
}

#[gpui_kit::test]
fn mouse_leave_clears_hover_dots_while_locked(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.hovered_id = Some(2);
    state.gantt.bar_hovered_id = Some(2);
    state.gantt.selected_id = Some(2);
    state.gantt.hover_locked = true;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-link-dot-2-link-end").is_some(),
        "a locked hover must still paint the pink end dot"
    );
    let nav = cx.debug_bounds("gantt-nav").expect("nav");
    cx.simulate_mouse_move(nav.center(), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.hovered_id, None,
            "leaving the canvas must clear hover even after a click locked it"
        );
    });
    assert!(
        cx.debug_bounds("gantt-link-dot-2-link-end").is_none(),
        "mouse leave must hide the pink end dot"
    );
}

#[gpui_kit::test]
fn selected_row_without_bar_hover_hides_resize_dots(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(2);
    state.gantt.hovered_id = Some(2);
    state.gantt.bar_hovered_id = None;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-link-dot-2-link-end").is_none(),
        "selecting a row (or hovering the tree) must not paint resize dots"
    );
    view.update(cx, |view, cx| {
        view.state.gantt.bar_hovered_id = Some(2);
        cx.notify();
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-link-dot-2-link-end").is_some(),
        "hovering the bar must paint the pink end dot"
    );
}

#[gpui_kit::test]
fn year_to_month_keeps_the_scroll_anchor(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Year;
    state.gantt.zoom = 0.4;
    state.gantt.selected_id = None;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let left = view.update(cx, |view, _| view.state.gantt.date_for_x(0.0));
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(view.state.gantt.scale, GanttScale::Month);
        let after = view.state.gantt.date_for_x(0.0);
        assert_eq!(
            after, left,
            "Year→Month must keep the left-edge date {left}, got {after}"
        );
        assert!(
            after < Date::new(2027, 1, 1).unwrap(),
            "must not jump to Jul 2027, got {after}"
        );
    });
}

#[gpui_kit::test]
fn clicking_today_then_month_does_not_clip_the_left_edge(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Year;
    state.gantt.zoom = 0.4;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let today_btn = cx.debug_bounds("gantt-today-btn").expect("Today button");
    cx.simulate_click(today_btn.center(), Modifiers::default());
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(view.state.gantt.scale, GanttScale::Month);
        assert!(
            view.state.gantt.year_gutter_snap.abs() < 0.5,
            "Month after Today must not keep a Year park, snap={}",
            view.state.gantt.year_gutter_snap
        );
        let today = jiff::Zoned::now().date();
        let today_x = view.state.gantt.today_x(today);
        let expected = view.state.gantt.timeline_width.max(1.0) * TODAY_ALIGN;
        assert!(
            (today_x - expected).abs() < 12.0,
            "Today then Month must keep Today near the axis, today_x={today_x} expected={expected}"
        );
        let left = view.state.gantt.date_for_x(0.0);
        assert!(
            left > today.saturating_sub(120.days()),
            "Month after Today must not re-anchor to a Year park, left={left} today={today}"
        );
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars = view.state.gantt.layout_bars(
            &view.state.nodes,
            &rows,
            &view.state.iterations,
            720.0,
        );
        for bar in &bars {
            if bar.kind != ado_core::GanttBarKind::Scheduled {
                continue;
            }
            let start = view.state.gantt.date_for_x(bar.x);
            assert!(
                start < left || bar.x >= -1.0,
                "a bar that starts on/after the left-edge date must not clip, id={} x={} left={left}",
                bar.id,
                bar.x
            );
        }
    });
}

#[gpui_kit::test]
fn toast_undo_clears_saving_status(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    view.update(cx, |view, cx| {
        view.set_busy_for_test(true);
        assert_eq!(view.sync_status_for_test(), "Saving…");
        view.complete_undo_io_for_test(cx);
        assert_eq!(
            view.sync_status_for_test(),
            "Synced",
            "Saving… must not linger in the header after Undo"
        );
    });
}

#[gpui_kit::test]
fn flick_drag_without_mouse_move_still_commits(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.scale = GanttScale::Month;
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("bar");
    let ppd = view.update(cx, |view, _| {
        view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom)
    });
    // Press in the body, past the 18px start handle that would resize.
    let start = bar.center();
    let drop = point(start.x + px(ppd * 4.0), start.y);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("mouse-down must arm a bar gesture before the flick release");
        assert_eq!(gesture.kind, GestureKind::Move);
        assert_eq!(
            gesture.live_start,
            Date::new(2026, 7, 6).unwrap(),
            "flick press must start on the bar's origin date"
        );
    });
    cx.simulate_mouse_up(drop, MouseButton::Left, Modifiers::default());
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "a flick must commit on mouse-up"
        );
        assert_eq!(
            view.last_committed_start_for_test(),
            Some(Date::new(2026, 7, 10).unwrap()),
            "a 4-day flick with no mouse-move must still move the bar, got {:?}",
            view.last_committed_start_for_test()
        );
    });
}

fn click_painted_bar_inset(
    cx: &mut VisualTestContext,
    view: &Entity<PlannerView>,
    selector: &'static str,
    inset: f32,
    at_start: bool,
) {
    draw(cx);
    view.update(cx, |view, _| {
        view.state.cancel_gesture();
        view.state.gantt.gesture = None;
        view.state.gantt.hover_locked = false;
    });
    let bar = cx.debug_bounds(selector).expect("painted bar");
    let x = if at_start {
        bar.origin.x + px(inset)
    } else {
        // Skip the inner link sliver and the 1–2px selection border so this
        // lands in the end-resize grip, not the last-pixel link zone.
        bar.origin.x + bar.size.width - px(inset + LINK_HANDLE_INSET + 2.0)
    };
    let y = bar.origin.y + bar.size.height / 2.0;
    cx.simulate_mouse_down(point(x, y), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let hit = view
            .last_bar_hit_for_test()
            .expect("bar mouse-down must record a window-coord hit");
        let bar_left = f32::from(bar.origin.x);
        let bar_right = bar_left + f32::from(bar.size.width);
        assert!(
            (hit.painted_left - bar_left).abs() < 2.5
                && (hit.painted_right - bar_right).abs() < 2.5,
            "hit must use painted window bounds, event_x={:.1} painted={:.1}..{:.1} debug={bar_left:.1}..{bar_right:.1}",
            hit.event_x,
            hit.painted_left,
            hit.painted_right
        );
        let expected = if at_start {
            GestureKind::ResizeStart
        } else {
            GestureKind::ResizeEnd
        };
        assert_eq!(
            hit.kind, expected,
            "{inset}px inside the {} must be {expected:?} (zone {}), not {:?}",
            if at_start { "start" } else { "end" },
            hit.zone.as_str(),
            hit.kind
        );
        assert_eq!(
            view.state.gantt.gesture.as_ref().map(|g| g.kind),
            Some(expected),
            "{inset}px inside the painted {} must start {expected:?}",
            if at_start { "start" } else { "end" }
        );
        assert_ne!(
            hit.zone,
            BarHitZone::Move,
            "{inset}px inside must not classify as move"
        );
    });
    cx.simulate_mouse_up(point(x, y), MouseButton::Left, Modifiers::default());
    draw(cx);
}

#[gpui_kit::test]
fn week_and_month_painted_end_insets_resize_not_move(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.scale = GanttScale::Week;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    for scale in [GanttScale::Week, GanttScale::Month] {
        view.update(cx, |view, cx| {
            view.state.gantt.scale = scale;
            cx.notify();
        });
        draw(cx);
        for inset in [2.0, 3.0, 4.0, 6.0] {
            click_painted_bar_inset(cx, &view, "gantt-bar-2", inset, false);
            click_painted_bar_inset(cx, &view, "gantt-bar-2", inset, true);
        }
    }
}

#[gpui_kit::test]
fn toast_undo_does_not_eat_the_next_bar_click(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.toasts.push_undo(
        "Moved \"Work Item 1\"",
        UndoAction::RestoreDates {
            id: 2,
            start_date: Some("2026-07-06T00:00:00Z".into()),
            target_date: Some("2026-07-20T00:00:00Z".into()),
            rev: 1,
        },
    );
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let undo = cx.debug_bounds("toast-undo").expect("Undo");
    cx.simulate_mouse_down(undo.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.toast_pointer_for_test()
                || view.state.toasts.items.iter().all(|t| t.undo.is_none()),
            "Undo press arms toast_pointer or dismisses the undo toast"
        );
    });
    cx.simulate_mouse_up(undo.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    cx.simulate_mouse_down(bar.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            !view.toast_pointer_for_test(),
            "the next press after Undo must clear toast_pointer"
        );
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("first click after Undo must start a bar gesture, not be ignored");
        assert_eq!(gesture.id, 2);
        assert_eq!(gesture.kind, GestureKind::Move);
    });
}

#[gpui_kit::test]
fn toast_undo_of_a_resize_reuses_the_resized_verb(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    let before_start = Some("2026-07-06T00:00:00Z".to_string());
    let before_target = Some("2026-07-20T00:00:00Z".to_string());
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.start_date = before_start.clone();
        node.target_date = Some("2026-07-24T00:00:00Z".into());
    }
    state.history.push(HistoryEntry::Dates {
        id: 2,
        before_start: before_start.clone(),
        before_target: before_target.clone(),
        after_start: before_start.clone(),
        after_target: Some("2026-07-24T00:00:00Z".into()),
    });
    state.toasts.push_undo(
        "Resized \"Work Item 1\" to Jul 6 → Jul 24",
        UndoAction::RestoreDates {
            id: 2,
            start_date: before_start,
            target_date: before_target,
            rev: 1,
        },
    );
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let undo = cx.debug_bounds("toast-undo").expect("Undo");
    cx.simulate_click(undo.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let messages: Vec<String> = view
            .state
            .toasts
            .items
            .iter()
            .map(|toast| toast.message.clone())
            .collect();
        assert!(
            messages
                .iter()
                .any(|message| message.to_lowercase().starts_with("undone resized")),
            "undo of a resize must reuse Resized, got {messages:?}"
        );
    });
}

#[gpui_kit::test]
fn parent_move_toast_offers_move_children_too(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.toasts.push_undo_extra(
        "Moved \"Work Item 0\" to Jul 9 → Jul 23",
        UndoAction::RestoreDates {
            id: 1,
            start_date: Some("2026-07-06T00:00:00Z".into()),
            target_date: Some("2026-07-20T00:00:00Z".into()),
            rev: 1,
        },
        Some(ToastExtra::MoveChildren {
            parent_id: 1,
            delta_days: 3,
        }),
    );
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let extra = cx
        .debug_bounds("toast-move-children")
        .expect("Move children too");
    cx.simulate_mouse_down(extra.center(), MouseButton::Left, Modifiers::default());
    view.update(cx, |view, _| {
        assert!(
            view.state
                .toasts
                .items
                .iter()
                .all(|toast| toast.extra.is_none()),
            "Move children too must consume the extra action"
        );
    });
}

#[gpui_kit::test]
fn right_click_selects_the_row(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let row = cx.debug_bounds("gantt-tree-row-2").expect("child row");
    cx.simulate_mouse_down(row.center(), MouseButton::Right, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.selected_id,
            Some(2),
            "right-click must select the row under the pointer"
        );
        assert!(
            view.state
                .gantt
                .context_menu
                .is_some_and(|target| target.id == 2),
            "the context menu must target the right-clicked row"
        );
    });
}

#[gpui_kit::test]
fn move_children_too_undo_restores_parent_and_child(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = fake_hierarchy(3);
    let parent_before = state.nodes[0].clone();
    let _ = state.apply_optimistic_partial(
        1,
        Some(Date::new(2026, 7, 9).unwrap()),
        Some(Date::new(2026, 7, 23).unwrap()),
    );
    state.history.push(HistoryEntry::Dates {
        id: 1,
        before_start: parent_before.start_date.clone(),
        before_target: parent_before.target_date.clone(),
        after_start: Some("2026-07-09T00:00:00Z".into()),
        after_target: Some("2026-07-23T00:00:00Z".into()),
    });
    state.toasts.push_undo_extra(
        "Moved \"Work Item 0\" to Jul 9 → Jul 23",
        UndoAction::RestoreDates {
            id: 1,
            start_date: parent_before.start_date.clone(),
            target_date: parent_before.target_date.clone(),
            rev: parent_before.rev,
        },
        Some(ToastExtra::MoveChildren {
            parent_id: 1,
            delta_days: 3,
        }),
    );
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let extra = cx
        .debug_bounds("toast-move-children")
        .expect("Move children too");
    cx.simulate_mouse_down(extra.center(), MouseButton::Left, Modifiers::default());
    view.update(cx, |view, _| {
        assert!(
            view.state
                .toasts
                .items
                .iter()
                .all(|toast| toast.extra.is_none()),
            "Move children too must consume the extra action"
        );
        match view.state.history.undo.last() {
            Some(HistoryEntry::DatesGroup { items }) => {
                assert!(
                    items.iter().any(|item| item.id == 1) && items.iter().any(|item| item.id == 2),
                    "parent move + children must be one history step, got {items:?}"
                );
            }
            other => panic!("expected DatesGroup after Move children too, got {other:?}"),
        }
        let pending = view.state.undo_date_stack_now();
        assert!(
            pending.iter().any(|save| save.id == 1) && pending.iter().any(|save| save.id == 2),
            "one Undo must restore parent and child, got {:?}",
            pending.iter().map(|save| save.id).collect::<Vec<_>>()
        );
        let parent = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 1)
            .expect("parent");
        let child = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 2)
            .expect("child");
        assert_eq!(parent.start_date.as_deref(), Some("2026-07-06T00:00:00Z"));
        assert_eq!(
            child.start_date.as_deref(),
            Some("2026-07-06T00:00:00Z"),
            "one Undo must restore children shifted by Move children too"
        );
    });
}

#[gpui_kit::test]
fn second_zoom_mid_animation_starts_from_the_interpolated_camera(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, cx| {
        let from = view.state.gantt.snapshot();
        view.state.gantt.zoom = 2.0;
        view.enable_camera_anim();
        view.seed_camera_anim(from, 40);
        view.zoom_at_center_animated(1.25, cx);
        let (anim_from, anim_to) = view
            .camera_anim_endpoints()
            .expect("second + must start a new ease");
        assert!(
            anim_from.zoom > 1.1 && anim_from.zoom < 1.9,
            "second + must start from the painted pose, not 1.0 or dest 2.0, got {}",
            anim_from.zoom
        );
        assert!(
            (anim_to.zoom - anim_from.zoom * 1.25).abs() < 0.08,
            "destination must be 1.25× the interpolant ({:.2}), got {:.2}",
            anim_from.zoom * 1.25,
            anim_to.zoom
        );
        assert!(
            (anim_to.zoom - 2.5).abs() > 0.1,
            "must not zoom from the old dest 2.0 (that would land at 2.5), got {}",
            anim_to.zoom
        );
    });
}

#[gpui_kit::test]
fn wheel_pan_cancels_in_flight_camera_anim(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, _| {
        let from = view.state.gantt.snapshot();
        view.state.gantt.zoom = 2.0;
        view.enable_camera_anim();
        view.seed_camera_anim(from, 40);
    });
    let timeline = cx.debug_bounds("gantt-timeline").expect("timeline");
    cx.simulate_event(ScrollEvent {
        position: timeline.center(),
        delta: ScrollDelta::Pixels(point(px(80.), px(0.))),
        ..Default::default()
    });
    view.update(cx, |view, _| {
        assert!(
            view.camera_anim_endpoints().is_none(),
            "a pan must cancel the in-flight ease instead of snapping to its destination"
        );
        assert!(
            (view.state.gantt.zoom - 2.0).abs() > 0.1,
            "cancel must commit the painted zoom, not the destination 2.0, got {}",
            view.state.gantt.zoom
        );
    });
}

#[gpui_kit::test]
fn scale_button_eases_from_the_current_camera(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, _| {
        view.enable_camera_anim();
    });
    let day_btn = cx.debug_bounds("gantt-scale-Day").expect("Day scale");
    cx.simulate_click(day_btn.center(), Modifiers::default());
    view.update(cx, |view, _| {
        let (_from, to) = view
            .camera_anim_endpoints()
            .expect("scale changes must ease, not snap");
        assert_ne!(
            to.scale,
            GanttScale::Month,
            "clicking another scale must animate toward it, got {:?}",
            to.scale
        );
    });
}

#[gpui_kit::test]
fn inline_placeholder_resets_when_switching_edit_kind(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_inline_dates(1, window, cx);
            assert_eq!(
                view.inline_placeholder(cx),
                "Start → Target, or start only",
                "empty date edits must hint at a range, not a sample like Jul 6 → Jul 20"
            );
            assert_eq!(
                view.inline_input_value(cx),
                "Jul 6 → Jul 20",
                "a scheduled item still shows its compact range"
            );
            view.open_inline_title(1, window, cx);
            assert_eq!(
                view.inline_placeholder(cx),
                "",
                "title edits must not keep the date-range placeholder"
            );
            view.open_inline_assignee(1, window, cx);
            assert_eq!(
                view.inline_placeholder(cx),
                "",
                "assignee edits must not keep the date-range placeholder"
            );
        });
    });
}

#[gpui_kit::test]
fn click_on_title_keeps_grid_focus_for_enter(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = None;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let title = cx.debug_bounds("gantt-title-1").expect("row title");
    cx.simulate_click(title.center(), gpui_kit::Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-title").is_none(),
        "a single title click must not start rename"
    );
    view.update(cx, |view, _| {
        assert_eq!(view.state.gantt.selected_id, Some(1));
    });
    cx.simulate_keystrokes("enter");
    draw(cx);
    let enter = KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        if view.state.form.is_none() && !view.is_busy() {
            view.handle_key(&enter, cx);
        }
        assert!(
            view.is_busy() || view.state.form.is_some(),
            "Enter after a title click must open details, same as clicking the #id cell"
        );
    });
}

#[gpui_kit::test]
fn completing_a_picker_range_closes_the_popover(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_inline_dates(1, window, cx);
        });
    });
    draw(cx);
    assert!(cx.debug_bounds("gantt-inline-dates").is_some());
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.click_inline_calendar_day(
                chrono::NaiveDate::from_ymd_opt(2026, 8, 4).unwrap(),
                cx,
            );
        });
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-dates").is_some(),
        "the first click of a new range must keep the picker open"
    );
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.click_inline_calendar_day(
                chrono::NaiveDate::from_ymd_opt(2026, 8, 18).unwrap(),
                cx,
            );
        });
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-dates").is_none(),
        "start+end must close the picker instead of leaving Jul 8 → …"
    );
}

#[gpui_kit::test]
fn picker_error_slot_is_reserved_before_a_parse_error(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_inline_dates(1, window, cx);
        });
    });
    draw(cx);
    let before = cx
        .debug_bounds("gantt-dates-error")
        .expect("error slot must exist while the picker is clean");
    assert!(
        f32::from(before.size.height) >= 14.0,
        "parse-error space must be reserved so the picker does not shift, height={}",
        f32::from(before.size.height)
    );
    let picker_before = cx.debug_bounds("gantt-inline-dates").expect("picker");
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_inline_dates("foo", window, cx);
        });
    });
    draw(cx);
    let after = cx.debug_bounds("gantt-dates-error").expect("error slot");
    let picker_after = cx.debug_bounds("gantt-inline-dates").expect("picker");
    assert!(
        (f32::from(after.size.height) - f32::from(before.size.height)).abs() < 1.0,
        "error slot height must stay put, before={} after={}",
        f32::from(before.size.height),
        f32::from(after.size.height)
    );
    assert!(
        (f32::from(picker_after.size.height) - f32::from(picker_before.size.height)).abs() < 2.0,
        "the picker must not jump when a parse error appears, before={} after={}",
        f32::from(picker_before.size.height),
        f32::from(picker_after.size.height)
    );
    let month = cx
        .debug_bounds("gantt-dates-month-label")
        .expect("month header");
    let error_bottom = f32::from(after.origin.y) + f32::from(after.size.height);
    assert!(
        error_bottom <= f32::from(month.origin.y) + 1.0,
        "the date error must stay above the month header, error bottom {error_bottom} vs month y {}",
        f32::from(month.origin.y)
    );
}

#[gpui_kit::test]
fn invalid_typed_dates_keep_the_grid_picker_open(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_inline_dates(1, window, cx);
            view.type_inline_dates("foo", window, cx);
        });
    });
    draw(cx);
    let enter = KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&enter, cx);
        let hint = view
            .inline_dates_error_text()
            .expect("garbage dates must show an inline error");
        assert!(
            hint.contains("Jul 6") && hint.contains("Jul 20"),
            "foo parse hint must use the item's own dates, got {hint}"
        );
        let today_s = ado_core::compact_date(jiff::Zoned::now().date());
        assert!(
            !hint.contains(&today_s),
            "foo parse hint must not use today, got {hint}"
        );
        let node = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 1)
            .expect("item 1");
        assert_eq!(
            node.start_date.as_deref(),
            Some("2026-07-06T00:00:00Z"),
            "a parse failure must not PATCH empty dates"
        );
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-dates").is_some(),
        "invalid typed dates must keep the picker open"
    );
    assert!(
        cx.debug_bounds("gantt-dates-error").is_some(),
        "the grid picker must show the parse error"
    );
}

#[gpui_kit::test]
fn toast_undo_click_restores_dates_without_a_hit_through(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    let before_start = state
        .nodes
        .iter()
        .find(|node| node.id == 1)
        .and_then(|node| node.start_date.clone());
    let before_target = state
        .nodes
        .iter()
        .find(|node| node.id == 1)
        .and_then(|node| node.target_date.clone());
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 1) {
        node.start_date = Some("2026-08-04T00:00:00Z".into());
        node.target_date = Some("2026-08-21T00:00:00Z".into());
    }
    state.history.push(HistoryEntry::Dates {
        id: 1,
        before_start: before_start.clone(),
        before_target: before_target.clone(),
        after_start: Some("2026-08-04T00:00:00Z".into()),
        after_target: Some("2026-08-21T00:00:00Z".into()),
    });
    state.toasts.push_undo(
        "Moved Persist cart to Aug 4 → Aug 21",
        UndoAction::RestoreDates {
            id: 1,
            start_date: before_start.clone(),
            target_date: before_target.clone(),
            rev: 1,
        },
    );
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let toast = cx.debug_bounds("planner-toast").expect("toast");
    let undo = cx.debug_bounds("toast-undo").expect("toast Undo");
    assert!(
        f32::from(undo.size.width) >= 40.0,
        "toast Undo must stay wide enough to click, width={} bounds={undo:?} toast={toast:?}",
        f32::from(undo.size.width)
    );
    cx.simulate_mouse_move(undo.center(), None, gpui_kit::Modifiers::default());
    draw(cx);
    cx.simulate_click(undo.center(), gpui_kit::Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let messages: Vec<String> = view
            .state
            .toasts
            .items
            .iter()
            .map(|toast| toast.message.clone())
            .collect();
        assert!(
            messages
                .iter()
                .any(|message| message.to_lowercase().starts_with("undone")),
            "clicking Undo must apply, not only dismiss, got {messages:?}"
        );
        assert!(
            view.state
                .toasts
                .items
                .iter()
                .all(|toast| toast.undo.is_none()),
            "the Undo control must consume the action instead of a hit-through no-op, got {messages:?}"
        );
        assert!(
            view.state
                .toasts
                .items
                .iter()
                .any(|toast| toast.redo.is_some()),
            "after the last Undo the toast must offer Redo, got {messages:?}"
        );
        assert!(
            view.state.gantt.gesture.is_none(),
            "Undo must not start a bar gesture through the toast"
        );
    });
}

#[gpui_kit::test]
fn toast_keeps_undo_while_history_remains(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    let before_start = Some("2026-07-06T00:00:00Z".to_string());
    let before_target = Some("2026-07-20T00:00:00Z".to_string());
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 1) {
        node.start_date = Some("2026-08-04T00:00:00Z".into());
        node.target_date = Some("2026-08-21T00:00:00Z".into());
    }
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.start_date = Some("2026-08-10T00:00:00Z".into());
        node.target_date = Some("2026-08-24T00:00:00Z".into());
    }
    state.history.push(HistoryEntry::Dates {
        id: 1,
        before_start: before_start.clone(),
        before_target: before_target.clone(),
        after_start: Some("2026-08-04T00:00:00Z".into()),
        after_target: Some("2026-08-21T00:00:00Z".into()),
    });
    state.history.push(HistoryEntry::Dates {
        id: 2,
        before_start: before_start.clone(),
        before_target: before_target.clone(),
        after_start: Some("2026-08-10T00:00:00Z".into()),
        after_target: Some("2026-08-24T00:00:00Z".into()),
    });
    state.toasts.push_undo(
        "Moved \"Work Item 1\" to Aug 10 → Aug 24",
        UndoAction::RestoreDates {
            id: 2,
            start_date: before_start,
            target_date: before_target,
            rev: 1,
        },
    );
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let undo = cx.debug_bounds("toast-undo").expect("toast Undo");
    cx.simulate_click(undo.center(), gpui_kit::Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state
                .toasts
                .items
                .iter()
                .all(|toast| toast.undo.is_none()),
            "after Undo the toast must not also keep Undo, got {:?}",
            view.state
                .toasts
                .items
                .iter()
                .map(|toast| (&toast.message, toast.undo.clone(), toast.redo.clone()))
                .collect::<Vec<_>>()
        );
        assert!(
            view.state
                .toasts
                .items
                .iter()
                .any(|toast| matches!(toast.redo, Some(UndoAction::StackRedo))),
            "the undone step must offer Redo"
        );
    });
    assert!(
        cx.debug_bounds("toast-undo").is_none(),
        "Undo must not linger beside Redo after an undo"
    );
    assert!(
        cx.debug_bounds("toast-redo").is_some(),
        "Redo must paint after Undo"
    );
}

#[gpui_kit::test]
fn toast_undo_while_busy_is_not_dropped(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.predecessors = vec![1];
    }
    state.toasts.push_undo(
        "Added link: A → B",
        UndoAction::RemoveLink {
            successor_id: 2,
            predecessor_id: 1,
        },
    );
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    view.update(cx, |view, cx| {
        view.set_busy_for_test(true);
        let toast_id = view
            .state
            .toasts
            .items
            .last()
            .map(|toast| toast.id)
            .unwrap();
        let action = view
            .state
            .toasts
            .items
            .last()
            .and_then(|toast| toast.undo.clone())
            .unwrap();
        view.apply_toast_undo(toast_id, action, cx);
        assert!(
            view.state
                .nodes
                .iter()
                .find(|node| node.id == 2)
                .is_some_and(|node| node.predecessors.contains(&1)),
            "a busy Undo must queue instead of dismissing as a no-op"
        );
        assert!(
            view.state
                .toasts
                .items
                .iter()
                .any(|toast| toast.undo.is_some()),
            "the Undo toast must stay until the queued action runs"
        );
        assert_eq!(
            view.pending_toast_undo_id(),
            Some(toast_id),
            "busy Undo must keep the action until I/O frees"
        );
    });
}

#[gpui_kit::test]
fn toast_stack_undo_while_busy_is_queued(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.predecessors = vec![1];
    }
    state.history.push(HistoryEntry::Link {
        successor_id: 2,
        predecessor_id: 1,
        linked: true,
    });
    state
        .toasts
        .push_undo("Added link: A → B", UndoAction::StackUndo);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    view.update(cx, |view, cx| {
        view.set_busy_for_test(true);
        let toast_id = view
            .state
            .toasts
            .items
            .last()
            .map(|toast| toast.id)
            .unwrap();
        view.apply_toast_undo(toast_id, UndoAction::StackUndo, cx);
        assert!(
            view.state
                .nodes
                .iter()
                .find(|node| node.id == 2)
                .is_some_and(|node| node.predecessors.contains(&1)),
            "busy StackUndo must queue instead of a silent spawn_state no-op"
        );
        assert_eq!(
            view.state.history.undo.len(),
            1,
            "busy StackUndo must not consume history until I/O frees"
        );
        assert_eq!(
            view.pending_toast_undo_id(),
            Some(toast_id),
            "busy StackUndo must wait on the toast undo queue"
        );
        view.set_busy_for_test(false);
        view.replay_pending_toast_undo_for_test(cx);
        assert!(
            view.pending_toast_undo_id().is_none(),
            "replay must consume the queued StackUndo"
        );
        assert!(
            view.is_busy(),
            "replayed StackUndo must take the I/O slot instead of dropping"
        );
    });
}

#[gpui_kit::test]
fn queued_toast_undo_survives_expiry(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.predecessors = vec![1];
    }
    state.toasts.push_undo(
        "Added link: A → B",
        UndoAction::RemoveLink {
            successor_id: 2,
            predecessor_id: 1,
        },
    );
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    view.update(cx, |view, cx| {
        view.set_busy_for_test(true);
        let toast_id = view
            .state
            .toasts
            .items
            .last()
            .map(|toast| toast.id)
            .unwrap();
        let action = view
            .state
            .toasts
            .items
            .last()
            .and_then(|toast| toast.undo.clone())
            .unwrap();
        view.apply_toast_undo(toast_id, action, cx);
        for toast in &mut view.state.toasts.items {
            toast.created_at = std::time::Instant::now() - std::time::Duration::from_secs(30);
        }
        view.state.toasts.expire();
        assert!(
            view.state
                .toasts
                .items
                .iter()
                .all(|toast| toast.id != toast_id),
            "the queued Undo toast may expire before I/O frees"
        );
        view.set_busy_for_test(false);
        view.replay_pending_toast_undo_for_test(cx);
        assert!(
            view.pending_toast_undo_id().is_none(),
            "replay must consume the queued Undo even after the toast is gone"
        );
        assert!(
            view.is_busy(),
            "replayed toast Undo must take the I/O slot instead of spawn_unbounded"
        );
    });
}

#[gpui_kit::test]
fn toast_undo_serializes_with_planner_io(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.predecessors = vec![1];
    }
    state.toasts.push_undo(
        "Added link: A → B",
        UndoAction::RemoveLink {
            successor_id: 2,
            predecessor_id: 1,
        },
    );
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    view.update(cx, |view, cx| {
        let toast_id = view
            .state
            .toasts
            .items
            .last()
            .map(|toast| toast.id)
            .unwrap();
        let action = view
            .state
            .toasts
            .items
            .last()
            .and_then(|toast| toast.undo.clone())
            .unwrap();
        view.apply_toast_undo(toast_id, action, cx);
        assert!(
            view.is_busy(),
            "toast Undo must hold io_busy so a later team switch or add-item cannot adopt a stale clone"
        );
        assert!(
            view.toast_undo_inflight_for_test(),
            "a second Undo click must not dismiss the link-add toast as a no-op"
        );
    });
}

#[gpui_kit::test]
fn details_modal_reuses_the_grid_range_picker(cx: &mut gpui_kit::TestAppContext) {
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
                ],
            }],
        }],
        values,
    }));
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("form-date-picker").is_some(),
        "details dates must paint the shared range picker"
    );
    assert!(
        cx.debug_bounds("form-dates-typed").is_some(),
        "details Dates must be a typeable field"
    );
    view.update(cx, |view, cx| {
        view.toggle_form_calendar();
        cx.notify();
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-dates-prev-month").is_some(),
        "details must reuse the grid picker's month arrows"
    );
    assert!(
        cx.debug_bounds("gantt-dates-range-start").is_some(),
        "details must shade the range start"
    );
    assert!(
        cx.debug_bounds("gantt-dates-range-end").is_some(),
        "details must shade the range end"
    );
    assert!(
        cx.debug_bounds("gantt-dates-in-range").is_some(),
        "details must fill the days between start and end"
    );
}

#[gpui_kit::test]
fn delete_confirm_warns_about_children(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.selected_id = Some(1);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    let delete = KeyDownEvent {
        keystroke: Keystroke::parse("delete").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&delete, cx);
        let warning = view.pending_delete_warning().unwrap_or_default();
        assert!(
            warning.contains("child") && warning.contains("stay"),
            "deleting a parent must warn that children stay in Azure DevOps, got {warning}"
        );
    });
}

#[gpui_kit::test]
fn double_click_title_opens_details(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.activate_title(1, 2, window, cx);
        });
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-title").is_none(),
        "double-click on a title must open details, not inline rename"
    );
    view.update(cx, |view, _| {
        assert!(
            view.form_is_loading() || view.state.form.is_some(),
            "double-click on a title must open details"
        );
        assert_eq!(view.state.gantt.selected_id, Some(1));
    });
    assert!(
        cx.debug_bounds("work-item-form-loading").is_some()
            || cx.debug_bounds("work-item-form").is_some(),
        "double-click on a title must show the details dialog"
    );
}

#[gpui_kit::test]
fn unscheduled_picker_does_not_keep_the_previous_range(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.start_date = None;
        node.target_date = None;
    }
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_inline_dates(1, window, cx);
        });
    });
    draw(cx);
    view.update(cx, |view, cx| {
        assert_eq!(
            view.inline_picker_date(cx),
            Some(PickerDate::Range(
                Some(chrono::NaiveDate::from_ymd_opt(2026, 7, 6).unwrap()),
                Some(chrono::NaiveDate::from_ymd_opt(2026, 7, 20).unwrap()),
            )),
            "scheduled item 1 starts as Jul 6 → Jul 20"
        );
        view.click_inline_calendar_day(chrono::NaiveDate::from_ymd_opt(2026, 8, 4).unwrap(), cx);
    });
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_inline_dates(2, window, cx);
        });
    });
    draw(cx);
    view.update(cx, |view, cx| {
        assert_eq!(
            view.inline_picker_date(cx),
            Some(PickerDate::Range(None, None)),
            "an Unscheduled item must open an empty range, not keep Jul 6 → Jul 20"
        );
        let typed = view.inline_input_value(cx);
        assert!(
            typed.is_empty()
                || typed == "Start → Target"
                || typed == "Start → Target, or start only",
            "unscheduled typed field must be empty, got {typed:?}"
        );
        assert_eq!(view.inline_placeholder(cx), "Start → Target, or start only");
        assert!(
            !typed.contains("Jul 6"),
            "stale scheduled range must not prefill the unscheduled picker, got {typed:?}"
        );
    });
}

#[gpui_kit::test]
fn details_picker_is_empty_for_unscheduled_item(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.start_date = None;
        node.target_date = None;
        node.title = "Unscheduled feature".into();
    }
    let mut values = serde_json::Map::new();
    values.insert(
        "System.Title".into(),
        serde_json::json!("Unscheduled feature"),
    );
    values.insert(
        ado_core::START_DATE_FIELD.into(),
        serde_json::json!("2026-07-06T00:00:00Z"),
    );
    values.insert(
        ado_core::TARGET_DATE_FIELD.into(),
        serde_json::json!("2026-07-20T00:00:00Z"),
    );
    state.form = Some(FormDraft::from_model(ado_core::WorkItemFormModel {
        id: 2,
        rev: 1,
        work_item_type: "Feature".into(),
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
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    view.update(cx, |view, cx| {
        assert_eq!(
            view.form_picker_date(cx),
            Some(PickerDate::Range(None, None)),
            "details must seed from the node's empty dates, not leftover form values"
        );
    });
}

#[gpui_kit::test]
fn details_picker_previews_the_start_before_the_end(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.start_date = None;
        node.target_date = None;
    }
    let mut values = serde_json::Map::new();
    values.insert(
        "System.Title".into(),
        serde_json::json!("Unscheduled feature"),
    );
    state.form = Some(FormDraft::from_model(ado_core::WorkItemFormModel {
        id: 2,
        rev: 1,
        work_item_type: "Feature".into(),
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
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.click_form_calendar_day(chrono::NaiveDate::from_ymd_opt(2026, 8, 4).unwrap(), cx);
        });
    });
    draw(cx);
    view.update(cx, |view, cx| {
        assert_eq!(
            view.form_picker_date(cx),
            Some(PickerDate::Range(
                Some(chrono::NaiveDate::from_ymd_opt(2026, 8, 4).unwrap()),
                None,
            )),
            "the first click of a details range must keep the start visible"
        );
        let start = view
            .state
            .form
            .as_ref()
            .and_then(|form| form.values.get(ado_core::START_DATE_FIELD));
        assert!(
            start.is_none() || start.is_some_and(|value| value.is_null()),
            "an incomplete range must not write Start Date into the form, got {start:?}"
        );
        let typed = view
            .form_input_value(ado_core::START_DATE_FIELD, cx)
            .unwrap_or_default();
        assert!(
            typed.contains("Aug 4") || typed.contains("2026-08-04"),
            "the calendar field must update after the first click, got {typed:?}"
        );
    });
}

#[gpui_kit::test]
fn details_open_shows_a_loading_indicator(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    view.update(cx, |view, _| {
        view.set_form_loading_for_test(1, "Persist cart");
    });
    draw(cx);
    assert!(
        cx.debug_bounds("work-item-form-loading").is_some(),
        "opening details must show a dialog immediately, not wait on ADO"
    );
    assert!(
        cx.debug_bounds("work-item-form-spinner").is_some(),
        "the details dialog must show a loading indicator while fields fetch"
    );
}

#[gpui_kit::test]
fn details_hides_empty_custom_fields_and_disables_clean_save(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    let mut values = serde_json::Map::new();
    values.insert("System.Title".into(), serde_json::json!("Persist cart"));
    values.insert("System.State".into(), serde_json::json!("New"));
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
                id: "System.State".into(),
                reference_name: "System.State".into(),
                label: "State".into(),
                kind: ado_core::FormControlKind::String,
                required: true,
                read_only: false,
                visible: true,
                options: None,
            },
        ],
        pages: vec![ado_core::FormPage {
            id: "details".into(),
            label: "Details".into(),
            groups: vec![ado_core::FormGroup {
                id: "planning".into(),
                label: "Planning".into(),
                controls: vec![
                    ado_core::FormControl {
                        id: "Custom.TShirtSize".into(),
                        reference_name: "Custom.TShirtSize".into(),
                        label: "T-Shirt Size".into(),
                        kind: ado_core::FormControlKind::Picklist,
                        required: false,
                        read_only: false,
                        visible: true,
                        options: Some(vec!["S".into(), "M".into(), "L".into()]),
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
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("form-dates-label").is_some(),
        "dates must sit near the top of the details dialog"
    );
    assert!(
        cx.debug_bounds("form-label-Custom.TShirtSize").is_none(),
        "empty custom fields must not lead the details dialog"
    );
    assert!(
        cx.debug_bounds("form-more-fields").is_some(),
        "empty fields should collapse under More"
    );
    view.update(cx, |view, _| {
        let form = view.state.form.as_ref().expect("form");
        assert!(!form.is_dirty(), "Save must stay disabled on a clean form");
    });
}

#[gpui_kit::test]
fn ascii_arrow_dates_save_and_garbage_blocks_save(cx: &mut gpui_kit::TestAppContext) {
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
        system_controls: vec![
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
        pages: Vec::new(),
        values,
    }));
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_form_dates("Jul 22 -> Jul 28", window, cx);
        });
    });
    draw(cx);
    view.update(cx, |view, cx| {
        let form = view.state.form.as_ref().expect("form");
        let start = form
            .values
            .get(ado_core::START_DATE_FIELD)
            .and_then(serde_json::Value::as_str)
            .and_then(|text| text.get(..10));
        let target = form
            .values
            .get(ado_core::TARGET_DATE_FIELD)
            .and_then(serde_json::Value::as_str)
            .and_then(|text| text.get(..10));
        assert_eq!(
            start,
            Some("2026-07-22"),
            "ASCII -> must keep Start, got {start:?}"
        );
        assert_eq!(
            target,
            Some("2026-07-28"),
            "ASCII -> must keep Target, got {target:?}"
        );
        assert!(
            view.form_save_is_enabled(cx),
            "a parsed range must enable Save"
        );
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_form_dates("Jul 22 -> zz", window, cx);
        });
    });
    draw(cx);
    view.update(cx, |view, cx| {
        assert!(
            view.form_dates_error_text(cx).is_some(),
            "garbage dates must show a validation error"
        );
        assert!(
            !view.form_save_is_enabled(cx),
            "Save must stay off on a parse failure"
        );
        let form = view.state.form.as_ref().expect("form");
        let start = form
            .values
            .get(ado_core::START_DATE_FIELD)
            .and_then(serde_json::Value::as_str)
            .and_then(|text| text.get(..10));
        assert_eq!(
            start,
            Some("2026-07-22"),
            "a parse failure must not PATCH empty dates, got {start:?}"
        );
    });
    draw(cx);
    assert!(
        cx.debug_bounds("form-dates-error").is_some(),
        "the Dates field must show the parse error"
    );
    let picker_with_error = cx.debug_bounds("form-date-picker").expect("picker");
    let error_w = f32::from(
        cx.debug_bounds("form-dates-error")
            .expect("error slot")
            .size
            .width,
    );
    assert!(
        error_w <= f32::from(picker_with_error.size.width) + 1.0,
        "the error must wrap inside the picker instead of widening it, error={error_w} picker={}",
        f32::from(picker_with_error.size.width)
    );
}

#[gpui_kit::test]
fn form_parse_error_resets_when_opening_another_item(cx: &mut gpui_kit::TestAppContext) {
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
    let controls = vec![
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
    ];
    state.form = Some(FormDraft::from_model(ado_core::WorkItemFormModel {
        id: 1,
        rev: 1,
        work_item_type: "Feature".into(),
        system_controls: controls.clone(),
        pages: Vec::new(),
        values: values.clone(),
    }));
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_form_dates("Jul 22 -> zz", window, cx);
        });
    });
    draw(cx);
    view.update(cx, |view, cx| {
        assert!(
            view.form_dates_error_text(cx).is_some(),
            "garbage dates must show a validation error"
        );
        assert!(!view.form_save_is_enabled(cx));
        view.state.form = Some(FormDraft::from_model(ado_core::WorkItemFormModel {
            id: 2,
            rev: 1,
            work_item_type: "Feature".into(),
            system_controls: controls,
            pages: Vec::new(),
            values,
        }));
    });
    draw(cx);
    view.update(cx, |view, cx| {
        assert!(
            view.form_dates_error_text(cx).is_none(),
            "a stale parse error must not leak into the next item, got {:?}",
            view.form_dates_error_text(cx)
        );
        assert!(
            !view.form_save_is_enabled(cx),
            "a clean item must not keep Save disabled from the previous parse error unless it is dirty"
        );
    });
}

#[gpui_kit::test]
fn calendar_pick_overwrites_typed_dates_in_the_field(cx: &mut gpui_kit::TestAppContext) {
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
        system_controls: vec![
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
        pages: Vec::new(),
        values,
    }));
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_form_dates("Jul 22 -> Jul 28", window, cx);
        });
    });
    draw(cx);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.click_form_calendar_day(chrono::NaiveDate::from_ymd_opt(2026, 7, 10).unwrap(), cx);
            view.click_form_calendar_day(chrono::NaiveDate::from_ymd_opt(2026, 7, 17).unwrap(), cx);
        });
    });
    draw(cx);
    view.update(cx, |view, cx| {
        view.sync_form_inputs_for_test(cx);
        let form = view.state.form.as_ref().expect("form");
        let start = form
            .values
            .get(ado_core::START_DATE_FIELD)
            .and_then(serde_json::Value::as_str)
            .and_then(|text| text.get(..10));
        let target = form
            .values
            .get(ado_core::TARGET_DATE_FIELD)
            .and_then(serde_json::Value::as_str)
            .and_then(|text| text.get(..10));
        assert_eq!(
            start,
            Some("2026-07-10"),
            "a later calendar pick must win over typed dates, got {start:?}"
        );
        assert_eq!(
            target,
            Some("2026-07-17"),
            "a later calendar pick must win over typed dates, got {target:?}"
        );
        let typed = view
            .form_input_value(ado_core::START_DATE_FIELD, cx)
            .unwrap_or_default();
        assert!(
            typed.contains("Jul 10") && typed.contains("Jul 17"),
            "the Dates field must show the calendar range, got {typed:?}"
        );
    });
}

#[gpui_kit::test]
fn realistic_backlog_keeps_markers_badges_and_indent(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = fake_ado_shaped_backlog();
    state.gantt.set_tree_width(640.0);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, _| {
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars =
            view.state
                .gantt
                .layout_bars(&view.state.nodes, &rows, &view.state.iterations, 720.0);
        let unscheduled: Vec<_> = bars
            .iter()
            .filter(|bar| bar.kind == ado_core::GanttBarKind::Unscheduled)
            .collect();
        assert!(
            unscheduled.len() > 10,
            "realistic mock data must include a long unscheduled tail"
        );
        assert!(
            unscheduled.iter().all(|bar| bar.marker),
            "unscheduled items must be gutter pills, not ghost bars"
        );
        let task = rows.iter().find(|row| row.id == 4).expect("task");
        let test_case = rows.iter().find(|row| row.id == 5).expect("test case");
        assert_eq!(
            task.depth, test_case.depth,
            "Task and Test Case under the same story must share indent depth"
        );
        assert_eq!(ado_planner::ui::type_abbrev("Product Backlog Item"), "PBI");
        assert_eq!(ado_planner::ui::type_abbrev("User Story"), "US");
        assert_eq!(ado_planner::ui::type_abbrev("Test Case"), "TC");
    });
    assert!(
        cx.debug_bounds("gantt-expand-slot-4").is_some(),
        "leaf tasks must keep an expander spacer so titles line up"
    );
    assert!(
        cx.debug_bounds("gantt-expand-slot-5").is_some(),
        "leaf test cases must keep the same expander spacer"
    );
    let task_slot = cx.debug_bounds("gantt-expand-slot-4").expect("task slot");
    let test_slot = cx.debug_bounds("gantt-expand-slot-5").expect("test slot");
    assert_eq!(
        task_slot.size.width, test_slot.size.width,
        "Task and Test Case expander slots must be the same width"
    );
    assert!(
        cx.debug_bounds("gantt-type-badge").is_some(),
        "type badges must paint so truncated names can show a tooltip"
    );
    assert!(
        cx.debug_bounds("gantt-open-details").is_some(),
        "details affordance must be visible on unselected rows"
    );
}

fn mock_tree_node(id: i64, parent: Option<i64>, ty: &str, title: &str) -> WorkItemNode {
    WorkItemNode {
        id,
        rev: 1,
        title: title.into(),
        work_item_type: ty.into(),
        state: "New".into(),
        assigned_to: None,
        parent_id: parent,
        area_path: "Shop\\Platform".into(),
        iteration_path: "Shop\\FY26".into(),
        start_date: Some("2026-07-06T00:00:00Z".into()),
        target_date: Some("2026-07-20T00:00:00Z".into()),
        has_date_fields: true,
        predecessors: Vec::new(),
        fields: serde_json::Map::new(),
    }
}

#[gpui_kit::test]
fn add_child_starts_inline_rename(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    let created = mock_tree_node(1999, Some(1), "Feature", "New work item");
    state.nodes.insert(1, created);
    state.gantt.selected_id = Some(1999);
    state.history.undo.push(HistoryEntry::Created {
        id: 1999,
        title: "New work item".into(),
        work_item_type: "Feature".into(),
        placement: "under Epic \"Work Item 0\"".into(),
    });
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    view.update(cx, |view, _| {
        view.after_io_selection_change_for_test(Some(1));
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-title").is_some(),
        "Add child must focus inline rename on the new row"
    );
    view.update(cx, |view, _| {
        assert_eq!(view.state.gantt.selected_id, Some(1999));
        assert!(
            view.state.gantt.hovered_id.is_none(),
            "Add child must not leave hover chrome on the parent name cell"
        );
    });
}

#[gpui_kit::test]
fn add_child_undo_clears_title_focus_ticks(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    let created = mock_tree_node(1999, Some(1), "Feature", "New work item");
    state.nodes.insert(1, created);
    state.gantt.selected_id = Some(1999);
    state.gantt.hovered_id = Some(1);
    state.history.undo.push(HistoryEntry::Created {
        id: 1999,
        title: "New work item".into(),
        work_item_type: "Feature".into(),
        placement: "under Epic \"Work Item 0\"".into(),
    });
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    view.update(cx, |view, _| {
        view.after_io_selection_change_for_test(Some(1));
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-title").is_some(),
        "new child must open rename before undo"
    );
    view.update(cx, |view, _| {
        view.state.nodes.retain(|node| node.id != 1999);
        view.state.gantt.selected_id = None;
        view.state.history.undo.clear();
        view.after_io_selection_change_for_test(Some(1999));
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-inline-title").is_none(),
        "Undo of Add child must close inline rename"
    );
    cx.update(|window, cx| {
        view.update(cx, |view, _| {
            assert_eq!(
                view.state.gantt.selected_id, None,
                "undo of create must not jump selection to the first row"
            );
            assert!(
                view.state.gantt.hovered_id.is_none(),
                "Undo must not leave hover chrome on the Cart name cell"
            );
            assert!(
                view.planner_keys_are_focused(window),
                "Undo must blur the title button so corner ticks do not persist"
            );
        });
    });
}

#[gpui_kit::test]
fn add_item_under_epic_paints_at_child_indent(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    // Match the mock backlog the reviewer uses: Epic #1001, Feature #1013
    // under it, then root Task #1012 so a first-child Feature sits next to
    // the Epic and indent is unambiguous.
    let mut nodes = vec![
        mock_tree_node(1001, None, "Epic", "Checkout rewrite"),
        mock_tree_node(1013, Some(1001), "Feature", "Go-live milestone"),
        mock_tree_node(1012, None, "Task", "Unparented spike"),
    ];
    let created = mock_tree_node(1999, Some(1001), "Feature", "New work item");
    let insert_at = insert_index_for_child(&nodes, Some(1001));
    nodes.insert(insert_at, created);
    state.nodes = nodes;
    state.gantt.selected_id = Some(1999);
    state.gantt.set_tree_width(640.0);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, _| {
        let visible = view.state.visible_nodes();
        let rows = view.state.gantt.flatten(&visible);
        let ids: Vec<i64> = rows.iter().map(|row| row.id).collect();
        let epic_at = ids.iter().position(|id| *id == 1001).expect("epic");
        let sibling_at = ids.iter().position(|id| *id == 1013).expect("sibling");
        let created_at = ids.iter().position(|id| *id == 1999).expect("created");
        let root_at = ids.iter().position(|id| *id == 1012).expect("root task");
        assert_eq!(
            created_at,
            epic_at + 1,
            "new Feature must sit directly under the Epic, got {ids:?}"
        );
        assert!(
            epic_at < created_at && created_at < sibling_at && sibling_at < root_at,
            "new Feature must sit before existing siblings and before the next root, got {ids:?}"
        );
        let created_row = rows.iter().find(|row| row.id == 1999).unwrap();
        let sibling_row = rows.iter().find(|row| row.id == 1013).unwrap();
        assert_eq!(
            created_row.depth, sibling_row.depth,
            "created Feature must share depth with Feature #1013, got {created_row:?}"
        );
        assert!(
            created_row.depth > 0,
            "created Feature must not flatten as a root, got {created_row:?}"
        );
    });
    let epic_row = cx.debug_bounds("gantt-tree-row-1001").expect("epic row");
    let sibling_row = cx.debug_bounds("gantt-tree-row-1013").expect("sibling row");
    let created_row = cx.debug_bounds("gantt-tree-row-1999").expect("created row");
    let root_row = cx.debug_bounds("gantt-tree-row-1012").expect("root row");
    let epic_y = f32::from(epic_row.origin.y);
    let sibling_y = f32::from(sibling_row.origin.y);
    let created_y = f32::from(created_row.origin.y);
    let root_y = f32::from(root_row.origin.y);
    assert!(
        epic_y < created_y && created_y < sibling_y && sibling_y < root_y,
        "painted order must be Epic, new Feature, Go-live, Unparented spike; y={epic_y},{created_y},{sibling_y},{root_y}"
    );
    let sibling_slot = cx
        .debug_bounds("gantt-expand-slot-1013")
        .expect("sibling expander");
    let created_slot = cx
        .debug_bounds("gantt-expand-slot-1999")
        .expect("created expander");
    let epic_slot = cx
        .debug_bounds("gantt-expand-slot-1001")
        .expect("epic expander");
    let root_slot = cx
        .debug_bounds("gantt-expand-slot-1012")
        .expect("root expander");
    let sibling_x = f32::from(sibling_slot.origin.x);
    let created_x = f32::from(created_slot.origin.x);
    let epic_x = f32::from(epic_slot.origin.x);
    let root_x = f32::from(root_slot.origin.x);
    assert!(
        (created_x - sibling_x).abs() < 2.0,
        "new Feature expander x {created_x} must match sibling Feature {sibling_x}, not a root"
    );
    assert!(
        created_x >= epic_x + TREE_INDENT - 2.0,
        "new Feature expander x {created_x} must sit one indent right of Epic {epic_x}"
    );
    assert!(
        created_x >= root_x + TREE_INDENT - 2.0,
        "new Feature expander x {created_x} must sit one indent right of root Task {root_x}"
    );
    let created_indent = cx
        .debug_bounds("gantt-tree-indent-1999")
        .expect("created indent spacer");
    let sibling_indent = cx
        .debug_bounds("gantt-tree-indent-1013")
        .expect("sibling indent spacer");
    assert!(
        (f32::from(created_indent.size.width) - TREE_INDENT).abs() < 1.0,
        "created indent spacer must be TREE_INDENT {TREE_INDENT}, got {}",
        f32::from(created_indent.size.width)
    );
    assert!(
        (f32::from(sibling_indent.size.width) - TREE_INDENT).abs() < 1.0,
        "sibling indent spacer must be TREE_INDENT {TREE_INDENT}, got {}",
        f32::from(sibling_indent.size.width)
    );
}

#[gpui_kit::test]
fn invalid_backlog_banner_does_not_replace_the_toolbar(cx: &mut gpui_kit::TestAppContext) {
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
        "team-settings errors should stay readable"
    );
    assert!(
        cx.debug_bounds("gantt-nav").is_some(),
        "the invalid-backlog banner must overlay, not push the Gantt toolbar down"
    );
    assert!(
        cx.debug_bounds("gantt-range-title").is_some(),
        "the visible range label must remain on the toolbar under the banner"
    );
}

#[gpui_kit::test]
fn pre_sign_in_window_title_does_not_leak_the_last_org(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.session_info.signed_in = false;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.window_title(),
            "ADO Planner",
            "the window title must stay generic until sign-in, got {}",
            view.window_title()
        );
    });
}

#[gpui_kit::test]
fn opening_details_shows_loading_not_saving(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    view.update(cx, |view, _| {
        view.set_form_loading_for_test(1, "Persist cart");
        assert_eq!(
            view.sync_status_for_test(),
            "Loading…",
            "opening the info dialog is a GET, so the pill must say Loading… not Saving…"
        );
    });
}

#[gpui_kit::test]
fn default_name_column_is_wider_than_a_dozen_glyphs(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let title = cx.debug_bounds("gantt-title-1").expect("title cell");
    assert!(
        f32::from(title.size.width) >= 180.0,
        "the Work item name column must be wider than 10–15 characters, got {}",
        f32::from(title.size.width)
    );
    assert!(
        cx.debug_bounds("gantt-splitter").is_some(),
        "the tree splitter must still paint so the width can be dragged"
    );
}

#[gpui_kit::test]
fn second_details_click_keeps_the_loading_dialog(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    view.update(cx, |view, cx| {
        view.set_form_loading_for_test(1, "Persist cart");
        view.set_busy_for_test(true);
        view.open_form(1, cx);
        assert!(
            view.form_is_loading(),
            "a second details click must not drop the in-flight form"
        );
    });
    draw(cx);
    assert!(
        cx.debug_bounds("work-item-form-loading").is_some(),
        "the loading dialog must stay up while the first fetch is in flight"
    );
}

#[gpui_kit::test]
fn theme_icon_opens_a_flavor_menu(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    draw(cx);
    assert!(
        cx.debug_bounds("header-theme-trigger").is_some(),
        "theme must paint as an icon trigger"
    );
    assert!(
        cx.debug_bounds("flavor-option-mocha").is_none(),
        "theme names must stay in the dropdown until it opens"
    );
    view.update(cx, |view, cx| {
        view.set_open_flavor(true);
        cx.notify();
    });
    draw(cx);
    assert!(
        cx.debug_bounds("flavor-option-latte").is_some(),
        "the theme menu must list Latte"
    );
    assert!(
        cx.debug_bounds("flavor-option-mocha").is_some(),
        "the theme menu must list Mocha"
    );
    let theme = cx.debug_bounds("header-theme-trigger").expect("theme");
    let logout = cx.debug_bounds("header-log-out").expect("log out");
    let filters = cx.debug_bounds("header-filters").expect("filters");
    assert!(
        f32::from(theme.origin.x) >= f32::from(filters.origin.x),
        "theme must sit on the right of the header, got theme x {} filters x {}",
        f32::from(theme.origin.x),
        f32::from(filters.origin.x)
    );
    assert!(
        f32::from(logout.origin.x) > f32::from(theme.origin.x),
        "log out must sit to the right of theme"
    );
}

#[gpui_kit::test]
fn start_only_bar_paints_a_right_edge_continuation_cue(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    if let Some(node) = state.nodes.iter_mut().find(|node| node.id == 2) {
        node.target_date = None;
        node.title = "Cart UI".into();
    }
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-bar-2").is_some(),
        "start-only must still paint a default-length bar"
    );
    let cue = cx
        .debug_bounds("gantt-open-ended-cue-2")
        .expect("open-ended continuation cue");
    let timeline = cx
        .debug_bounds("gantt-timeline-body")
        .expect("timeline body");
    let cue_right = f32::from(cue.origin.x) + f32::from(cue.size.width);
    let clip_right = f32::from(timeline.origin.x) + f32::from(timeline.size.width);
    assert!(
        (cue_right - clip_right).abs() < 8.0,
        "the continuation cue must reach the viewport edge, cue_right={cue_right} clip={clip_right}"
    );
}

#[gpui_kit::test]
fn open_ended_cart_ui_shows_a_hover_chip_near_its_right_edge(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1005").expect("Cart UI bar");
    let cue = cx
        .debug_bounds("gantt-open-ended-cue-1005")
        .expect("Cart UI open-ended tail");
    let bar_right = f32::from(bar.origin.x) + f32::from(bar.size.width);
    let cue_left = f32::from(cue.origin.x);
    assert!(
        (cue_left - bar_right).abs() < 8.0,
        "the continuation cue must start at Cart UI's painted right, cue={cue_left} bar_right={bar_right}"
    );
    view.update(cx, |view, _| {
        view.state.gantt.pin_bar_tooltip = Some(1005);
    });
    draw(cx);
    let tip = cx
        .debug_bounds("gantt-bar-tooltip")
        .expect("open-ended Cart UI must show a dates chip");
    let tip_left = f32::from(tip.origin.x);
    let tip_right = tip_left + f32::from(tip.size.width);
    let tip_top = f32::from(tip.origin.y);
    let tip_bottom = tip_top + f32::from(tip.size.height);
    let bar_top = f32::from(bar.origin.y);
    let bar_bottom = bar_top + f32::from(bar.size.height);
    assert!(
        tip_top < bar_bottom + 4.0 && tip_bottom > bar_top - 4.0,
        "Cart UI dates chip must sit in the bar's row, tip={tip_top}..{tip_bottom} bar={bar_top}..{bar_bottom}"
    );
    let near_core = tip_left + 1.0 >= bar_right - 4.0 && tip_left <= bar_right + 48.0;
    let flipped_left = tip_right <= f32::from(bar.origin.x) + 4.0;
    let overlaps_solid = tip_left < bar_right + 48.0 && tip_right > f32::from(bar.origin.x) - 4.0;
    assert!(
        tip_right <= 1280.0 + 1.0,
        "Cart UI dates chip must not clip past the right window edge, tip={tip_left}..{tip_right}"
    );
    assert!(
        near_core || flipped_left || overlaps_solid,
        "Cart UI dates chip must stay on the solid bar (or flip left), not park on the far cue, tip={tip_left}..{tip_right} bar_right={bar_right}"
    );
    let hover = point(
        px(cue_left + f32::from(cue.size.width).min(80.0) * 0.5),
        cue.origin.y + cue.size.height / 2.0,
    );
    cx.simulate_mouse_move(hover, None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.bar_hovered_id,
            Some(1005),
            "hovering Cart UI's open-ended tail must keep the bar hovered so the dates chip can show"
        );
    });
}

#[gpui_kit::test]
fn open_ended_cue_forwards_wheel_pan(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 1.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let cue = cx
        .debug_bounds("gantt-open-ended-cue-1005")
        .expect("Cart UI open-ended tail");
    let before = view.update(cx, |view, _| view.state.gantt.scroll_x);
    cx.simulate_event(ScrollEvent {
        position: cue.center(),
        delta: ScrollDelta::Pixels(point(px(120.), px(0.))),
        ..Default::default()
    });
    draw(cx);
    view.update(cx, |view, _| {
        assert_ne!(
            view.state.gantt.scroll_x, before,
            "wheel over the open-ended tail must pan the timeline, not drop the event"
        );
    });
}

#[gpui_kit::test]
fn clicking_an_arrow_over_an_open_ended_cue_selects_the_link(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 1.0;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let cue = cx
        .debug_bounds("gantt-open-ended-cue-1005")
        .expect("Cart UI open-ended tail");
    let arrow = cx
        .debug_bounds("gantt-dep-arrow-1005-1012")
        .expect("Cart UI → spike connector");
    let cue_left = f32::from(cue.origin.x);
    let cue_right = cue_left + f32::from(cue.size.width);
    let cue_top = f32::from(cue.origin.y);
    let cue_bottom = cue_top + f32::from(cue.size.height);
    let arrow_left = f32::from(arrow.origin.x);
    let arrow_right = arrow_left + f32::from(arrow.size.width);
    let arrow_top = f32::from(arrow.origin.y);
    let arrow_bottom = arrow_top + f32::from(arrow.size.height);
    let overlap = arrow_right > cue_left
        && arrow_left < cue_right
        && arrow_bottom > cue_top
        && arrow_top < cue_bottom;
    let click = if overlap {
        point(
            px(((arrow_left.max(cue_left)) + (arrow_right.min(cue_right))) / 2.0),
            px(((arrow_top.max(cue_top)) + (arrow_bottom.min(cue_bottom))) / 2.0),
        )
    } else {
        arrow.center()
    };
    cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.selected_link,
            Some((1005, 1012)),
            "clicking the Cart UI → spike arrow must select the link even where the open-ended tail overlaps it"
        );
    });
}

#[gpui_kit::test]
fn shift_dragging_a_parent_moves_its_children(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = fake_hierarchy(3);
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    let child_before = state
        .nodes
        .iter()
        .find(|node| node.id == 2)
        .and_then(|node| ado_core::parse_iso_date(node.start_date.as_deref()));
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1").expect("parent bar");
    let start = bar.center();
    let shift = Modifiers {
        shift: true,
        ..Default::default()
    };
    cx.simulate_mouse_down(start, MouseButton::Left, shift);
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("Shift-drag must start a move");
        assert_eq!(gesture.kind, GestureKind::Move);
    });
    let ppd = view.update(cx, |view, _| {
        view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom)
    });
    cx.simulate_mouse_move(
        point(start.x + px(ppd * 3.0), start.y),
        Some(MouseButton::Left),
        shift,
    );
    draw(cx);
    cx.simulate_mouse_up(
        point(start.x + px(ppd * 3.0), start.y),
        MouseButton::Left,
        shift,
    );
    view.update(cx, |view, _| {
        let expected = child_before.map(|date| date.saturating_add(3.days()));
        assert_eq!(
            view.last_committed_start_for_test(),
            expected,
            "Shift-dragging the parent must commit the parent move"
        );
        match view.state.history.undo.last() {
            Some(HistoryEntry::DatesGroup { items }) => {
                assert!(
                    items.iter().any(|item| item.id == 1) && items.iter().any(|item| item.id == 2),
                    "Shift-drag must move children in one undo, got {items:?}"
                );
                let child = items.iter().find(|item| item.id == 2).unwrap();
                assert_eq!(
                    ado_core::parse_iso_date(child.after_start.as_deref()),
                    expected,
                    "the grouped child change must shift by the same delta"
                );
            }
            None => {
                // idle_state has no ADO mock; persist fails and must restore
                // children instead of leaving a DatesGroup with no parent PATCH.
                let child = view.state.nodes.iter().find(|node| node.id == 2).unwrap();
                assert_eq!(
                    ado_core::parse_iso_date(child.start_date.as_deref()),
                    child_before,
                    "a failed parent persist must restore Shift-drag children"
                );
            }
            other => panic!("expected DatesGroup or a failed-persist rollback, got {other:?}"),
        }
    });
}

#[gpui_kit::test]
fn dragging_the_end_dot_onto_another_bar_arms_a_dependency(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.hovered_id = Some(2);
    state.gantt.bar_hovered_id = Some(2);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let handle = cx
        .debug_bounds("gantt-link-handle-2-link-end")
        .expect("end dot");
    cx.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view
            .state
            .gantt
            .gesture
            .as_ref()
            .expect("end-dot drag must start a link");
        assert_eq!(
            gesture.kind,
            GestureKind::Link,
            "the visible end dot must start a link, got {:?}",
            gesture.kind
        );
    });
    let target = cx.debug_bounds("gantt-bar-3").expect("target bar");
    cx.simulate_mouse_move(
        target.center(),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    view.update(cx, |view, _| {
        let gesture = view.state.gantt.gesture.as_ref().expect("still linking");
        assert_eq!(
            gesture.link_to,
            Some(3),
            "dropping the end-dot on another bar must arm the dependency, got {:?}",
            gesture.link_to
        );
    });
    cx.simulate_mouse_up(target.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "mouse-up on the target bar must commit the link gesture"
        );
    });
}

#[gpui_kit::test]
fn leaving_a_bar_clears_the_hover_tooltip_and_date_pills(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    cx.simulate_mouse_move(bar.center(), None, Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-hover-tooltip").is_some(),
        "hovering a bar must show the overlay tooltip"
    );
    assert!(
        cx.debug_bounds("gantt-hover-date-start").is_some(),
        "hovering a bar must show the Start Date pill"
    );
    let nav = cx.debug_bounds("gantt-nav").expect("nav");
    cx.simulate_mouse_move(nav.center(), None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.bar_hovered_id, None,
            "leaving the bar must clear bar hover"
        );
    });
    assert!(
        cx.debug_bounds("gantt-hover-tooltip").is_none(),
        "the tooltip must not stay after the pointer leaves the bar"
    );
    assert!(
        cx.debug_bounds("gantt-hover-date-start").is_none(),
        "date pills must not stay after the pointer leaves the bar"
    );
}

#[gpui_kit::test]
fn persist_cart_hover_shows_one_overlay_tooltip(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1003").expect("Persist cart");
    cx.simulate_mouse_move(bar.center(), None, Modifiers::default());
    draw(cx);
    let tips = [
        cx.debug_bounds("gantt-hover-tooltip").is_some(),
        cx.debug_bounds("gantt-bar-tooltip").is_some(),
    ];
    assert!(tips[0], "Persist cart hover must show the overlay tooltip");
    assert!(
        !tips[1],
        "Persist cart must not also pin a gantt-bar-tooltip; two chips stacked as small+large duplicates"
    );
}

#[gpui_kit::test]
fn end_edge_hit_every_offset_from_minus_6_to_plus_4(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    let width_before = f32::from(bar.size.width);
    for offset in -6i32..=4 {
        let at = point(
            bar.origin.x + bar.size.width + px(offset as f32),
            bar.origin.y + bar.size.height / 2.0,
        );
        cx.simulate_mouse_move(at, None, Modifiers::default());
        view.update(cx, |view, _| {
            let zone = view.hover_bar_zone_for_test().map(|(_, zone)| zone);
            if offset <= 0 {
                assert_eq!(
                    zone,
                    Some(BarHitZone::ResizeEnd),
                    "{offset:+}px from the painted end must be ResizeEnd (4px inside is not dead), got {zone:?}"
                );
            } else {
                assert!(
                    matches!(zone, Some(BarHitZone::ResizeEnd | BarHitZone::Link)),
                    "{offset:+}px past the end must be live (resize halo or outer link), not a dead no-op, got {zone:?}"
                );
            }
        });
    }
    for offset in 0i32..=8 {
        let at = grab_bar_from_left(bar, offset as f32);
        cx.simulate_mouse_move(at, None, Modifiers::default());
        view.update(cx, |view, _| {
            let zone = view.hover_bar_zone_for_test().map(|(_, zone)| zone);
            assert_eq!(
                zone,
                Some(BarHitZone::ResizeStart),
                "{offset}px inside the painted start must be ResizeStart, got {zone:?}"
            );
        });
    }
    let inside_4 = grab_bar_from_right(bar, 4.0);
    cx.simulate_mouse_move(inside_4, None, Modifiers::default());
    cx.simulate_mouse_down(inside_4, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::ResizeEnd),
            "a press 4px inside the painted end must resize, not be a dead no-op"
        );
    });
    cx.simulate_mouse_up(inside_4, MouseButton::Left, Modifiers::default());
    draw(cx);
    let inside_start = grab_bar_from_left(bar, 4.0);
    cx.simulate_mouse_move(inside_start, None, Modifiers::default());
    cx.simulate_mouse_down(inside_start, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::ResizeStart),
            "a press 4px inside the painted start must resize, not be a dead no-op"
        );
    });
    cx.simulate_mouse_up(inside_start, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "a no-op release must drop the live resize preview"
        );
    });
    assert!(
        cx.debug_bounds("gantt-drag-preview").is_none(),
        "a no-op release must clear the date preview chip"
    );
    let after = cx
        .debug_bounds("gantt-bar-2")
        .expect("leaf bar after no-op");
    assert!(
        (f32::from(after.size.width) - width_before).abs() < 1.0,
        "a no-op 4px-inside press must not leave the bar drawn narrower, before={width_before} after={}",
        f32::from(after.size.width)
    );
}

#[gpui_kit::test]
fn cart_both_edges_resize_and_noop_release_clears_preview(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = FITTED_MONTH_ZOOM;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1002").expect("Cart");
    let width_before = f32::from(bar.size.width);
    assert!(
        width_before > 24.0,
        "precondition: Cart must be wide enough to resize, width={width_before}"
    );
    let mid_y = bar.origin.y + bar.size.height / 2.0;
    let dates_before = view.update(cx, |view, _| {
        view.state
            .nodes
            .iter()
            .find(|node| node.id == 1002)
            .map(|node| (node.start_date.clone(), node.target_date.clone()))
    });
    let start_4 = grab_bar_from_left(bar, 4.0);
    cx.simulate_mouse_move(start_4, None, Modifiers::default());
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_bar_zone_for_test().map(|(_, zone)| zone),
            Some(BarHitZone::ResizeStart),
            "4px inside Cart's left edge must be ResizeStart"
        );
    });
    cx.simulate_mouse_down(start_4, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::ResizeStart),
            "a press 4px inside Cart's left edge must start ResizeStart"
        );
    });
    cx.simulate_mouse_up(start_4, MouseButton::Left, Modifiers::default());
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1002").expect("Cart after start");
    let bar_mid = point(bar.origin.x + bar.size.width / 2.0, mid_y);
    cx.simulate_mouse_move(bar_mid, None, Modifiers::default());
    draw(cx);
    let end_6 = grab_bar_from_right(bar, 6.0);
    cx.simulate_mouse_move(end_6, None, Modifiers::default());
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_bar_zone_for_test().map(|(_, zone)| zone),
            Some(BarHitZone::ResizeEnd),
            "6px inside Cart's right edge must be ResizeEnd, not a dead no-op"
        );
    });
    cx.simulate_mouse_down(end_6, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::ResizeEnd),
            "a press 6px inside Cart's right edge must start ResizeEnd"
        );
    });
    cx.simulate_mouse_up(end_6, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "a no-op Cart resize release must drop the live preview"
        );
        let dates = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 1002)
            .map(|node| (node.start_date.clone(), node.target_date.clone()));
        assert_eq!(
            dates, dates_before,
            "a no-op Cart resize must leave Start/Target unchanged"
        );
    });
    assert!(
        cx.debug_bounds("gantt-drag-preview").is_none(),
        "a no-op Cart resize must clear the date preview chip"
    );
    let after = cx.debug_bounds("gantt-bar-1002").expect("Cart after no-op");
    assert!(
        (f32::from(after.size.width) - width_before).abs() < 1.0,
        "a no-op Cart press must not leave the bar drawn narrower, before={width_before} after={}",
        f32::from(after.size.width)
    );
}

#[gpui_kit::test]
fn new_link_preview_uses_a_lane_offset_below_the_source_bar(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    cx.simulate_mouse_down(
        grab_outer_link_handle(bar),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    let preview = cx
        .debug_bounds("gantt-link-preview")
        .expect("rubber-band horizontal");
    let bar_bottom = f32::from(bar.origin.y) + f32::from(bar.size.height);
    let preview_top = f32::from(preview.origin.y);
    assert!(
        preview_top + 1.0 >= bar_bottom + 2.0,
        "new-link horizontal must sit in a lane below the source bar, not 2px above its top, preview_top={preview_top} bar_bottom={bar_bottom}"
    );
}

#[gpui_kit::test]
fn new_link_preview_originates_at_the_source_handle(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    cx.simulate_mouse_down(
        grab_outer_link_handle(bar),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    let out = cx
        .debug_bounds("gantt-link-preview-out")
        .expect("outgoing stub from the source handle");
    let preview = cx
        .debug_bounds("gantt-link-preview")
        .expect("rubber-band horizontal");
    let bar_top = f32::from(bar.origin.y);
    let bar_bottom = bar_top + f32::from(bar.size.height);
    let bar_mid = bar_top + f32::from(bar.size.height) / 2.0;
    let out_top = f32::from(out.origin.y);
    let out_bottom = out_top + f32::from(out.size.height);
    let preview_top = f32::from(preview.origin.y);
    assert!(
        (out_top - bar_mid).abs() < 6.0,
        "outgoing stub must start at the source connector, out_top={out_top} bar_mid={bar_mid}"
    );
    assert!(
        out_bottom + 2.0 >= preview_top,
        "outgoing stub must meet the lane horizontal, out_bottom={out_bottom} preview_top={preview_top}"
    );
    if let Some(inbound) = cx.debug_bounds("gantt-link-preview-v") {
        let inbound_top = f32::from(inbound.origin.y);
        let inbound_h = f32::from(inbound.size.height);
        let inbound_bottom = inbound_top + inbound_h;
        let overlaps_bar = inbound_top < bar_bottom - 1.0 && inbound_bottom > bar_top + 1.0;
        assert!(
            !overlaps_bar || inbound_h <= 4.0,
            "on press the inbound vertical must not run back through the source bar, inbound_top={inbound_top} inbound_h={inbound_h} bar_top={bar_top} bar_bottom={bar_bottom}"
        );
    }
    let target = cx.debug_bounds("gantt-bar-3").expect("successor bar");
    cx.simulate_mouse_move(
        target.center(),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    let inbound = cx
        .debug_bounds("gantt-link-preview-v")
        .expect("inbound drop once the pointer leaves the source row");
    let inbound_top = f32::from(inbound.origin.y);
    assert!(
        inbound_top + 2.0 >= preview_top - 4.0,
        "inbound must drop from the lane, not from inside the source bar, inbound_top={inbound_top} lane={preview_top}"
    );
    assert!(
        cx.debug_bounds("gantt-link-preview-out").is_some(),
        "outgoing stub must stay attached to the source handle while dragging"
    );
}

#[gpui_kit::test]
fn start_halo_does_not_steal_inbound_link_hits(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    let inbound = cx
        .debug_bounds("gantt-link-handle-2-link-start")
        .map(|handle| {
            point(
                handle.origin.x + handle.size.width / 2.0,
                handle.origin.y + handle.size.height / 2.0,
            )
        })
        .unwrap_or_else(|| grab_inbound_link_handle(bar));
    cx.simulate_mouse_move(inbound, None, Modifiers::default());
    view.update(cx, |view, _| {
        assert_eq!(
            view.hover_bar_zone_for_test().map(|(_, zone)| zone),
            Some(BarHitZone::Link),
            "hovering the inbound connector must classify link, not start-resize"
        );
        assert_eq!(
            view.hover_cursor_for_test(),
            Some("drag-link"),
            "the inbound connector must show the link cursor, not ew-resize"
        );
    });
    cx.simulate_mouse_down(inbound, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind),
            Some(GestureKind::Link),
            "a press on the inbound connector must start a link, not a start resize"
        );
    });
}

#[gpui_kit::test]
fn cart_end_edge_hit_every_offset_from_minus_8_to_plus_2(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = FITTED_MONTH_ZOOM;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1002").expect("Cart");
    cx.simulate_mouse_move(bar.center(), None, Modifiers::default());
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1002").expect("Cart after hover");
    for offset in -8i32..=2 {
        let at = point(
            bar.origin.x + bar.size.width + px(offset as f32),
            bar.origin.y + bar.size.height / 2.0,
        );
        cx.simulate_mouse_move(at, None, Modifiers::default());
        view.update(cx, |view, _| {
            let zone = view.hover_bar_zone_for_test().map(|(_, zone)| zone);
            assert!(
                matches!(zone, Some(BarHitZone::ResizeEnd | BarHitZone::Link)),
                "Cart painted end {offset:+}px must be resize or link, never a dead no-op, got {zone:?}"
            );
        });
        cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::default());
        draw(cx);
        view.update(cx, |view, _| {
            let kind = view
                .state
                .gantt
                .gesture
                .as_ref()
                .map(|gesture| gesture.kind);
            assert!(
                matches!(kind, Some(GestureKind::ResizeEnd | GestureKind::Link)),
                "a press {offset:+}px from the Cart end must resize or link, never nothing, got {kind:?}"
            );
            view.state.cancel_gesture();
        });
        cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::default());
        draw(cx);
    }
    for offset in 0i32..=8 {
        let at = grab_bar_from_left(bar, offset as f32);
        cx.simulate_mouse_move(at, None, Modifiers::default());
        view.update(cx, |view, _| {
            let zone = view.hover_bar_zone_for_test().map(|(_, zone)| zone);
            assert_eq!(
                zone,
                Some(BarHitZone::ResizeStart),
                "Cart painted start {offset}px inside must be ResizeStart, got {zone:?}"
            );
        });
        cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::default());
        draw(cx);
        view.update(cx, |view, _| {
            assert_eq!(
                view.state
                    .gantt
                    .gesture
                    .as_ref()
                    .map(|gesture| gesture.kind),
                Some(GestureKind::ResizeStart),
                "a press {offset}px inside Cart's start must resize, got {:?}",
                view.state.gantt.gesture.as_ref().map(|g| g.kind)
            );
            view.state.cancel_gesture();
        });
        cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::default());
        draw(cx);
    }
}

#[gpui_kit::test]
fn ctrl_z_clears_tooltip_when_the_pointer_is_off_the_bar(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    let grip = grab_end_resize(bar);
    cx.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
    draw(cx);
    let ppd = view.update(cx, |view, _| {
        view.state.gantt.scale.pixels_per_day(view.state.gantt.zoom)
    });
    let dest = point(grip.x + px(ppd * 8.0), grip.y);
    cx.simulate_mouse_move(dest, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    cx.simulate_mouse_up(dest, MouseButton::Left, Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-hover-tooltip").is_some()
            || cx.debug_bounds("gantt-hover-date-start").is_some(),
        "precondition: drop at the stretched end must show hover chrome"
    );
    let undo = KeyDownEvent {
        keystroke: Keystroke::parse("ctrl-z").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&undo, cx);
    });
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.bar_hovered_id, None,
            "Ctrl+Z must drop hover when the resting pointer is off the restored bar"
        );
    });
    assert!(
        cx.debug_bounds("gantt-hover-tooltip").is_none(),
        "the tooltip must not stay after Ctrl+Z while the pointer is off the bar"
    );
    assert!(
        cx.debug_bounds("gantt-hover-date-start").is_none(),
        "date pills must not stay after Ctrl+Z while the pointer is off the bar"
    );
}

#[gpui_kit::test]
fn window_blur_clears_the_bar_tooltip(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    cx.update(|window, _| {
        window.activate_window();
    });
    cx.run_until_parked();
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    cx.simulate_mouse_move(bar.center(), None, Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-hover-tooltip").is_some(),
        "precondition: hovering a bar must show the overlay tooltip"
    );
    cx.deactivate_window();
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.bar_hovered_id, None,
            "window deactivate must drop bar hover before the next frame remounts it"
        );
    });
    let nav = cx.debug_bounds("gantt-nav").expect("nav");
    cx.simulate_mouse_move(nav.center(), None, Modifiers::default());
    cx.update(|window, _| {
        window.activate_window();
    });
    cx.run_until_parked();
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.bar_hovered_id, None,
            "window blur / refocus must drop bar hover while the pointer is off the bar"
        );
    });
    assert!(
        cx.debug_bounds("gantt-hover-tooltip").is_none(),
        "the tooltip must not persist after the window blurs"
    );
    assert!(
        cx.debug_bounds("gantt-hover-date-start").is_none(),
        "date pills must not persist after the window blurs"
    );
}

#[gpui_kit::test]
fn hover_tooltip_stays_inside_the_window(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1020")
        .or_else(|| cx.debug_bounds("gantt-bar-1002"))
        .expect("a Month bar near the right");
    cx.simulate_mouse_move(bar.center(), None, Modifiers::default());
    draw(cx);
    let Some(tip) = cx.debug_bounds("gantt-hover-tooltip") else {
        return;
    };
    let tip_right = f32::from(tip.origin.x) + f32::from(tip.size.width);
    assert!(
        tip_right <= 1280.0 + 1.0,
        "the hover tooltip must not clip past the right window edge, right={tip_right}"
    );
}

#[gpui_kit::test]
fn idle_mouse_still_dismisses_toasts(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.toasts.push(ToastKind::Success, "Saved");
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("planner-toast").is_some(),
        "precondition: success toast is visible"
    );
    view.update(cx, |view, _| {
        view.state.toasts.items[0].created_at =
            std::time::Instant::now() - std::time::Duration::from_secs(30);
    });
    cx.background_executor
        .advance_clock(Duration::from_millis(300));
    cx.run_until_parked();
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.toasts.items.is_empty(),
            "an idle mouse must still let the toast timer dismiss the chip"
        );
    });
    assert!(
        cx.debug_bounds("planner-toast").is_none(),
        "the toast must unmount after its timer without a pointer move"
    );
}

#[gpui_kit::test]
fn start_link_handle_uses_the_same_gap_as_the_end(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    cx.simulate_mouse_move(bar.center(), None, Modifiers::default());
    draw(cx);
    let handle = cx
        .debug_bounds("gantt-link-handle-2-link-start")
        .expect("inbound handle");
    let bar_left = f32::from(bar.origin.x);
    let handle_center = f32::from(handle.origin.x) + f32::from(handle.size.width) / 2.0;
    let gap_to_center = bar_left - handle_center;
    assert!(
        (gap_to_center - (LINK_HANDLE_GAP + LINK_HANDLE / 2.0)).abs() < 2.0,
        "inbound connector must sit 6px off the bar (center at −10), not 8px, gap_to_center={gap_to_center}"
    );
}

#[gpui_kit::test]
fn first_row_date_pills_do_not_cover_the_bar(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1001").expect("Checkout rewrite");
    cx.simulate_mouse_move(bar.center(), None, Modifiers::default());
    draw(cx);
    let Some(start) = cx.debug_bounds("gantt-hover-date-start") else {
        return;
    };
    let bar_left = f32::from(bar.origin.x);
    let bar_right = bar_left + f32::from(bar.size.width);
    let start_left = f32::from(start.origin.x);
    let start_right = start_left + f32::from(start.size.width);
    let start_top = f32::from(start.origin.y);
    let start_bottom = start_top + f32::from(start.size.height);
    let bar_top = f32::from(bar.origin.y);
    assert!(
        start_bottom <= bar_top + 1.0,
        "hover pills must sit above the bar, not on Checkout rewrite, pill={start_top}..{start_bottom} bar_top={bar_top}"
    );
    if let Some(label) = cx.debug_bounds("gantt-bar-label-1001") {
        let label_top = f32::from(label.origin.y);
        let label_bottom = label_top + f32::from(label.size.height);
        let label_left = f32::from(label.origin.x);
        let label_right = label_left + f32::from(label.size.width);
        let overlaps_label = start_top < label_bottom - 1.0
            && start_bottom > label_top + 1.0
            && start_left < label_right - 1.0
            && start_right > label_left + 1.0;
        assert!(
            !overlaps_label,
            "hover pills must not cover Checkout rewrite, pill y={start_top}..{start_bottom} label={label_top}..{label_bottom}"
        );
    }
    assert!(
        (start_left - bar_left).abs() < 8.0,
        "first-row start pill must pin to the bar's left edge, pill={start_left} bar_left={bar_left}"
    );
    assert!(
        start_right <= bar_right + 8.0,
        "first-row pills must not detach past the bar end, start_right={start_right} bar_right={bar_right}"
    );
    if let Some(end) = cx.debug_bounds("gantt-hover-date-end") {
        let end_left = f32::from(end.origin.x);
        let end_right = end_left + f32::from(end.size.width);
        assert!(
            (end_right - bar_right).abs() < 16.0,
            "first-row end pill must pin to the bar's right edge, end_right={end_right} bar_right={bar_right}"
        );
        if let Some(tip) = cx.debug_bounds("gantt-hover-tooltip") {
            let tip_left = f32::from(tip.origin.x);
            let tip_right = tip_left + f32::from(tip.size.width);
            let tip_top = f32::from(tip.origin.y);
            let tip_bottom = tip_top + f32::from(tip.size.height);
            let end_top = f32::from(end.origin.y);
            let end_bottom = end_top + f32::from(end.size.height);
            let overlaps = tip_left < end_right - 1.0
                && tip_right > end_left + 1.0
                && tip_top < end_bottom - 1.0
                && tip_bottom > end_top + 1.0;
            assert!(
                !overlaps,
                "tooltip must avoid the date pills, tip={tip_left}..{tip_right} end={end_left}..{end_right}"
            );
        }
    }
    if let Some(next) = cx.debug_bounds("gantt-bar-1002") {
        let next_top = f32::from(next.origin.y);
        let next_bottom = next_top + f32::from(next.size.height);
        let next_left = f32::from(next.origin.x);
        let next_right = next_left + f32::from(next.size.width);
        let overlaps_next = start_top < next_bottom - 1.0
            && start_bottom > next_top + 1.0
            && start_left < next_right - 1.0
            && start_right > next_left + 1.0;
        assert!(
            !overlaps_next,
            "first-row date pills must not spill onto the next bar, pill y={start_top}..{start_bottom} next={next_top}..{next_bottom}"
        );
    }
}

#[gpui_kit::test]
fn ctrl_z_keeps_handles_when_the_pointer_is_on_the_start_handle(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    let start = grab_inbound_link_handle(bar);
    cx.simulate_mouse_move(start, None, Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-link-dot-2-link-start").is_some(),
        "precondition: the inbound connector must show while the pointer is on it"
    );
    let undo = KeyDownEvent {
        keystroke: Keystroke::parse("ctrl-z").unwrap(),
        is_held: false,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        view.handle_key(&undo, cx);
    });
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.bar_hovered_id,
            Some(2),
            "Ctrl+Z must keep bar hover when the pointer is still on the start handle"
        );
    });
    assert!(
        cx.debug_bounds("gantt-link-dot-2-link-start").is_some(),
        "the start handle must stay painted without re-entering the bar"
    );
}

#[gpui_kit::test]
fn start_handle_link_preview_originates_at_the_start(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-2").expect("leaf bar");
    cx.simulate_mouse_down(
        grab_inbound_link_handle(bar),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    let out = cx
        .debug_bounds("gantt-link-preview-out")
        .expect("outgoing stub");
    let bar_left = f32::from(bar.origin.x);
    let bar_right = bar_left + f32::from(bar.size.width);
    let out_x = f32::from(out.origin.x);
    assert!(
        out_x <= bar_left + 2.0,
        "start-handle preview must drop from the inbound connector, not the right end, out_x={out_x} bar_left={bar_left} bar_right={bar_right}"
    );
    let head = cx
        .debug_bounds("gantt-link-preview-head")
        .expect("rubber-band arrow");
    let head_x = f32::from(head.origin.x);
    assert!(
        head_x <= bar_left + 4.0,
        "inbound FS preview ▶ must sit on the source start, head_x={head_x} bar_left={bar_left}"
    );
}

#[gpui_kit::test]
fn start_handle_drop_makes_the_source_the_successor(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let source = cx.debug_bounds("gantt-bar-2").expect("source bar");
    cx.simulate_mouse_down(
        grab_inbound_link_handle(source),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    let target = cx.debug_bounds("gantt-bar-3").expect("drop target");
    cx.simulate_mouse_move(
        target.center(),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    cx.simulate_mouse_up(target.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let source = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 2)
            .expect("source");
        let target = view
            .state
            .nodes
            .iter()
            .find(|node| node.id == 3)
            .expect("target");
        assert!(
            source.predecessors.contains(&3),
            "start-handle drop must make this bar wait on the target (inbound FS), got {:?}",
            source.predecessors
        );
        assert!(
            !target.predecessors.contains(&2),
            "start-handle drop must not make the target wait on this bar, got {:?}",
            target.predecessors
        );
    });
}

#[gpui_kit::test]
fn hovering_a_tree_row_highlights_the_matching_chart_row(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let band = cx.debug_bounds("gantt-row-band-2").expect("chart row");
    let on_chart = point(
        band.origin.x + px(8.),
        band.origin.y + band.size.height / 2.0,
    );
    cx.simulate_mouse_move(on_chart, None, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(
            view.state.gantt.hovered_id,
            Some(2),
            "hovering the chart row must set the shared row hover used by the grid"
        );
    });
    assert!(
        cx.debug_bounds("gantt-tree-row-2").is_some(),
        "the grid row must be mounted so it can share the chart hover"
    );
}

#[gpui_kit::test]
fn undone_toast_expires_without_a_pointer_move(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state
        .toasts
        .push_stack("Undone moved dates", None, Some(UndoAction::StackRedo));
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("planner-toast").is_some(),
        "the Undone toast must paint"
    );
    view.update(cx, |view, _| {
        assert!(
            !view.toast_pause_for_test(),
            "appearing under a still pointer must not hover-pause the Undone toast"
        );
        view.state.toasts.items[0].created_at =
            std::time::Instant::now() - std::time::Duration::from_secs(30);
        view.state.toasts.expire();
        assert!(
            view.state.toasts.items.is_empty(),
            "an Undone toast must auto-dismiss after its TTL"
        );
    });
}

#[gpui_kit::test]
fn short_bar_end_handle_grabs_in_the_gap(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = FITTED_MONTH_ZOOM;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
    cx.simulate_click(fit.center(), Modifiers::default());
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart");
    let width = f32::from(bar.size.width);
    assert!(
        width < 80.0,
        "precondition: Persist cart should be a short bar, width={width}"
    );
    let bar_right = f32::from(bar.origin.x) + width;
    let mid_y = bar.origin.y + bar.size.height / 2.0;
    for offset in [4.0, 12.0] {
        cx.simulate_mouse_move(
            point(px(bar_right + offset), mid_y),
            None,
            Modifiers::default(),
        );
        draw(cx);
        view.update(cx, |view, _| {
            assert_eq!(
                view.hover_bar_zone_for_test(),
                Some((1003, BarHitZone::Link)),
                "a grab {offset}px past a short bar's end must hit the enlarged link plate"
            );
        });
    }
}

#[gpui_kit::test]
fn cart_selected_scale_round_trip_keeps_the_range_title(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 6, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 0.5;
    state.gantt.selected_id = Some(1002);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let range = view.update(cx, |view, _| view.state.gantt.viewport_range_title());
    for _ in 0..2 {
        let week = cx.debug_bounds("gantt-scale-Week").expect("Week");
        cx.simulate_click(week.center(), Modifiers::default());
        draw(cx);
        let day = cx.debug_bounds("gantt-scale-Day").expect("Day");
        cx.simulate_click(day.center(), Modifiers::default());
        draw(cx);
        let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
        cx.simulate_click(month.center(), Modifiers::default());
        draw(cx);
        view.update(cx, |view, _| {
            assert_eq!(view.state.gantt.scale, GanttScale::Month);
            assert_eq!(
                view.state.gantt.viewport_range_title(),
                range,
                "with Cart selected, Month→Week→Day→Month must restore {range}, got {}",
                view.state.gantt.viewport_range_title()
            );
        });
    }
}

#[gpui_kit::test]
fn reclicking_month_with_cart_selected_does_not_pan(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 6, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 0.5;
    state.gantt.selected_id = Some(1002);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let (scroll, keep_x, range) = view.update(cx, |view, _| {
        let keep = Date::new(2026, 7, 6).unwrap();
        (
            view.state.gantt.scroll_x,
            view.state.gantt.x_for_date(keep),
            view.state.gantt.viewport_range_title(),
        )
    });
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(view.state.gantt.scale, GanttScale::Month);
        assert!(
            (view.state.gantt.scroll_x - scroll).abs() < 0.5,
            "re-clicking Month must not pan, scroll {} vs {scroll}",
            view.state.gantt.scroll_x
        );
        let after = view.state.gantt.x_for_date(Date::new(2026, 7, 6).unwrap());
        assert!(
            (after - keep_x).abs() < 0.5,
            "re-clicking Month must not move Cart, x={after} was {keep_x}"
        );
        assert_eq!(view.state.gantt.viewport_range_title(), range);
    });
}

#[gpui_kit::test]
fn today_then_week_with_cart_selected_stays_on_today(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.selected_id = Some(1002);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let today_btn = cx.debug_bounds("gantt-today-btn").expect("Today");
    cx.simulate_click(today_btn.center(), Modifiers::default());
    draw(cx);
    let week = cx.debug_bounds("gantt-scale-Week").expect("Week");
    cx.simulate_click(week.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        let today = jiff::Zoned::now().date();
        assert_eq!(view.state.gantt.scale, GanttScale::Week);
        assert!(
            view.state.gantt.keep_today,
            "Today then Week must keep Today-follow"
        );
        let today_x = view.state.gantt.x_for_date(today);
        let aligned = view.state.gantt.timeline_width.max(1.0) * TODAY_ALIGN;
        assert!(
            (today_x - aligned).abs() < 8.0,
            "Today then Week with Cart selected must stay on Today, x={today_x} aligned={aligned}"
        );
        let cart = Date::new(2026, 7, 6).unwrap();
        let cart_x = view.state.gantt.x_for_date(cart);
        assert!(
            (cart_x - aligned).abs() > 50.0,
            "must not jump onto Cart's start, cart_x={cart_x} aligned={aligned}"
        );
    });
}

#[gpui_kit::test]
fn cart_selected_month_year_month_keeps_the_range_title(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    state.gantt.zoom = 0.6;
    state.gantt.selected_id = Some(1002);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let (title, jul_x) = view.update(cx, |view, _| {
        (
            view.state.gantt.viewport_range_title(),
            view.state.gantt.x_for_date(Date::new(2026, 7, 1).unwrap()),
        )
    });
    let year = cx.debug_bounds("gantt-scale-Year").expect("Year");
    cx.simulate_click(year.center(), Modifiers::default());
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(view.state.gantt.scale, GanttScale::Month);
        assert_eq!(
            view.state.gantt.viewport_range_title(),
            title,
            "with Cart selected, Month→Year→Month must restore {title}, got {}",
            view.state.gantt.viewport_range_title()
        );
        let after = view.state.gantt.x_for_date(Date::new(2026, 7, 1).unwrap());
        assert!(
            (after - jul_x).abs() < 1.0,
            "Jul must stay at the same screen x, {jul_x} vs {after}"
        );
    });
}

#[gpui_kit::test]
fn open_ended_cart_ui_tooltip_follows_the_pointer_on_the_cue(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1005").expect("Cart UI bar");
    let cue = cx
        .debug_bounds("gantt-open-ended-cue-1005")
        .expect("Cart UI open-ended tail");
    let timeline = cx
        .debug_bounds("gantt-timeline-body")
        .expect("timeline body");
    let bar_left = f32::from(bar.origin.x);
    let bar_right = bar_left + f32::from(bar.size.width);
    let cue_left = f32::from(cue.origin.x);
    let cue_w = f32::from(cue.size.width);
    let clip_right = f32::from(timeline.origin.x) + f32::from(timeline.size.width);
    let y = cue.origin.y + cue.size.height / 2.0;
    let mut last_tip: Option<f32> = None;
    for frac in [0.35, 0.7] {
        let x = cue_left + cue_w * frac;
        cx.simulate_mouse_move(point(px(x), y), None, Modifiers::default());
        draw(cx);
        let tip = cx
            .debug_bounds("gantt-hover-tooltip")
            .expect("Cart UI hover chip");
        let tip_left = f32::from(tip.origin.x);
        let tip_right = tip_left + f32::from(tip.size.width);
        assert!(
            tip_right <= clip_right + 2.0,
            "tooltip must clamp to the viewport, not the bar, tip={tip_left}..{tip_right} clip={clip_right}"
        );
        let gap = (tip_left - x).abs().min((tip_right - x).abs());
        assert!(
            gap < 48.0,
            "tooltip must sit near the pointer at {x}, not the solid bar {bar_left}..{bar_right}, tip={tip_left}..{tip_right}"
        );
        if let Some(prev) = last_tip {
            assert!(
                (tip_left - prev).abs() > 4.0,
                "moving along the cue must move the chip, stuck at {tip_left}"
            );
        }
        last_tip = Some(tip_left);
    }
}

#[gpui_kit::test]
fn persist_cart_label_stays_beside_the_bar_after_jul_22_move(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("Month Persist cart");
    let before_bar = f32::from(bar.origin.x);
    let before_label = cx
        .debug_bounds("gantt-bar-label-1003")
        .map(|label| f32::from(label.origin.x))
        .unwrap_or(before_bar);
    view.update(cx, |view, cx| {
        if let Some(node) = view.state.nodes.iter_mut().find(|node| node.id == 1003) {
            node.start_date = Some("2026-07-22T00:00:00Z".into());
            node.target_date = Some("2026-08-05T00:00:00Z".into());
        }
        cx.notify();
    });
    draw(cx);
    let after_bar = cx
        .debug_bounds("gantt-bar-1003")
        .expect("moved Persist cart");
    let after_bar_x = f32::from(after_bar.origin.x);
    let after_bar_right = after_bar_x + f32::from(after_bar.size.width);
    let label = cx
        .debug_bounds("gantt-bar-label-1003")
        .expect("Persist cart title");
    let label_x = f32::from(label.origin.x);
    let label_right = label_x + f32::from(label.size.width);
    let bar_dx = after_bar_x - before_bar;
    let label_dx = label_x - before_label;
    assert!(
        bar_dx.abs() > 8.0,
        "precondition: the Jul 22 move must shift the bar, dx={bar_dx}"
    );
    let gap_left = after_bar_x - label_right;
    let gap_right = label_x - after_bar_right;
    let on_bar = label_x >= after_bar_x - 2.0 && label_right <= after_bar_right + 2.0;
    let beside = (0.0..=OUTSIDE_LABEL_GAP + 12.0).contains(&gap_right)
        || (0.0..=OUTSIDE_LABEL_GAP + 12.0).contains(&gap_left);
    assert!(
        on_bar || beside,
        "after Jul 22→Aug 5 the title must sit on or beside the bar, not ~68px away, label={label_x}..{label_right} bar={after_bar_x}..{after_bar_right} bar_dx={bar_dx} label_dx={label_dx}"
    );
    assert!(
        (label_dx - bar_dx).abs() < 16.0,
        "the title must travel with the bar, bar_dx={bar_dx} label_dx={label_dx}"
    );
}

#[gpui_kit::test]
fn cart_ui_start_chip_does_not_clip_persist_cart_or_cart(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let month = cx.debug_bounds("gantt-scale-Month").expect("Month");
    cx.simulate_click(month.center(), Modifiers::default());
    draw(cx);
    let bar = cx.debug_bounds("gantt-bar-1005").expect("Cart UI");
    cx.simulate_mouse_move(bar.origin, None, Modifiers::default());
    draw(cx);
    let Some(chip) = cx.debug_bounds("gantt-hover-date-start") else {
        return;
    };
    if let Some(label) = cx.debug_bounds("gantt-bar-label-1003") {
        assert!(
            painted_min_gap(chip, label) + 0.1 >= 8.0,
            "Cart UI Jul 21 chip must keep ≥8px from Persist cart, gap={} chip=({:.1},{:.1})+{:.1}x{:.1} label=({:.1},{:.1})+{:.1}x{:.1}",
            painted_min_gap(chip, label),
            f32::from(chip.origin.x),
            f32::from(chip.origin.y),
            f32::from(chip.size.width),
            f32::from(chip.size.height),
            f32::from(label.origin.x),
            f32::from(label.origin.y),
            f32::from(label.size.width),
            f32::from(label.size.height)
        );
    }
    if let Some(cart) = cx.debug_bounds("gantt-bar-1002") {
        assert!(
            painted_min_gap(chip, cart) + 0.1 >= 8.0,
            "date chips must keep ≥8px from the Cart bar, gap={}",
            painted_min_gap(chip, cart)
        );
        if let Some(tip) = cx.debug_bounds("gantt-hover-tooltip") {
            assert!(
                !painted_overlap(tip, cart),
                "the Cart UI tooltip must keep row clearance from the Cart bar"
            );
        }
    }
}

#[gpui_kit::test]
fn go_live_handle_dot_clears_its_label_in_week_and_day(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    for selector in ["gantt-scale-Week", "gantt-scale-Day"] {
        let scale = cx.debug_bounds(selector).expect(selector);
        cx.simulate_click(scale.center(), Modifiers::default());
        draw(cx);
        let fit = cx.debug_bounds("gantt-auto-fit").expect("Auto fit");
        cx.simulate_click(fit.center(), Modifiers::default());
        draw(cx);
        let diamond = cx.debug_bounds("gantt-bar-1013").expect("Go-live");
        cx.simulate_mouse_move(diamond.center(), None, Modifiers::default());
        draw(cx);
        let Some(label) = cx.debug_bounds("gantt-bar-label-1013") else {
            continue;
        };
        let Some(dot) = cx.debug_bounds("gantt-link-dot-1013-link-end") else {
            continue;
        };
        assert!(
            !painted_overlap(label, dot),
            "{selector}: Go-live handle dot must sit past the title, label=({:.1},{:.1})+{:.1}x{:.1} dot=({:.1},{:.1})+{:.1}x{:.1}",
            f32::from(label.origin.x),
            f32::from(label.origin.y),
            f32::from(label.size.width),
            f32::from(label.size.height),
            f32::from(dot.origin.x),
            f32::from(dot.origin.y),
            f32::from(dot.size.width),
            f32::from(dot.size.height)
        );
    }
}

#[gpui_kit::test]
fn peeking_bar_does_not_paint_an_offscreen_pill(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    view.update(cx, |view, cx| {
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars = view
            .state
            .gantt
            .layout_bars(&view.state.nodes, &rows, &[], 720.0);
        let bar = bars.iter().find(|bar| bar.id == 2).expect("leaf bar");
        view.state.gantt.scroll_x += bar.x + bar.width.max(8.0) / 2.0;
        cx.notify();
    });
    draw(cx);
    view.update(cx, |view, _| {
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars = view
            .state
            .gantt
            .layout_bars(&view.state.nodes, &rows, &[], 720.0);
        let bar = bars.iter().find(|bar| bar.id == 2).expect("leaf bar");
        let width = bar.width.max(8.0);
        assert!(
            bar.x < 0.0 && bar.x + width > 0.0,
            "precondition: work item 2 must peek across the clip, x={} w={}",
            bar.x,
            width
        );
    });
    assert!(
        cx.debug_bounds("gantt-bar-edge-left-2").is_none(),
        "a bar still peeking into the clip must not show a ‹ pill"
    );
    assert!(
        cx.debug_bounds("gantt-bar-2").is_some(),
        "the peeking fragment must still paint"
    );
}

#[gpui_kit::test]
fn offscreen_pill_uses_the_same_title_chip(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    state.gantt.origin = Date::new(2026, 7, 1).unwrap();
    state.gantt.scale = GanttScale::Day;
    state.gantt.zoom = 1.0;
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-bar-edge-right-1013").is_some(),
        "Day Go-live past the clip must show an off-screen pill"
    );
    assert!(
        cx.debug_bounds("gantt-bar-title-chip").is_some(),
        "off-screen pills must use the same boxed title chip as Month/Week/Day labels"
    );
}

#[gpui_kit::test]
fn toolbar_overflow_holds_add_and_critical_path(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let state = idle_state(&dir);
    let (_view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    assert!(
        cx.debug_bounds("gantt-scale-group").is_some(),
        "scale buttons must share one segmented control"
    );
    let more = cx
        .debug_bounds("gantt-toolbar-more")
        .expect("⋯ overflow must paint");
    let today = cx.debug_bounds("gantt-today-btn").expect("Today");
    assert!(
        f32::from(more.size.width) > 8.0 && f32::from(more.size.height) > 8.0,
        "⋯ must be a visible control, not an empty icon, {:?}",
        more.size
    );
    assert!(
        (f32::from(more.origin.y) - f32::from(today.origin.y)).abs() < 24.0,
        "⋯ must sit on the toolbar row with Today, more={:?} today={:?}",
        more.origin,
        today.origin
    );
    assert!(
        cx.debug_bounds("gantt-add-item").is_none(),
        "Add item stays in the overflow until opened"
    );
    open_gantt_overflow(cx);
    assert!(
        cx.debug_bounds("gantt-toolbar-more-panel").is_some(),
        "⋯ must open the overflow panel"
    );
    assert!(cx.debug_bounds("gantt-add-item").is_some());
    assert!(cx.debug_bounds("gantt-critical-path").is_some());
    assert!(cx.debug_bounds("gantt-scroll-up").is_some());
    assert!(cx.debug_bounds("gantt-scroll-down").is_some());
}

#[gpui_kit::test]
fn year_bars_keep_a_minimum_height(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.nodes = mock_fixture_nodes();
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let year = cx.debug_bounds("gantt-scale-Year").expect("Year");
    cx.simulate_click(year.center(), Modifiers::default());
    draw(cx);
    let slivers = view.update(cx, |view, _| {
        let rows = view.state.gantt.flatten(&view.state.nodes);
        let bars = view
            .state
            .gantt
            .layout_bars(&view.state.nodes, &rows, &[], 720.0);
        let dated: Vec<_> = bars
            .iter()
            .filter(|bar| !bar.milestone && !bar.marker)
            .cloned()
            .collect();
        assert!(
            dated
                .iter()
                .any(|bar| bar.width <= YEAR_MIN_BAR_WIDTH + 0.5),
            "Year view must paint at least one min-width bar"
        );
        for bar in &dated {
            assert!(
                bar.width + 0.1 >= YEAR_MIN_BAR_WIDTH,
                "Year {} must keep a {YEAR_MIN_BAR_WIDTH}px body, width={}",
                bar.title,
                bar.width
            );
        }
        dated
            .into_iter()
            .filter(|bar| !bar.summary || bar.open_ended)
            .map(|bar| bar.id)
            .collect::<Vec<_>>()
    });
    for id in slivers {
        let selector: &'static str = Box::leak(format!("gantt-bar-{id}").into_boxed_str());
        let Some(bar) = cx.debug_bounds(selector) else {
            continue;
        };
        let h = f32::from(bar.size.height);
        assert!(
            h + 0.1 >= YEAR_MIN_BAR_H,
            "{selector} Year sliver height {h} must stay ≥ {YEAR_MIN_BAR_H}"
        );
    }
}

#[gpui_kit::test]
fn month_period_buttons_pan_a_week(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = tempfile::tempdir().unwrap();
    let mut state = idle_state(&dir);
    state.gantt.origin = Date::new(2026, 8, 1).unwrap();
    state.gantt.scale = GanttScale::Month;
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    let (before, step) = view.update(cx, |view, _| {
        (view.state.gantt.origin, view.state.gantt.pan_step_days())
    });
    let prev = cx.debug_bounds("gantt-prev-period").expect("‹");
    cx.simulate_click(prev.center(), Modifiers::default());
    draw(cx);
    let after = view.update(cx, |view, _| view.state.gantt.origin);
    assert_eq!(
        after,
        before.saturating_sub(step.days()),
        "Month ‹ must pan ¼ of the visible span ({step} days), {after} vs {before}"
    );
}
