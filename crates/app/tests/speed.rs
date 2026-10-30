use std::time::Instant;

use ado_planner::gantt::GanttView;
use ado_planner::state::fake_hierarchy;

#[test]
fn gantt_from_warm_cache_within_500ms() {
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
