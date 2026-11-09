//! Launch milestones for the CI speed budgets (`crates/app/tests/launch.rs`).
//!
//! Off unless `ADO_PLANNER_STARTUP_PROBE=1`. When on, the app prints one
//! stdout line once the first frame has been presented and one once the
//! Gantt has painted loaded rows. The harness times each line from the
//! moment it spawned the process.

use gpui_kit::Window;

pub const ENV: &str = "ADO_PLANNER_STARTUP_PROBE";
pub const FIRST_FRAME: &str = "startup-probe first-frame";
pub const GANTT_ROWS: &str = "startup-probe gantt-rows";

#[derive(Debug, Default)]
pub struct StartupProbe {
    enabled: bool,
    first_frame: bool,
    gantt: bool,
}

impl StartupProbe {
    pub fn from_env() -> Self {
        Self {
            enabled: std::env::var(ENV).is_ok_and(|value| value == "1"),
            ..Self::default()
        }
    }

    /// Whether the Gantt milestone is still waiting, so callers can skip
    /// counting rows once it has been reported.
    pub fn wants_gantt(&self) -> bool {
        self.enabled && !self.gantt
    }

    /// Call from every render. `gantt_rows` is how many rows this frame
    /// paints after the Hierarchy finished loading, or 0.
    pub fn note_render(&mut self, gantt_rows: usize, window: &mut Window) {
        if !self.enabled {
            return;
        }
        // The next frame's callbacks run once this one has been presented.
        if !self.first_frame {
            self.first_frame = true;
            window.on_next_frame(|_, _| report(FIRST_FRAME));
        }
        if !self.gantt && gantt_rows > 0 {
            self.gantt = true;
            window.on_next_frame(move |_, _| report(&format!("{GANTT_ROWS} {gantt_rows}")));
        }
    }
}

fn report(line: &str) {
    use std::io::Write as _;
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
    let _ = stdout.flush();
}
