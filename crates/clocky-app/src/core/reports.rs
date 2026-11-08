//! M7b: the reports, the settings table and session deletion, on top of [`AppCore`].
//!
//! Report errors are not tracker errors, so these calls return [`ReportResult`]; the manage
//! window logs either kind the same way.

use std::time::Duration;

use chrono::{NaiveDate, TimeZone};
use clocky_core::domain::{SessionId, SubtaskId, TimerId};
use clocky_core::reports::{self, Daily, Overview, Period, ReportError, SessionReport, Weekly};
use clocky_core::time::Timestamp;

use super::{AppCore, CoreResult};

/// Result of a report read.
pub type ReportResult<T> = Result<T, ReportError>;

/// Reports (spec, section 6). Periods are local to `tz`.
impl AppCore {
    pub fn overview(&self, period: Period) -> ReportResult<Overview> {
        reports::overview(self.store(), period)
    }

    pub fn weekly<Tz: TimeZone>(&self, date: NaiveDate, tz: &Tz) -> ReportResult<Weekly> {
        reports::weekly(self.store(), date, tz)
    }

    pub fn daily<Tz: TimeZone>(&self, date: NaiveDate, tz: &Tz) -> ReportResult<Daily> {
        reports::daily(self.store(), date, tz)
    }

    pub fn session_report(&self, id: SessionId) -> ReportResult<SessionReport> {
        reports::session_report(self.store(), id)
    }

    /// Deletes a recorded session ("Apagar sessão"); its subtask events stay, without it.
    pub fn delete_session(&mut self, id: SessionId) -> CoreResult<()> {
        Ok(self.store_mut().delete_session(id)?)
    }
}

/// The `settings` table (spec, section 3).
impl AppCore {
    /// A setting; `None` when it was never set.
    pub fn setting(&self, key: &str) -> CoreResult<Option<String>> {
        Ok(self.store().setting(key)?)
    }

    pub fn set_setting(&mut self, key: &str, value: &str) -> CoreResult<()> {
        Ok(self.store_mut().set_setting(key, value)?)
    }

    /// Stopwatch limit of the "Cronômetro esquecido?" dialog.
    pub fn forgotten_limit(&self) -> CoreResult<Duration> {
        Ok(self.store().forgotten_limit()?)
    }

    pub fn set_forgotten_limit(&mut self, limit: Duration) -> CoreResult<()> {
        Ok(self.store_mut().set_forgotten_limit(limit)?)
    }
}

/// Demo data only: a subtask checked inside timer `run`, so the session recorded next for that
/// run gets it as a milestone.
impl AppCore {
    pub fn complete_subtask_in_run(
        &mut self,
        id: SubtaskId,
        at: Timestamp,
        run: TimerId,
    ) -> CoreResult<()> {
        Ok(self.store_mut().complete_subtask(id, at, Some(run))?)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use clocky_core::clock::FakeClock;
    use clocky_core::engine::run::Segment;
    use clocky_core::engine::session::{CompletedSession, TimerKind};

    use super::*;

    fn at(mins: i64) -> Timestamp {
        Timestamp::from_millis(mins * 60_000)
    }

    fn core() -> AppCore {
        AppCore::in_memory(Box::new(FakeClock::new(at(24 * 60))))
    }

    #[test]
    fn settings_roundtrip_through_the_core() {
        let mut core = core();
        assert_eq!(core.setting("theme").unwrap(), None);
        core.set_setting("theme", "light").unwrap();
        assert_eq!(core.setting("theme").unwrap().as_deref(), Some("light"));
        core.set_forgotten_limit(Duration::from_hours(2)).unwrap();
        assert_eq!(core.forgotten_limit().unwrap(), Duration::from_hours(2));
    }

    #[test]
    fn a_recorded_session_shows_in_the_reports_until_deleted() {
        let mut core = core();
        let task = core.create_task(None, "x").unwrap();
        let sub = core.add_subtask(task, "a").unwrap();
        core.complete_subtask_in_run(sub, at(10), TimerId(1))
            .unwrap();
        let session = CompletedSession::new(
            TimerKind::Stopwatch,
            vec![
                Segment {
                    start: at(0),
                    end: at(5),
                },
                Segment {
                    start: at(8),
                    end: at(20),
                },
            ],
        )
        .unwrap();
        core.record_session(Some(task), TimerId(1), &session)
            .unwrap();
        let day = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
        let daily = core.daily(day, &Utc).unwrap();
        assert_eq!(daily.blocks.len(), 1);
        let id = daily.blocks[0].session;
        let report = core.session_report(id).unwrap();
        assert_eq!(report.pauses.len(), 1);
        assert_eq!(report.milestones.len(), 1);
        let week = core.weekly(day, &Utc).unwrap();
        assert_eq!(week.total, Duration::from_mins(17));
        let overview = core.overview(daily.period).unwrap();
        assert_eq!(overview.total, Duration::from_mins(17));
        core.delete_session(id).unwrap();
        assert!(core.session_report(id).is_err());
        assert!(core.daily(day, &Utc).unwrap().blocks.is_empty());
    }
}
