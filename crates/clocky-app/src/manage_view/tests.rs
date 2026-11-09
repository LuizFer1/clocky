use chrono::FixedOffset;
use clocky_core::clock::FakeClock;
use clocky_core::time::Timestamp;

use super::*;
use crate::demo;

/// 2026-11-08 20:00 at UTC-3.
const NOW: i64 = 1_794_178_800_000;

fn tz() -> FixedOffset {
    FixedOffset::west_opt(3 * 3600).unwrap()
}

fn demo_core() -> AppCore {
    let mut core = AppCore::in_memory(Box::new(FakeClock::new(Timestamp::from_millis(NOW))));
    demo::seed_if_empty(&mut core, &tz()).unwrap();
    core
}

fn project_id(core: &AppCore, name: &str) -> ProjectId {
    core.projects()
        .unwrap()
        .into_iter()
        .find(|p| p.project.name == name)
        .unwrap()
        .project
        .id
}

fn task_id(core: &AppCore, name: &str) -> TaskId {
    core.search_tasks(name).unwrap()[0].id
}

#[test]
fn page_follows_the_navigation() {
    let mut nav = Nav::default();
    assert_eq!(nav.page(), Page::AllProjects);
    nav.search = "  x ".into();
    assert_eq!(nav.page(), Page::Search);
    nav.project = Some(ProjectId(1));
    assert_eq!(nav.page(), Page::Project);
    nav.task = Some(TaskId(1));
    assert_eq!(nav.page(), Page::Task);
}

#[test]
fn all_projects_has_cards_with_counts() {
    let core = demo_core();
    let view = build(&core, &Nav::default(), &tz()).unwrap();
    assert_eq!(view.page, Page::AllProjects);
    assert_eq!(view.crumb, "Projetos");
    assert_eq!(view.subheading, "3 projetos");
    let faculdade = &view.projects[1];
    assert_eq!(faculdade.name, "faculdade");
    assert_eq!(faculdade.open, 1);
    assert_eq!(faculdade.counts, "1 abertas · 1 concluídas");
    assert!((faculdade.progress - 0.5).abs() < f32::EPSILON);
    assert!(view.projects.iter().all(|p| !p.selected));
}

#[test]
fn project_page_lists_open_tasks() {
    let core = demo_core();
    let nav = Nav {
        project: Some(project_id(&core, "connect")),
        ..Nav::default()
    };
    let view = build(&core, &nav, &tz()).unwrap();
    assert_eq!(view.crumb, "connect");
    assert_eq!(view.subheading, "4 abertas · 0 concluídas");
    assert!(view.project_description.starts_with("App de mensagens"));
    assert!(view.projects[0].selected);
    let names: Vec<_> = view.tasks.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(
        names,
        ["autenticação", "testes", "code review", "standup diário"]
    );
    let auth = &view.tasks[0];
    assert_eq!(auth.subtasks, "1/3 subtarefas");
    // 3h 40min recorded of a 4h estimate.
    assert_eq!(auth.time, "3h 40min / 4h");
    assert!(auth.has_pomodoro && !auth.has_timer);
    assert!(view.tasks[1].has_timer);
    assert_eq!(view.tasks[3].time, "—");
}

#[test]
fn completed_tasks_show_only_on_request() {
    let core = demo_core();
    let mut nav = Nav {
        project: Some(project_id(&core, "faculdade")),
        ..Nav::default()
    };
    assert_eq!(build(&core, &nav, &tz()).unwrap().tasks.len(), 1);
    nav.show_completed = true;
    let view = build(&core, &nav, &tz()).unwrap();
    assert_eq!(view.tasks.len(), 2);
    assert!(view.tasks[1].done);
}

#[test]
fn search_filters_the_project_list() {
    let core = demo_core();
    let nav = Nav {
        project: Some(project_id(&core, "connect")),
        search: "TES".into(),
        ..Nav::default()
    };
    let view = build(&core, &nav, &tz()).unwrap();
    let names: Vec<_> = view.tasks.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(names, ["testes"]);
}

#[test]
fn search_without_a_project_lists_every_match() {
    let core = demo_core();
    let nav = Nav {
        search: "autenticacao".into(),
        ..Nav::default()
    };
    let view = build(&core, &nav, &tz()).unwrap();
    assert_eq!(view.page, Page::Search);
    assert_eq!(view.subheading, "1 tarefa");
    assert_eq!(view.tasks[0].project, "connect");
    let none = Nav {
        search: "e-mails".into(),
        ..Nav::default()
    };
    let view = build(&core, &none, &tz()).unwrap();
    assert_eq!(view.tasks[0].project, NO_PROJECT);
}

#[test]
fn task_page_has_checklist_links_and_sessions() {
    let core = demo_core();
    let nav = Nav {
        project: Some(project_id(&core, "connect")),
        task: Some(task_id(&core, "autenticação")),
        ..Nav::default()
    };
    let view = build(&core, &nav, &tz()).unwrap();
    assert_eq!(view.crumb, "connect / autenticação");
    assert_eq!(view.subheading, "Aberta · 3h 40min registrados");
    let task = view.task.unwrap();
    assert_eq!(task.total, "3h 40min");
    assert_eq!(task.checklist, "1/3 concluídas");
    assert!(task.subtasks[0].done && !task.subtasks[1].done);
    assert_eq!(task.links[0].label, "Clássico · 25/5/15");
    let first = &task.sessions[0];
    assert_eq!(
        (
            first.day.as_str(),
            first.range.as_str(),
            first.kind.as_str()
        ),
        ("Hoje", "14:02 – 14:27", "Pomodoro")
    );
    assert_eq!(task.sessions[1].duration, "1h 25min");
    assert_eq!(task.sessions[2].day, "Ontem");
}

#[test]
fn task_page_shows_the_description_and_estimates() {
    let core = demo_core();
    let nav = Nav {
        project: Some(project_id(&core, "connect")),
        task: Some(task_id(&core, "autenticação")),
        ..Nav::default()
    };
    let task = build(&core, &nav, &tz()).unwrap().task.unwrap();
    assert!(task.description.starts_with("Login com e-mail"));
    // 3h 40min recorded of 4h.
    assert_eq!(task.estimate, "4h 00min");
    assert!(!task.estimate_over);
    assert_eq!(task.estimate_label, "91% · faltam 20min");
    let tests = &task.subtasks[1];
    assert_eq!(tests.estimate, "1h 00min");
    assert!(tests.has_note);
    assert_eq!(
        tests.note,
        markdown::styled("Cobrir token expirado e senha errada.")
    );
    assert_eq!(task.subtasks[2].estimate, "");
}

#[test]
fn a_missing_project_falls_back_to_all_projects() {
    let core = demo_core();
    let nav = Nav {
        project: Some(ProjectId(999)),
        ..Nav::default()
    };
    assert_eq!(build(&core, &nav, &tz()).unwrap().page, Page::AllProjects);
}

#[test]
fn a_deleted_task_page_is_an_error() {
    let mut core = demo_core();
    let task = task_id(&core, "testes");
    core.delete_task(task).unwrap();
    let nav = Nav {
        project: Some(project_id(&core, "connect")),
        task: Some(task),
        ..Nav::default()
    };
    assert!(build(&core, &nav, &tz()).is_err());
}

#[test]
fn search_results_mark_tasks_without_a_project() {
    let core = demo_core();
    let nav = Nav {
        search: "e-mails".into(),
        ..Nav::default()
    };
    let view = build(&core, &nav, &tz()).unwrap();
    assert!(!view.tasks[0].has_project);
    let nav = Nav {
        search: "testes".into(),
        ..Nav::default()
    };
    assert!(build(&core, &nav, &tz()).unwrap().tasks[0].has_project);
}

// M7a: alarms in "Vinculado a" and in the task rows.
#[test]
fn task_page_lists_the_alarms_of_the_task_first() {
    let core = demo_core();
    let connect = project_id(&core, "connect");
    let standup = task_id(&core, "standup diário");
    let nav = Nav {
        project: Some(connect),
        task: Some(standup),
        ..Nav::default()
    };
    let task = build(&core, &nav, &tz()).unwrap().task.unwrap();
    let link = &task.links[0];
    assert!(link.is_alarm && !link.is_pomodoro);
    assert_eq!(link.kind, "Alarme");
    assert_eq!(link.label, "Daily standup · 07:30 · seg–sex");
    let alarm = &core.alarms().unwrap()[0];
    assert_eq!(i64::from(link.alarm_id), alarm.id.0);
    let project = build(
        &core,
        &Nav {
            project: Some(connect),
            ..Nav::default()
        },
        &tz(),
    )
    .unwrap();
    let rows: Vec<(&str, bool)> = project
        .tasks
        .iter()
        .map(|t| (t.name.as_str(), t.has_alarm))
        .collect();
    assert!(rows.contains(&("standup diário", true)));
    assert!(rows.contains(&("autenticação", false)));
}
