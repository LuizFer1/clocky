use super::super::test_support::{at_time, brt, daily, nov, time};
use super::super::{AlarmId, NewAlarm, Store, StoreError};
use crate::alarms::{Firing, Weekdays};
use crate::test_util::{TempDb, mins};

#[test]
fn pending_alarms_are_sorted_and_firing_handles_them() {
    let mut store = Store::open_in_memory().unwrap();
    let on_time = daily(&mut store, 10, 0);
    let late = daily(&mut store, 9, 50);
    let missed = daily(&mut store, 8, 0);
    daily(&mut store, 11, 0);
    let off = daily(&mut store, 9, 55);
    store.set_alarm_enabled(off, false, nov(2, 7, 0)).unwrap();

    let now = nov(2, 10, 0);
    let pending = store.pending_alarms(now, &brt()).unwrap();
    assert_eq!(
        pending,
        vec![
            (missed, nov(2, 8, 0), Firing::Missed),
            (late, nov(2, 9, 50), Firing::Late { by: mins(10) }),
            (on_time, nov(2, 10, 0), Firing::OnTime),
        ]
    );

    for (id, due, _) in pending {
        store.mark_alarm_fired(id, due).unwrap();
        let alarm = store.alarm(id).unwrap();
        assert_eq!(alarm.last_fired_at, Some(due));
        assert!(alarm.enabled, "a repeating alarm stays enabled");
    }
    assert_eq!(store.pending_alarms(now, &brt()).unwrap(), vec![]);
}

#[test]
fn once_alarm_is_disabled_after_firing_and_rings_again_when_rearmed() {
    let mut store = Store::open_in_memory().unwrap();
    let once = NewAlarm {
        repeat: Weekdays::ONCE,
        ..at_time("once", 10, 0)
    };
    let id = store.create_alarm(&once, nov(2, 6, 0)).unwrap();
    let pending = store.pending_alarms(nov(2, 10, 0), &brt()).unwrap();
    assert_eq!(pending, vec![(id, nov(2, 10, 0), Firing::OnTime)]);

    store.mark_alarm_fired(id, nov(2, 10, 0)).unwrap();
    assert!(!store.alarm(id).unwrap().enabled);
    assert_eq!(store.pending_alarms(nov(3, 10, 0), &brt()).unwrap(), vec![]);
    assert_eq!(store.next_alarm(nov(2, 10, 1), &brt()).unwrap(), None);

    let moved = NewAlarm {
        time: time(11, 0),
        ..once
    };
    store.update_alarm(id, &moved, nov(2, 10, 20)).unwrap();
    store.set_alarm_enabled(id, true, nov(2, 10, 30)).unwrap();
    let pending = store.pending_alarms(nov(2, 11, 0), &brt()).unwrap();
    assert_eq!(pending, vec![(id, nov(2, 11, 0), Firing::OnTime)]);
}

#[test]
fn snooze_survives_reopening_the_file() {
    let db = TempDb::new("alarms-snooze");
    let mut store = Store::open(db.path()).unwrap();
    let id = daily(&mut store, 10, 0);
    store.mark_alarm_fired(id, nov(2, 10, 0)).unwrap();
    let until = store.snooze_alarm(id, nov(2, 10, 1)).unwrap();
    assert_eq!(until, nov(2, 10, 6));
    drop(store);

    let mut store = Store::open(db.path()).unwrap();
    assert_eq!(store.alarm(id).unwrap().snoozed_until, Some(nov(2, 10, 6)));
    assert_eq!(store.pending_alarms(nov(2, 10, 3), &brt()).unwrap(), vec![]);
    assert_eq!(
        store.next_alarm(nov(2, 10, 3), &brt()).unwrap(),
        Some((id, nov(2, 10, 6)))
    );
    let pending = store.pending_alarms(nov(2, 10, 6), &brt()).unwrap();
    assert_eq!(pending, vec![(id, nov(2, 10, 6), Firing::OnTime)]);

    store.mark_alarm_fired(id, nov(2, 10, 6)).unwrap();
    let alarm = store.alarm(id).unwrap();
    assert_eq!(alarm.snoozed_until, None);
    assert_eq!(alarm.last_fired_at, Some(nov(2, 10, 6)));
}

#[test]
fn snoozing_a_fired_once_alarm_enables_it_until_the_snooze_rings() {
    let mut store = Store::open_in_memory().unwrap();
    let once = NewAlarm {
        repeat: Weekdays::ONCE,
        snooze_minutes: 9,
        ..at_time("once", 10, 0)
    };
    let id = store.create_alarm(&once, nov(2, 6, 0)).unwrap();
    store.mark_alarm_fired(id, nov(2, 10, 0)).unwrap();
    assert_eq!(
        store.snooze_alarm(id, nov(2, 10, 0)).unwrap(),
        nov(2, 10, 9)
    );
    assert!(store.alarm(id).unwrap().enabled);
    store.mark_alarm_fired(id, nov(2, 10, 9)).unwrap();
    assert!(!store.alarm(id).unwrap().enabled);
}

#[test]
fn next_alarm_is_the_earliest_enabled_one() {
    let mut store = Store::open_in_memory().unwrap();
    assert_eq!(store.next_alarm(nov(2, 7, 0), &brt()).unwrap(), None);
    daily(&mut store, 9, 0);
    let first = daily(&mut store, 8, 0);
    let off = daily(&mut store, 7, 30);
    store.set_alarm_enabled(off, false, nov(2, 7, 0)).unwrap();
    assert_eq!(
        store.next_alarm(nov(2, 7, 0), &brt()).unwrap(),
        Some((first, nov(2, 8, 0)))
    );
    assert_eq!(
        store.next_alarm(nov(2, 9, 30), &brt()).unwrap(),
        Some((first, nov(3, 8, 0)))
    );
}

#[test]
fn firing_or_snoozing_a_missing_alarm_is_not_found() {
    let mut store = Store::open_in_memory().unwrap();
    assert!(matches!(
        store.mark_alarm_fired(AlarmId(9), nov(2, 10, 0)),
        Err(StoreError::NotFound {
            entity: "alarm",
            ..
        })
    ));
    assert!(matches!(
        store.snooze_alarm(AlarmId(9), nov(2, 10, 0)),
        Err(StoreError::NotFound {
            entity: "alarm",
            ..
        })
    ));
}

#[test]
fn a_corrupted_alarm_fails_the_runtime_operations() {
    let mut store = Store::open_in_memory().unwrap();
    let id = daily(&mut store, 10, 0);
    store
        .conn
        .execute_batch("PRAGMA ignore_check_constraints = ON; UPDATE alarms SET enabled = 7;")
        .unwrap();
    assert!(store.pending_alarms(nov(2, 10, 0), &brt()).is_err());
    assert!(store.next_alarm(nov(2, 10, 0), &brt()).is_err());
    assert!(store.mark_alarm_fired(id, nov(2, 10, 0)).is_err());
    assert!(store.snooze_alarm(id, nov(2, 10, 0)).is_err());
}
