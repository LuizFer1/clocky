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

fn ids(tasks: &[Task]) -> Vec<TaskId> {
    tasks.iter().map(|task| task.id).collect()
}

#[test]
fn recent_tasks_follow_the_last_session() {
    let mut store = Store::open_in_memory().unwrap();
    let a = store.create_task(None, "a", at(0)).unwrap();
    let b = store.create_task(None, "b", at(0)).unwrap();
    let c = store.create_task(None, "c", at(0)).unwrap();
    store.create_task(None, "never tracked", at(0)).unwrap();
    record(&mut store, a, 0, 10);
    record(&mut store, b, 20, 30);
    record(&mut store, a, 40, 50);
    record(&mut store, c, 60, 70);
    assert_eq!(ids(&store.recent_tasks(10).unwrap()), vec![c, a, b]);
    assert_eq!(ids(&store.recent_tasks(2).unwrap()), vec![c, a]);
}

#[test]
fn recent_tasks_skip_archived_tasks_and_projects() {
    let mut store = Store::open_in_memory().unwrap();
    let hidden = store.create_project("hidden", "#000000", at(0)).unwrap();
    let in_hidden = store.create_task(Some(hidden), "in hidden", at(0)).unwrap();
    let archived = store.create_task(None, "archived", at(0)).unwrap();
    let visible = store.create_task(None, "visible", at(0)).unwrap();
    for task in [in_hidden, archived, visible] {
        record(&mut store, task, 0, 10);
    }
    store.archive_project(hidden, at(20)).unwrap();
    store.archive_task(archived, at(20)).unwrap();
    assert_eq!(ids(&store.recent_tasks(10).unwrap()), vec![visible]);
}

#[test]
fn search_matches_task_or_project_name_ignoring_case() {
    let mut store = Store::open_in_memory().unwrap();
    let connect = store.create_project("Connect", "#000000", at(0)).unwrap();
    let work = store.create_task(Some(connect), "work", at(0)).unwrap();
    let report = store.create_task(None, "Weekly report", at(0)).unwrap();
    store.create_task(None, "groceries", at(0)).unwrap();
    assert_eq!(ids(&store.search_tasks("REPORT").unwrap()), vec![report]);
    assert_eq!(ids(&store.search_tasks("connect").unwrap()), vec![work]);
    assert_eq!(ids(&store.search_tasks("w").unwrap()), vec![report, work]);
    assert_eq!(store.search_tasks("").unwrap().len(), 3);
}

#[test]
fn search_skips_archived_tasks_and_projects() {
    let mut store = Store::open_in_memory().unwrap();
    let hidden = store.create_project("hidden", "#000000", at(0)).unwrap();
    store.create_task(Some(hidden), "task one", at(0)).unwrap();
    let archived = store.create_task(None, "task two", at(0)).unwrap();
    let visible = store.create_task(None, "task three", at(0)).unwrap();
    store.archive_project(hidden, at(1)).unwrap();
    store.archive_task(archived, at(1)).unwrap();
    assert_eq!(ids(&store.search_tasks("task").unwrap()), vec![visible]);
}

#[test]
fn search_ignores_accents_and_case() {
    let mut store = Store::open_in_memory().unwrap();
    let ethics = store.create_task(None, "Ética", at(0)).unwrap();
    let auth = store.create_task(None, "Autenticação", at(0)).unwrap();
    let clinic = store
        .create_project("Pesquisa Clínica", "#000000", at(0))
        .unwrap();
    let in_clinic = store.create_task(Some(clinic), "coleta", at(0)).unwrap();
    assert_eq!(ids(&store.search_tasks("ética").unwrap()), vec![ethics]);
    assert_eq!(
        ids(&store.search_tasks("autenticacao").unwrap()),
        vec![auth]
    );
    assert_eq!(
        ids(&store.search_tasks("AUTENTICAÇÃO").unwrap()),
        vec![auth]
    );
    assert_eq!(
        ids(&store.search_tasks("clinica").unwrap()),
        vec![in_clinic]
    );
}

#[test]
fn search_sorts_by_folded_name() {
    let mut store = Store::open_in_memory().unwrap();
    let b = store.create_task(None, "b", at(0)).unwrap();
    let accented = store.create_task(None, "Á", at(0)).unwrap();
    let a = store.create_task(None, "a", at(0)).unwrap();
    assert_eq!(ids(&store.search_tasks("").unwrap()), vec![accented, a, b]);
}

#[test]
fn links_and_summaries_include_active_timers() {
    use crate::engine::stopwatch::Stopwatch;
    use crate::engine::timers::{ActiveTimer, Timer};
    use crate::store::ActiveTimerLink;

    let mut store = Store::open_in_memory().unwrap();
    let task = store.create_task(None, "report", at(0)).unwrap();
    let other = store.create_task(None, "other", at(0)).unwrap();
    let running = |id, task| {
        ActiveTimer::new(
            TimerId(id),
            Some(task),
            None,
            Timer::Stopwatch(Stopwatch::start(at(0))),
        )
    };
    store.save_active(&running(1, task)).unwrap();
    store.save_active(&running(2, other)).unwrap();

    let expected = vec![ActiveTimerLink {
        timer: TimerId(1),
        kind: TimerKind::Stopwatch,
    }];
    assert_eq!(store.task_links(task).unwrap().active_timers, expected);
    let summaries = store.task_summaries(None, false).unwrap();
    assert_eq!(summaries[0].links.active_timers, expected);
    store.delete_active(TimerId(1)).unwrap();
    assert_eq!(store.task_links(task).unwrap().active_timers.len(), 0);
}

#[test]
fn queries_read_the_details_and_their_extra_columns() {
    let mut store = Store::open_in_memory().unwrap();
    let work = store.create_project("work", "#000000", at(0)).unwrap();
    store.set_project_description(work, "Client").unwrap();
    let task = store.create_task(Some(work), "report", at(0)).unwrap();
    store.set_task_description(task, "Q4 numbers").unwrap();
    store.set_task_estimate(task, Some(mins(90))).unwrap();
    let sub = store.add_subtask(task, "outline").unwrap();
    store.complete_subtask(sub, at(5), None).unwrap();
    record(&mut store, task, 0, 60);

    let overview = store.projects_overview().unwrap();
    assert_eq!(overview[0].project.description, "Client");
    assert_eq!(
        (overview[0].open_tasks, overview[0].completed_tasks),
        (1, 0)
    );

    let summary = &store.task_summaries(Some(work), false).unwrap()[0];
    assert_eq!(summary.task.description, "Q4 numbers");
    assert_eq!(summary.task.estimate, Some(mins(90)));
    assert_eq!((summary.subtasks_done, summary.subtasks_total), (1, 1));
    assert_eq!(summary.tracked, secs(60));

    let found = store.search_tasks("work").unwrap();
    assert_eq!(found[0].estimate, Some(mins(90)));
    assert_eq!(store.recent_tasks(5).unwrap()[0].description, "Q4 numbers");
}
