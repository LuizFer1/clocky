//! Demo alarms of the handoff screens, seeded with the demo projects of [`crate::demo`] (same
//! guard: `CLOCKY_DEMO=1` with `CLOCKY_DB` set and an empty database, or the screenshot hook).

use clocky_core::alarms::{AlarmTime, Weekdays};
use clocky_core::domain::TaskId;
use clocky_core::store::NewAlarm;

use crate::core::{AppCore, CoreResult};

/// One demo alarm: name, time, days, message, snooze, task name and whether it stays on.
struct DemoAlarm(
    &'static str,
    (u8, u8),
    Weekdays,
    &'static str,
    u32,
    &'static str,
    bool,
);

const ALARMS: [DemoAlarm; 3] = [
    DemoAlarm(
        "Daily standup",
        (7, 30),
        Weekdays::WEEKDAYS,
        "Hora da daily!",
        5,
        "standup diário",
        true,
    ),
    DemoAlarm(
        "Enviar capítulo",
        (16, 0),
        Weekdays::ONCE,
        "Mandar o capítulo 2 ao orientador",
        10,
        "TCC",
        true,
    ),
    DemoAlarm(
        "Academia",
        (18, 30),
        Weekdays::WEEKEND,
        "",
        5,
        "academia",
        false,
    ),
];

/// Adds the demo alarms when there is none yet. Tasks are looked up by name (a missing one
/// leaves the alarm without a task).
///
/// # Errors
/// When the database fails.
pub fn seed(core: &mut AppCore) -> CoreResult<()> {
    if !core.alarms()?.is_empty() {
        return Ok(());
    }
    for DemoAlarm(name, (h, m), repeat, message, snooze, task, enabled) in ALARMS {
        let Ok(time) = AlarmTime::new(h, m) else {
            continue;
        };
        let task = find_task(core, task)?;
        let id = core.create_alarm(&NewAlarm {
            name,
            time,
            repeat,
            sound: None,
            message,
            snooze_minutes: snooze,
            task,
        })?;
        if !enabled {
            core.set_alarm_enabled(id, false)?;
        }
    }
    Ok(())
}

fn find_task(core: &AppCore, name: &str) -> CoreResult<Option<TaskId>> {
    Ok(core
        .search_tasks(name)?
        .into_iter()
        .find(|t| t.name == name)
        .map(|t| t.id))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use clocky_core::clock::FakeClock;
    use clocky_core::time::Timestamp;

    use super::*;

    #[test]
    fn seeds_three_alarms_on_the_demo_tasks_once() {
        let clock = FakeClock::new(Timestamp::from_millis(1_794_159_120_000));
        let mut core = AppCore::in_memory(Box::new(clock));
        assert!(crate::demo::seed_if_empty(&mut core, &Utc).unwrap());
        seed(&mut core).unwrap();
        let alarms = core.alarms().unwrap();
        assert_eq!(alarms.len(), 3);
        assert!(alarms.iter().all(|a| a.task.is_some()));
        assert_eq!(alarms.iter().filter(|a| a.enabled).count(), 2);
        let standup = alarms.iter().find(|a| a.name == "Daily standup").unwrap();
        let links = core.task_links(standup.task.unwrap()).unwrap();
        assert_eq!(links.alarms.len(), 1);
    }

    #[test]
    fn deleting_the_task_of_an_alarm_keeps_the_alarm_without_a_task() {
        let clock = FakeClock::new(Timestamp::from_millis(1_794_159_120_000));
        let mut core = AppCore::in_memory(Box::new(clock));
        crate::demo::seed_if_empty(&mut core, &Utc).unwrap();
        let standup = core.alarms().unwrap()[0].clone();
        core.delete_task(standup.task.unwrap()).unwrap();
        let alarm = core.alarm(standup.id).unwrap();
        assert_eq!((alarm.name.as_str(), alarm.task), ("Daily standup", None));
    }
}
