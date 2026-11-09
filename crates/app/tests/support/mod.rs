//! Shared harness for `gpui_kit::test` scenarios that drive `PlannerView`
//! against `ado-mock`.
//!
//! Mock replies land on the `ado-client` runtime thread. The GPUI test
//! scheduler records any wake from another thread as "Your test is not
//! deterministic" unless parking was allowed first, so [`open`] allows parking
//! before the view exists and nothing can spawn I/O ahead of it. Time only
//! moves through `advance_clock` (or real parking in [`settle`]), so timers
//! such as the Nudge idle flush stay exact.
#![allow(dead_code)]

use ado_planner::paths::AppPaths;
use ado_planner::state::PlannerState;
use ado_planner::ui::PlannerView;
use ado_session::{EncryptedStore, MemoryStore, PlannerSession};
use gpui_kit::{Entity, Modifiers, MouseButton, VisualTestContext, px};
use jiff::civil::Date;
use std::path::Path;
use std::time::{Duration, Instant};

pub const PAT: &str = "pat-secret-value";
pub const ORG_URL: &str = "https://dev.azure.com/contoso";

pub struct MockApp {
    pub runtime: tokio::runtime::Runtime,
    pub mock: ado_mock::AdoMock,
    pub dir: tempfile::TempDir,
}

impl MockApp {
    pub fn start() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("ado-mock-ui")
            .build()
            .expect("ado-mock runtime");
        let mock = runtime.block_on(ado_mock::start());
        let dir = tempfile::tempdir().expect("tempdir");
        Self { runtime, mock, dir }
    }

    /// A signed-out PAT shell whose data dir is `self.dir`, so prefs written
    /// by an earlier window are read back.
    pub fn signed_out(&self) -> PlannerState {
        let mut state = self.fresh_state(self.dir.path());
        state.session_loading = false;
        state
    }

    /// Signed in on Contoso / Shop / Platform with the mock hierarchy loaded
    /// and the camera on July 2026 so every fixture bar is on screen.
    pub fn signed_in(&self) -> PlannerState {
        let mut state = self.fresh_state(self.dir.path());
        state.sign_in_org = ORG_URL.into();
        state.sign_in_pat = PAT.into();
        self.runtime
            .block_on(state.login_pat())
            .expect("mock login");
        state.session_loading = false;
        state.gantt.origin = Date::new(2026, 7, 1).unwrap();
        state.gantt.zoom = 1.0;
        state.gantt.selected_id = None;
        state.gantt.viewport_h = 720.0;
        state
    }

    fn fresh_state(&self, data_dir: &Path) -> PlannerState {
        let paths = AppPaths::new(data_dir.to_path_buf());
        let store = EncryptedStore::new(Box::new(MemoryStore::new(true)), Some([3u8; 32]));
        let session = PlannerSession::pat(store).with_bases(&self.mock.url, &self.mock.url);
        PlannerState::new(
            paths,
            session,
            Some(self.mock.url.clone()),
            Some(self.mock.url.clone()),
        )
    }

    /// PATCHes the mock accepted for one work item.
    pub fn patches_to(&self, id: i64) -> usize {
        let suffix = format!("/workitems/{id}");
        self.mock
            .mutating_writes()
            .iter()
            .filter(|(method, path)| method == "PATCH" && path.ends_with(&suffix))
            .count()
    }

    pub fn mock_field(&self, id: i64, field: &str) -> Option<String> {
        self.runtime.block_on(async {
            self.mock
                .work_item(id)
                .await?
                .get("fields")?
                .get(field)?
                .as_str()
                .map(str::to_string)
        })
    }
}

pub fn open(
    cx: &mut gpui_kit::TestAppContext,
    state: PlannerState,
) -> (Entity<PlannerView>, &mut VisualTestContext) {
    cx.update(gpui_kit::init);
    cx.executor().allow_parking();
    let (view, cx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    cx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    draw(cx);
    (view, cx)
}

pub fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

pub fn bounds(
    cx: &mut VisualTestContext,
    selector: &'static str,
) -> gpui_kit::Bounds<gpui_kit::Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("`{selector}` is not painted"))
}

pub fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let at = bounds(cx, selector).center();
    cx.simulate_mouse_move(at, None, Modifiers::default());
    cx.simulate_click(at, Modifiers::default());
    draw(cx);
}

/// Press and release on a Gantt bar without moving, which selects it and
/// focuses the planner keys.
pub fn select_bar(cx: &mut VisualTestContext, selector: &'static str) {
    let at = bounds(cx, selector).center();
    cx.simulate_mouse_move(at, None, Modifiers::default());
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::default());
    draw(cx);
}

pub fn right_click(cx: &mut VisualTestContext, selector: &'static str) {
    let at = bounds(cx, selector).center();
    cx.simulate_mouse_move(at, None, Modifiers::default());
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
    draw(cx);
}

/// Pump the foreground until mock I/O has finished and `done` holds.
pub fn settle(
    view: &Entity<PlannerView>,
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&PlannerView) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        draw(cx);
        if view.read_with(cx, |view, _| !view.io_pending() && done(view)) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn node_dates(view: &PlannerView, id: i64) -> (Option<String>, Option<String>) {
    view.state
        .nodes
        .iter()
        .find(|node| node.id == id)
        .map(|node| (node.start_date.clone(), node.target_date.clone()))
        .unwrap_or_default()
}

pub fn date_only(iso: Option<&str>) -> Option<&str> {
    iso.map(|value| value.get(..10).unwrap_or(value))
}
