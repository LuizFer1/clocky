use super::*;
use crate::domain::TimerId;
use crate::engine::session::TimerKind;
use crate::store::NewPomodoroPreset;
use crate::test_util::{at, completed, mins, secs};

fn record(store: &mut Store, task: TaskId, start: i64, end: i64) {
    let span = completed(TimerKind::Stopwatch, &[(start, end)]);
    store
        .record_session(Some(task), TimerId(1), None, &span)
        .unwrap();
}

#[test]
fn overview_counts_open_and_completed_tasks() {
    let mut store = Store::open_in_memory().unwrap();
    let work = store.create_project("work", "#000000", at(0)).unwrap();
    let empty = store.create_project("empty", "#000000", at(1)).unwrap();
    let hidden = store.create_project("hidden", "#000000", at(2)).unwrap();
    store.archive_project(hidden, at(3)).unwrap();
    store.create_task(Some(work), "open", at(0)).unwrap();
    let done = store.create_task(Some(work), "done", at(0)).unwrap();
    store.complete_task(done, at(5)).unwrap();
    let archived = store.create_task(Some(work), "archived", at(0)).unwrap();
    store.archive_task(archived, at(5)).unwrap();

    let overview = store.projects_overview().unwrap();
    let rows: Vec<_> = overview
        .iter()
        .map(|o| (o.project.id, o.open_tasks, o.completed_tasks))
        .collect();
    assert_eq!(rows, vec![(work, 1, 1), (empty, 0, 0)]);
}

#[test]
fn summaries_show_progress_time_and_links() {
    let mut store = Store::open_in_memory().unwrap();
    let work = store.create_project("work", "#000000", at(0)).unwrap();
    let task = store.create_task(Some(work), "report", at(0)).unwrap();
    let a = store.add_subtask(task, "outline").unwrap();
    store.add_subtask(task, "draft").unwrap();
    store.complete_subtask(a, at(10), None).unwrap();
    record(&mut store, task, 0, 60);
    record(&mut store, task, 100, 130);
    let timer = store
        .create_timer_preset("tea", mins(3), Some(task))
        .unwrap();
    let pomodoro = store
        .create_pomodoro_preset(&NewPomodoroPreset {
            name: "classic",
            focus: mins(25),
            short_break: mins(5),
            long_break: mins(15),
            focus_per_round: 4,
            auto_start: false,
            task: Some(task),
        })
        .unwrap();
    store
        .create_timer_preset("unlinked", mins(1), None)
        .unwrap();

    let summaries = store.task_summaries(Some(work), false).unwrap();
    assert_eq!(summaries.len(), 1);
    let summary = &summaries[0];
    assert_eq!(summary.task.id, task);
    assert_eq!((summary.subtasks_done, summary.subtasks_total), (1, 2));
    assert_eq!(summary.tracked, secs(90));
    assert_eq!(summary.links, store.task_links(task).unwrap());
    let timer_ids: Vec<_> = summary.links.timer_presets.iter().map(|p| p.id).collect();
    assert_eq!(timer_ids, vec![timer]);
    assert_eq!(summary.links.pomodoro_presets[0].id, pomodoro);
}

#[test]
fn summaries_filter_by_project_completion_and_archive() {
    let mut store = Store::open_in_memory().unwrap();
    let work = store.create_project("work", "#000000", at(0)).unwrap();
    let open = store.create_task(Some(work), "open", at(0)).unwrap();
    let done = store.create_task(Some(work), "done", at(0)).unwrap();
    store.complete_task(done, at(5)).unwrap();
    let archived = store.create_task(Some(work), "archived", at(0)).unwrap();
    store.archive_task(archived, at(5)).unwrap();
    let loose = store.create_task(None, "loose", at(0)).unwrap();

    let only_open: Vec<_> = store
        .task_summaries(Some(work), false)
        .unwrap()
        .iter()
        .map(|s| s.task.id)
        .collect();
    assert_eq!(only_open, vec![open]);
    let with_done: Vec<_> = store
        .task_summaries(Some(work), true)
        .unwrap()
        .iter()
        .map(|s| s.task.id)
        .collect();
    assert_eq!(with_done, vec![open, done]);
    let without_project: Vec<_> = store
        .task_summaries(None, true)
        .unwrap()
        .iter()
        .map(|s| s.task.id)
        .collect();
    assert_eq!(without_project, vec![loose]);
}

#[test]
fn task_time_sums_sessions_and_keeps_archived_history() {
    let mut store = Store::open_in_memory().unwrap();
    let task = store.create_task(None, "report", at(0)).unwrap();
    assert_eq!(store.task_time(task).unwrap(), Duration::ZERO);
    record(&mut store, task, 0, 60);
    record(&mut store, task, 100, 130);
    store.archive_task(task, at(200)).unwrap();
    assert_eq!(store.task_time(task).unwrap(), secs(90));
    assert_eq!(store.recent_sessions(task, 10).unwrap().len(), 2);
}

#[test]
fn corrupted_tracked_time_is_an_error() {
    let mut store = Store::open_in_memory().unwrap();
    let task = store.create_task(None, "report", at(0)).unwrap();
    record(&mut store, task, 0, 60);
    store
        .conn
        .execute("UPDATE sessions SET effective_ms = -1", [])
        .unwrap();
    assert!(matches!(
        store.task_time(task),
        Err(StoreError::OutOfRange("tracked time"))
    ));
    assert!(store.task_summaries(None, true).is_err());
}
