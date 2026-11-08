use clocky_core::clock::FakeClock;
use clocky_core::time::Timestamp;

use super::*;

fn core() -> AppCore {
    AppCore::in_memory(Box::new(FakeClock::new(Timestamp::from_millis(0))))
}

#[test]
fn fresh_database_shows_the_defaults() {
    let form = SettingsForm::load(&core());
    assert_eq!(
        form,
        SettingsForm {
            theme: ThemeMode::Dark,
            forgotten_minutes: 8 * 60,
            wizard: [25, 5, 15, 4],
            alarm_sound: String::new(),
        }
    );
    assert_eq!(form.forgotten_label(), "8h");
}

#[test]
fn theme_is_saved_and_reported() {
    let mut core = core();
    assert_eq!(
        set_theme(&mut core, ThemeMode::Light).unwrap(),
        Applied::Theme(ThemeMode::Light)
    );
    assert_eq!(SettingsForm::load(&core).theme, ThemeMode::Light);
    assert_eq!(core.setting("theme").unwrap().as_deref(), Some("light"));
    set_theme(&mut core, ThemeMode::System).unwrap();
    assert_eq!(SettingsForm::load(&core).theme, ThemeMode::System);
}

#[test]
fn forgotten_limit_steps_by_half_hours_within_limits() {
    let mut core = core();
    assert_eq!(step_forgotten(&mut core, 1).unwrap(), Applied::Stored);
    assert_eq!(
        core.forgotten_limit().unwrap(),
        Duration::from_mins(8 * 60 + 30)
    );
    assert_eq!(SettingsForm::load(&core).forgotten_label(), "8h 30min");
    core.set_forgotten_limit(Duration::from_mins(30)).unwrap();
    step_forgotten(&mut core, -1).unwrap();
    assert_eq!(SettingsForm::load(&core).forgotten_minutes, FORGOTTEN_MIN);
    assert_eq!(SettingsForm::load(&core).forgotten_label(), "30min");
    core.set_forgotten_limit(Duration::from_hours(24)).unwrap();
    step_forgotten(&mut core, 1).unwrap();
    assert_eq!(SettingsForm::load(&core).forgotten_minutes, FORGOTTEN_MAX);
}

#[test]
fn forgotten_limit_off_the_grid_snaps_to_it() {
    let mut core = core();
    core.set_forgotten_limit(Duration::from_mins(100)).unwrap();
    step_forgotten(&mut core, 1).unwrap();
    assert_eq!(SettingsForm::load(&core).forgotten_minutes, 120);
    core.set_forgotten_limit(Duration::from_mins(100)).unwrap();
    step_forgotten(&mut core, -1).unwrap();
    assert_eq!(SettingsForm::load(&core).forgotten_minutes, 90);
}

#[test]
fn wizard_values_use_the_keys_the_timer_panels_read() {
    let mut core = core();
    assert_eq!(step_wizard(&mut core, 0, 1).unwrap(), Applied::Wizard);
    assert_eq!(
        core.setting("pomodoro_focus_min").unwrap().as_deref(),
        Some("30")
    );
    step_wizard(&mut core, 1, -1).unwrap();
    assert_eq!(
        core.setting("pomodoro_short_min").unwrap().as_deref(),
        Some("4")
    );
    step_wizard(&mut core, 2, 1).unwrap();
    step_wizard(&mut core, 3, 1).unwrap();
    assert_eq!(SettingsForm::load(&core).wizard, [30, 4, 20, 5]);
    assert_eq!(
        core.setting("pomodoro_rounds").unwrap().as_deref(),
        Some("5")
    );
    // Out of range index: nothing happens.
    assert_eq!(step_wizard(&mut core, 9, 1).unwrap(), Applied::Stored);
}

#[test]
fn wizard_values_stay_within_the_wizard_limits() {
    let mut core = core();
    core.set_setting("pomodoro_rounds", "1").unwrap();
    step_wizard(&mut core, 3, -1).unwrap();
    assert_eq!(SettingsForm::load(&core).wizard[3], 1);
    core.set_setting("pomodoro_focus_min", "180").unwrap();
    step_wizard(&mut core, 0, 1).unwrap();
    assert_eq!(SettingsForm::load(&core).wizard[0], 180);
    // Stored values out of range or invalid are shown clamped or as the default.
    core.set_setting("pomodoro_short_min", "0").unwrap();
    core.set_setting("pomodoro_long_min", "soon").unwrap();
    let form = SettingsForm::load(&core);
    assert_eq!((form.wizard[1], form.wizard[2]), (1, 15));
}

#[test]
fn alarm_sound_is_stored_trimmed_under_its_key() {
    let mut core = core();
    set_alarm_sound(&mut core, "  C:/sons/galo.mp3 ").unwrap();
    assert_eq!(
        core.setting("alarm.default_sound").unwrap().as_deref(),
        Some("C:/sons/galo.mp3")
    );
    assert_eq!(SettingsForm::load(&core).alarm_sound, "C:/sons/galo.mp3");
    set_alarm_sound(&mut core, "").unwrap();
    assert_eq!(SettingsForm::load(&core).alarm_sound, "");
}

#[test]
fn steps_snap_to_the_grid() {
    assert_eq!(stepped(52, 1, 5, 1, 180), 55);
    assert_eq!(stepped(55, 1, 5, 1, 180), 60);
    assert_eq!(stepped(52, -1, 5, 1, 180), 50);
    assert_eq!(stepped(55, -1, 5, 1, 180), 50);
    assert_eq!(stepped(5, -1, 5, 1, 180), 1);
    assert_eq!(stepped(7, 0, 5, 1, 180), 7);
}

#[test]
fn hours_and_minutes_labels() {
    assert_eq!(hours_minutes(30), "30min");
    assert_eq!(hours_minutes(60), "1h");
    assert_eq!(hours_minutes(90), "1h 30min");
    assert_eq!(hours_minutes(24 * 60), "24h");
}
