//! Geral: time per project, then per task, with bars in the project color.

use clocky_core::reports::Overview;

use super::{NO_TASK, project_label, share};
use crate::{format, ui};

/// One row per project (with its share of the period) followed by its tasks. Every bar is
/// relative to the largest project, so the rows compare at a glance.
pub fn rows(overview: &Overview) -> Vec<ui::ReportRow> {
    let largest = overview
        .projects
        .iter()
        .map(|p| p.total)
        .max()
        .unwrap_or_default();
    let mut rows = Vec::new();
    for project in &overview.projects {
        let (name, color, none) = project_label(project.project.as_ref());
        rows.push(ui::ReportRow {
            is_task: false,
            name: name.into(),
            color,
            none,
            archived: project.project.as_ref().is_some_and(|p| p.archived),
            total: format::duration(project.total).into(),
            share: percent(share(project.total, overview.total)).into(),
            bar: share(project.total, largest),
        });
        for task in &project.tasks {
            rows.push(ui::ReportRow {
                is_task: true,
                name: task
                    .task
                    .as_ref()
                    .map_or(NO_TASK, |t| t.name.as_str())
                    .into(),
                color,
                none: none || task.task.is_none(),
                archived: task.task.as_ref().is_some_and(|t| t.archived),
                total: format::duration(task.total).into(),
                share: "".into(),
                bar: share(task.total, largest),
            });
        }
    }
    rows
}

/// "42%", rounded; "<1%" for a sliver.
fn percent(fraction: f32) -> String {
    let value = (fraction * 100.0).round();
    if value < 1.0 && fraction > 0.0 {
        "<1%".into()
    } else {
        format!("{value:.0}%")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_rounds_and_keeps_slivers_visible() {
        assert_eq!(percent(0.424), "42%");
        assert_eq!(percent(1.0), "100%");
        assert_eq!(percent(0.001), "<1%");
        assert_eq!(percent(0.0), "0%");
    }
}
