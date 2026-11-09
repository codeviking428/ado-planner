use std::time::{Duration, Instant};

use ado_core::{HierarchyResult, IdentityValue, SessionInfo, WorkItemNode};
use ado_planner::gantt::GanttView;
use ado_planner::paths::AppPaths;
use ado_planner::state::{PlannerState, fake_hierarchy};
use ado_planner::ui::PlannerView;
use ado_session::{EncryptedStore, MemoryStore, PlannerSession};
use gpui_kit::{Modifiers, ScrollDelta, ScrollEvent, VisualTestContext, point, px};
use jiff::civil::Date;
use serde_json::json;

fn speed_tests_enabled() -> bool {
    std::env::var("ADO_PLANNER_SPEED_TESTS")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[test]
fn gantt_from_warm_cache_within_500ms() {
    if !speed_tests_enabled() {
        return;
    }
    let nodes = fake_hierarchy(5_000);
    let gantt = GanttView::default();
    let started = Instant::now();
    let rows = gantt.flatten(&nodes);
    let _ = gantt.layout_bars(&nodes, &rows, &[], 720.0);
    let elapsed = started.elapsed();
    assert!(
        elapsed.as_millis() < 500,
        "warm gantt layout took {elapsed:?}"
    );
}

#[test]
fn visible_window_layout_fits_a_60fps_frame() {
    if !speed_tests_enabled() {
        return;
    }
    let nodes = fake_hierarchy(5_000);
    let gantt = GanttView::default();
    let rows = gantt.flatten(&nodes);
    let started = Instant::now();
    for _ in 0..10 {
        let _ = gantt.visible_window(&rows, 720.0);
        let _ = gantt.layout_bars(&nodes, &rows, &[], 720.0);
    }
    let per_frame = started.elapsed() / 10;
    assert!(
        per_frame.as_millis() < 16,
        "visible-window layout took {per_frame:?} (budget 16ms)"
    );
}

#[test]
fn first_frame_state_within_300ms() {
    if !speed_tests_enabled() {
        return;
    }
    let started = Instant::now();
    let nodes = fake_hierarchy(200);
    let gantt = GanttView::default();
    let _ = gantt.flatten(&nodes);
    let elapsed = started.elapsed();
    assert!(elapsed.as_millis() < 300, "first frame took {elapsed:?}");
}

#[test]
fn idle_memory_budget_documented() {
    // RSS of this test process is not the packaged app. The CI speed job
    // records /proc/self/status VmRSS after the 5,000-row layout instead.
    let nodes = fake_hierarchy(5_000);
    let gantt = GanttView::default();
    let rows = gantt.flatten(&nodes);
    let _ = gantt.layout_bars(&nodes, &rows, &[], 720.0);
    if let Ok(status) = std::fs::read_to_string("/proc/self/status")
        && let Some(rss_kb) = status.lines().find_map(|line| {
            line.strip_prefix("VmRSS:")
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|n| n.parse::<u64>().ok())
        })
    {
        // Debug tests pull in gpui-kit; the packaged release binary is the
        // 150 MB budget. Guard a generous ceiling so a leak still fails CI.
        assert!(
            rss_kb < 1_500_000,
            "test RSS {rss_kb} kB is unexpectedly high"
        );
    }
}

/// Epic → Feature → User Story → Task forest shaped like a loaded Team:
/// Epics and Features are unscheduled (their bars are Rollups), most Stories
/// and Tasks are scheduled, every node carries the hierarchy batch fields, and
/// sibling Stories chain as Dependencies.
fn ado_shaped_hierarchy(count: usize) -> Vec<WorkItemNode> {
    const FEATURES_PER_EPIC: usize = 4;
    const STORIES_PER_FEATURE: usize = 5;
    const TASKS_PER_STORY: usize = 4;
    let states = ["New", "Active", "Resolved", "Closed"];
    let people = ["Ada Lovelace", "Grace Hopper", "Alan Turing"];
    let mut nodes = Vec::with_capacity(count);
    let push = |nodes: &mut Vec<WorkItemNode>, parent: Option<i64>, ty: &str, dated: bool| {
        let id = nodes.len() as i64 + 1;
        let n = id as usize;
        let start = Date::new(2026, 1, 5)
            .unwrap()
            .saturating_add(jiff::Span::new().days((n % 300) as i64));
        let target = start.saturating_add(jiff::Span::new().days(3 + (n % 20) as i64));
        let (start, target) = if dated {
            (
                Some(format!("{start}T00:00:00Z")),
                Some(format!("{target}T00:00:00Z")),
            )
        } else {
            (None, None)
        };
        let person = people[n % people.len()];
        let assigned = (!n.is_multiple_of(5)).then(|| IdentityValue {
            display_name: person.into(),
            unique_name: format!("{}@contoso.com", person.to_lowercase().replace(' ', ".")),
            id: None,
        });
        let title = format!("{ty} {id} for the checkout and catalog rework");
        let state = states[n % states.len()];
        let mut fields = serde_json::Map::new();
        fields.insert("System.Id".into(), json!(id));
        fields.insert("System.Rev".into(), json!(3));
        fields.insert("System.Title".into(), json!(title));
        fields.insert("System.WorkItemType".into(), json!(ty));
        fields.insert("System.State".into(), json!(state));
        if let Some(assigned) = &assigned {
            fields.insert(
                "System.AssignedTo".into(),
                json!({ "displayName": assigned.display_name, "uniqueName": assigned.unique_name }),
            );
        }
        if let Some(parent) = parent {
            fields.insert("System.Parent".into(), json!(parent));
        }
        fields.insert("System.AreaPath".into(), json!("Shop\\Platform"));
        fields.insert("System.IterationPath".into(), json!("Shop\\Sprint 12"));
        if let (Some(start), Some(target)) = (&start, &target) {
            fields.insert("Microsoft.VSTS.Scheduling.StartDate".into(), json!(start));
            fields.insert("Microsoft.VSTS.Scheduling.TargetDate".into(), json!(target));
        }
        nodes.push(WorkItemNode {
            id,
            rev: 3,
            title,
            work_item_type: ty.into(),
            state: state.into(),
            assigned_to: assigned,
            parent_id: parent,
            area_path: "Shop\\Platform".into(),
            iteration_path: "Shop\\Sprint 12".into(),
            start_date: start,
            target_date: target,
            has_date_fields: true,
            predecessors: Vec::new(),
            fields,
        });
        id
    };
    'epics: loop {
        let epic = push(&mut nodes, None, "Epic", false);
        for _ in 0..FEATURES_PER_EPIC {
            let feature = push(&mut nodes, Some(epic), "Feature", false);
            let mut previous_story = None;
            for s in 0..STORIES_PER_FEATURE {
                if nodes.len() >= count {
                    break 'epics;
                }
                let story = push(&mut nodes, Some(feature), "User Story", s % 4 != 3);
                if let Some(previous) = previous_story {
                    nodes[story as usize - 1].predecessors.push(previous);
                }
                previous_story = Some(story);
                for t in 0..TASKS_PER_STORY {
                    if nodes.len() >= count {
                        break 'epics;
                    }
                    push(&mut nodes, Some(story), "Task", t % 3 != 2);
                }
            }
        }
        if nodes.len() >= count {
            break;
        }
    }
    nodes.truncate(count);
    nodes
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
    collapse: Duration,
    filter: Duration,
}

impl FrameTimes {
    const MARKDOWN_HEADER: &str = "| Rows | First paint | Scroll | Hover | Collapse | Filter |\n\
                                   | ---: | ---: | ---: | ---: | ---: | ---: |";

    fn markdown_row(&self) -> String {
        format!(
            "| {} | {:.1?} | {:.1?} | {:.1?} | {:.1?} | {:.1?} |",
            self.rows, self.first_paint, self.scroll, self.hover, self.collapse, self.filter,
        )
    }
}

/// Paints a real `PlannerView` over `count` ADO-shaped rows at 1280×800,
/// then times `samples` wheel-scroll and row-hover frames and `samples / 4`
/// Epic collapse/expand and State-filter frames. The GPUI test platform does
/// layout but no GPU work, so these are CPU frame-build times, not on-screen
/// latency.
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
        collapse: median(collapse),
        filter: median(filter),
    }
}

/// Keeps the frame-time harness building and running on every CI platform.
/// Times nothing: debug builds on shared runners are too noisy for a budget.
#[gpui_kit::test]
fn frame_time_harness_paints_scrolls_and_filters(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let times = measure_frame_times(cx, 200, 2);
    assert_eq!(times.rows, 200);
}

/// Prints the table in `docs/performance.md` for the same synthetic datasets,
/// so before/after runs compare like for like. Run with
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
