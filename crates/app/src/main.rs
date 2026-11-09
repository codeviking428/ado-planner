// Release builds use the GUI subsystem on Windows so launching the app doesn't
// open a console window. Debug builds keep the console for tracing output.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use ado_planner::paths::AppPaths;
use ado_planner::ui::start_app;

fn main() {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "--version" | "-V" => {
                attach_parent_console();
                println!("{}", ado_planner::version_line());
                return;
            }
            "--help" | "-h" => {
                attach_parent_console();
                println!(
                    "{}\n\nUsage: ado-planner [--version | --help]",
                    ado_planner::version_line()
                );
                return;
            }
            _ => {}
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let instance =
        single_instance::SingleInstance::new("ado-planner").expect("single-instance lock");
    if !instance.is_single() {
        eprintln!("ADO Planner is already running");
        return;
    }

    let data_dir = std::env::var_os("ADO_PLANNER_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(AppPaths::default_dir);
    let ado_base = std::env::var("ADO_PLANNER_ADO_BASE_URL").ok();
    let vssps_base = std::env::var("ADO_PLANNER_VSSPS_URL").ok();
    start_app(Some(data_dir), ado_base, vssps_base);
    drop(instance);
}

/// A GUI-subsystem process has no stdout when started from a terminal, so CLI
/// flags print into the parent's console. Redirected stdout (a pipe or file)
/// already works and is left alone.
#[cfg(windows)]
#[allow(unsafe_code)]
fn attach_parent_console() {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };

    // SAFETY: both calls take plain values and only read or set this process's
    // standard handles.
    unsafe {
        let stdout = GetStdHandle(STD_OUTPUT_HANDLE);
        if stdout.is_null() || stdout == INVALID_HANDLE_VALUE {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}
