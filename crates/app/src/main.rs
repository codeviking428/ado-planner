use ado_planner::paths::AppPaths;
use ado_planner::ui::start_app;

fn main() {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("{}", ado_planner::version_line());
                return;
            }
            "--help" | "-h" => {
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

    let ado_base = std::env::var("ADO_PLANNER_ADO_BASE_URL").ok();
    let vssps_base = std::env::var("ADO_PLANNER_VSSPS_URL").ok();
    start_app(Some(AppPaths::default_dir()), ado_base, vssps_base);
    drop(instance);
}
