//! M7b: "Arquivar projeto" and "Arquivar tarefa" ask first, in the neutral dialog (archiving
//! loses nothing: the history stays in the reports).

use clocky_core::domain::{ProjectId, TaskId};

use super::{Manage, Pending};

impl Manage {
    /// "Arquivar projeto" asks first (M7b).
    pub fn ask_archive_project(&self) {
        let Some(id) = self.nav().project else { return };
        let name = self.core.borrow().project(id).map(|p| p.name);
        let name = match name {
            Ok(name) => name,
            Err(e) => return tracing::warn!("cannot archive project {}: {e}", id.0),
        };
        self.ask_archive(
            Pending::ArchiveProject(id),
            &format!("Arquivar o projeto “{name}”?"),
            "O projeto e as tarefas dele saem das listas e do seletor de tarefas. O tempo \
             registrado continua nos relatórios.",
            "Arquivar projeto",
        );
    }

    pub(super) fn archive_project(&self, id: ProjectId) {
        self.leave(true);
        self.mutate("archive the project", |c| c.archive_project(id));
        self.show_all();
    }

    /// The neutral dialog of "Arquivar …" (nothing is lost).
    fn ask_archive(&self, pending: Pending, title: &str, text: &str, action: &str) {
        self.ask(pending, title, text, action);
        self.state().set_confirm_archive(true);
    }

    /// "Arquivar tarefa" asks first (M7b).
    pub fn ask_archive_task(&self) {
        let Some(id) = self.nav().task else { return };
        let name = self.core.borrow().task(id).map(|t| t.name);
        let name = match name {
            Ok(name) => name,
            Err(e) => return tracing::warn!("cannot archive task {}: {e}", id.0),
        };
        self.ask_archive(
            Pending::ArchiveTask(id),
            &format!("Arquivar a tarefa “{name}”?"),
            "A tarefa sai da lista do projeto e do seletor de tarefas. As sessões continuam \
             nos relatórios.",
            "Arquivar tarefa",
        );
    }

    pub(super) fn archive_task(&self, id: TaskId) {
        self.leave(false);
        self.mutate("archive the task", |c| c.archive_task(id));
        self.back_to_project();
    }
}
