use super::super::test_support::{at_time, brt, daily, nov, time};
use super::super::{AlarmId, NewAlarm, PendingAlarm, Store, StoreError};
use crate::alarms::{Firing, Weekdays};
use crate::test_util::{TempDb, mins};
use crate::time::Timestamp;

/// Marks the occurrence due at `due` as handled, noticed right on time.
fn fire(store: &mut Store, id: AlarmId, due: Timestamp) {
    store.mark_alarm_fired(id, due, due).unwrap();
}

/// The pending occurrences at `now` in BRT, as `(alarm, due, firing)`.
fn pending_at(store: &Store, now: Timestamp) -> Vec<(AlarmId, Timestamp, Firing)> {
    store
        .pending_alarms(now, &brt())
        .unwrap()
        .into_iter()
        .map(|p| (p.alarm.id, p.due, p.firing))
        .collect()
}

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
    let pending = pending_at(&store, now);
    assert_eq!(
        pending,
        vec![
            (missed, nov(2, 8, 0), Firing::Missed),
            (late, nov(2, 9, 50), Firing::Late { by: mins(10) }),
            (on_time, nov(2, 10, 0), Firing::OnTime),
        ]
    );

    for (id, due, _) in pending {
        fire(&mut store, id, due);
        let alarm = store.alarm(id).unwrap();
        assert_eq!(alarm.last_fired_at, Some(due));
        assert!(alarm.enabled, "a repeating alarm stays enabled");
    }
    assert_eq!(pending_at(&store, now), vec![]);
}

#[test]
fn once_alarm_is_disabled_after_firing_and_rings_again_when_rearmed() {
    let mut store = Store::open_in_memory().unwrap();
    let once = NewAlarm {
        repeat: Weekdays::ONCE,
        ..at_time("once", 10, 0)
    };
    let id = store.create_alarm(&once, nov(2, 6, 0)).unwrap();
    let pending = pending_at(&store, nov(2, 10, 0));
    assert_eq!(pending, vec![(id, nov(2, 10, 0), Firing::OnTime)]);

    fire(&mut store, id, nov(2, 10, 0));
    assert!(!store.alarm(id).unwrap().enabled);
    assert_eq!(pending_at(&store, nov(3, 10, 0)), vec![]);
    assert_eq!(store.next_alarm(nov(2, 10, 1), &brt()).unwrap(), None);

    let moved = NewAlarm {
        time: time(11, 0),
        ..once
    };
    store.update_alarm(id, &moved, nov(2, 10, 20)).unwrap();
    store.set_alarm_enabled(id, true, nov(2, 10, 30)).unwrap();
    let pending = pending_at(&store, nov(2, 11, 0));
    assert_eq!(pending, vec![(id, nov(2, 11, 0), Firing::OnTime)]);
}

#[test]
fn snooze_survives_reopening_the_file() {
    let db = TempDb::new("alarms-snooze");
    let mut store = Store::open(db.path()).unwrap();
    let id = daily(&mut store, 10, 0);
    fire(&mut store, id, nov(2, 10, 0));
    let until = store.snooze_alarm(id, nov(2, 10, 1)).unwrap();
    assert_eq!(until, Some(nov(2, 10, 6)));
    drop(store);

    let mut store = Store::open(db.path()).unwrap();
    assert_eq!(store.alarm(id).unwrap().snoozed_until, Some(nov(2, 10, 6)));
    assert_eq!(pending_at(&store, nov(2, 10, 3)), vec![]);
    assert_eq!(
        store.next_alarm(nov(2, 10, 3), &brt()).unwrap(),
        Some((id, nov(2, 10, 6)))
    );
    let pending = pending_at(&store, nov(2, 10, 6));
    assert_eq!(pending, vec![(id, nov(2, 10, 6), Firing::OnTime)]);

    fire(&mut store, id, nov(2, 10, 6));
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
    fire(&mut store, id, nov(2, 10, 0));
    assert_eq!(
        store.snooze_alarm(id, nov(2, 10, 0)).unwrap(),
        Some(nov(2, 10, 9))
    );
    assert!(store.alarm(id).unwrap().enabled);
    fire(&mut store, id, nov(2, 10, 9));
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
        store.mark_alarm_fired(AlarmId(9), nov(2, 10, 0), nov(2, 10, 0)),
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
    assert!(
        store
            .mark_alarm_fired(id, nov(2, 10, 0), nov(2, 10, 0))
            .is_err()
    );
    assert!(store.snooze_alarm(id, nov(2, 10, 0)).is_err());
}

#[test]
fn snoozing_a_disabled_repeating_alarm_does_nothing() {
    let mut store = Store::open_in_memory().unwrap();
    let id = daily(&mut store, 10, 0);
    fire(&mut store, id, nov(2, 10, 0));
    store.set_alarm_enabled(id, false, nov(2, 10, 1)).unwrap();
    assert_eq!(store.snooze_alarm(id, nov(2, 10, 1)).unwrap(), None);
    let alarm = store.alarm(id).unwrap();
    assert!(!alarm.enabled);
    assert_eq!(alarm.snoozed_until, None);

    store.set_alarm_enabled(id, true, nov(3, 9, 0)).unwrap();
    assert_eq!(pending_at(&store, nov(3, 9, 0)), vec![]);
    let pending = pending_at(&store, nov(3, 10, 0));
    assert_eq!(pending, vec![(id, nov(3, 10, 0), Firing::OnTime)]);
}

#[test]
fn reenabling_clears_a_leftover_snooze() {
    let mut store = Store::open_in_memory().unwrap();
    let id = daily(&mut store, 10, 0);
    fire(&mut store, id, nov(2, 10, 0));
    store
        .conn
        .execute(
            "UPDATE alarms SET enabled = 0, snoozed_until_ms = ?1",
            [nov(2, 10, 6).as_millis()],
        )
        .unwrap();

    store.set_alarm_enabled(id, true, nov(3, 9, 0)).unwrap();
    assert_eq!(store.alarm(id).unwrap().snoozed_until, None);
    assert_eq!(pending_at(&store, nov(3, 9, 0)), vec![]);
    assert_eq!(
        store.next_alarm(nov(3, 9, 0), &brt()).unwrap(),
        Some((id, nov(3, 10, 0)))
    );
}

#[test]
fn editing_a_snoozed_once_alarm_does_not_ring_again() {
    let mut store = Store::open_in_memory().unwrap();
    let once = NewAlarm {
        repeat: Weekdays::ONCE,
        ..at_time("once", 10, 0)
    };
    let id = store.create_alarm(&once, nov(2, 6, 0)).unwrap();
    fire(&mut store, id, nov(2, 10, 0));
    store.snooze_alarm(id, nov(2, 10, 0)).unwrap();

    let reworded = NewAlarm {
        message: "Take the pills",
        ..once
    };
    store.update_alarm(id, &reworded, nov(2, 10, 2)).unwrap();
    let alarm = store.alarm(id).unwrap();
    assert!(!alarm.enabled);
    assert_eq!(alarm.snoozed_until, None);
    assert_eq!(pending_at(&store, nov(3, 10, 0)), vec![]);
    assert_eq!(store.next_alarm(nov(2, 10, 2), &brt()).unwrap(), None);
}

#[test]
fn editing_after_firing_today_does_not_fire_again() {
    let mut store = Store::open_in_memory().unwrap();
    let id = daily(&mut store, 10, 0);
    fire(&mut store, id, nov(2, 10, 0));
    let earlier = NewAlarm {
        repeat: Weekdays::EVERY_DAY,
        ..at_time("daily", 10, 10)
    };
    store.update_alarm(id, &earlier, nov(2, 10, 20)).unwrap();
    assert!(store.alarm(id).unwrap().enabled);
    assert_eq!(pending_at(&store, nov(2, 10, 30)), vec![]);
    assert_eq!(
        store.next_alarm(nov(2, 10, 30), &brt()).unwrap(),
        Some((id, nov(3, 10, 10)))
    );
}

#[test]
fn a_stale_mark_keeps_the_snooze() {
    let mut store = Store::open_in_memory().unwrap();
    let id = daily(&mut store, 10, 0);
    fire(&mut store, id, nov(2, 10, 0));
    store.snooze_alarm(id, nov(2, 10, 1)).unwrap();
    store
        .mark_alarm_fired(id, nov(2, 10, 0), nov(2, 10, 2))
        .unwrap();
    assert_eq!(store.alarm(id).unwrap().snoozed_until, Some(nov(2, 10, 6)));
    let pending = pending_at(&store, nov(2, 10, 6));
    assert_eq!(pending, vec![(id, nov(2, 10, 6), Firing::OnTime)]);
}

#[test]
fn an_out_of_order_mark_keeps_the_later_fire() {
    let mut store = Store::open_in_memory().unwrap();
    let id = daily(&mut store, 10, 0);
    fire(&mut store, id, nov(3, 10, 0));
    store
        .mark_alarm_fired(id, nov(2, 10, 0), nov(3, 10, 1))
        .unwrap();
    assert_eq!(store.alarm(id).unwrap().last_fired_at, Some(nov(3, 10, 0)));
    assert_eq!(pending_at(&store, nov(3, 10, 5)), vec![]);
}

#[test]
fn a_mark_after_the_clock_moved_back_is_kept() {
    let mut store = Store::open_in_memory().unwrap();
    let id = daily(&mut store, 10, 0);
    fire(&mut store, id, nov(3, 10, 0));

    // The clock moves back a day: the fire on the 3rd is in the future, so the 10:00 of the
    // 2nd rings again, and marking it must stop it from ringing on every tick.
    let now = nov(2, 10, 0);
    let pending = pending_at(&store, now);
    assert_eq!(pending, vec![(id, nov(2, 10, 0), Firing::OnTime)]);
    store.mark_alarm_fired(id, nov(2, 10, 0), now).unwrap();
    assert_eq!(store.alarm(id).unwrap().last_fired_at, Some(nov(2, 10, 0)));
    assert_eq!(pending_at(&store, nov(2, 10, 1)), vec![]);
}

#[test]
fn pending_alarms_carry_the_whole_alarm() {
    let mut store = Store::open_in_memory().unwrap();
    let id = daily(&mut store, 10, 0);
    let found = store.pending_alarms(nov(2, 10, 0), &brt()).unwrap();
    assert_eq!(
        found,
        vec![PendingAlarm {
            alarm: store.alarm(id).unwrap(),
            due: nov(2, 10, 0),
            firing: Firing::OnTime,
        }]
    );
}

#[test]
fn reenabling_a_once_alarm_after_its_time_rings_the_next_day() {
    let mut store = Store::open_in_memory().unwrap();
    let once = NewAlarm {
        repeat: Weekdays::ONCE,
        ..at_time("once", 10, 0)
    };
    let skipped = store.create_alarm(&once, nov(2, 6, 0)).unwrap();
    store
        .set_alarm_enabled(skipped, false, nov(2, 8, 0))
        .unwrap();
    let fired = store.create_alarm(&once, nov(2, 6, 0)).unwrap();
    fire(&mut store, fired, nov(2, 10, 0));

    for id in [skipped, fired] {
        store.set_alarm_enabled(id, true, nov(2, 11, 0)).unwrap();
    }
    assert_eq!(pending_at(&store, nov(2, 11, 0)), vec![]);
    assert_eq!(
        store.next_alarm(nov(2, 11, 0), &brt()).unwrap(),
        Some((skipped, nov(3, 10, 0)))
    );
    assert_eq!(
        pending_at(&store, nov(3, 10, 0)),
        vec![
            (skipped, nov(3, 10, 0), Firing::OnTime),
            (fired, nov(3, 10, 0), Firing::OnTime),
        ]
    );
}

#[test]
fn marking_an_expired_snooze_before_the_last_fire_stops_it() {
    let mut store = Store::open_in_memory().unwrap();
    let once = NewAlarm {
        repeat: Weekdays::ONCE,
        ..at_time("once", 10, 0)
    };
    let id = store.create_alarm(&once, nov(2, 6, 0)).unwrap();
    fire(&mut store, id, nov(2, 10, 0));
    // The clock moves back to 09:00, the user snoozes, and the computer sleeps until 10:30.
    store.snooze_alarm(id, nov(2, 9, 0)).unwrap();
    let now = nov(2, 10, 30);
    assert_eq!(
        pending_at(&store, now),
        vec![(id, nov(2, 9, 5), Firing::Missed)]
    );

    store.mark_alarm_fired(id, nov(2, 9, 5), now).unwrap();
    assert_eq!(pending_at(&store, nov(2, 10, 31)), vec![]);
    let alarm = store.alarm(id).unwrap();
    assert_eq!(alarm.snoozed_until, None);
    assert_eq!(alarm.last_fired_at, Some(nov(2, 10, 0)));
    assert!(!alarm.enabled);
}

#[test]
fn editing_a_snoozed_once_alarm_into_a_repeating_one_keeps_it_enabled() {
    let mut store = Store::open_in_memory().unwrap();
    let once = NewAlarm {
        repeat: Weekdays::ONCE,
        ..at_time("once", 10, 0)
    };
    let id = store.create_alarm(&once, nov(2, 6, 0)).unwrap();
    fire(&mut store, id, nov(2, 10, 0));
    store.snooze_alarm(id, nov(2, 10, 0)).unwrap();

    let every_day = NewAlarm {
        repeat: Weekdays::EVERY_DAY,
        ..once
    };
    store.update_alarm(id, &every_day, nov(2, 10, 2)).unwrap();
    let alarm = store.alarm(id).unwrap();
    assert!(alarm.enabled);
    assert_eq!(alarm.snoozed_until, None);
    assert_eq!(
        pending_at(&store, nov(3, 10, 0)),
        vec![(id, nov(3, 10, 0), Firing::OnTime)]
    );
}
