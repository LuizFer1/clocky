//! The task picker inside the panel window. [`TaskPicker::open`] shows it with the current
//! choice; "Vincular" calls back with the staged `Option<TaskId>` (`None` = "Sem tarefa"), and
//! "Cancelar", back or Esc close it without calling back.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use clocky_core::domain::{ProjectId, TaskId};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::core::AppCore;
use crate::picker::{self, Dot, Row};
use crate::{chips, manage_view, palette, ui};

/// Width available to the create card's chips (panel 380 minus the paddings).
const CHIPS_WIDTH: f32 = 308.0;

type DoneHook = Box<dyn FnOnce(Option<TaskId>)>;

#[derive(Default)]
struct Session {
    query: String,
    selected: Option<TaskId>,
    /// Project of a task created from the search text (`None` = "Sem projeto").
    new_project: Option<ProjectId>,
    on_done: Option<DoneHook>,
}

pub struct TaskPicker {
    core: Rc<RefCell<AppCore>>,
    window: slint::Weak<ui::PanelWindow>,
    session: RefCell<Session>,
    /// Runs when the picker closes, confirmed or not (the panel restores its size).
    on_close: RefCell<Option<Box<dyn Fn()>>>,
    weak: Weak<TaskPicker>,
}

impl TaskPicker {
    /// Binds the picker to the `PickerState` of `window`.
    pub fn new(core: Rc<RefCell<AppCore>>, window: &ui::PanelWindow) -> Rc<Self> {
        let picker = Rc::new_cyclic(|weak| Self {
            core,
            window: window.as_weak(),
            session: RefCell::new(Session::default()),
            on_close: RefCell::new(None),
            weak: weak.clone(),
        });
        picker.connect(window);
        picker
    }

    fn connect(&self, window: &ui::PanelWindow) {
        let state = window.global::<ui::PickerState>();
        let weak = self.weak.clone();
        state.on_query_edited(move |text| {
            if let Some(p) = weak.upgrade() {
                p.set_query(&text);
            }
        });
        let weak = self.weak.clone();
        state.on_pick(move |id| {
            if let Some(p) = weak.upgrade() {
                p.pick(id);
            }
        });
        let weak = self.weak.clone();
        state.on_pick_project(move |id| {
            if let Some(p) = weak.upgrade() {
                p.session.borrow_mut().new_project = db_id(id).map(ProjectId);
                p.refresh();
            }
        });
        let weak = self.weak.clone();
        state.on_create(move || weak.upgrade().iter().for_each(|p| p.create()));
        let weak = self.weak.clone();
        state.on_confirm(move || weak.upgrade().iter().for_each(|p| p.finish(true)));
        let weak = self.weak.clone();
        state.on_cancel(move || weak.upgrade().iter().for_each(|p| p.finish(false)));
    }

    /// What to do when the picker closes (confirmed or cancelled).
    pub fn set_on_close(&self, hook: impl Fn() + 'static) {
        *self.on_close.borrow_mut() = Some(Box::new(hook));
    }

    /// Shows the picker with `current` selected. `on_done` runs only on "Vincular".
    pub fn open(&self, current: Option<TaskId>, on_done: impl FnOnce(Option<TaskId>) + 'static) {
        *self.session.borrow_mut() = Session {
            selected: current,
            on_done: Some(Box::new(on_done)),
            ..Session::default()
        };
        if let Some(window) = self.window.upgrade() {
            window
                .global::<ui::PickerState>()
                .set_query(SharedString::new());
            window.set_picking(true);
        }
        self.refresh();
    }

    pub fn is_open(&self) -> bool {
        self.window.upgrade().is_some_and(|w| w.get_picking())
    }

    /// Sets the search text (the search field, and the screenshot hook).
    pub fn set_query(&self, text: &str) {
        text.clone_into(&mut self.session.borrow_mut().query);
        if let Some(window) = self.window.upgrade() {
            let state = window.global::<ui::PickerState>();
            if state.get_query() != text {
                state.set_query(text.into());
            }
        }
        self.refresh();
    }

    fn pick(&self, id: i32) {
        self.session.borrow_mut().selected = db_id(id).map(TaskId);
        self.refresh();
    }

    /// "Criar tarefa “texto”": creates the task, selects it and clears the search.
    fn create(&self) {
        let (name, project) = {
            let session = self.session.borrow();
            (session.query.trim().to_owned(), session.new_project)
        };
        if name.is_empty() {
            return;
        }
        let created = self.core.borrow_mut().create_task(project, &name);
        match created {
            Ok(id) => {
                self.session.borrow_mut().selected = Some(id);
                self.set_query("");
            }
            Err(e) => tracing::warn!("failed to create the task from the picker: {e}"),
        }
    }

    /// Closes the picker; `confirmed` hands the selection to the caller.
    pub fn finish(&self, confirmed: bool) {
        let (hook, selected) = {
            let mut session = self.session.borrow_mut();
            (session.on_done.take(), session.selected)
        };
        if let Some(window) = self.window.upgrade() {
            window.set_picking(false);
        }
        if let Some(close) = self.on_close.borrow().as_ref() {
            close();
        }
        if let (true, Some(hook)) = (confirmed, hook) {
            hook(selected);
        }
    }

    /// Rebuilds the list from the core (short borrow), then updates the window.
    fn refresh(&self) {
        let (query, selected, new_project) = {
            let s = self.session.borrow();
            (s.query.clone(), s.selected, s.new_project)
        };
        let loaded = {
            let core = self.core.borrow();
            picker::load(&core, &query).map(|data| (data, picker::selection(&core, selected)))
        };
        let (data, (label, color)) = match loaded {
            Ok(loaded) => loaded,
            Err(e) => return tracing::warn!("failed to load the task picker: {e}"),
        };
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let state = window.global::<ui::PickerState>();
        let rows: Vec<ui::PickerRow> = picker::rows(&data, selected).iter().map(row).collect();
        state.set_rows(ModelRc::new(VecModel::from(rows)));
        state.set_selected_label(label.into());
        state.set_selected_has_project(color.is_some());
        state.set_selected_color(color.as_deref().map(palette::color).unwrap_or_default());
        state.set_can_create(picker::can_create(&data, &query));
        state.set_create_label(picker::create_label(&query).into());
        state.set_create_chips(create_chips(&data, new_project));
    }
}

fn create_chips(data: &picker::PickerData, chosen: Option<ProjectId>) -> ModelRc<ui::ChipRow> {
    let mut items: Vec<ui::ProjectItem> = data
        .projects
        .iter()
        .map(|p| ui::ProjectItem {
            id: manage_view::slint_id(p.id.0),
            name: p.name.as_str().into(),
            color: palette::color(&p.color),
            selected: chosen == Some(p.id),
            ..ui::ProjectItem::default()
        })
        .collect();
    items.push(chips::no_project_chip(chosen.is_none()));
    chips::rows(&items, CHIPS_WIDTH)
}

/// A picker row for Slint.
fn row(row: &Row) -> ui::PickerRow {
    match row {
        Row::NoTask { selected } => ui::PickerRow {
            kind: ui::PickerRowKind::None,
            id: -1,
            name: "Sem tarefa".into(),
            selected: *selected,
            ..ui::PickerRow::default()
        },
        Row::Header { title, dot } => ui::PickerRow {
            kind: ui::PickerRowKind::Header,
            id: -1,
            name: title.as_str().into(),
            color: match dot {
                Dot::Project(hex) => palette::color(hex),
                Dot::Accent | Dot::Line => slint::Color::default(),
            },
            dot: match dot {
                Dot::Project(_) => ui::DotStyle::Project,
                Dot::Accent => ui::DotStyle::Accent,
                Dot::Line => ui::DotStyle::Line,
            },
            ..ui::PickerRow::default()
        },
        Row::Task {
            id,
            name,
            subtasks,
            selected,
        } => ui::PickerRow {
            kind: ui::PickerRowKind::Task,
            id: manage_view::slint_id(id.0),
            name: name.as_str().into(),
            subtasks: subtasks.as_str().into(),
            selected: *selected,
            ..ui::PickerRow::default()
        },
    }
}

fn db_id(id: i32) -> Option<i64> {
    (id >= 0).then_some(i64::from(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_map_to_slint_kinds() {
        let none = row(&Row::NoTask { selected: true });
        assert_eq!(
            (none.kind, none.id, none.selected),
            (ui::PickerRowKind::None, -1, true)
        );
        let header = row(&Row::Header {
            title: "CONNECT".into(),
            dot: Dot::Project("#3d8fd6".into()),
        });
        assert_eq!(header.dot, ui::DotStyle::Project);
        assert_eq!(header.color, palette::color("#3d8fd6"));
        let recent = row(&Row::Header {
            title: "RECENTES".into(),
            dot: Dot::Accent,
        });
        assert_eq!(recent.dot, ui::DotStyle::Accent);
        let task = row(&Row::Task {
            id: TaskId(7),
            name: "x".into(),
            subtasks: "1/3".into(),
            selected: false,
        });
        assert_eq!(
            (task.kind, task.id, task.subtasks.as_str()),
            (ui::PickerRowKind::Task, 7, "1/3")
        );
    }
}
