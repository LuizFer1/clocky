//! Per-user directories (config, data and logs).

use std::path::PathBuf;

use directories::ProjectDirs;

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", "Clocky")
}

/// Directory of `settings.json`. `None` when the OS gives no home directory.
pub fn config_dir() -> Option<PathBuf> {
    project_dirs().map(|dirs| dirs.config_dir().to_path_buf())
}

/// Directory of the log file. Falls back to the temp dir when the OS gives no home.
pub fn log_dir() -> PathBuf {
    project_dirs().map_or_else(
        || std::env::temp_dir().join("clocky"),
        |dirs| dirs.data_local_dir().join("logs"),
    )
}

/// Environment variable that points the app at another database file (tests, screenshots).
pub const DB_ENV_VAR: &str = "CLOCKY_DB";

/// Path of the database: `CLOCKY_DB` when set, otherwise `clocky.db` in the user data dir.
/// `None` when neither is available (the OS gives no home directory).
pub fn db_path() -> Option<PathBuf> {
    db_path_from(
        std::env::var_os(DB_ENV_VAR).map(PathBuf::from),
        project_dirs().map(|dirs| dirs.data_dir().to_path_buf()),
    )
}

fn db_path_from(env: Option<PathBuf>, data_dir: Option<PathBuf>) -> Option<PathBuf> {
    env.filter(|path| !path.as_os_str().is_empty())
        .or_else(|| data_dir.map(|dir| dir.join("clocky.db")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_wins_over_the_data_dir() {
        let path = db_path_from(Some("x/test.db".into()), Some("data".into()));
        assert_eq!(path, Some(PathBuf::from("x/test.db")));
    }

    #[test]
    fn data_dir_is_the_default() {
        let path = db_path_from(None, Some("data".into()));
        assert_eq!(path, Some(PathBuf::from("data").join("clocky.db")));
        let empty = db_path_from(Some(PathBuf::new()), Some("data".into()));
        assert_eq!(empty, path);
        assert_eq!(db_path_from(None, None), None);
    }
}
