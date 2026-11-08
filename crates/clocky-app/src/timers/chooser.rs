//! Choosing the task of a timer.
//!
//! [`TaskChooser`] is the narrow seam the panels depend on. [`ListChooser`] is a temporary
//! implementation: a plain list of every open task plus "Sem tarefa", drawn over the panel body.
//! Merge note (M6a): M6a's full picker (search, recents, groups, create) implements
//! `TaskChooser` and replaces `ListChooser` in `TimerPanels::new`; nothing else changes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use clocky_core::domain::{ProjectId, TaskId};
use clocky_core::store::Store;
use slint::{ComponentHandle, Model, VecModel};

use super::convert::{color, row_from_ui, row_to_ui};
use crate::core::AppCore;
use crate::ui;

/// Called with the chosen task (`None` = "Sem tarefa"). Not called when the user cancels.
pub type OnChosen = Box<dyn FnOnce(Option<TaskId>)>;

pub trait TaskChooser {
    /// Opens the chooser with `current` marked as selected.
    fn choose(&self, current: Option<TaskId>, on_chosen: OnChosen);
}

pub struct ListChooser {
    ui: slint::Weak<ui::PanelWindow>,
    handle: Rc<RefCell<AppCore>>,
    rows: Rc<VecModel<ui::ChooserRow>>,
    pending: Rc<RefCell<Option<OnChosen>>>,
}

impl ListChooser {
    pub fn new(ui: &ui::PanelWindow, handle: Rc<RefCell<AppCore>>) -> Self {
        let chooser = Self {
            ui: ui.as_weak(),
            handle,
            rows: Rc::new(VecModel::default()),
            pending: Rc::new(RefCell::new(None)),
        };
        let global = ui.global::<ui::TimerUi>();
        global.set_chooser_rows(chooser.rows.clone().into());
        let (weak, pending) = (chooser.ui.clone(), chooser.pending.clone());
        global.on_chooser_pick(move |id| {
            // Take the callback out first: it may re-enter the panels (and this chooser).
            let on_chosen = pending.borrow_mut().take();
            hide(&weak);
            if let Some(on_chosen) = on_chosen {
                on_chosen(row_from_ui(id).map(TaskId));
            }
        });
        let (weak, pending) = (chooser.ui.clone(), chooser.pending.clone());
        global.on_chooser_cancel(move || {
            drop(pending.borrow_mut().take());
            hide(&weak);
        });
        chooser
    }
}

fn hide(ui: &slint::Weak<ui::PanelWindow>) {
    if let Some(ui) = ui.upgrade() {
        ui.global::<ui::TimerUi>().set_chooser_visible(false);
    }
}

impl TaskChooser for ListChooser {
    fn choose(&self, current: Option<TaskId>, on_chosen: OnChosen) {
        let rows = {
            let handle = self.handle.borrow();
            rows(handle.tracker().store(), current)
        };
        *self.pending.borrow_mut() = Some(on_chosen);
        self.rows.set_vec(rows);
        if let Some(ui) = self.ui.upgrade() {
            let global = ui.global::<ui::TimerUi>();
            global.set_chooser_none_selected(current.is_none());
            global.set_chooser_visible(true);
        }
        tracing::debug!("task chooser open with {} tasks", self.rows.row_count());
    }
}

/// Every open task, with its project, in search order.
fn rows(store: &Store, current: Option<TaskId>) -> Vec<ui::ChooserRow> {
    let projects: HashMap<ProjectId, (String, String)> = store
        .projects(false)
        .unwrap_or_else(|e| {
            tracing::warn!("cannot read the projects: {e}");
            Vec::new()
        })
        .into_iter()
        .map(|p| (p.id, (p.name, p.color)))
        .collect();
    let tasks = store.search_tasks("").unwrap_or_else(|e| {
        tracing::warn!("cannot list the tasks: {e}");
        Vec::new()
    });
    tasks
        .into_iter()
        .filter(|task| task.completed_at.is_none())
        .map(|task| {
            let (project, hex) = task
                .project
                .and_then(|p| projects.get(&p).cloned())
                .unwrap_or_else(|| ("Sem projeto".to_owned(), String::new()));
            ui::ChooserRow {
                id: row_to_ui(task.id.0),
                project: project.into(),
                task: task.name.into(),
                color: color(&hex),
                selected: Some(task.id) == current,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use clocky_core::time::Timestamp;

    use super::*;

    #[test]
    fn rows_list_open_tasks_with_their_project() {
        let t0 = Timestamp::from_millis(0);
        let mut store = Store::open_in_memory().unwrap();
        let p = store.create_project("connect", "#3d8fd6", t0).unwrap();
        let a = store.create_task(Some(p), "autenticação", t0).unwrap();
        let done = store.create_task(Some(p), "deploy", t0).unwrap();
        store.complete_task(done, t0).unwrap();
        store.create_task(None, "avulsa", t0).unwrap();
        let rows = rows(&store, Some(a));
        let names: Vec<(String, String, bool)> = rows
            .iter()
            .map(|r| (r.project.to_string(), r.task.to_string(), r.selected))
            .collect();
        assert_eq!(
            names,
            [
                ("connect".into(), "autenticação".into(), true),
                ("Sem projeto".into(), "avulsa".into(), false),
            ]
        );
    }
}
