//! Sound and desktop notification of a ringing alarm (spec, section 5). [`SystemRinger`] loops
//! the sound with rodio and notifies with notify-rust; screenshot runs use [`SilentRinger`]
//! (no audio and no registry write). Both degrade to a log line: the overlay is what the user
//! always gets.

use std::cell::RefCell;
use std::path::PathBuf;

use crate::{notify, sound};

/// Notice shown in the overlay (or the editor) when a custom sound cannot be played.
pub fn fallback_notice(path: &str) -> String {
    let name = super::summary::sound_label(Some(path));
    format!("Não foi possível tocar “{name}”; tocando o som padrão.")
}

/// Whether a custom sound can be played; `Err` carries the notice for the user.
fn check_file(path: &str) -> Result<PathBuf, String> {
    let file = PathBuf::from(path);
    sound::open_file(&file).map(|_| file).map_err(|e| {
        tracing::warn!("cannot play the alarm sound {path}: {e:#}");
        fallback_notice(path)
    })
}

/// The sound an alarm plays: its own file, else the `alarm.default_sound` setting of
/// Configurações, else the built-in chime (`None`). Blank values count as unset.
pub fn effective_sound(own: Option<&str>, default: Option<&str>) -> Option<String> {
    let set = |s: Option<&str>| {
        s.map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    set(own).or_else(|| set(default))
}

pub trait Ringer {
    /// Starts the looping sound of a ringing alarm (replacing any other). Returns a notice when
    /// its custom file failed and the built-in sound plays instead.
    fn start(&self, sound: Option<&str>) -> Option<String>;
    /// Stops the looping sound.
    fn stop(&self);
    /// Plays the sound once (the editor's ▶). Returns a notice as [`Ringer::start`].
    fn preview(&self, sound: Option<&str>) -> Option<String>;
    /// Shows the desktop notification.
    fn notify(&self, title: &str, body: &str);
}

/// Real audio and notifications.
#[derive(Default)]
pub struct SystemRinger {
    looping: RefCell<Option<sound::Looping>>,
}

impl Ringer for SystemRinger {
    fn start(&self, sound: Option<&str>) -> Option<String> {
        let (file, notice) = match sound {
            None => (None, None),
            Some(path) => match check_file(path) {
                Ok(file) => (Some(file), None),
                Err(notice) => (None, Some(notice)),
            },
        };
        // Stop the previous loop before the next one opens the device.
        self.stop();
        *self.looping.borrow_mut() = Some(sound::play_looping(file, sound::ALARM_AUTO_STOP));
        notice
    }

    fn stop(&self) {
        self.looping.borrow_mut().take();
    }

    fn preview(&self, sound: Option<&str>) -> Option<String> {
        match sound.map(check_file) {
            None => {
                sound::play_default();
                None
            }
            Some(Ok(file)) => {
                sound::play_file(file);
                None
            }
            Some(Err(notice)) => {
                sound::play_default();
                Some(notice)
            }
        }
    }

    fn notify(&self, title: &str, body: &str) {
        notify::show(title, body);
    }
}

/// Logs only (screenshot runs and tests). A custom file is still checked, so the fallback
/// notice can be seen.
pub struct SilentRinger;

impl Ringer for SilentRinger {
    fn start(&self, sound: Option<&str>) -> Option<String> {
        tracing::info!("alarm sound (silent): {sound:?}");
        sound.and_then(|path| check_file(path).err())
    }

    fn stop(&self) {
        tracing::info!("alarm sound stops (silent)");
    }

    fn preview(&self, sound: Option<&str>) -> Option<String> {
        self.start(sound)
    }

    fn notify(&self, title: &str, body: &str) {
        tracing::info!("alarm notification (silent): {title} / {body}");
    }
}

/// Title and body of the desktop notification: the alarm name, then the notification text (or
/// "Alarme das 07:30"), the late tag and the task.
pub fn notification(ring: &super::queue::Ringing, task: Option<&str>) -> (String, String) {
    let title = super::summary::display_name(&ring.name).to_owned();
    let mut parts = vec![if ring.message.trim().is_empty() {
        format!("Alarme das {}", ring.time)
    } else {
        ring.message.trim().to_owned()
    }];
    if let Some(by) = ring.late {
        parts.push(super::summary::late_label(by));
    }
    if let Some(task) = task {
        parts.push(task.to_owned());
    }
    (title, parts.join(" · "))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use clocky_core::alarms::AlarmTime;
    use clocky_core::domain::AlarmId;
    use clocky_core::time::Timestamp;

    use super::*;
    use crate::alarms::queue::Ringing;

    fn ring(message: &str, late: Option<Duration>) -> Ringing {
        Ringing {
            id: AlarmId(1),
            due: Timestamp::from_millis(0),
            name: "Daily standup".into(),
            message: message.into(),
            time: AlarmTime::new(7, 30).unwrap(),
            task: None,
            sound: None,
            snooze_minutes: 5,
            late,
        }
    }

    #[test]
    fn notification_has_name_text_late_tag_and_task() {
        let (title, body) = notification(&ring("Hora da daily!", None), Some("connect › standup"));
        assert_eq!(title, "Daily standup");
        assert_eq!(body, "Hora da daily! · connect › standup");
        let (_, body) = notification(&ring(" ", Some(Duration::from_mins(12))), None);
        assert_eq!(body, "Alarme das 07:30 · atrasado 12 min");
    }

    #[test]
    fn own_sound_wins_over_the_default_setting_and_blank_means_unset() {
        assert_eq!(
            effective_sound(Some("C:/galo.mp3"), Some("C:/sino.wav")).as_deref(),
            Some("C:/galo.mp3")
        );
        assert_eq!(
            effective_sound(None, Some(" C:/sino.wav ")).as_deref(),
            Some("C:/sino.wav")
        );
        assert_eq!(
            effective_sound(Some("  "), Some("C:/sino.wav")).as_deref(),
            Some("C:/sino.wav")
        );
        assert_eq!(effective_sound(None, Some("")), None);
        assert_eq!(effective_sound(None, None), None);
    }

    #[test]
    fn a_missing_custom_sound_falls_back_with_a_notice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("galo.mp3");
        assert!(!path.exists());
        let notice = SilentRinger.start(path.to_str()).unwrap();
        assert_eq!(
            notice,
            "Não foi possível tocar “galo.mp3”; tocando o som padrão."
        );
        assert_eq!(SilentRinger.start(None), None);
        assert!(SilentRinger.preview(path.to_str()).is_some());
    }
}
