//! Demo data of the handoff screens (connect, faculdade, pessoal), for the screenshot hook and
//! the tests. Seeded only with `CLOCKY_DEMO=1`, `CLOCKY_DB` set and an empty database, so the
//! user's real database is never touched.

use std::time::Duration;

use chrono::{Days, NaiveTime, TimeZone};
use clocky_core::domain::{ProjectId, TaskId, TimerId};
use clocky_core::engine::run::Segment;
use clocky_core::engine::session::{CompletedSession, TimerKind};
use clocky_core::store::NewPomodoroPreset;
use clocky_core::time::Timestamp;

use crate::core::{AppCore, CoreResult, Reopened};

/// Environment variable that asks for the demo data.
pub const ENV_VAR: &str = "CLOCKY_DEMO";

/// Whether the environment asks for the demo data (`CLOCKY_DEMO=1` and `CLOCKY_DB` set).
pub fn requested() -> bool {
    requested_from(
        std::env::var_os(ENV_VAR).as_deref(),
        std::env::var_os(crate::paths::DB_ENV_VAR).as_deref(),
    )
}

/// The guard of [`requested`]: the demo is only written to a database named on purpose.
fn requested_from(demo: Option<&std::ffi::OsStr>, db: Option<&std::ffi::OsStr>) -> bool {
    demo.is_some_and(|v| v == "1") && db.is_some_and(|v| !v.is_empty())
}

/// Seeds the demo into `core` (opened on `path`) and, when it wrote anything, opens `path`
/// again: the tracker counts the next timer id when it opens, and the demo sessions use run ids
/// it has not seen, so the next timers would reuse them.
///
/// # Errors
/// When seeding or reopening fails.
pub fn seed_and_reopen<Tz: TimeZone>(
    mut core: AppCore,
    reopened: Reopened,
    path: &std::path::Path,
    tz: &Tz,
) -> anyhow::Result<(AppCore, Reopened)> {
    if !seed_if_empty(&mut core, tz)? {
        return Ok((core, reopened));
    }
    drop(core);
    AppCore::open(path)
        .map_err(|e| anyhow::anyhow!("cannot reopen the database after the demo seeding: {e}"))
}

/// A session of the demo: days before today, local start (h, m), minutes and kind.
struct DemoSession(u64, (u32, u32), u64, TimerKind);

/// Seeds the demo when the database has no project yet (archived ones included). Times are local to `tz`, relative to
/// the core's clock, so "Hoje" and "Ontem" show up.
pub fn seed_if_empty<Tz: TimeZone>(core: &mut AppCore, tz: &Tz) -> CoreResult<bool> {
    if core.any_project()? {
        return Ok(false);
    }
    let mut seeder = Seeder { core, tz, run: 0 };
    seeder.connect()?;
    seeder.faculdade()?;
    seeder.pessoal()?;
    seeder.core.create_task(None, "organizar e-mails")?;
    seeder.reports()?;
    Ok(true)
}

struct Seeder<'a, Tz: TimeZone> {
    core: &'a mut AppCore,
    tz: &'a Tz,
    run: u64,
}

fn minutes(n: u64) -> Duration {
    Duration::from_secs(n * 60)
}

impl<Tz: TimeZone> Seeder<'_, Tz> {
    fn connect(&mut self) -> CoreResult<()> {
        let p = self.core.create_project("connect", "#3d8fd6")?;
        let auth = self.task(
            p,
            "autenticação",
            &["Endpoint de login"],
            &["Testes de integração", "Atualizar documentação"],
        )?;
        self.pomodoro("Clássico", (25, 5, 15), auth)?;
        self.sessions(
            auth,
            &[
                DemoSession(0, (14, 2), 25, TimerKind::Pomodoro),
                DemoSession(0, (10, 15), 85, TimerKind::Stopwatch),
                DemoSession(1, (16, 0), 50, TimerKind::Pomodoro),
                DemoSession(3, (9, 0), 60, TimerKind::Stopwatch),
            ],
        )?;
        let tests = self.task(p, "testes", &[], &[])?;
        self.core
            .create_timer_preset("pipeline", minutes(12), Some(tests))?;
        self.sessions(tests, &[DemoSession(2, (15, 0), 65, TimerKind::Stopwatch)])?;
        let review = self.task(p, "code review", &[], &[])?;
        self.core
            .create_timer_preset("revisão de PR", minutes(25), Some(review))?;
        self.sessions(review, &[DemoSession(2, (11, 0), 45, TimerKind::Countdown)])?;
        self.task(p, "standup diário", &[], &[])?;
        Ok(())
    }

    fn faculdade(&mut self) -> CoreResult<()> {
        let p = self.core.create_project("faculdade", "#9b7be0")?;
        let tcc = self.task(
            p,
            "TCC",
            &["Revisar capítulo 2", "Referências"],
            &["Gráficos", "Resumo", "Enviar ao orientador"],
        )?;
        self.pomodoro("Deep work", (50, 10, 30), tcc)?;
        self.sessions(
            tcc,
            &[
                DemoSession(1, (20, 0), 70, TimerKind::Pomodoro),
                DemoSession(4, (14, 0), 120, TimerKind::Pomodoro),
                DemoSession(5, (14, 0), 120, TimerKind::Pomodoro),
            ],
        )?;
        let list = self.task(p, "lista de cálculo", &[], &[])?;
        self.sessions(list, &[DemoSession(6, (19, 0), 90, TimerKind::Stopwatch)])?;
        self.core.set_task_done(list, true)
    }

    fn pessoal(&mut self) -> CoreResult<()> {
        let p = self.core.create_project("pessoal", "#3fae6a")?;
        self.task(p, "academia", &[], &[])?;
        let read = self.task(p, "ler 20 páginas", &[], &[])?;
        self.pomodoro("Leitura", (30, 5, 20), read)?;
        self.sessions(read, &[DemoSession(3, (21, 0), 135, TimerKind::Pomodoro)])
    }

    /// A task with checked (`done`) and unchecked (`open`) subtasks.
    fn task(
        &mut self,
        p: ProjectId,
        name: &str,
        done: &[&str],
        open: &[&str],
    ) -> CoreResult<TaskId> {
        let task = self.core.create_task(Some(p), name)?;
        for name in done {
            let sub = self.core.add_subtask(task, name)?;
            self.core.set_subtask_done(sub, true)?;
        }
        for name in open {
            self.core.add_subtask(task, name)?;
        }
        Ok(task)
    }

    fn pomodoro(&mut self, name: &str, (f, s, l): (u64, u64, u64), task: TaskId) -> CoreResult<()> {
        self.core.create_pomodoro_preset(&NewPomodoroPreset {
            name,
            focus: minutes(f),
            short_break: minutes(s),
            long_break: minutes(l),
            focus_per_round: 4,
            auto_start: false,
            task: Some(task),
        })
    }

    fn sessions(&mut self, task: TaskId, sessions: &[DemoSession]) -> CoreResult<()> {
        for DemoSession(days_ago, (h, m), length, kind) in sessions {
            let Some(start) = self.local(*days_ago, *h, *m) else {
                continue;
            };
            let segment = Segment {
                start,
                end: start.plus(minutes(*length)),
            };
            let Some(session) = CompletedSession::new(*kind, vec![segment]) else {
                continue;
            };
            self.run += 1;
            self.core
                .record_session(Some(task), TimerId(self.run), &session)?;
        }
        Ok(())
    }

    /// `h:m` local time, `days_ago` days before the core's today.
    fn local(&self, days_ago: u64, h: u32, m: u32) -> Option<Timestamp> {
        let now = self
            .tz
            .timestamp_millis_opt(self.core.now().as_millis())
            .single()?;
        let day = now.date_naive().checked_sub_days(Days::new(days_ago))?;
        let at = day.and_time(NaiveTime::from_hms_opt(h, m, 0)?);
        let local = self.tz.from_local_datetime(&at).earliest()?;
        Some(Timestamp::from_millis(local.timestamp_millis()))
    }
}

/// M7b: a session with pauses and milestones, so the Sessão report and the Diário markers
/// have something to show. Friday 08:00 (two days before a Sunday), on "ler 20 páginas":
/// older than the other sessions of the Recentes list, so it leaves that list as it was.
impl<Tz: TimeZone> Seeder<'_, Tz> {
    fn reports(&mut self) -> CoreResult<()> {
        let Some(read) = self
            .core
            .search_tasks("ler 20 páginas")?
            .first()
            .map(|t| t.id)
        else {
            return Ok(());
        };
        let Some(start) = self.local(2, 8, 0) else {
            return Ok(());
        };
        self.run += 1;
        let run = TimerId(self.run);
        let at = |m: u64| start.plus(minutes(m));
        for (name, checked) in [
            ("Capítulo 3", Some(18)),
            ("Capítulo 4", Some(52)),
            ("Anotações", None),
        ] {
            let sub = self.core.add_subtask(read, name)?;
            if let Some(m) = checked {
                self.core.complete_subtask_in_run(sub, at(m), run)?;
            }
        }
        // 08:00–08:25, 08:30–08:50, 08:55–09:10: 60 min of reading, two 5-minute pauses.
        let segments = [(0, 25), (30, 50), (55, 70)]
            .map(|(a, b)| Segment {
                start: at(a),
                end: at(b),
            })
            .to_vec();
        let Some(session) = CompletedSession::new(TimerKind::Stopwatch, segments) else {
            return Ok(());
        };
        self.core.record_session(Some(read), run, &session)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use clocky_core::clock::FakeClock;

    use super::*;

    #[test]
    fn seeds_once() {
        let clock = FakeClock::new(Timestamp::from_millis(1_794_000_000_000));
        let mut core = AppCore::in_memory(Box::new(clock));
        assert!(seed_if_empty(&mut core, &Utc).unwrap());
        assert!(!seed_if_empty(&mut core, &Utc).unwrap());
        let projects = core.projects().unwrap();
        let names: Vec<_> = projects.iter().map(|p| p.project.name.as_str()).collect();
        assert_eq!(names, ["connect", "faculdade", "pessoal"]);
        assert_eq!(projects[1].completed_tasks, 1);
        let auth = core
            .task_summaries(Some(projects[0].project.id), false)
            .unwrap();
        assert_eq!(auth[0].tracked, minutes(220));
        assert_eq!((auth[0].subtasks_done, auth[0].subtasks_total), (1, 3));
    }

    #[test]
    fn timers_started_after_the_demo_get_unused_run_ids() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clocky.db");
        let (core, reopened) = AppCore::open(&path).unwrap();
        let (mut core, _) = seed_and_reopen(core, reopened, &path, &Utc).unwrap();
        let last_run: i64 = rusqlite::Connection::open(&path)
            .unwrap()
            .query_row("SELECT MAX(run_id) FROM sessions", [], |row| row.get(0))
            .unwrap();
        let now = core.now();
        let id = core.tracker_mut().start_stopwatch(None, now).unwrap();
        assert!(
            i64::try_from(id.0).unwrap() > last_run,
            "{id:?} reuses a demo run id"
        );
    }

    #[test]
    fn the_demo_needs_clocky_db() {
        let os = |s: &'static str| Some(std::ffi::OsStr::new(s));
        assert!(requested_from(os("1"), os("D:/tmp/clocky.db")));
        assert!(!requested_from(os("1"), None));
        assert!(!requested_from(os("1"), os("")));
        assert!(!requested_from(None, os("D:/tmp/clocky.db")));
        assert!(!requested_from(os("0"), os("D:/tmp/clocky.db")));
    }
}
