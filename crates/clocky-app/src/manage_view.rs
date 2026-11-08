//! What the manage window shows, built from the core (pure: no window, no clock, no time zone
//! lookup). `manage.rs` pushes the result into the Slint models after each change.

use std::collections::{HashMap, HashSet};

use chrono::TimeZone;
use clocky_core::domain::{ProjectId, TaskId};
use clocky_core::store::{ProjectOverview, Session, TaskLinks, TaskSummary};
use slint::Color;

use crate::core::{AppCore, CoreResult};
use crate::{format, palette, ui};

/// Sessions listed under "Tempo registrado".
const RECENT_SESSIONS: u32 = 5;
/// Label of the tasks without a project.
pub const NO_PROJECT: &str = "Sem projeto";

/// Where the user is. The page follows from it (see [`Nav::page`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Nav {
    pub project: Option<ProjectId>,
    pub task: Option<TaskId>,
    pub show_completed: bool,
    /// Sidebar search text.
    pub search: String,
}

/// The content page. The discriminant is the `page` property of `ManageState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    AllProjects = 0,
    Project = 1,
    Task = 2,
    Search = 3,
}

impl Nav {
    pub fn page(&self) -> Page {
        if self.task.is_some() {
            Page::Task
        } else if self.project.is_some() {
            Page::Project
        } else if self.search.trim().is_empty() {
            Page::AllProjects
        } else {
            Page::Search
        }
    }
}

/// Everything the manage window shows.
#[derive(Debug, Clone)]
pub struct ManageView {
    pub page: Page,
    pub projects: Vec<ui::ProjectItem>,
    /// "Clocky / <crumb>"
    pub crumb: String,
    pub heading: String,
    pub subheading: String,
    /// Dot before the heading (project and task pages).
    pub heading_color: Option<Color>,
    pub tasks: Vec<ui::TaskRow>,
    pub task: Option<TaskDetail>,
}

/// The task page.
#[derive(Debug, Clone)]
pub struct TaskDetail {
    pub done: bool,
    /// Project name, or "Sem projeto".
    pub project: String,
    /// "3h 40min"
    pub total: String,
    pub subtasks: Vec<ui::SubtaskRow>,
    /// "1/3 concluídas"
    pub checklist: String,
    pub progress: f32,
    pub links: Vec<ui::LinkRow>,
    pub sessions: Vec<ui::SessionRow>,
}

/// A database id as a Slint `int`. `None` (logged) when it does not fit: the row is left out
/// rather than given an id that could collide with another one.
pub fn slint_id(id: i64) -> Option<i32> {
    let fits = i32::try_from(id).ok();
    if fits.is_none() {
        tracing::warn!("id {id} does not fit the UI model; its row is left out");
    }
    fits
}

/// Builds the whole view for `nav`. `tz` is the local time zone of the session rows.
pub fn build<Tz: TimeZone>(core: &AppCore, nav: &Nav, tz: &Tz) -> CoreResult<ManageView> {
    let overview = core.projects()?;
    let projects = project_items(&overview, nav);
    let mut view = ManageView {
        page: nav.page(),
        projects,
        crumb: "Projetos".into(),
        heading: "Todos os projetos".into(),
        subheading: format::project_count(overview.len()),
        heading_color: None,
        tasks: Vec::new(),
        task: None,
    };
    match view.page {
        Page::AllProjects => {}
        Page::Project => fill_project(core, nav, &overview, &mut view)?,
        Page::Task => fill_task(core, nav, &overview, tz, &mut view)?,
        Page::Search => fill_search(core, nav, &overview, &mut view)?,
    }
    Ok(view)
}

fn project_items(overview: &[ProjectOverview], nav: &Nav) -> Vec<ui::ProjectItem> {
    overview
        .iter()
        .filter_map(|p| {
            Some(ui::ProjectItem {
                id: slint_id(p.project.id.0)?,
                name: p.project.name.as_str().into(),
                color: palette::color(&p.project.color),
                open: i32::try_from(p.open_tasks).unwrap_or(i32::MAX),
                counts: format::task_counts(p.open_tasks, p.completed_tasks).into(),
                progress: format::fraction(p.completed_tasks, p.open_tasks + p.completed_tasks),
                selected: nav.project == Some(p.project.id),
                none: false,
            })
        })
        .collect()
}

fn find(overview: &[ProjectOverview], id: ProjectId) -> Option<&ProjectOverview> {
    overview.iter().find(|p| p.project.id == id)
}

fn fill_project(
    core: &AppCore,
    nav: &Nav,
    overview: &[ProjectOverview],
    view: &mut ManageView,
) -> CoreResult<()> {
    let Some(project) = nav.project.and_then(|id| find(overview, id)) else {
        view.page = Page::AllProjects;
        return Ok(());
    };
    view.crumb.clone_from(&project.project.name);
    view.heading.clone_from(&project.project.name);
    view.subheading = format::task_counts(project.open_tasks, project.completed_tasks);
    view.heading_color = Some(palette::color(&project.project.color));
    let summaries = core.task_summaries(Some(project.project.id), nav.show_completed)?;
    let matches = search_ids(core, &nav.search)?;
    view.tasks = summaries
        .iter()
        .filter(|s| matches.as_ref().is_none_or(|ids| ids.contains(&s.task.id)))
        .filter_map(|s| task_row(s, None))
        .collect();
    Ok(())
}

/// Ids matching the search text; `None` when there is no search.
fn search_ids(core: &AppCore, text: &str) -> CoreResult<Option<HashSet<TaskId>>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        core.search_tasks(text)?.iter().map(|t| t.id).collect(),
    ))
}

/// A task row. `project` is the label shown in the search results: the project name and
/// color, or "Sem projeto" without a color.
fn task_row(summary: &TaskSummary, project: Option<(&str, Option<Color>)>) -> Option<ui::TaskRow> {
    let (name, color) = project.unwrap_or(("", None));
    Some(ui::TaskRow {
        id: slint_id(summary.task.id.0)?,
        name: summary.task.name.as_str().into(),
        done: summary.task.completed_at.is_some(),
        subtasks: format::subtasks_label(summary.subtasks_done, summary.subtasks_total).into(),
        project: name.into(),
        has_project: color.is_some(),
        project_color: color.unwrap_or_default(),
        has_pomodoro: !summary.links.pomodoro_presets.is_empty(),
        has_timer: !summary.links.timer_presets.is_empty(),
        time: format::tracked(summary.tracked).into(),
    })
}

fn fill_search(
    core: &AppCore,
    nav: &Nav,
    overview: &[ProjectOverview],
    view: &mut ManageView,
) -> CoreResult<()> {
    let found = core.search_tasks(nav.search.trim())?;
    let mut summaries: HashMap<TaskId, TaskSummary> = HashMap::new();
    let owners: HashSet<Option<ProjectId>> = found.iter().map(|t| t.project).collect();
    for owner in owners {
        for summary in core.task_summaries(owner, true)? {
            summaries.insert(summary.task.id, summary);
        }
    }
    view.tasks = found
        .iter()
        .filter_map(|task| summaries.get(&task.id))
        .filter(|s| nav.show_completed || s.task.completed_at.is_none())
        .filter_map(|s| {
            let owner = s.task.project.and_then(|id| find(overview, id));
            let label = owner.map_or((NO_PROJECT, None), |p| {
                (
                    p.project.name.as_str(),
                    Some(palette::color(&p.project.color)),
                )
            });
            task_row(s, Some(label))
        })
        .collect();
    view.crumb = "Busca".into();
    view.heading = format!("Busca: “{}”", nav.search.trim());
    view.subheading = match view.tasks.len() {
        0 => "Nenhuma tarefa encontrada".into(),
        1 => "1 tarefa".into(),
        n => format!("{n} tarefas"),
    };
    Ok(())
}

fn fill_task<Tz: TimeZone>(
    core: &AppCore,
    nav: &Nav,
    overview: &[ProjectOverview],
    tz: &Tz,
    view: &mut ManageView,
) -> CoreResult<()> {
    let Some(id) = nav.task else { return Ok(()) };
    let task = core.task(id)?;
    let owner = task.project.and_then(|p| find(overview, p));
    let project = owner.map_or(NO_PROJECT, |p| p.project.name.as_str());
    let tracked = core.task_time(id)?;
    let subtasks = core.subtasks(id)?;
    let done = u32::try_from(subtasks.iter().filter(|s| s.completed_at.is_some()).count())
        .unwrap_or(u32::MAX);
    let total = u32::try_from(subtasks.len()).unwrap_or(u32::MAX);
    view.crumb = format!("{project} / {}", task.name);
    view.heading.clone_from(&task.name);
    view.subheading = format::task_status(task.completed_at.is_some(), tracked);
    view.heading_color = owner.map(|p| palette::color(&p.project.color));
    let now = core.now();
    view.task = Some(TaskDetail {
        done: task.completed_at.is_some(),
        project: project.to_owned(),
        total: format::duration(tracked),
        subtasks: subtasks
            .iter()
            .filter_map(|s| {
                Some(ui::SubtaskRow {
                    id: slint_id(s.id.0)?,
                    name: s.name.as_str().into(),
                    done: s.completed_at.is_some(),
                })
            })
            .collect(),
        checklist: format::checklist_label(done, total),
        progress: format::fraction(done, total),
        links: link_rows(&core.task_links(id)?),
        sessions: core
            .recent_sessions(id, RECENT_SESSIONS)?
            .iter()
            .map(|s| session_row(s, now, tz))
            .collect(),
    });
    Ok(())
}

fn link_rows(links: &TaskLinks) -> Vec<ui::LinkRow> {
    let pomodoros = links.pomodoro_presets.iter().map(|p| ui::LinkRow {
        kind: "Pomodoro".into(),
        label: format::pomodoro_link(&p.name, p.focus, p.short_break, p.long_break).into(),
        is_pomodoro: true,
    });
    let timers = links.timer_presets.iter().map(|t| ui::LinkRow {
        kind: "Temporizador".into(),
        label: format::timer_link(&t.name, t.duration).into(),
        is_pomodoro: false,
    });
    pomodoros.chain(timers).collect()
}

fn session_row<Tz: TimeZone>(
    session: &Session,
    now: clocky_core::time::Timestamp,
    tz: &Tz,
) -> ui::SessionRow {
    let (day, range) = format::session_when(session.started_at, session.ended_at, now, tz);
    ui::SessionRow {
        // M7b: opens the Sessão detail (-1, ignored, for an id the UI cannot hold).
        id: slint_id(session.id.0).unwrap_or(-1),
        day: day.into(),
        range: range.into(),
        kind: format::kind_label(session.kind).into(),
        duration: format::duration(session.effective).into(),
    }
}

#[cfg(test)]
mod tests;
