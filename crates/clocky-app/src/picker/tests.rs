use chrono::FixedOffset;
use clocky_core::clock::FakeClock;
use clocky_core::time::Timestamp;

use super::*;
use crate::demo;

fn demo_core() -> AppCore {
    let clock = FakeClock::new(Timestamp::from_millis(1_794_178_800_000));
    let mut core = AppCore::in_memory(Box::new(clock));
    let tz = FixedOffset::west_opt(3 * 3600).unwrap();
    demo::seed_if_empty(&mut core, &tz).unwrap();
    core
}

fn titles(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .map(|row| match row {
            Row::NoTask { .. } => "-".to_owned(),
            Row::Header { title, .. } => format!("#{title}"),
            Row::Task { name, .. } => name.clone(),
        })
        .collect()
}

fn id_of(core: &AppCore, name: &str) -> TaskId {
    core.search_tasks(name).unwrap()[0].id
}

#[test]
fn without_search_lists_recent_then_projects() {
    let core = demo_core();
    let data = load(&core, "").unwrap();
    let rows = rows(&data, Some(id_of(&core, "autenticação")));
    assert_eq!(
        titles(&rows)[..8],
        [
            "-",
            "#RECENTES",
            "autenticação",
            "TCC",
            "testes",
            "#CONNECT",
            "autenticação",
            "testes"
        ]
    );
    let t = titles(&rows);
    assert!(t.contains(&"#FACULDADE".to_owned()));
    assert_eq!(t.last().unwrap(), "organizar e-mails");
    assert_eq!(t[t.len() - 2], "#SEM PROJETO");
    // Completed tasks are not offered.
    assert!(!t.contains(&"lista de cálculo".to_owned()));
}

#[test]
fn a_completed_recent_task_is_not_offered() {
    let mut core = demo_core();
    let auth = id_of(&core, "autenticação");
    core.set_task_done(auth, true).unwrap();
    let t = titles(&rows(&load(&core, "").unwrap(), None));
    assert_eq!(t[..4], ["-", "#RECENTES", "TCC", "testes"]);
    assert!(!t.contains(&"autenticação".to_owned()));
}

#[test]
fn selection_marks_rows_and_counts_subtasks() {
    let core = demo_core();
    let auth = id_of(&core, "autenticação");
    let rows = rows(&load(&core, "").unwrap(), Some(auth));
    let selected: Vec<_> = rows
        .iter()
        .filter(|r| matches!(r, Row::Task { selected: true, .. }))
        .collect();
    assert_eq!(selected.len(), 2, "in Recentes and in its project");
    assert!(rows.contains(&Row::NoTask { selected: false }));
    assert!(rows.iter().any(|r| matches!(
        r,
        Row::Task { name, subtasks, .. } if name == "TCC" && subtasks == "2/5"
    )));
}

#[test]
fn search_filters_by_task_or_project_and_hides_recent() {
    let core = demo_core();
    let by_project = rows(&load(&core, "faculdade").unwrap(), None);
    assert_eq!(titles(&by_project), ["-", "#FACULDADE", "TCC"]);
    let by_task = rows(&load(&core, "REVIEW").unwrap(), None);
    assert_eq!(titles(&by_task), ["-", "#CONNECT", "code review"]);
    assert!(by_task.contains(&Row::NoTask { selected: true }));
}

#[test]
fn create_card_only_without_an_exact_match() {
    let core = demo_core();
    let data = load(&core, "relatório mensal").unwrap();
    assert!(can_create(&data, "relatório mensal"));
    assert_eq!(titles(&rows(&data, None)), ["-"]);
    assert!(!can_create(&data, "  Autenticacao "));
    assert!(can_create(&data, "autent"));
    assert!(!can_create(&data, "   "));
    assert_eq!(create_label(" x "), "Criar tarefa “x”");
}

#[test]
fn selection_chip_labels() {
    let mut core = demo_core();
    let auth = id_of(&core, "autenticação");
    assert_eq!(
        selection(&core, Some(auth)),
        (
            "connect › autenticação".to_owned(),
            Some("#3d8fd6".to_owned())
        )
    );
    let loose = core.create_task(None, "solta").unwrap();
    assert_eq!(selection(&core, Some(loose)), ("solta".to_owned(), None));
    assert_eq!(selection(&core, None), ("Sem tarefa".to_owned(), None));
}

#[test]
fn creating_from_the_search_text_lands_in_the_chosen_project() {
    let mut core = demo_core();
    let faculdade = core.projects().unwrap()[1].project.id;
    let id = create_task(&mut core, "  relatório mensal ", Some(faculdade))
        .unwrap()
        .unwrap();
    let task = core.task(id).unwrap();
    assert_eq!(
        (task.name.as_str(), task.project),
        ("relatório mensal", Some(faculdade))
    );
    let t = titles(&rows(&load(&core, "").unwrap(), Some(id)));
    let group = t.iter().position(|x| x == "#FACULDADE").unwrap();
    assert!(t[group..].contains(&"relatório mensal".to_owned()));
    assert_eq!(create_task(&mut core, "   ", None).unwrap(), None);
}

#[test]
fn folding_ignores_case_and_accents() {
    assert_eq!(fold("Autenticação ÉÑ"), "autenticacao en");
}
