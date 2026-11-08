-- Migration 1: domain, sessions, subtask events and settings.
-- Instants and durations are INTEGER milliseconds (UTC epoch for instants).

CREATE TABLE projects (
    id             INTEGER PRIMARY KEY,
    name           TEXT    NOT NULL,
    color          TEXT    NOT NULL,
    archived_at_ms INTEGER NULL,
    created_at_ms  INTEGER NOT NULL
);

CREATE TABLE tasks (
    id              INTEGER PRIMARY KEY,
    project_id      INTEGER NULL REFERENCES projects (id) ON DELETE SET NULL,
    name            TEXT    NOT NULL,
    completed_at_ms INTEGER NULL,
    archived_at_ms  INTEGER NULL,
    position        INTEGER NOT NULL,
    created_at_ms   INTEGER NOT NULL
);

CREATE TABLE subtasks (
    id              INTEGER PRIMARY KEY,
    task_id         INTEGER NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    name            TEXT    NOT NULL,
    completed_at_ms INTEGER NULL,
    position        INTEGER NOT NULL
);

CREATE TABLE timer_presets (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL,
    duration_ms INTEGER NOT NULL CHECK (duration_ms > 0),
    task_id     INTEGER NULL REFERENCES tasks (id) ON DELETE SET NULL
);

CREATE TABLE pomodoro_presets (
    id              INTEGER PRIMARY KEY,
    name            TEXT    NOT NULL,
    focus_ms        INTEGER NOT NULL,
    short_break_ms  INTEGER NOT NULL,
    long_break_ms   INTEGER NOT NULL,
    focus_per_round INTEGER NOT NULL CHECK (focus_per_round >= 1),
    auto_start      INTEGER NOT NULL CHECK (auto_start IN (0, 1)),
    task_id         INTEGER NULL REFERENCES tasks (id) ON DELETE SET NULL
);

CREATE TABLE sessions (
    id            INTEGER PRIMARY KEY,
    kind          TEXT    NOT NULL CHECK (kind IN ('pomodoro', 'stopwatch', 'countdown')),
    task_id       INTEGER NULL REFERENCES tasks (id) ON DELETE SET NULL,
    run_id        INTEGER NOT NULL,
    round         INTEGER NULL,
    started_at_ms INTEGER NOT NULL,
    ended_at_ms   INTEGER NOT NULL,
    effective_ms  INTEGER NOT NULL
);

CREATE TABLE session_segments (
    session_id INTEGER NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    start_ms   INTEGER NOT NULL,
    end_ms     INTEGER NOT NULL,
    CHECK (end_ms > start_ms)
);

CREATE TABLE subtask_events (
    id         INTEGER PRIMARY KEY,
    subtask_id INTEGER NOT NULL REFERENCES subtasks (id) ON DELETE CASCADE,
    kind       TEXT    NOT NULL CHECK (kind IN ('completed', 'reopened')),
    at_ms      INTEGER NOT NULL,
    run_id     INTEGER NULL,
    session_id INTEGER NULL REFERENCES sessions (id) ON DELETE SET NULL
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE INDEX sessions_task_id ON sessions (task_id);
CREATE INDEX sessions_started_at_ms ON sessions (started_at_ms);
CREATE INDEX sessions_run_id ON sessions (run_id);
CREATE INDEX subtask_events_run_id ON subtask_events (run_id);
CREATE INDEX session_segments_session_id ON session_segments (session_id);
