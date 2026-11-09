//! Native GPUI shell for ADO Planner.

pub mod app_icon;
pub mod assets;
pub mod cache;
pub mod error;
pub mod flavor_theme;
pub mod form;
pub mod gantt;
pub mod paths;
pub mod prefs;
pub mod startup_probe;
pub mod state;
pub mod toasts;
pub mod ui;
pub mod updater;

pub use error::AppError;
pub use state::PlannerState;

/// Workspace version from `Cargo.toml`, bumped by the release PR.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn version_line() -> String {
    format!("ado-planner {VERSION}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_names_the_binary_and_semver() {
        assert_eq!(version_line(), format!("ado-planner {VERSION}"));
        assert_eq!(VERSION.split('.').count(), 3);
    }
}
