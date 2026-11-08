//! Running timers for the screenshot hook, on top of the demo projects of [`crate::demo`]
//! (same guard: `CLOCKY_DEMO=1` with `CLOCKY_DB` set, same database). It looks the demo tasks
//! and presets up by name, adds the "chá" preset and starts the timers of the handoff screens.

use std::time::Duration;

use anyhow::Context as _;
use clocky_core::domain::{PomodoroPresetId, TaskId, TimerId, TimerPresetId};
use clocky_core::store::{PomodoroPreset, Store};
use clocky_core::time::Timestamp;

use super::frame_pomodoro::preset_config;
use crate::core::AppCore;

/// Timers the screenshots select.
#[derive(Debug, Clone, Copy)]
pub struct DemoTimers {
    pub countdown: TimerId,
    pub focus: TimerId,
}

fn mins(m: u64) -> Duration {
    Duration::from_mins(m)
}

fn ago(now: Timestamp, secs: i64) -> Timestamp {
    Timestamp::from_millis(now.as_millis() - secs * 1000)
}

/// Starts a stopwatch, two countdowns and two pomodoros like the handoff screens. The tea
/// countdown and the deep-work focus are already due: the next tick rings them. Returns `None`
/// (and changes nothing) when timers already run, so a second run on the same database adds
/// nothing.
///
/// # Errors
/// When the demo projects of [`crate::demo`] are missing or the database fails.
pub fn seed(core: &mut AppCore) -> anyhow::Result<Option<DemoTimers>> {
    if core.tracker().timers().iter().next().is_some() {
        return Ok(None);
    }
    let now = core.now();
    let store = core.tracker_mut().store_mut();
    let task = |store: &Store, name: &str| find_task(store, name);
    let (auth, tests, tcc) = (
        task(store, "autenticação")?,
        task(store, "testes")?,
        task(store, "TCC")?,
    );
    let pipeline = timer_preset(store, "pipeline")?;
    let tea = store.create_timer_preset("chá", mins(4), None)?;
    let classic = pomodoro_preset(store, "Clássico")?;
    let deep = pomodoro_preset(store, "Deep work")?;
    let config = |p: &PomodoroPreset| preset_config(p).context("invalid demo pomodoro preset");
    let (classic_config, deep_config) = (config(&classic)?, config(&deep)?);
    let tracker = core.tracker_mut();
    tracker.start_stopwatch(Some(auth), ago(now, 4_330))?;
    let countdown = tracker.start_countdown(Some(tests), mins(12), Some(pipeline), ago(now, 3))?;
    tracker.start_countdown(None, mins(4), Some(tea), ago(now, 4 * 60 + 20))?;
    let focus = start_pomodoro(core, auth, &classic, classic_config, ago(now, 630))?;
    // Due a few seconds ago (its focus is `deep.focus` long).
    let deep_at = ago(now, i64::try_from(deep.focus.as_secs())? + 8);
    start_pomodoro(core, tcc, &deep, deep_config, deep_at)?;
    Ok(Some(DemoTimers { countdown, focus }))
}

fn start_pomodoro(
    core: &mut AppCore,
    task: TaskId,
    preset: &PomodoroPreset,
    config: clocky_core::engine::pomodoro::PomodoroConfig,
    at: Timestamp,
) -> anyhow::Result<TimerId> {
    let preset: PomodoroPresetId = preset.id;
    Ok(core
        .tracker_mut()
        .start_pomodoro(Some(task), config, Some(preset), at)?)
}

fn find_task(store: &Store, name: &str) -> anyhow::Result<TaskId> {
    store
        .search_tasks(name)?
        .into_iter()
        .find(|t| t.name == name)
        .map(|t| t.id)
        .with_context(|| format!("demo task {name:?} is missing"))
}

fn timer_preset(store: &Store, name: &str) -> anyhow::Result<TimerPresetId> {
    store
        .timer_presets()?
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.id)
        .with_context(|| format!("demo timer {name:?} is missing"))
}

fn pomodoro_preset(store: &Store, name: &str) -> anyhow::Result<PomodoroPreset> {
    store
        .pomodoro_presets()?
        .into_iter()
        .find(|p| p.name == name)
        .with_context(|| format!("demo pomodoro {name:?} is missing"))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use clocky_core::clock::FakeClock;
    use clocky_core::engine::timers::Timer;

    use super::*;

    #[test]
    fn seed_starts_five_timers_on_the_demo_projects_and_only_once() {
        let clock = FakeClock::new(Timestamp::from_millis(1_794_159_120_000));
        let mut core = AppCore::in_memory(Box::new(clock));
        assert!(seed(&mut core).is_err(), "needs the demo projects");
        assert!(crate::demo::seed_if_empty(&mut core, &Utc).unwrap());
        let demo = seed(&mut core).unwrap().unwrap();
        assert_eq!(core.tracker().timers().iter().count(), 5);
        assert!(seed(&mut core).unwrap().is_none());
        assert_eq!(core.tracker().timers().iter().count(), 5);
        // No second "connect" project.
        assert_eq!(core.projects().unwrap().len(), 3);
        let now = core.now();
        let events = core.tracker_mut().tick(now).unwrap();
        assert_ne!(events, Vec::new());
        // The tea countdown left the set; the deep-work pomodoro waits for its break.
        assert_eq!(core.tracker().timers().iter().count(), 4);
        let waiting = core.tracker().timers().iter().any(|a| match &a.timer {
            Timer::Pomodoro(p) => p.is_waiting(),
            _ => false,
        });
        assert!(waiting, "the deep-work pomodoro should wait for its break");
        assert!(core.tracker().timers().get(demo.countdown).is_some());
        assert!(core.tracker().timers().get(demo.focus).is_some());
    }
}
