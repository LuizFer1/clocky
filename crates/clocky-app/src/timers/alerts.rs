//! What the user is told when a timer finishes: the sound plus a notification that carries the
//! task name (spec, section 4), and the notices about what happened while the app was closed.

use std::collections::HashMap;

use clocky_core::domain::{TaskId, TimerId};
use clocky_core::engine::pomodoro::Phase;
use clocky_core::engine::timers::TimersEvent;
use clocky_core::time::Timestamp;
use clocky_core::tracker::TrackerEvent;

use super::cards::phase_label;
use crate::{notify, sound};

/// Name and task of a timer, captured before a tick (a finished countdown leaves the set).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimerInfo {
    /// Preset name, else the kind ("Temporizador", "Pomodoro").
    pub name: String,
    /// "connect › testes"; `None` = no task.
    pub task: Option<String>,
    pub task_id: Option<TaskId>,
}

impl TimerInfo {
    fn task_text(&self) -> &str {
        self.task.as_deref().unwrap_or("Sem tarefa")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    pub title: String,
    pub body: String,
}

/// The alert for an engine event, if it deserves one.
pub fn alert_for(event: &TimersEvent, info: &TimerInfo) -> Option<Alert> {
    match event {
        TimersEvent::CountdownFinished { .. } => Some(Alert {
            title: format!("{} terminou", info.name),
            body: info.task_text().to_owned(),
        }),
        TimersEvent::PomodoroPhaseEnded {
            ended,
            next,
            auto_started,
            ..
        } => Some(Alert {
            title: format!("{} terminou", phase_label(*ended)),
            body: format!(
                "{} · {} · {}",
                info.task_text(),
                info.name,
                next_text(*next, *auto_started)
            ),
        }),
        TimersEvent::SessionCompleted { .. } => None,
    }
}

fn next_text(next: Phase, auto_started: bool) -> String {
    if auto_started {
        format!("{} começou", phase_label(next))
    } else {
        format!("próxima: {}", phase_label(next).to_lowercase())
    }
}

/// Notices for the events of the reopen tick ("Temporizador terminou às HH:MM").
pub fn reopen_notices(
    events: &[TrackerEvent],
    infos: &HashMap<TimerId, TimerInfo>,
    hh_mm: impl Fn(Timestamp) -> String,
) -> Vec<String> {
    let mut focus_end: HashMap<TimerId, Timestamp> = HashMap::new();
    let mut notices = Vec::new();
    let name = |timer: &TimerId| {
        infos
            .get(timer)
            .map_or_else(|| "Timer".to_owned(), |i| i.name.clone())
    };
    for event in events {
        let TrackerEvent::Timer(event) = event else {
            continue;
        };
        match event {
            TimersEvent::CountdownFinished { timer, at } => {
                notices.push(format!("{} terminou às {}", name(timer), hh_mm(*at)));
            }
            TimersEvent::SessionCompleted {
                timer,
                session,
                round: Some(_),
                ..
            } => {
                focus_end.insert(*timer, session.ended_at());
            }
            TimersEvent::PomodoroPhaseEnded { timer, ended, .. } => {
                let phase = phase_label(*ended).to_lowercase();
                notices.push(match focus_end.get(timer) {
                    Some(at) => format!("{}: {phase} terminou às {}", name(timer), hh_mm(*at)),
                    None => format!("{}: {phase} terminou com o Clocky fechado", name(timer)),
                });
            }
            TimersEvent::SessionCompleted { .. } => {}
        }
    }
    notices
}

/// Where alerts go. The real one plays the sound and shows a notification; screenshot runs and
/// tests use [`SilentAlerts`] (the real one writes the `AppUserModelId` registry key).
pub trait Alerts {
    fn alert(&self, alert: &Alert);
}

/// Sound plus notification. Both degrade to a log line on failure (spec, section 5).
pub struct SystemAlerts;

impl Alerts for SystemAlerts {
    fn alert(&self, alert: &Alert) {
        sound::play_default();
        notify::show(&alert.title, &alert.body);
    }
}

/// Only logs.
pub struct SilentAlerts;

impl Alerts for SilentAlerts {
    fn alert(&self, alert: &Alert) {
        tracing::info!("alert (silent): {} / {}", alert.title, alert.body);
    }
}

#[cfg(test)]
mod tests {
    use clocky_core::engine::run::Segment;
    use clocky_core::engine::session::{CompletedSession, TimerKind};

    use super::*;

    fn info(name: &str, task: Option<&str>) -> TimerInfo {
        TimerInfo {
            name: name.into(),
            task: task.map(Into::into),
            task_id: None,
        }
    }

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_millis(secs * 1000)
    }

    #[test]
    fn countdown_alert_names_timer_and_task() {
        let event = TimersEvent::CountdownFinished {
            timer: TimerId(1),
            at: at(60),
        };
        let alert = alert_for(&event, &info("pipeline", Some("connect › testes"))).unwrap();
        assert_eq!(alert.title, "pipeline terminou");
        assert_eq!(alert.body, "connect › testes");
        let none = alert_for(&event, &info("chá", None)).unwrap();
        assert_eq!(none.body, "Sem tarefa");
    }

    #[test]
    fn phase_alert_carries_the_task_first() {
        let event = TimersEvent::PomodoroPhaseEnded {
            timer: TimerId(2),
            ended: Phase::Focus,
            next: Phase::ShortBreak,
            auto_started: false,
        };
        let alert = alert_for(&event, &info("Clássico", Some("connect › trabalho"))).unwrap();
        assert_eq!(alert.title, "Foco terminou");
        assert_eq!(
            alert.body,
            "connect › trabalho · Clássico · próxima: pausa curta"
        );
    }

    #[test]
    fn auto_started_phase_says_it_began() {
        let event = TimersEvent::PomodoroPhaseEnded {
            timer: TimerId(2),
            ended: Phase::ShortBreak,
            next: Phase::Focus,
            auto_started: true,
        };
        let alert = alert_for(&event, &info("Clássico", None)).unwrap();
        assert_eq!(alert.title, "Pausa curta terminou");
        assert!(alert.body.ends_with("Foco começou"));
    }

    #[test]
    fn reopen_notices_tell_when_things_ended() {
        let session = CompletedSession::new(
            TimerKind::Pomodoro,
            vec![Segment {
                start: at(0),
                end: at(1500),
            }],
        )
        .unwrap();
        let events = vec![
            TrackerEvent::Timer(TimersEvent::CountdownFinished {
                timer: TimerId(1),
                at: at(60),
            }),
            TrackerEvent::Timer(TimersEvent::SessionCompleted {
                timer: TimerId(2),
                task: None,
                session,
                round: Some(1),
            }),
            TrackerEvent::Timer(TimersEvent::PomodoroPhaseEnded {
                timer: TimerId(2),
                ended: Phase::Focus,
                next: Phase::ShortBreak,
                auto_started: false,
            }),
            TrackerEvent::Timer(TimersEvent::PomodoroPhaseEnded {
                timer: TimerId(3),
                ended: Phase::LongBreak,
                next: Phase::Focus,
                auto_started: false,
            }),
        ];
        let infos = HashMap::from([
            (TimerId(1), info("chá", None)),
            (TimerId(2), info("Clássico", None)),
        ]);
        let notices = reopen_notices(&events, &infos, |t| format!("{}s", t.as_millis() / 1000));
        assert_eq!(
            notices,
            [
                "chá terminou às 60s",
                "Clássico: foco terminou às 1500s",
                "Timer: pausa longa terminou com o Clocky fechado",
            ]
        );
    }
}
