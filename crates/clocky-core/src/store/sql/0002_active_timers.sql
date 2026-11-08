-- Migration 2: active timers, so they survive closing the app.
-- The id is the TimerId (never reused). preset_id points to timer_presets for a countdown and
-- to pomodoro_presets for a pomodoro; the triggers below clear it when that preset is deleted.

CREATE TABLE active_timers (
    id              INTEGER PRIMARY KEY,
    kind            TEXT    NOT NULL CHECK (kind IN ('pomodoro', 'stopwatch', 'countdown')),
    task_id         INTEGER NULL REFERENCES tasks (id) ON DELETE SET NULL,
    preset_id       INTEGER NULL,
    target_ms       INTEGER NULL,
    focus_ms        INTEGER NULL,
    short_break_ms  INTEGER NULL,
    long_break_ms   INTEGER NULL,
    focus_per_round INTEGER NULL,
    auto_start      INTEGER NULL CHECK (auto_start IN (0, 1)),
    phase           TEXT    NULL CHECK (phase IN ('focus', 'short_break', 'long_break')),
    focus_index     INTEGER NULL,
    round           INTEGER NULL,
    phase_started   INTEGER NULL CHECK (phase_started IN (0, 1))
);

-- Segments of the active timer's run; end_ms NULL = the open (running) segment.
CREATE TABLE active_timer_segments (
    timer_id INTEGER NOT NULL REFERENCES active_timers (id) ON DELETE CASCADE,
    start_ms INTEGER NOT NULL,
    end_ms   INTEGER NULL,
    CHECK (end_ms IS NULL OR end_ms > start_ms)
);

CREATE INDEX active_timers_task_id ON active_timers (task_id);
CREATE INDEX active_timer_segments_timer_id ON active_timer_segments (timer_id);
CREATE UNIQUE INDEX active_timer_segments_one_open
    ON active_timer_segments (timer_id) WHERE end_ms IS NULL;

CREATE TRIGGER timer_presets_unlink_active AFTER DELETE ON timer_presets
BEGIN
    UPDATE active_timers SET preset_id = NULL
    WHERE kind = 'countdown' AND preset_id = old.id;
END;

CREATE TRIGGER pomodoro_presets_unlink_active AFTER DELETE ON pomodoro_presets
BEGIN
    UPDATE active_timers SET preset_id = NULL
    WHERE kind = 'pomodoro' AND preset_id = old.id;
END;
