//! Batched writes of the tracker: the sessions and active-timer changes of one command,
//! committed in a single transaction (design decisions 7 and 8).

use super::active::{delete_active_tx, save_active_tx};
use super::sessions::record_session_tx;
use super::{Store, StoreError};
use crate::domain::{SessionId, TaskId, TimerId};
use crate::engine::session::CompletedSession;
use crate::engine::timers::ActiveTimer;

/// A session to record as part of [`TimerWrites`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct SessionToRecord<'a> {
    pub(crate) task: Option<TaskId>,
    pub(crate) run: TimerId,
    pub(crate) round: Option<u32>,
    pub(crate) session: &'a CompletedSession,
}

/// Everything one tracker command writes, committed in a single transaction.
#[derive(Debug, Default)]
pub(crate) struct TimerWrites<'a> {
    /// Sessions to record (written first, so subtask events get linked).
    pub(crate) sessions: Vec<SessionToRecord<'a>>,
    /// Active timers to insert or update.
    pub(crate) save: Vec<&'a ActiveTimer>,
    /// Active timers that left the set.
    pub(crate) delete: Vec<TimerId>,
}

impl TimerWrites<'_> {
    /// Nothing to write.
    pub(crate) fn is_empty(&self) -> bool {
        self.sessions.is_empty() && self.save.is_empty() && self.delete.is_empty()
    }
}

impl Store {
    /// Commits the sessions and active-timer changes of one tracker command in a single
    /// transaction (decisions 7 and 8); returns the ids of the recorded sessions, in order.
    pub(crate) fn write_timers(
        &mut self,
        writes: &TimerWrites<'_>,
    ) -> Result<Vec<SessionId>, StoreError> {
        let tx = self.conn.transaction()?;
        let mut ids = Vec::with_capacity(writes.sessions.len());
        for s in &writes.sessions {
            ids.push(record_session_tx(&tx, s.task, s.run, s.round, s.session)?);
        }
        for active in &writes.save {
            save_active_tx(&tx, active)?;
        }
        for &id in &writes.delete {
            delete_active_tx(&tx, id)?;
        }
        tx.commit()?;
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::session::TimerKind;
    use crate::engine::stopwatch::Stopwatch;
    use crate::engine::timers::Timer;
    use crate::test_util::{at, completed};

    fn stopwatch(id: u64, task: Option<TaskId>) -> ActiveTimer {
        ActiveTimer::new(
            TimerId(id),
            task,
            None,
            Timer::Stopwatch(Stopwatch::start(at(0))),
        )
    }

    fn count(store: &Store, table: &str) -> i64 {
        store
            .conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    #[test]
    fn write_timers_is_all_or_nothing() {
        let mut store = Store::open_in_memory().unwrap();
        store.save_active(&stopwatch(1, None)).unwrap();
        let span = completed(TimerKind::Stopwatch, &[(0, 10)]);
        let bad_task = stopwatch(2, Some(TaskId(999)));
        let writes = TimerWrites {
            sessions: vec![SessionToRecord {
                task: None,
                run: TimerId(1),
                round: None,
                session: &span,
            }],
            save: vec![&bad_task],
            delete: vec![TimerId(1)],
        };
        assert!(!writes.is_empty());
        assert!(matches!(
            store.write_timers(&writes),
            Err(StoreError::Sqlite(_))
        ));
        assert_eq!(count(&store, "sessions"), 0);
        assert_eq!(count(&store, "active_timers"), 1);
        assert!(TimerWrites::default().is_empty());
    }
}
