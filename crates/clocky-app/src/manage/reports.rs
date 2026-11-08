//! M7b: the Relatórios page of the manage window (sidebar footer) and the Sessão detail, also
//! opened from "Tempo registrado" on a task page.

use chrono::{NaiveDate, TimeZone};
use clocky_core::domain::SessionId;
use slint::ComponentHandle;

use super::{Manage, Pending, Section, db_id, model};
use crate::format;
use crate::reports_view::{self, Back, Content, Range, ReportNav, ReportsView, Tab};
use crate::ui;

impl Manage {
    pub(super) fn connect_reports(&self) {
        let reports = self.reports_state();
        reports.on_select_tab(self.handler1(|m, tab| {
            m.edit_reports(|nav| nav.tab = Tab::from_index(tab));
        }));
        reports.on_select_range(self.handler1(|m, range| {
            m.edit_reports(|nav| nav.range = Range::from_index(range));
        }));
        reports.on_previous(self.handler0(|m| m.edit_reports(|nav| nav.shift(false))));
        reports.on_next(self.handler0(|m| m.edit_reports(|nav| nav.shift(true))));
        let today = self.weak.clone();
        reports.on_current(move || {
            if let Some(m) = today.upgrade() {
                let date = m.today();
                m.edit_reports(|nav| nav.date = date);
            }
        });
        reports.on_open_session(self.handler1(|m, id| m.open_session(id, Back::Reports)));
        reports.on_close_session(self.handler0(Self::close_session));
        reports.on_delete_session(self.handler0(Self::ask_delete_session));
        let state = self.state();
        state.on_open_reports(self.handler0(Self::open_reports));
        state.on_open_session(self.handler1(|m, id| {
            let back = m.nav().task.map_or(Back::Reports, Back::Task);
            m.open_session(id, back);
        }));
    }

    fn reports_state(&self) -> ui::ReportsState<'_> {
        self.ui.global::<ui::ReportsState>()
    }

    /// Opens Relatórios where it was left (this week the first time).
    pub fn open_reports(&self) {
        self.section.set(Section::Reports);
        if let Some(nav) = self.reports.borrow_mut().as_mut() {
            nav.session = None;
        }
        self.refresh();
    }

    /// Moves Relatórios to `nav` (screenshot tour) and shows it.
    pub fn show_reports(&self, nav: ReportNav) {
        *self.reports.borrow_mut() = Some(nav);
        self.section.set(Section::Reports);
        self.refresh();
    }

    /// The local date of the core's clock.
    pub fn today(&self) -> NaiveDate {
        let now = self.core.borrow().now();
        chrono::Local
            .timestamp_millis_opt(now.as_millis())
            .earliest()
            .map_or(NaiveDate::MIN, |t| t.date_naive())
    }

    fn report_nav(&self) -> ReportNav {
        let current = *self.reports.borrow();
        current.unwrap_or_else(|| {
            let nav = ReportNav::new(self.today());
            *self.reports.borrow_mut() = Some(nav);
            nav
        })
    }

    fn edit_reports(&self, edit: impl FnOnce(&mut ReportNav)) {
        let mut nav = self.report_nav();
        edit(&mut nav);
        *self.reports.borrow_mut() = Some(nav);
        self.refresh();
    }

    /// Opens the Sessão detail of session `id`; "‹ Voltar" goes to `back`.
    pub fn open_session(&self, id: i32, back: Back) {
        let Some(id) = db_id(id).map(SessionId) else {
            return;
        };
        let mut nav = self.report_nav();
        nav.session = Some((id, back));
        if back == Back::Reports {
            nav.tab = Tab::Daily;
        }
        self.show_reports(nav);
    }

    fn close_session(&self) {
        let back = self.report_nav().session.map(|(_, back)| back);
        if let Some(nav) = self.reports.borrow_mut().as_mut() {
            nav.session = None;
        }
        if let Some(Back::Task(task)) = back {
            self.open_task_id(task);
        }
        // Still here when it came from Diário, or when the task is gone.
        if self.section.get() == Section::Reports {
            self.refresh();
        }
    }

    pub fn ask_delete_session(&self) {
        let Some((id, _)) = self.report_nav().session else {
            return;
        };
        let report = self.core.borrow().session_report(id);
        let detail = match report {
            Ok(report) => {
                let task = report.task.map_or_else(
                    || reports_view::NO_TASK.to_owned(),
                    |t| format!("“{}”", t.name),
                );
                format!("{} de {task}", format::duration(report.effective))
            }
            Err(e) => return tracing::warn!("cannot delete session {}: {e}", id.0),
        };
        self.ask(
            Pending::DeleteSession(id),
            "Apagar esta sessão?",
            &format!(
                "A sessão ({detail}) sai dos relatórios e do tempo registrado da tarefa. Não \
                 dá para desfazer."
            ),
            "Apagar sessão",
        );
    }

    /// The confirmed "Apagar sessão": deletes it and goes back.
    pub(super) fn delete_session(&self, id: SessionId) {
        let deleted = self.core.borrow_mut().delete_session(id);
        if let Err(e) = deleted {
            tracing::warn!("failed to delete session {}: {e}", id.0);
        }
        self.close_session();
    }

    /// Fills the Relatórios page and the header. Runs after every refresh while it is open.
    pub(super) fn apply_reports(&self) {
        let nav = self.report_nav();
        let today = self.today();
        let built = {
            let core = self.core.borrow();
            reports_view::build(&core, &nav, today, &chrono::Local)
        };
        let view = match built {
            Ok(view) => view,
            Err(e) if nav.session.is_some() => {
                // The session is gone: back to the reports.
                tracing::warn!("session detail unavailable: {e}");
                self.reports.borrow_mut().replace(ReportNav {
                    session: None,
                    ..nav
                });
                return self.apply_reports();
            }
            Err(e) => return tracing::error!("failed to read the reports: {e}"),
        };
        self.fill_reports(&nav, &view);
        let state = self.state();
        state.set_page(4);
        state.set_crumb(view.crumb.as_str().into());
        state.set_heading(view.heading.as_str().into());
        state.set_subheading(view.subheading.as_str().into());
        state.set_has_heading_color(false);
    }

    fn fill_reports(&self, nav: &ReportNav, view: &ReportsView) {
        let rs = self.reports_state();
        rs.set_tab(nav.tab as i32);
        rs.set_range(nav.range as i32);
        rs.set_period_label(view.period_label.as_str().into());
        rs.set_current_label(view.current_label.into());
        rs.set_at_current(view.at_current);
        rs.set_total(view.total.as_str().into());
        rs.set_empty(view.empty);
        rs.set_empty_title(view.empty_title.into());
        rs.set_empty_text(view.empty_text.into());
        rs.set_session_open(matches!(view.content, Content::Session(_)));
        match &view.content {
            Content::Overview(rows) => rs.set_rows(model(rows.clone())),
            Content::Weekly(week) => {
                rs.set_days(model(week.days.clone()));
                rs.set_scale_top(week.scale_top.as_str().into());
                rs.set_scale_mid(week.scale_mid.as_str().into());
                rs.set_week_projects(model(week.projects.clone()));
            }
            Content::Daily(day) => {
                rs.set_blocks(model(day.blocks.clone()));
                rs.set_lanes(day.lanes);
                rs.set_markers(model(day.markers.clone()));
                rs.set_ticks(model(day.ticks.clone()));
                rs.set_sessions(model(day.sessions.clone()));
            }
            Content::Session(s) => {
                let back = match nav.session {
                    Some((_, Back::Task(_))) => "Tempo registrado",
                    _ => "Diário",
                };
                rs.set_session_back(back.into());
                rs.set_session_task(s.task.as_str().into());
                rs.set_session_project(s.project.as_str().into());
                rs.set_session_color(s.color);
                rs.set_session_none(s.none);
                rs.set_session_kind(s.kind.as_str().into());
                rs.set_session_when(s.when.as_str().into());
                rs.set_session_effective(s.effective.as_str().into());
                rs.set_session_pauses(s.pauses.as_str().into());
                rs.set_session_wall(s.wall.as_str().into());
                rs.set_session_parts(model(s.parts.clone()));
                rs.set_session_marks(model(s.marks.clone()));
                rs.set_session_ticks(model(s.ticks.clone()));
                rs.set_session_events(model(s.events.clone()));
            }
        }
    }
}
