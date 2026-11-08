//! Floating panel opened by a radial option, anchored next to the dock. One window is reused
//! for every option. M5 shows a placeholder body; M6b fills the contents. The task picker
//! ("Vincular tarefa") opens inside it: see [`Panel::pick_task`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use clocky_core::domain::TaskId;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{ComponentHandle, PhysicalPosition};

use crate::core::AppCore;
use crate::dock::hide_from_taskbar;
use crate::geometry::{Anchor, PANEL_H};
use crate::picker_ui::TaskPicker;
use crate::{palette, picker, ui};

/// Height of the panel while it shows the task picker (logical px).
pub const PICKER_H: f64 = 680.0;

/// Radial options that open a panel ("Gerenciar" opens the manage window instead).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    Alarm,
    Countdown,
    Stopwatch,
    Pomodoro,
    Overview,
}

/// What the placeholder body shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelContent {
    pub title: &'static str,
    pub subtitle: &'static str,
    pub empty_title: &'static str,
    pub empty_text: &'static str,
    pub action_label: &'static str,
    pub has_footer: bool,
    /// The task line of the timer panels (SPEC 2).
    pub has_task_line: bool,
}

const COMING_SOON: &str = "Em breve";
const COMING_SOON_TEXT: &str = "Este painel chega na próxima versão do Clocky.";

impl PanelKind {
    /// Maps the radial option id (`radial-menu.slint`) to a panel.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "alarme" => Some(Self::Alarm),
            "temporizador" => Some(Self::Countdown),
            "cronometro" => Some(Self::Stopwatch),
            "pomodoro" => Some(Self::Pomodoro),
            "visualizar" => Some(Self::Overview),
            _ => None,
        }
    }

    pub fn content(self) -> PanelContent {
        let placeholder = |title| PanelContent {
            title,
            subtitle: "",
            empty_title: COMING_SOON,
            empty_text: COMING_SOON_TEXT,
            action_label: "",
            has_footer: true,
            has_task_line: false,
        };
        let timer = |title| PanelContent {
            has_task_line: true,
            ..placeholder(title)
        };
        match self {
            Self::Alarm => placeholder("Alarme"),
            Self::Countdown => timer("Temporizador"),
            Self::Stopwatch => timer("Cronômetro"),
            Self::Pomodoro => timer("Pomodoro"),
            // Screen 50, empty state: nothing runs until M6.
            Self::Overview => PanelContent {
                title: "Ativos",
                subtitle: "0 timers · 0 alarmes",
                empty_title: "Nada rodando agora",
                empty_text: "Inicie um timer pelo menu.",
                action_label: "Abrir menu",
                has_footer: false,
                has_task_line: false,
            },
        }
    }

    fn icon_svg(self) -> &'static [u8] {
        match self {
            Self::Alarm => include_bytes!("../icons/alarm.svg"),
            Self::Countdown => include_bytes!("../icons/hourglass.svg"),
            Self::Stopwatch => include_bytes!("../icons/stopwatch.svg"),
            Self::Pomodoro => include_bytes!("../icons/pomodoro.svg"),
            Self::Overview => include_bytes!("../icons/eye.svg"),
        }
    }

    pub fn icon(self) -> Result<slint::Image, slint::LoadImageError> {
        slint::Image::load_from_svg_data(self.icon_svg())
    }
}

pub struct Panel {
    pub ui: ui::PanelWindow,
    pub picker: Rc<TaskPicker>,
    core: Rc<RefCell<AppCore>>,
    /// Where the panel was last placed (the picker re-places it with its own height).
    anchor: Cell<Option<Anchor>>,
    /// Task of the placeholder's task line. M6b moves this into each timer.
    linked: Cell<Option<TaskId>>,
    /// Taskbar and focus-out handling are set once, when the winit window first exists.
    prepared: Cell<bool>,
    weak: Weak<Panel>,
}

impl Panel {
    pub fn new(core: Rc<RefCell<AppCore>>) -> anyhow::Result<Rc<Self>> {
        let ui = ui::PanelWindow::new()?;
        let picker = TaskPicker::new(core.clone(), &ui);
        let panel = Rc::new_cyclic(|weak| Self {
            ui,
            picker,
            core,
            anchor: Cell::new(None),
            linked: Cell::new(None),
            prepared: Cell::new(false),
            weak: weak.clone(),
        });
        let weak = panel.weak.clone();
        panel.picker.set_on_close(move || {
            if let Some(p) = weak.upgrade() {
                p.place(PANEL_H);
            }
        });
        let weak = panel.weak.clone();
        panel.ui.on_link_task(move || {
            if let Some(p) = weak.upgrade() {
                p.link_placeholder_task();
            }
        });
        let weak = panel.weak.clone();
        panel.ui.on_dismiss(move || {
            if let Some(p) = weak.upgrade() {
                p.hide();
            }
        });
        Ok(panel)
    }

    pub fn set_dark(&self, dark: bool) {
        self.ui.global::<ui::Theme>().set_dark(dark);
    }

    /// Shows the panel of `kind` next to the dock. `focus` brings it to the front (user clicks
    /// only: on Windows `focus_window` simulates an Alt key press).
    pub fn open(&self, kind: PanelKind, anchor: Option<Anchor>, focus: bool) {
        if self.picker.is_open() {
            self.picker.finish(false);
        }
        self.fill(kind);
        self.anchor.set(anchor);
        self.place(PANEL_H);
        if let Err(e) = self.ui.show() {
            tracing::warn!("failed to show the {kind:?} panel: {e}");
            return;
        }
        self.ui.invoke_focus_keys();
        let weak = self.weak.clone();
        let spawned = slint::spawn_local(async move {
            let Some(panel) = weak.upgrade() else { return };
            match panel.ui.window().winit_window().await {
                Ok(window) => panel.prepare(&window, focus),
                Err(e) => tracing::warn!("panel window is not available: {e}"),
            }
        });
        if let Err(e) = spawned {
            tracing::warn!("failed to prepare the panel window: {e}");
        }
    }

    /// Sizes the window to `height` logical px and places it next to the dock.
    fn place(&self, height: f64) {
        #[expect(clippy::cast_possible_truncation, reason = "a few hundred px")]
        self.ui.set_panel_height(height as f32);
        if let Some(anchor) = self.anchor.get() {
            let origin = anchor.panel_origin_for(height);
            self.ui
                .window()
                .set_position(PhysicalPosition::new(origin.x, origin.y));
        }
    }

    /// Opens the task picker in this panel with `current` selected. `on_done` runs with the
    /// choice on "Vincular" (`None` = "Sem tarefa"); cancelling does not call it. This is the
    /// entry point the M6b timer panels use for their task line.
    pub fn pick_task(
        &self,
        current: Option<TaskId>,
        on_done: impl FnOnce(Option<TaskId>) + 'static,
    ) {
        self.place(PICKER_H);
        self.picker.open(current, on_done);
    }

    /// The placeholder's "Vincular tarefa" / "Trocar" (M6a stand-in for the timer panels).
    fn link_placeholder_task(&self) {
        let weak = self.weak.clone();
        self.pick_task(self.linked.get(), move |choice| {
            tracing::info!("task picker returned {:?}", choice.map(|t| t.0));
            if let Some(p) = weak.upgrade() {
                p.linked.set(choice);
                p.show_linked_task();
            }
        });
    }

    fn show_linked_task(&self) {
        let linked = self.linked.get();
        let (label, color) = picker::selection(&self.core.borrow(), linked);
        self.ui.set_task_linked(linked.is_some());
        self.ui.set_task_label(label.into());
        self.ui.set_task_has_project(color.is_some());
        self.ui
            .set_task_color(color.as_deref().map(palette::color).unwrap_or_default());
    }

    fn fill(&self, kind: PanelKind) {
        let content = kind.content();
        self.ui.set_panel_title(content.title.into());
        self.ui.set_panel_subtitle(content.subtitle.into());
        self.ui.set_empty_title(content.empty_title.into());
        self.ui.set_empty_text(content.empty_text.into());
        self.ui.set_action_label(content.action_label.into());
        self.ui.set_has_footer(content.has_footer);
        self.ui.set_has_task_line(content.has_task_line);
        self.show_linked_task();
        match kind.icon() {
            Ok(icon) => self.ui.set_panel_icon(icon),
            Err(e) => tracing::warn!("invalid icon for the {kind:?} panel: {e}"),
        }
    }

    fn prepare(&self, window: &winit::window::Window, focus: bool) {
        if focus {
            window.focus_window();
        }
        if self.prepared.replace(true) {
            return;
        }
        hide_from_taskbar(window);
        let weak = self.weak.clone();
        // A click outside the panel takes the focus away from it: close it, as the handoff asks.
        self.ui.window().on_winit_window_event(move |_, event| {
            if matches!(event, winit::event::WindowEvent::Focused(false)) {
                let weak = weak.clone();
                // Hiding from inside the event handler is deferred to the next loop iteration.
                slint::Timer::single_shot(Duration::ZERO, move || {
                    if let Some(p) = weak.upgrade() {
                        p.hide();
                    }
                });
            }
            EventResult::Propagate
        });
    }

    pub fn hide(&self) {
        if self.picker.is_open() {
            self.picker.finish(false);
        }
        if let Err(e) = self.ui.hide() {
            tracing::warn!("failed to hide the panel: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [PanelKind; 5] = [
        PanelKind::Alarm,
        PanelKind::Countdown,
        PanelKind::Stopwatch,
        PanelKind::Pomodoro,
        PanelKind::Overview,
    ];

    #[test]
    fn every_timer_option_maps_to_a_panel() {
        assert_eq!(PanelKind::from_id("alarme"), Some(PanelKind::Alarm));
        assert_eq!(
            PanelKind::from_id("temporizador"),
            Some(PanelKind::Countdown)
        );
        assert_eq!(PanelKind::from_id("cronometro"), Some(PanelKind::Stopwatch));
        assert_eq!(PanelKind::from_id("pomodoro"), Some(PanelKind::Pomodoro));
        assert_eq!(PanelKind::from_id("visualizar"), Some(PanelKind::Overview));
    }

    #[test]
    fn manage_is_not_a_panel() {
        assert_eq!(PanelKind::from_id("gerenciar"), None);
        assert_eq!(PanelKind::from_id(""), None);
    }

    #[test]
    fn titles_are_the_option_labels() {
        let titles: Vec<_> = ALL.iter().map(|k| k.content().title).collect();
        assert_eq!(
            titles,
            ["Alarme", "Temporizador", "Cronômetro", "Pomodoro", "Ativos"]
        );
    }

    #[test]
    fn overview_shows_the_empty_state_of_screen_50() {
        let c = PanelKind::Overview.content();
        assert_eq!(c.empty_title, "Nada rodando agora");
        assert_eq!(c.action_label, "Abrir menu");
        assert!(!c.has_footer);
    }

    #[test]
    fn every_panel_icon_is_valid_svg() {
        for kind in ALL {
            let icon = kind.icon().unwrap();
            assert_eq!(icon.size().width, 24, "{kind:?}");
        }
    }
}
