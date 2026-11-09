//! Frame budgets from #53, measured on a real `PlannerView` in the GPUI test
//! platform. Launch, first-frame and memory budgets are in `launch.rs`;
//! `docs/speed-budgets.md` says how every budget was chosen.

mod dataset;

use std::time::{Duration, Instant};

use ado_core::{GanttBarKind, HierarchyResult, SessionInfo, WorkItemNode};
use ado_planner::gantt::{GanttView, ROW_HEIGHT, TIMELINE_HEADER_H};
use ado_planner::paths::AppPaths;
use ado_planner::state::PlannerState;
use ado_planner::ui::PlannerView;
use ado_session::{EncryptedStore, MemoryStore, PlannerSession};
use gpui_kit::{Modifiers, MouseButton, ScrollDelta, ScrollEvent, VisualTestContext, point, px};
use jiff::civil::Date;
use dataset::ado_shaped_hierarchy;

/// Median wheel-scroll frame at 5,000 rows. #53 asks for 60 fps (16.7 ms).
const SCROLL_BUDGET: Duration = Duration::from_millis(17);
/// Median bar-drag frame at 5,000 rows. #53 asks for 60 fps (16.7 ms).
const DRAG_BUDGET: Duration = Duration::from_millis(17);
const BUDGET_ROWS: usize = 5_000;
/// Measured runs per budget; the budget takes the median of their medians.
const BUDGET_RUNS: usize = 3;

fn speed_tests_enabled() -> bool {
    std::env::var("ADO_PLANNER_SPEED_TESTS")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn planner_state(dir: &tempfile::TempDir, nodes: Vec<WorkItemNode>) -> PlannerState {
    let paths = AppPaths::new(dir.path().to_path_buf());
    let store = EncryptedStore::new(Box::new(MemoryStore::new(false)), None);
    let session = PlannerSession::pat(store);
    let mut state = PlannerState::new(paths, session, None, None);
    state.session_loading = false;
    state.session_info = SessionInfo {
        signed_in: true,
        display_name: Some("Ada Lovelace".into()),
        username: Some("ada.lovelace@contoso.com".into()),
        auth_mode: ado_core::AuthMode::Pat,
    };
    state.prefs.planner.org = "contoso".into();
    state.prefs.planner.project = "Shop".into();
    state.prefs.planner.team = "Platform".into();
    // A Team load stores the result in `hierarchy` and copies its nodes.
    state.hierarchy = Some(HierarchyResult {
        nodes: nodes.clone(),
        types: ["Epic", "Feature", "Task", "User Story"]
            .map(String::from)
            .to_vec(),
        top_backlog_types: vec!["Epic".into()],
        truncated: false,
        warning: None,
    });
    state.nodes = nodes;
    state.gantt = GanttView {
        origin: Date::new(2026, 1, 1).unwrap(),
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

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

const FRAME_SAMPLES: usize = 40;

/// Median CPU frame-build times for one synthetic dataset.
#[derive(Debug)]
struct FrameTimes {
    rows: usize,
    first_paint: Duration,
    scroll: Duration,
    hover: Duration,
    drag: Duration,
    collapse: Duration,
    filter: Duration,
}

impl FrameTimes {
    const MARKDOWN_HEADER: &str = "| Rows | First paint | Scroll | Hover | Drag | Collapse | Filter |\n\
                                   | ---: | ---: | ---: | ---: | ---: | ---: | ---: |";

    fn markdown_row(&self) -> String {
        format!(
            "| {} | {:.1?} | {:.1?} | {:.1?} | {:.1?} | {:.1?} | {:.1?} |",
            self.rows,
            self.first_paint,
            self.scroll,
            self.hover,
            self.drag,
            self.collapse,
            self.filter,
        )
    }
}

/// Window centre of a plain scheduled bar wide enough to grab mid-body and
/// far enough from the right edge to drag.
fn draggable_bar(view: &gpui_kit::Entity<PlannerView>, vcx: &mut VisualTestContext) -> (f32, f32) {
    view.update(vcx, |view, _| {
        let gantt = &view.state.gantt;
        view.state
            .layout_visible_bars(gantt.viewport_h)
            .into_iter()
            .find(|bar| {
                bar.kind == GanttBarKind::Scheduled
                    && !bar.summary
                    && !bar.milestone
                    && !bar.marker
                    && !bar.open_ended
                    && bar.width >= 48.0
                    && bar.x >= 0.0
                    && bar.x + bar.width + 200.0 < gantt.timeline_width
            })
            .map(|bar| {
                (
                    gantt.timeline_origin_x + bar.x + bar.width / 2.0,
                    gantt.timeline_origin_y + TIMELINE_HEADER_H + bar.y + ROW_HEIGHT / 2.0,
                )
            })
            .expect("a draggable bar in the first rows")
    })
}

/// Paints a real `PlannerView` over `count` ADO-shaped rows at 1280×800,
/// then times `samples` wheel-scroll, row-hover and bar-drag frames and
/// `samples / 4` Epic collapse/expand and State-filter frames. The GPUI test
/// platform does layout but no GPU work, so these are CPU frame-build
/// times, not on-screen latency.
fn measure_frame_times(
    cx: &mut gpui_kit::TestAppContext,
    count: usize,
    samples: usize,
) -> FrameTimes {
    let dir = tempfile::tempdir().unwrap();
    let state = planner_state(&dir, ado_shaped_hierarchy(count));
    let (view, vcx) = cx.add_window_view(|window, cx| PlannerView::for_state(window, cx, state));
    vcx.simulate_resize(gpui_kit::Size {
        width: px(1280.),
        height: px(800.),
    });
    let started = Instant::now();
    draw(vcx);
    let first_paint = started.elapsed();
    // A second paint lays rows out for the measured body.
    draw(vcx);

    // Drag before scrolling, while the first Stories' bars are on screen.
    let (bar_x, bar_y) = draggable_bar(&view, vcx);
    vcx.simulate_mouse_move(point(px(bar_x), px(bar_y)), None, Modifiers::default());
    draw(vcx);
    vcx.simulate_mouse_down(
        point(px(bar_x), px(bar_y)),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(vcx);
    let mut drag = Vec::with_capacity(samples);
    for i in 1..=samples {
        let x = px(bar_x + 3.0 * i as f32);
        let started = Instant::now();
        vcx.simulate_mouse_move(
            point(x, px(bar_y)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        draw(vcx);
        drag.push(started.elapsed());
    }
    view.update(vcx, |view, cx| {
        assert!(
            view.state.gantt.gesture.is_some(),
            "the pointer must be dragging a bar"
        );
        view.state.cancel_gesture();
        cx.notify();
    });
    vcx.simulate_mouse_up(
        point(px(bar_x), px(bar_y)),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(vcx);

    // Release builds drop debug selectors; this point is inside the tree body.
    let tree = point(px(150.), px(420.));
    let mut scroll = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        vcx.simulate_event(ScrollEvent {
            position: tree,
            delta: ScrollDelta::Pixels(point(px(0.), px(-36.))),
            ..Default::default()
        });
        draw(vcx);
        scroll.push(started.elapsed());
    }

    let mut hover = Vec::with_capacity(samples);
    for i in 0..samples {
        let y = px(300. + 36. * (i % 10) as f32);
        let started = Instant::now();
        vcx.simulate_mouse_move(point(tree.x, y), None, Modifiers::default());
        draw(vcx);
        hover.push(started.elapsed());
    }

    let scroll_y = vcx.update(|_, cx| view.read(cx).state.gantt.scroll_y);
    assert!(scroll_y > 0.0, "wheel events must scroll the rows");

    // Rebuilds rows only (collapse) or every visible node (filter).
    let toggles = (samples / 4).max(1);
    let mut collapse = Vec::with_capacity(toggles);
    let mut filter = Vec::with_capacity(toggles);
    for _ in 0..toggles {
        let started = Instant::now();
        view.update(vcx, |view, cx| {
            view.state.gantt.toggle_expand(1);
            cx.notify();
        });
        draw(vcx);
        collapse.push(started.elapsed());
        let started = Instant::now();
        view.update(vcx, |view, cx| {
            let hidden = &mut view.state.prefs.planner.hidden_states;
            if hidden.is_empty() {
                hidden.push("Closed".into());
            } else {
                hidden.clear();
            }
            cx.notify();
        });
        draw(vcx);
        filter.push(started.elapsed());
    }

    FrameTimes {
        rows: count,
        first_paint,
        scroll: median(scroll),
        hover: median(hover),
        drag: median(drag),
        collapse: median(collapse),
        filter: median(filter),
    }
}

/// Keeps the frame-time harness building and running on every CI platform.
/// Times nothing: debug builds on shared runners are too noisy for a budget.
#[gpui_kit::test]
fn frame_time_harness_paints_scrolls_drags_and_filters(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let times = measure_frame_times(cx, 200, 2);
    assert_eq!(times.rows, 200);
}

/// Prints the frame-time table in `docs/performance.md` for the same
/// synthetic datasets, so before/after runs compare like for like. Run with
/// `ADO_PLANNER_SPEED_TESTS=1 cargo test --release -p ado-planner --test speed
/// -- --nocapture planner_frame_times`.
#[gpui_kit::test]
fn planner_frame_times(cx: &mut gpui_kit::TestAppContext) {
    if !speed_tests_enabled() {
        return;
    }
    cx.update(gpui_kit::init);
    eprintln!("{}", FrameTimes::MARKDOWN_HEADER);
    for count in [1_000, 5_000, 10_000] {
        let times = measure_frame_times(cx, count, FRAME_SAMPLES);
        eprintln!("{}", times.markdown_row());
    }
}

#[gpui_kit::test]
fn scroll_and_drag_at_5000_rows_meet_the_frame_budgets(cx: &mut gpui_kit::TestAppContext) {
    if !speed_tests_enabled() {
        return;
    }
    cx.update(gpui_kit::init);
    eprintln!("{}", FrameTimes::MARKDOWN_HEADER);
    let runs: Vec<FrameTimes> = (0..BUDGET_RUNS)
        .map(|_| {
            let times = measure_frame_times(cx, BUDGET_ROWS, FRAME_SAMPLES);
            eprintln!("{}", times.markdown_row());
            times
        })
        .collect();
    let scroll = median(runs.iter().map(|run| run.scroll).collect());
    let drag = median(runs.iter().map(|run| run.drag).collect());
    eprintln!("medians: scroll {scroll:.1?}, drag {drag:.1?}");
    assert!(
        scroll <= SCROLL_BUDGET,
        "scroll frame {scroll:?} at {BUDGET_ROWS} rows is over the {SCROLL_BUDGET:?} budget"
    );
    assert!(
        drag <= DRAG_BUDGET,
        "drag frame {drag:?} at {BUDGET_ROWS} rows is over the {DRAG_BUDGET:?} budget"
    );
}
