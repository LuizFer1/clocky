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

/// Where the app keeps its data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Db {
    /// A database file.
    File(PathBuf),
    /// A throwaway in-memory database: screenshot runs without `CLOCKY_DB`, so they never
    /// open, tick or seed the user's real `clocky.db`.
    Memory,
    /// No file to use (the OS gives no home directory).
    Unavailable,
}

/// The database to open: `CLOCKY_DB` when set, otherwise `clocky.db` in the user data dir;
/// in a screenshot run, only `CLOCKY_DB` (else [`Db::Memory`]).
pub fn db(screenshot: bool) -> Db {
    db_from(
        screenshot,
        std::env::var_os(DB_ENV_VAR).map(PathBuf::from),
        project_dirs().map(|dirs| dirs.data_dir().to_path_buf()),
    )
}

fn db_from(screenshot: bool, env: Option<PathBuf>, data_dir: Option<PathBuf>) -> Db {
    if let Some(path) = env.filter(|path| !path.as_os_str().is_empty()) {
        return Db::File(path);
    }
    if screenshot {
        return Db::Memory;
    }
    data_dir.map_or(Db::Unavailable, |dir| Db::File(dir.join("clocky.db")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_wins_over_the_data_dir() {
        let db = db_from(false, Some("x/test.db".into()), Some("data".into()));
        assert_eq!(db, Db::File(PathBuf::from("x/test.db")));
        let shot = db_from(true, Some("x/test.db".into()), Some("data".into()));
        assert_eq!(shot, Db::File(PathBuf::from("x/test.db")));
    }

    #[test]
    fn data_dir_is_the_default() {
        let db = db_from(false, None, Some("data".into()));
        assert_eq!(db, Db::File(PathBuf::from("data").join("clocky.db")));
        let empty = db_from(false, Some(PathBuf::new()), Some("data".into()));
        assert_eq!(empty, db);
        assert_eq!(db_from(false, None, None), Db::Unavailable);
    }

    #[test]
    fn a_screenshot_run_never_falls_back_to_the_real_database() {
        assert_eq!(db_from(true, None, Some("data".into())), Db::Memory);
        assert_eq!(
            db_from(true, Some(PathBuf::new()), Some("data".into())),
            Db::Memory
        );
        assert_eq!(db_from(true, None, None), Db::Memory);
    }
}
