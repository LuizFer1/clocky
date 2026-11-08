//! Demo data for the screenshot hook (`CLOCKY_DEMO=1` with `CLOCKY_SCREENSHOT_DIR`), written
//! through the tracker and store API into the database `CLOCKY_DB` points at (use a temp file).
//!
//! Merge note (M6a): M6a seeds its own demo rows for the manage window in `core.rs`; keep the
//! two seeders apart or run them one after the other on the same temp database.

use std::time::Duration;

use clocky_core::domain::TimerId;
use clocky_core::engine::pomodoro::PomodoroConfig;
use clocky_core::store::NewPomodoroPreset;

use crate::core::AppCore;

/// Environment variable that turns the demo data on (screenshot runs only).
pub const ENV_VAR: &str = "CLOCKY_DEMO";

pub fn enabled() -> bool {
    std::env::var_os(ENV_VAR).is_some_and(|v| v == "1")
}

/// Timers the screenshots select.
#[derive(Debug, Clone, Copy)]
pub struct DemoTimers {
    pub countdown: TimerId,
    pub focus: TimerId,
}

fn mins(m: u64) -> Duration {
    Duration::from_mins(m)
}

fn ago(handle: &AppCore, secs: i64) -> clocky_core::time::Timestamp {
    clocky_core::time::Timestamp::from_millis(handle.now().as_millis() - secs * 1000)
}

/// Projects, tasks with subtasks, presets and running timers like the handoff screens. The
/// tea countdown and the deep-work focus are already due: the next tick rings them.
pub fn seed(handle: &mut AppCore) -> anyhow::Result<DemoTimers> {
    let now = handle.now();
    let store = handle.tracker_mut().store_mut();
    let connect = store.create_project("connect", "#3d8fd6", now)?;
    let faculdade = store.create_project("faculdade", "#9b7be0", now)?;
    let auth = store.create_task(Some(connect), "autenticação", now)?;
    let tests = store.create_task(Some(connect), "testes", now)?;
    let review = store.create_task(Some(connect), "code review", now)?;
    let tcc = store.create_task(Some(faculdade), "TCC", now)?;
    let login = store.add_subtask(auth, "Endpoint de login")?;
    store.add_subtask(auth, "Testes de integração")?;
    store.add_subtask(auth, "Atualizar documentação")?;
    store.complete_subtask(login, now, None)?;
    let pipeline = store.create_timer_preset("pipeline", mins(12), Some(tests))?;
    let tea = store.create_timer_preset("chá", mins(4), None)?;
    store.create_timer_preset("revisão de PR", mins(25), Some(review))?;
    let pomodoro = |name, focus, short, long, rounds, task| NewPomodoroPreset {
        name,
        focus: mins(focus),
        short_break: mins(short),
        long_break: mins(long),
        focus_per_round: rounds,
        auto_start: false,
        task,
    };
    let classic = store.create_pomodoro_preset(&pomodoro("Clássico", 25, 5, 15, 4, Some(auth)))?;
    let deep = store.create_pomodoro_preset(&pomodoro("Deep work", 50, 10, 30, 3, Some(tcc)))?;
    store.create_pomodoro_preset(&pomodoro("Leitura", 30, 5, 20, 4, None))?;

    let classic_config = PomodoroConfig::new(mins(25), mins(5), mins(15), 4, false)?;
    let deep_config = PomodoroConfig::new(mins(50), mins(10), mins(30), 3, false)?;
    let (sw_at, cd_at, tea_at) = (ago(handle, 4_330), ago(handle, 3), ago(handle, 4 * 60 + 20));
    let (focus_at, deep_at) = (ago(handle, 630), ago(handle, 50 * 60 + 8));
    let tracker = handle.tracker_mut();
    tracker.start_stopwatch(Some(auth), sw_at)?;
    let countdown = tracker.start_countdown(Some(tests), mins(12), Some(pipeline), cd_at)?;
    tracker.start_countdown(None, mins(4), Some(tea), tea_at)?;
    let focus = tracker.start_pomodoro(Some(auth), classic_config, Some(classic), focus_at)?;
    tracker.start_pomodoro(Some(tcc), deep_config, Some(deep), deep_at)?;
    Ok(DemoTimers { countdown, focus })
}

#[cfg(test)]
mod tests {
    use clocky_core::clock::FakeClock;
    use clocky_core::engine::timers::Timer;
    use clocky_core::store::Store;
    use clocky_core::time::Timestamp;

    use super::*;

    #[test]
    fn seed_starts_five_timers_and_the_due_ones_finish_on_the_next_tick() {
        let clock = Box::new(FakeClock::new(Timestamp::from_millis(1_794_159_120_000)));
        let store = Store::open_in_memory().unwrap();
        let mut handle = AppCore::with_store(store, clock).unwrap().0;
        let demo = seed(&mut handle).unwrap();
        assert_eq!(handle.tracker().timers().iter().count(), 5);
        let now = handle.now();
        let events = handle.tracker_mut().tick(now).unwrap();
        assert_ne!(events, Vec::new());
        // The tea countdown left the set; the deep-work pomodoro waits for its break.
        assert_eq!(handle.tracker().timers().iter().count(), 4);
        let waiting = handle.tracker().timers().iter().any(|a| match &a.timer {
            Timer::Pomodoro(p) => p.is_waiting(),
            _ => false,
        });
        assert!(waiting, "the deep-work pomodoro should wait for its break");
        assert!(handle.tracker().timers().get(demo.countdown).is_some());
        assert!(handle.tracker().timers().get(demo.focus).is_some());
    }
}
