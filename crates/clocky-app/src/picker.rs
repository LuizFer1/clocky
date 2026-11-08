//! Task picker logic (handoff SPEC 2.6, screens 60-61): what the list shows for a search text
//! and a selection. Pure, apart from [`load`], which reads the core. The window side is in
//! `picker_ui.rs`.

use std::collections::HashSet;

use clocky_core::domain::{ProjectId, TaskId};

use crate::core::{AppCore, CoreResult};
use crate::manage_view::NO_PROJECT;

/// Tasks listed under "Recentes".
const RECENT_LIMIT: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerProject {
    pub id: ProjectId,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerTask {
    pub id: TaskId,
    pub project: Option<ProjectId>,
    pub name: String,
    pub subtasks_done: u32,
    pub subtasks_total: u32,
}

/// What the picker needs from the core for one search text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PickerData {
    pub projects: Vec<PickerProject>,
    /// Open tasks, by project (in project order), then the ones without a project.
    pub tasks: Vec<PickerTask>,
    /// "Recentes" (only without a search text).
    pub recent: Vec<(TaskId, String)>,
    /// Tasks matching the search text; `None` without one.
    pub matches: Option<HashSet<TaskId>>,
}

/// Dot of a group header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dot {
    /// Project color (hex).
    Project(String),
    /// "Recentes".
    Accent,
    /// "Sem projeto".
    Line,
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// "Sem tarefa".
    NoTask { selected: bool },
    /// Group title (already in upper case).
    Header { title: String, dot: Dot },
    Task {
        id: TaskId,
        name: String,
        /// "1/3" or empty.
        subtasks: String,
        selected: bool,
    },
}

/// Reads the picker data for `query`.
pub fn load(core: &AppCore, query: &str) -> CoreResult<PickerData> {
    let projects: Vec<PickerProject> = core
        .projects()?
        .into_iter()
        .map(|p| PickerProject {
            id: p.project.id,
            name: p.project.name,
            color: p.project.color,
        })
        .collect();
    let owners = projects.iter().map(|p| Some(p.id)).chain([None]);
    let mut tasks = Vec::new();
    for owner in owners {
        tasks.extend(
            core.task_summaries(owner, false)?
                .into_iter()
                .map(|s| PickerTask {
                    id: s.task.id,
                    project: s.task.project,
                    name: s.task.name,
                    subtasks_done: s.subtasks_done,
                    subtasks_total: s.subtasks_total,
                }),
        );
    }
    let query = query.trim();
    let (recent, matches) = if query.is_empty() {
        let recent = core.recent_tasks(RECENT_LIMIT)?;
        (recent.into_iter().map(|t| (t.id, t.name)).collect(), None)
    } else {
        let found = core.search_tasks(query)?;
        (Vec::new(), Some(found.iter().map(|t| t.id).collect()))
    };
    Ok(PickerData {
        projects,
        tasks,
        recent,
        matches,
    })
}

/// The list: "Sem tarefa", "Recentes" (without search), then one group per project with
/// matching tasks and "Sem projeto" last.
pub fn rows(data: &PickerData, selected: Option<TaskId>) -> Vec<Row> {
    let mut rows = vec![Row::NoTask {
        selected: selected.is_none(),
    }];
    if data.matches.is_none() && !data.recent.is_empty() {
        rows.push(Row::Header {
            title: "RECENTES".into(),
            dot: Dot::Accent,
        });
        rows.extend(data.recent.iter().map(|(id, name)| Row::Task {
            id: *id,
            name: name.clone(),
            subtasks: String::new(),
            selected: selected == Some(*id),
        }));
    }
    let groups = data
        .projects
        .iter()
        .map(|p| (Some(p.id), p.name.as_str(), Dot::Project(p.color.clone())))
        .chain([(None, NO_PROJECT, Dot::Line)]);
    for (owner, name, dot) in groups {
        group(data, selected, (owner, name, dot), &mut rows);
    }
    rows
}

fn group(
    data: &PickerData,
    selected: Option<TaskId>,
    (owner, name, dot): (Option<ProjectId>, &str, Dot),
    rows: &mut Vec<Row>,
) {
    let tasks: Vec<&PickerTask> = data
        .tasks
        .iter()
        .filter(|t| t.project == owner)
        .filter(|t| data.matches.as_ref().is_none_or(|ids| ids.contains(&t.id)))
        .collect();
    if tasks.is_empty() {
        return;
    }
    rows.push(Row::Header {
        title: name.to_uppercase(),
        dot,
    });
    rows.extend(tasks.into_iter().map(|t| Row::Task {
        id: t.id,
        name: t.name.clone(),
        subtasks: if t.subtasks_total == 0 {
            String::new()
        } else {
            crate::format::ratio(t.subtasks_done, t.subtasks_total)
        },
        selected: selected == Some(t.id),
    }));
}

/// Whether the create card shows: a search text that no task name equals (ignoring case and
/// accents).
pub fn can_create(data: &PickerData, query: &str) -> bool {
    let needle = fold(query.trim());
    !needle.is_empty() && !data.tasks.iter().any(|t| fold(t.name.trim()) == needle)
}

/// Label of the create button: Criar tarefa “texto”.
pub fn create_label(query: &str) -> String {
    format!("Criar tarefa “{}”", query.trim())
}

/// The selection chip: "connect › autenticação", the bare name without a project, or
/// "Sem tarefa"; with the project color (hex) when there is one.
pub fn selection(core: &AppCore, selected: Option<TaskId>) -> (String, Option<String>) {
    let Some(task) = selected.and_then(|id| core.task(id).ok()) else {
        return ("Sem tarefa".into(), None);
    };
    match task.project.and_then(|p| core.project(p).ok()) {
        Some(project) => (
            format!("{} › {}", project.name, task.name),
            Some(project.color),
        ),
        None => (task.name, None),
    }
}

/// Lower case without the common Latin accents (the same folding as the core's search).
pub fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests;
