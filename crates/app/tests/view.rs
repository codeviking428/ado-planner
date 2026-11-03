//! Rendered `PlannerView` coverage for popups, the Gantt header, and the form.

use ado_core::{IdentityValue, SessionInfo, WorkItemNode};
use ado_planner::form::FormDraft;
use ado_planner::gantt::{
    BarHitZone, ContextTarget, GanttScale, GanttView, GestureKind, HANDLE_WIDTH,
    MIN_TIMELINE_WIDTH, ROW_HEIGHT, TODAY_ALIGN, TREE_INDENT, insert_index_for_child,
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
        cx.debug_bounds("unscheduled-empty-hint").is_none(),
        "placeholder bars replace the empty-timeline drop hint"
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
    assert!(
        cx.debug_bounds("gantt-menu-Add child").is_some(),
        "right-click must offer Add child"
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
    assert!(
        cx.debug_bounds("gantt-today").is_some(),
        "today marker should paint"
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
    cx.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
    let start = handle.center();
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
    cx.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
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
    cx.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
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
    cx.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
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
    cx.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
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
    cx.simulate_mouse_down(handle.center(), MouseButton::Left, Modifiers::default());
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
    let row = cx.debug_bounds("gantt-tree-row-1").expect("tree row");
    cx.simulate_click(row.center(), Modifiers::default());
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
            (after - jul_x).abs() < 2.0,
            "Jul must stay at the same screen x, {jul_x} vs {after}"
        );
    });
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
        (f32::from(resize.size.width) - HANDLE_WIDTH).abs() < 1.0,
        "end resize grip must be the inner 8px of the bar, got {}",
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
        handle_right <= bar_right + 0.5 && handle_right + 4.0 >= bar_right,
        "end resize grip must stay inside the painted bar, handle right {handle_right} vs bar right {bar_right}"
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
    assert!(
        cx.debug_bounds("gantt-summary-edge-1-end").is_some(),
        "a parent summary bar must show a not-allowed edge affordance"
    );
    assert!(
        cx.debug_bounds("gantt-summary-edge-1-start").is_some(),
        "a parent summary bar must show a not-allowed start-edge affordance"
    );
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
        bar.origin.x + bar.size.width - px(3.),
        bar.origin.y + bar.size.height / 2.0,
    );
    cx.simulate_mouse_down(edge, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "a summary edge must not start a move, got {:?}",
            view.state.gantt.gesture.as_ref().map(|g| g.kind)
        );
        let hit = view
            .last_bar_hit_for_test()
            .expect("summary edge must record a window-coord hit");
        assert_eq!(
            hit.zone,
            BarHitZone::RollupEdge,
            "3px inside a summary end must be rollup-edge, got {}",
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
fn selected_summary_edge_overlay_does_not_start_a_move(cx: &mut gpui_kit::TestAppContext) {
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
    // Inner 1px of the 8px start overlay: the 2px selection border would
    // reclassify this as Move if overlay mouse-down went through hit_zone.
    let inner = point(bar.origin.x + px(7.), bar.origin.y + bar.size.height / 2.0);
    cx.simulate_mouse_down(inner, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.update(cx, |view, _| {
        assert!(
            view.state.gantt.gesture.is_none(),
            "selected summary-edge overlay must not start a move, got {:?}",
            view.state.gantt.gesture.as_ref().map(|g| g.kind)
        );
        let hit = view
            .last_bar_hit_for_test()
            .expect("overlay press must record a rollup-edge hit");
        assert_eq!(
            hit.zone,
            BarHitZone::RollupEdge,
            "forced overlay press must stay rollup-edge, got {}",
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
        bar.origin.x + bar.size.width - px(3.),
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
        bar.origin.x + bar.size.width - px(3.),
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
        bar.origin.x + bar.size.width - px(3.),
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
        bar.origin.x + bar.size.width - px(inset)
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
                .any(|toast| matches!(toast.undo, Some(UndoAction::StackUndo))),
            "Undo must stay while history remains, got {:?}",
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
            "the undone step must also offer Redo"
        );
    });
    assert!(
        cx.debug_bounds("toast-undo").is_some(),
        "Undo must still paint while history remains"
    );
    assert!(
        cx.debug_bounds("toast-redo").is_some(),
        "Redo must paint after the first Undo"
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
fn double_click_title_starts_inline_rename(cx: &mut gpui_kit::TestAppContext) {
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
        cx.debug_bounds("gantt-inline-title").is_some(),
        "double-click on a title must start inline rename, matching the tooltip"
    );
    view.update(cx, |view, _| {
        assert!(
            view.state.form.is_none(),
            "double-click on a title must not open details"
        );
        assert_eq!(view.state.gantt.selected_id, Some(1));
    });
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
