//! Launch budgets from #53, measured on the real release binary.
//!
//! Gated like the other speed tests (`ADO_PLANNER_SPEED_TESTS=1`), and Linux
//! only: the binary needs a display (Xvfb), a GPU adapter (Mesa's llvmpipe
//! works) and a Secret Service keyring so the signed-in Session survives a
//! relaunch. The `speed` CI job provides all three; `docs/speed-budgets.md`
//! says how the budgets were chosen.
#![cfg(target_os = "linux")]

mod dataset;

use std::io::{BufRead as _, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ado_planner::paths::AppPaths;
use ado_planner::startup_probe;
use ado_planner::state::PlannerState;
use serde_json::{Value, json};

// Budgets: the slowest median seen on a hosted runner plus 25%, or #53's
// target where that is stricter. docs/speed-budgets.md has the runs.

/// Spawn to first presented frame. #53's target; runners measured 128–161 ms.
const FIRST_FRAME_BUDGET: Duration = Duration::from_millis(300);
/// Spawn to the Gantt painting a 5,000-row Team, with a warm query cache and a
/// saved Session. Runners measured 432–641 ms against #53's 500 ms target.
const GANTT_BUDGET: Duration = Duration::from_millis(850);
/// Private memory 2 s after that Gantt paints. #53's target; runners
/// measured 128–137 MB, and memory does not scale with runner speed.
const IDLE_PRIVATE_BUDGET_MB: f64 = 150.0;
/// All resident memory at the same point, software renderer included. A
/// regression guard only; runners measured 269–279 MB.
const IDLE_RSS_BUDGET_MB: f64 = 350.0;

const ROWS: usize = 5_000;
/// Measured launches after one warm-up launch; each budget takes the median.
const LAUNCHES: usize = 5;
const IDLE_SETTLE: Duration = Duration::from_secs(2);
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
/// `RUST_LOG` filter for the app; when set, its log is echoed to stderr with
/// the time since spawn, to see where a slow launch spends its time.
const LOG_ENV: &str = "ADO_PLANNER_LAUNCH_LOG";
/// Either one switches `create_session` to Entra sign-in; the budgets use a PAT.
const ENTRA_VARS: [&str; 2] = ["ENTRA_CLIENT_ID", "MAIN_VITE_ENTRA_CLIENT_ID"];

fn speed_tests_enabled() -> bool {
    std::env::var("ADO_PLANNER_SPEED_TESTS")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy)]
struct Launch {
    first_frame: Duration,
    gantt: Duration,
    idle: Memory,
}

/// ADO REST shape of the speed tests' synthetic Hierarchy, in the mock Team's
/// Area Path.
fn mock_work_items(count: usize) -> Vec<Value> {
    dataset::ado_shaped_hierarchy(count)
        .into_iter()
        .map(|node| {
            let mut fields = node.fields;
            fields.insert("System.AreaPath".into(), json!("Shop\\Platform"));
            json!({ "id": node.id, "rev": node.rev, "fields": fields })
        })
        .collect()
}

/// Signs in with a PAT against the mock and lets the app pick and load its
/// Team, as a first launch would. That leaves the encrypted Session, prefs
/// and query cache on disk for the measured relaunches.
fn sign_in_once(runtime: &tokio::runtime::Runtime, mock_url: &str, data_dir: &Path) -> usize {
    for var in ENTRA_VARS {
        assert!(
            std::env::var_os(var).is_none(),
            "unset {var}: the launch budgets sign in with a PAT"
        );
    }
    runtime.block_on(async {
        let paths = AppPaths::new(data_dir.to_path_buf());
        let session = ado_session::create_session(&paths.data_dir).with_bases(mock_url, mock_url);
        let mut state = PlannerState::new(
            paths,
            session,
            Some(mock_url.to_string()),
            Some(mock_url.to_string()),
        );
        state.sign_in_org = "https://dev.azure.com/contoso".into();
        state.sign_in_pat = "mock-pat-value".into();
        // A locked keyring waits on a password prompt nobody will answer.
        tokio::time::timeout(PROBE_TIMEOUT, state.login_pat())
            .await
            .expect("sign-in timed out; is the Secret Service keyring unlocked?")
            .expect("PAT sign-in against ado-mock");
        state.persist_prefs();
        state.nodes.len()
    })
}

/// Resident memory from `/proc/<pid>/status`, in MB.
#[derive(Debug, Clone, Copy)]
struct Memory {
    /// Everything resident, shared libraries included. Under llvmpipe about
    /// 85 MB of it is Mesa's software renderer (`libLLVM`, lavapipe, its
    /// shared buffers), which a machine with a GPU does not load.
    rss: f64,
    /// Anonymous pages only: heap, stacks and other memory no other process
    /// shares. The closest Linux reading of Task Manager's private working
    /// set, which is what "idle memory" means on Windows.
    private: f64,
}

fn memory(pid: u32) -> Memory {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).expect("proc status");
    let field = |name: &str| -> f64 {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|kb| kb.parse::<f64>().ok())
            .unwrap_or_else(|| panic!("{name} in /proc/{pid}/status"))
            / 1024.0
    };
    Memory {
        rss: field("VmRSS:"),
        private: field("RssAnon:"),
    }
}

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn launch(xdg_data_home: &Path, mock_url: &str) -> Launch {
    let started = Instant::now();
    let mut child = Running(
        Command::new(env!("CARGO_BIN_EXE_ado-planner"))
            .env("XDG_DATA_HOME", xdg_data_home)
            .env("ADO_PLANNER_ADO_BASE_URL", mock_url)
            .env("ADO_PLANNER_VSSPS_URL", mock_url)
            .env(startup_probe::ENV, "1")
            .env(
                "RUST_LOG",
                std::env::var(LOG_ENV).unwrap_or_else(|_| "warn".into()),
            )
            .env_remove(ENTRA_VARS[0])
            .env_remove(ENTRA_VARS[1])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn ado-planner"),
    );
    let stdout = child.0.stdout.take().expect("stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let echo = std::env::var(LOG_ENV).is_ok();
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if echo {
                eprintln!("{:>9.1?} {line}", started.elapsed());
            }
            if tx.send((line, started.elapsed())).is_err() {
                break;
            }
        }
    });
    let wait_for = |prefix: &str| loop {
        let (line, at) = rx
            .recv_timeout(PROBE_TIMEOUT)
            .unwrap_or_else(|_| panic!("no `{prefix}` line within {PROBE_TIMEOUT:?}"));
        if line.starts_with(prefix) {
            break (line, at);
        }
    };
    let (_, first_frame) = wait_for(startup_probe::FIRST_FRAME);
    let (line, gantt) = wait_for(startup_probe::GANTT_ROWS);
    let rows: usize = line
        .rsplit(' ')
        .next()
        .and_then(|rows| rows.parse().ok())
        .unwrap_or(0);
    assert_eq!(rows, ROWS, "the relaunch must paint the saved Team's rows");
    std::thread::sleep(IDLE_SETTLE);
    Launch {
        first_frame,
        gantt,
        idle: memory(child.0.id()),
    }
}

fn median<T: Copy + PartialOrd>(mut values: Vec<T>) -> T {
    values.sort_by(|a, b| a.partial_cmp(b).expect("comparable"));
    values[values.len() / 2]
}

#[test]
fn relaunch_meets_first_frame_gantt_and_memory_budgets() {
    if !speed_tests_enabled() {
        return;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let mock = runtime.block_on(async {
        let mock = ado_mock::start().await;
        mock.set_work_items(mock_work_items(ROWS)).await;
        mock
    });
    let xdg = tempfile::tempdir().unwrap();
    let data_dir = xdg.path().join("ado-planner");
    let loaded = sign_in_once(&runtime, &mock.url, &data_dir);
    assert_eq!(loaded, ROWS, "sign-in must load the mock Team");
    assert!(
        data_dir.join("session").join("session.bin").exists(),
        "the Session was not saved; run under a Secret Service keyring \
         (dbus-run-session with gnome-keyring-daemon --unlock)"
    );

    let _warm_up = launch(xdg.path(), &mock.url);
    let launches: Vec<Launch> = (0..LAUNCHES)
        .map(|_| launch(xdg.path(), &mock.url))
        .collect();
    eprintln!("| Launch | First frame | Gantt ({ROWS} rows) | Idle private | Idle RSS |");
    eprintln!("| ---: | ---: | ---: | ---: | ---: |");
    for (index, run) in launches.iter().enumerate() {
        eprintln!(
            "| {} | {:.1?} | {:.1?} | {:.1} MB | {:.1} MB |",
            index + 1,
            run.first_frame,
            run.gantt,
            run.idle.private,
            run.idle.rss,
        );
    }
    let first_frame = median(launches.iter().map(|run| run.first_frame).collect());
    let gantt = median(launches.iter().map(|run| run.gantt).collect());
    let private = median(launches.iter().map(|run| run.idle.private).collect());
    let rss = median(launches.iter().map(|run| run.idle.rss).collect());
    eprintln!(
        "medians: first frame {first_frame:.1?}, Gantt {gantt:.1?}, \
         idle private {private:.1} MB, idle RSS {rss:.1} MB"
    );
    assert!(
        first_frame <= FIRST_FRAME_BUDGET,
        "first frame {first_frame:?} is over the {FIRST_FRAME_BUDGET:?} budget"
    );
    assert!(
        gantt <= GANTT_BUDGET,
        "Gantt with {ROWS} rows took {gantt:?}, over the {GANTT_BUDGET:?} budget"
    );
    assert!(
        private <= IDLE_PRIVATE_BUDGET_MB,
        "idle private memory {private:.1} MB is over the {IDLE_PRIVATE_BUDGET_MB} MB budget"
    );
    assert!(
        rss <= IDLE_RSS_BUDGET_MB,
        "idle RSS {rss:.1} MB is over the {IDLE_RSS_BUDGET_MB} MB budget"
    );
}
