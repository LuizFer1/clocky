-- Migration 4: free-text descriptions of projects, tasks and subtasks ('' = none), and the
-- estimated time of tasks and subtasks (NULL = no estimate).

ALTER TABLE projects ADD COLUMN description TEXT NOT NULL DEFAULT '';
ALTER TABLE tasks ADD COLUMN description TEXT NOT NULL DEFAULT '';
ALTER TABLE tasks ADD COLUMN estimate_ms INTEGER NULL CHECK (estimate_ms > 0);
ALTER TABLE subtasks ADD COLUMN description TEXT NOT NULL DEFAULT '';
ALTER TABLE subtasks ADD COLUMN estimate_ms INTEGER NULL CHECK (estimate_ms > 0);
