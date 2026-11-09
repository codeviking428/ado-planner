//! Date cascade and Hierarchy Rollup against `ado-mock`: every trigger in
//! every Cascade mode, a failed write, one-step undo, and the setting across
//! a restart.
//!
//! The fixture's Dependency: #1003 (Jul 6 → Jul 20) precedes #1012
//! (Jul 21 → Aug 4). Its Hierarchy: Checkout rewrite (#1001) › Cart (#1002)
//! › #1003 › Cart UI (#1005, Start Date Jul 21 only).

use ado_core::{CascadeMode, START_DATE_FIELD, TARGET_DATE_FIELD};
use ado_planner::gantt::{GanttView, GestureKind, NudgeAxis};
use ado_planner::paths::AppPaths;
use ado_planner::state::PlannerState;
use ado_planner::toasts::{HistoryEntry, UndoAction};
use ado_session::{EncryptedStore, MemoryStore, PlannerSession};
use serde_json::{Value, json};

const PRED: i64 = 1003;
const SUCC: i64 = 1012;
/// #1003's parent, Jul 6 → Aug 14.
const CART: i64 = 1002;
/// #1002's parent, Jul 1 → Sep 30.
const EPIC: i64 = 1001;

async fn signed_in(mode: CascadeMode) -> (ado_mock::AdoMock, PlannerState, tempfile::TempDir) {
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
    state.set_cascade_mode(mode);
    let succ = node(&state, SUCC);
    assert_eq!(succ.predecessors, vec![PRED], "fixture link");
    (mock, state, dir)
}

fn node(state: &PlannerState, id: i64) -> ado_core::WorkItemNode {
    state
        .nodes
        .iter()
        .find(|node| node.id == id)
        .cloned()
        .unwrap_or_else(|| panic!("#{id} loaded"))
}

/// `(start, target)` as `MM-DD` in 2026, or empty.
fn local(state: &PlannerState, id: i64) -> (String, String) {
    let node = node(state, id);
    (
        short(node.start_date.as_deref()),
        short(node.target_date.as_deref()),
    )
}

async fn server(mock: &ado_mock::AdoMock, id: i64) -> (String, String) {
    let item = mock.work_item(id).await.expect("server item");
    let field = |name: &str| short(item["fields"].get(name).and_then(Value::as_str));
    (field(START_DATE_FIELD), field(TARGET_DATE_FIELD))
}

fn short(date: Option<&str>) -> String {
    date.map(|date| date[5..10].to_string()).unwrap_or_default()
}

fn dates(start: &str, target: &str) -> (String, String) {
    (start.to_string(), target.to_string())
}

fn last_toast(state: &PlannerState) -> String {
    state.toasts.last_message().unwrap_or_default().to_string()
}

fn begin(state: &mut PlannerState, id: i64, kind: GestureKind, days: f32) {
    let step = state.gantt.scale.pixels_per_day(state.gantt.zoom);
    state.begin_bar_gesture(id, kind, 0.0).expect("press");
    state.update_bar_gesture(step * days);
}

async fn drag(state: &mut PlannerState, id: i64, kind: GestureKind, days: f32) {
    begin(state, id, kind, days);
    state.commit_gesture().await.expect("drop");
}

async fn nudge(state: &mut PlannerState, id: i64, days: i32) {
    state.gantt.selected_id = Some(id);
    for _ in 0..days {
        state.nudge(NudgeAxis::Both, 1).expect("nudge");
    }
    state.flush_nudge().await.expect("flush");
}

async fn form_save(state: &mut PlannerState, id: i64, start: &str, target: &str) {
    state.open_form(id).await.expect("open");
    let form = state.form.as_mut().expect("form");
    form.values.insert(
        START_DATE_FIELD.into(),
        json!(format!("2026-{start}T00:00:00Z")),
    );
    form.values.insert(
        TARGET_DATE_FIELD.into(),
        json!(format!("2026-{target}T00:00:00Z")),
    );
    state.save_form().await.expect("save");
}

/// #1003 Target Date moves Jul 20 → Jul 25.
async fn each_trigger(mode: CascadeMode, expected: (&str, &str), toast_suffix: Option<&str>) {
    for trigger in ["drag", "resize", "nudge", "form"] {
        let (mock, mut state, _dir) = signed_in(mode).await;
        match trigger {
            "drag" => drag(&mut state, PRED, GestureKind::Move, 5.0).await,
            "resize" => drag(&mut state, PRED, GestureKind::ResizeEnd, 5.0).await,
            "nudge" => nudge(&mut state, PRED, 5).await,
            _ => form_save(&mut state, PRED, "07-11", "07-25").await,
        }
        assert_eq!(server(&mock, PRED).await.1, "07-25", "{trigger} {mode:?}");
        assert_eq!(
            server(&mock, CART).await.1,
            "07-25",
            "{trigger} {mode:?}: Cart rolls up to its latest child"
        );
        let want = dates(expected.0, expected.1);
        assert_eq!(local(&state, SUCC), want, "{trigger} {mode:?} live");
        assert_eq!(server(&mock, SUCC).await, want, "{trigger} {mode:?} server");
        let toast = last_toast(&state);
        match toast_suffix {
            Some(suffix) => assert!(toast.ends_with(suffix), "{trigger} {mode:?}: {toast}"),
            None => assert!(!toast.contains("successor"), "{trigger} {mode:?}: {toast}"),
        }
    }
}

#[tokio::test]
async fn date_changes_cascade_in_flexible() {
    each_trigger(
        CascadeMode::Flexible,
        ("07-25", "08-08"),
        Some(" · 1 successor moved"),
    )
    .await;
}

#[tokio::test]
async fn date_changes_cascade_in_strict() {
    each_trigger(
        CascadeMode::Strict,
        ("07-26", "08-09"),
        Some(" · 1 successor moved"),
    )
    .await;
}

#[tokio::test]
async fn date_changes_leave_successors_in_no_action() {
    each_trigger(CascadeMode::NoAction, ("07-21", "08-04"), None).await;
}

#[tokio::test]
async fn strict_pulls_successors_earlier_and_flexible_does_not() {
    for (mode, expected) in [
        (CascadeMode::Strict, ("07-18", "08-01")),
        (CascadeMode::Flexible, ("07-21", "08-04")),
    ] {
        let (mock, mut state, _dir) = signed_in(mode).await;
        drag(&mut state, PRED, GestureKind::Move, -3.0).await;
        let want = dates(expected.0, expected.1);
        assert_eq!(local(&state, SUCC), want, "{mode:?}");
        assert_eq!(server(&mock, SUCC).await, want, "{mode:?}");
    }
}

/// #1012 (Jul 21 → Aug 4) becomes the predecessor of #1005, a Task with
/// only a Start Date (Jul 21).
#[tokio::test]
async fn a_new_dependency_pushes_in_flexible_and_strict_only() {
    for (mode, expected) in [
        (CascadeMode::Flexible, "08-04"),
        (CascadeMode::Strict, "08-04"),
        (CascadeMode::NoAction, "07-21"),
    ] {
        let (mock, mut state, _dir) = signed_in(mode).await;
        state.add_dependency(1005, SUCC).await.expect("link");
        assert_eq!(local(&state, 1005), dates(expected, ""), "{mode:?} live");
        assert_eq!(server(&mock, 1005).await, dates(expected, ""), "{mode:?}");
        let toast = last_toast(&state);
        assert_eq!(
            toast.ends_with(" · 1 successor moved"),
            mode != CascadeMode::NoAction,
            "{mode:?}: {toast}"
        );
    }
}

#[tokio::test]
async fn deleting_a_dependency_never_cascades() {
    let (mock, mut state, _dir) = signed_in(CascadeMode::Strict).await;
    let writes = mock.mutating_writes().len();
    state.remove_dependency(SUCC, PRED).await.expect("unlink");
    assert_eq!(
        mock.mutating_writes().len(),
        writes + 1,
        "only the link PATCH"
    );
    assert_eq!(local(&state, SUCC), dates("07-21", "08-04"));
}

#[tokio::test]
async fn loading_never_cascades() {
    let (mock, _state, _dir) = signed_in(CascadeMode::Flexible).await;
    assert!(mock.mutating_writes().is_empty());
}

#[tokio::test]
async fn shift_drag_descendants_are_changes_planned_once() {
    let (mock, mut state, _dir) = signed_in(CascadeMode::Strict).await;
    let step = state.gantt.scale.pixels_per_day(state.gantt.zoom);
    state
        .begin_bar_gesture(1002, GestureKind::Move, 0.0)
        .expect("press #1002");
    state.update_bar_gesture(step * 2.0);
    let mut pending = state.commit_date_gesture_now().expect("move #1002");
    pending.cascade = state.shift_children_with_committed_move(&pending);
    assert!(pending.cascade.iter().any(|save| save.id == PRED));
    state.cascade_committed(&mut pending);
    let moved: Vec<i64> = pending.cascade.iter().map(|save| save.id).collect();
    assert_eq!(
        moved.iter().filter(|&&id| id == SUCC).count(),
        1,
        "#1012 follows its predecessor #1003, a Shift-drag child, once: {moved:?}"
    );
    assert_eq!(local(&state, SUCC), dates("07-23", "08-06"));
    let kids = pending.cascade.clone();
    state.persist_pending_dates(pending).await.expect("parent");
    for kid in kids {
        state.persist_pending_dates(kid).await.expect("kid");
    }
    assert_eq!(server(&mock, SUCC).await, dates("07-23", "08-06"));
    assert!(
        matches!(state.history.undo.as_slice(), [HistoryEntry::DatesGroup { items }] if items.len() >= 3),
        "one undo entry, got {:?}",
        state.history.undo
    );
}

#[tokio::test]
async fn flexible_snaps_a_drop_into_overlap_later() {
    let (mock, mut state, _dir) = signed_in(CascadeMode::Flexible).await;
    drag(&mut state, SUCC, GestureKind::Move, -3.0).await;
    assert_eq!(
        local(&state, SUCC),
        dates("07-20", "08-03"),
        "#1012 dropped on Jul 18 snaps to #1003's Jul 20 and keeps 15 days"
    );
    assert_eq!(server(&mock, SUCC).await, dates("07-20", "08-03"));

    let (mock, mut state, _dir) = signed_in(CascadeMode::Flexible).await;
    drag(&mut state, SUCC, GestureKind::ResizeStart, -4.0).await;
    assert_eq!(
        server(&mock, SUCC).await,
        dates("07-20", "08-04"),
        "a start-edge resize snaps Start Date only"
    );

    for mode in [CascadeMode::Strict, CascadeMode::NoAction] {
        let (mock, mut state, _dir) = signed_in(mode).await;
        drag(&mut state, SUCC, GestureKind::Move, -3.0).await;
        assert_eq!(
            server(&mock, SUCC).await,
            dates("07-18", "08-01"),
            "{mode:?} leaves the overlap"
        );
    }
}

#[tokio::test]
async fn drag_preview_plans_the_cascade_and_counts_moves() {
    // Cart (#1002) and Checkout rewrite (#1001) roll up in every mode.
    for (mode, moves, clash) in [
        (CascadeMode::Flexible, " · moves 3", false),
        (CascadeMode::Strict, " · moves 3", false),
        (CascadeMode::NoAction, " · moves 2", true),
    ] {
        let (mock, mut state, _dir) = signed_in(mode).await;
        begin(&mut state, PRED, GestureKind::Move, 5.0);
        let gesture = state.gantt.gesture.clone().expect("dragging");
        let chip = GanttView::drag_preview_text(&gesture);
        assert!(chip.ends_with(moves), "{mode:?}: {chip}");
        let visible = state.visible();
        let bars = visible.layout_bars(&state, 10_000.0);
        let marks = visible.clash_marks(&state, &bars);
        assert_eq!(
            marks.links.contains(&(PRED, SUCC)),
            clash,
            "{mode:?} paints what the drop leaves"
        );
        assert_eq!(
            local(&state, SUCC),
            dates("07-21", "08-04"),
            "moves on drop"
        );
        state.cancel_gesture();
        assert!(mock.mutating_writes().is_empty());
    }

    let (_mock, mut state, _dir) = signed_in(CascadeMode::Flexible).await;
    begin(&mut state, SUCC, GestureKind::Move, -3.0);
    let gesture = state.gantt.gesture.clone().expect("dragging");
    let chip = GanttView::drag_preview_text(&gesture);
    assert!(
        chip.starts_with("Jul 20 → Aug 3"),
        "the chip shows where the snap lands: {chip}"
    );
    let visible = state.visible();
    let bars = visible.layout_bars(&state, 10_000.0);
    let bar = bars.iter().find(|bar| bar.id == SUCC).expect("#1012 bar");
    let snapped_x = state
        .gantt
        .x_for_date(jiff::civil::Date::new(2026, 7, 20).unwrap());
    assert!(
        (bar.x - snapped_x).abs() < 1.0,
        "the bar paints at the snapped Jul 20, x={} want {snapped_x}",
        bar.x
    );
    assert!(
        !visible
            .clash_marks(&state, &bars)
            .links
            .contains(&(PRED, SUCC)),
        "a Flexible drop into overlap snaps, so it leaves no Clash"
    );
}

#[tokio::test]
async fn a_failed_cascade_write_reverts_that_row_and_badges_it() {
    let (mock, mut state, _dir) = signed_in(CascadeMode::Flexible).await;
    mock.fail_next(&format!("/workitems/{SUCC}"), 400, "TF401320: Rule error");
    drag(&mut state, PRED, GestureKind::Move, 5.0).await;
    assert_eq!(
        server(&mock, PRED).await.1,
        "07-25",
        "the user's change stands"
    );
    assert_eq!(local(&state, SUCC), dates("07-21", "08-04"), "row reverted");
    assert_eq!(server(&mock, SUCC).await, dates("07-21", "08-04"));
    let visible = state.visible();
    let bars = visible.layout_bars(&state, 10_000.0);
    let badge = visible
        .clash_marks(&state, &bars)
        .badges
        .remove(&SUCC)
        .expect("badge on #1012");
    assert!(
        badge
            .lines()
            .contains(&"Couldn't move: TF401320: Rule error".to_string()),
        "{:?}",
        badge.lines()
    );
    assert!(
        badge
            .lines()
            .iter()
            .any(|line| line.starts_with("Starts Jul 21")),
        "the Clash it left shows too: {:?}",
        badge.lines()
    );
    match state.history.undo.as_slice() {
        [HistoryEntry::DatesGroup { items }] => {
            assert_eq!(
                items.iter().map(|item| item.id).collect::<Vec<_>>(),
                vec![PRED, CART, EPIC],
                "the failed #1012 leaves the entry; the Rollups stay"
            );
        }
        [HistoryEntry::Dates { id, .. }] => assert_eq!(*id, PRED),
        other => panic!("one undo entry for #1003 and its Rollups, got {other:?}"),
    }

    nudge(&mut state, SUCC, 1).await;
    assert!(
        !state.gantt.move_failures.contains_key(&SUCC),
        "the next good write clears the badge"
    );
}

#[tokio::test]
async fn a_cascade_write_retries_server_errors() {
    let (mock, mut state, _dir) = signed_in(CascadeMode::Flexible).await;
    mock.fail_next(&format!("/workitems/{SUCC}"), 503, "busy");
    drag(&mut state, PRED, GestureKind::Move, 5.0).await;
    assert_eq!(server(&mock, SUCC).await, dates("07-25", "08-08"));
    assert!(state.gantt.move_failures.is_empty());
}

#[tokio::test]
async fn a_change_and_its_cascade_undo_and_redo_in_one_step() {
    let (mock, mut state, _dir) = signed_in(CascadeMode::Flexible).await;
    drag(&mut state, PRED, GestureKind::Move, 5.0).await;
    assert_eq!(state.history.undo.len(), 1);
    state.undo().await.expect("undo");
    assert_eq!(server(&mock, PRED).await, dates("07-06", "07-20"));
    assert_eq!(server(&mock, SUCC).await, dates("07-21", "08-04"));
    assert!(state.history.undo.is_empty());
    assert!(
        last_toast(&state).ends_with(" · 1 successor moved"),
        "{}",
        last_toast(&state)
    );
    state.redo().await.expect("redo");
    assert_eq!(server(&mock, SUCC).await, dates("07-25", "08-08"));
}

#[tokio::test]
async fn toast_undo_reverts_the_change_and_its_cascade() {
    let (mock, mut state, _dir) = signed_in(CascadeMode::Flexible).await;
    drag(&mut state, PRED, GestureKind::Move, 5.0).await;
    let undo = state
        .toasts
        .items
        .last()
        .and_then(|toast| toast.undo.clone())
        .expect("toast Undo");
    assert!(
        matches!(&undo, UndoAction::RestoreDatesBatch { items } if items.len() == 4),
        "{undo:?}"
    );
    for save in state.toast_restore_or_undo_dates(undo) {
        state.persist_pending_dates(save).await.expect("restore");
    }
    assert_eq!(server(&mock, PRED).await, dates("07-06", "07-20"));
    assert_eq!(server(&mock, SUCC).await, dates("07-21", "08-04"));
    assert_eq!(server(&mock, CART).await, dates("07-06", "08-14"));
}

#[tokio::test]
async fn a_new_dependency_and_its_cascade_undo_in_one_step() {
    let (mock, mut state, _dir) = signed_in(CascadeMode::Flexible).await;
    state.add_dependency(1005, SUCC).await.expect("link");
    match state.history.undo.as_slice() {
        [HistoryEntry::Link { dates, .. }] => assert_eq!(
            dates.iter().map(|change| change.id).collect::<Vec<_>>(),
            vec![1005, CART, EPIC],
            "#1005 moves, then Cart and Checkout rewrite roll up"
        ),
        other => panic!("one Link entry, got {other:?}"),
    }
    assert_eq!(server(&mock, CART).await, dates("07-06", "07-20"));
    state.undo().await.expect("undo");
    assert_eq!(server(&mock, 1005).await, dates("07-21", ""));
    assert_eq!(server(&mock, CART).await, dates("07-06", "08-14"));
    assert!(!node(&state, 1005).predecessors.contains(&SUCC));
    state.redo().await.expect("redo");
    assert_eq!(server(&mock, 1005).await, dates("08-04", ""));
    assert!(node(&state, 1005).predecessors.contains(&SUCC));

    let (mock, mut state, _dir) = signed_in(CascadeMode::Flexible).await;
    state.add_dependency(1005, SUCC).await.expect("link");
    let undo = state
        .toasts
        .items
        .last()
        .and_then(|toast| toast.undo.clone())
        .expect("toast Undo");
    state.apply_undo(undo).await.expect("toast undo");
    assert_eq!(server(&mock, 1005).await, dates("07-21", ""));
}

#[tokio::test]
async fn a_failed_link_reverts_its_cascade() {
    let (mock, mut state, _dir) = signed_in(CascadeMode::Flexible).await;
    mock.fail_next("/workitems/1005", 400, "link refused");
    let _ = state.add_dependency(1005, SUCC).await;
    assert_eq!(local(&state, 1005), dates("07-21", ""));
    assert_eq!(server(&mock, 1005).await, dates("07-21", ""));
    assert!(state.history.undo.is_empty());
}

#[tokio::test]
async fn cascade_mode_survives_a_restart() {
    let (_mock, mut state, dir) = signed_in(CascadeMode::Flexible).await;
    state.set_cascade_mode(CascadeMode::NoAction);
    let restarted = PlannerState::new(
        AppPaths::new(dir.path().to_path_buf()),
        PlannerSession::pat(EncryptedStore::new(Box::new(MemoryStore::new(false)), None)),
        None,
        None,
    );
    assert_eq!(restarted.cascade_mode(), CascadeMode::NoAction);
    let raw = std::fs::read_to_string(state.paths.prefs_file()).unwrap();
    assert!(raw.contains(r#""cascadeMode": "no_action""#), "{raw}");
}

fn writes_to(mock: &ado_mock::AdoMock, id: i64) -> usize {
    let path = format!("/workitems/{id}");
    mock.mutating_writes()
        .iter()
        .filter(|(_, written)| written.ends_with(&path) || written.contains(&format!("{path}?")))
        .count()
}

/// Go-live milestone (#1013, a Milestone on Aug 15 under Checkout rewrite)
/// becomes Cart's successor. It already starts after Cart's Aug 14, so the
/// link moves nothing, and the setup leaves no undo entry or toast.
async fn with_cart_successor(
    mode: CascadeMode,
) -> (ado_mock::AdoMock, PlannerState, tempfile::TempDir) {
    let (mock, mut state, dir) = signed_in(mode).await;
    state.add_dependency(GO_LIVE, CART).await.expect("link");
    assert_eq!(server(&mock, GO_LIVE).await, dates("", "08-15"));
    state.history.undo.clear();
    state.history.redo.clear();
    state.toasts.clear();
    (mock, state, dir)
}

const GO_LIVE: i64 = 1013;

/// #1003 moves 30 days to Aug 5 → Aug 19. Cart rolls up to Jul 21 (Cart UI's
/// Start Date) → Aug 19, so its successor Go-live milestone moves. Under
/// Strict that move would roll Checkout rewrite up a second time; the walk
/// ends there instead.
#[tokio::test]
async fn a_child_drag_writes_the_parents_rollup_and_moves_its_successor() {
    for (mode, successor, go_live) in [
        (CascadeMode::Flexible, ("08-19", "09-02"), "08-19"),
        (CascadeMode::Strict, ("08-20", "09-03"), "08-20"),
        (CascadeMode::NoAction, ("07-21", "08-04"), "08-15"),
    ] {
        let (mock, mut state, _dir) = with_cart_successor(mode).await;
        drag(&mut state, PRED, GestureKind::Move, 30.0).await;
        assert_eq!(server(&mock, PRED).await, dates("08-05", "08-19"));
        assert_eq!(
            server(&mock, CART).await,
            dates("07-21", "08-19"),
            "{mode:?}: Cart's Rollup"
        );
        assert_eq!(
            server(&mock, EPIC).await,
            dates("07-21", "08-19"),
            "{mode:?}: Checkout rewrite's Rollup"
        );
        assert_eq!(
            server(&mock, SUCC).await,
            dates(successor.0, successor.1),
            "{mode:?}"
        );
        assert_eq!(
            server(&mock, GO_LIVE).await,
            dates("", go_live),
            "{mode:?}: Cart's successor"
        );
        for id in [PRED, CART, EPIC, SUCC, GO_LIVE] {
            assert_eq!(local(&state, id), server(&mock, id).await, "{mode:?} #{id}");
        }
        assert_eq!(writes_to(&mock, CART), 1, "{mode:?}: one Rollup write");
        let toast = last_toast(&state);
        let suffix = match mode {
            CascadeMode::NoAction => " · 2 parents rolled up",
            _ => " · 2 parents rolled up · 2 successors moved",
        };
        assert!(toast.ends_with(suffix), "{mode:?}: {toast}");
    }
}

#[tokio::test]
async fn one_undo_restores_the_child_its_rollup_and_the_parents_successor() {
    let (mock, mut state, _dir) = with_cart_successor(CascadeMode::Flexible).await;
    drag(&mut state, PRED, GestureKind::Move, 30.0).await;
    match state.history.undo.as_slice() {
        [HistoryEntry::DatesGroup { items }] => assert_eq!(
            items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![PRED, SUCC, CART, EPIC, GO_LIVE]
        ),
        other => panic!("one undo entry, got {other:?}"),
    }
    state.undo().await.expect("undo");
    let loaded = [
        (PRED, dates("07-06", "07-20")),
        (CART, dates("07-06", "08-14")),
        (EPIC, dates("07-01", "09-30")),
        (SUCC, dates("07-21", "08-04")),
        (GO_LIVE, dates("", "08-15")),
    ];
    for (id, want) in &loaded {
        assert_eq!(&server(&mock, *id).await, want, "#{id} on the server");
        assert_eq!(&local(&state, *id), want, "#{id} live");
    }
    assert!(state.history.undo.is_empty());
    let toast = last_toast(&state);
    assert!(
        toast.starts_with("Undone")
            && toast.ends_with(" · 2 parents rolled up · 2 successors moved"),
        "{toast}"
    );

    state.redo().await.expect("redo");
    assert_eq!(server(&mock, CART).await, dates("07-21", "08-19"));
    assert_eq!(server(&mock, GO_LIVE).await, dates("", "08-19"));
}

#[tokio::test]
async fn toast_undo_restores_the_rollup_and_the_parents_successor() {
    let (mock, mut state, _dir) = with_cart_successor(CascadeMode::Flexible).await;
    drag(&mut state, PRED, GestureKind::Move, 30.0).await;
    let undo = state
        .toasts
        .items
        .last()
        .and_then(|toast| toast.undo.clone())
        .expect("toast Undo");
    for save in state.toast_restore_or_undo_dates(undo) {
        state.persist_pending_dates(save).await.expect("restore");
    }
    assert_eq!(server(&mock, PRED).await, dates("07-06", "07-20"));
    assert_eq!(server(&mock, CART).await, dates("07-06", "08-14"));
    assert_eq!(server(&mock, GO_LIVE).await, dates("", "08-15"));
}

#[tokio::test]
async fn moving_the_parent_itself_writes_no_rollup_for_it() {
    let (mock, mut state, _dir) = signed_in(CascadeMode::Flexible).await;
    drag(&mut state, CART, GestureKind::Move, 2.0).await;
    assert_eq!(
        server(&mock, CART).await,
        dates("07-08", "08-16"),
        "the user's dates stand, not its children's span"
    );
    assert_eq!(writes_to(&mock, CART), 1, "only the user's write");
    assert_eq!(server(&mock, PRED).await, dates("07-06", "07-20"));
    assert_eq!(
        server(&mock, EPIC).await,
        dates("07-06", "08-16"),
        "Cart's own parent still rolls up"
    );
}

#[tokio::test]
async fn loading_writes_no_rollup() {
    let (mock, state, _dir) = signed_in(CascadeMode::Flexible).await;
    assert!(mock.mutating_writes().is_empty());
    assert_eq!(
        local(&state, CART),
        dates("07-06", "08-14"),
        "Cart keeps its loaded dates though its children span less"
    );
}

#[tokio::test]
async fn a_failed_rollup_write_reverts_that_row_and_badges_it() {
    let (mock, mut state, _dir) = signed_in(CascadeMode::Flexible).await;
    mock.fail_next(&format!("/workitems/{CART}"), 400, "TF401320: Rule error");
    drag(&mut state, PRED, GestureKind::Move, 5.0).await;
    assert_eq!(
        server(&mock, PRED).await.1,
        "07-25",
        "the user's change stands"
    );
    assert_eq!(
        server(&mock, SUCC).await,
        dates("07-25", "08-08"),
        "the cascade stands"
    );
    assert_eq!(local(&state, CART), dates("07-06", "08-14"), "row reverted");
    assert_eq!(server(&mock, CART).await, dates("07-06", "08-14"));
    assert_eq!(
        state.gantt.move_failures.get(&CART),
        Some(&vec!["TF401320: Rule error".to_string()])
    );
}

#[tokio::test]
async fn a_rollup_write_retries_server_errors() {
    let (mock, mut state, _dir) = signed_in(CascadeMode::Flexible).await;
    mock.fail_next(&format!("/workitems/{CART}"), 503, "busy");
    drag(&mut state, PRED, GestureKind::Move, 5.0).await;
    assert_eq!(server(&mock, CART).await, dates("07-11", "07-25"));
    assert!(state.gantt.move_failures.is_empty());
}
