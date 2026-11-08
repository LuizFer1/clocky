//! Per-user directories (config, data and logs).

use std::path::PathBuf;

use directories::ProjectDirs;

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", "Clocky")
}

/// Directory of the log file. Falls back to the temp dir when the OS gives no home.
pub fn log_dir() -> PathBuf {
    project_dirs().map_or_else(
        || std::env::temp_dir().join("clocky"),
        |dirs| dirs.data_local_dir().join("logs"),
    )
}
