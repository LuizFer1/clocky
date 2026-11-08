-- Migration 3: alarms (spec, sections 3 and 5).
-- time_local is the local wall-clock time HH:MM; repeat_days is a weekday bitmask (bit 0 =
-- Monday ... bit 6 = Sunday; 0 = rings once). Instants are UTC epoch milliseconds.
-- created_at_ms is reset when the alarm is edited or enabled again: occurrences up to it are
-- never reported as missed.

CREATE TABLE alarms (
    id               INTEGER PRIMARY KEY,
    name             TEXT    NOT NULL,
    time_local       TEXT    NOT NULL CHECK (time_local GLOB '[01][0-9]:[0-5][0-9]'
                                          OR time_local GLOB '2[0-3]:[0-5][0-9]'),
    repeat_days      INTEGER NOT NULL CHECK (repeat_days BETWEEN 0 AND 127),
    sound_path       TEXT    NULL,
    message          TEXT    NOT NULL DEFAULT '',
    snooze_minutes   INTEGER NOT NULL CHECK (snooze_minutes >= 1),
    task_id          INTEGER NULL REFERENCES tasks (id) ON DELETE SET NULL,
    enabled          INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    snoozed_until_ms INTEGER NULL,
    last_fired_at_ms INTEGER NULL,
    created_at_ms    INTEGER NOT NULL
);

CREATE INDEX alarms_task_id ON alarms (task_id);
