//! Close confirmation dialog (pure, no GPU access): what a close request targets, the
//! programs it would kill, the text shown and where the box and its buttons sit.
//!
//! A close asks for confirmation only while a targeted pane runs a program other than
//! its shell in the foreground; otherwise it happens at once. A shell exit never asks.

use winit::keyboard::{Key, NamedKey};

use super::tab_bar::UiMetrics;
use crate::i18n::{self, CloseTarget, Lang, Msg};
use crate::layout::Rect;
use crate::terminal::PaneId;
use crate::workspace::Workspace;

/// Space between the box edges and its content, in logical pixels.
const PADDING: f32 = 16.0;
/// Space between the lines of the box, in logical pixels.
const GAP: f32 = 8.0;
/// Space around a button label, in logical pixels.
const BUTTON_PADDING: f32 = 12.0;
/// Space above and below a button label, in logical pixels.
const BUTTON_VERTICAL_PADDING: f32 = 6.0;
/// Narrowest box, in logical pixels.
const MIN_WIDTH: f32 = 320.0;
/// Space kept between the box and the window edges, in logical pixels.
const MARGIN: f32 = 16.0;

/// What a close request closes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Pane(PaneId),
    /// A tab, by its panes when the request was made: a pane closing meanwhile does
    /// not make the request close another tab.
    Tab(Vec<PaneId>),
    Window,
}

impl Target {
    /// The tab `index` of `workspace`, `None` when there is no such tab.
    pub fn tab<P>(workspace: &Workspace<P>, index: usize) -> Option<Self> {
        let tab = workspace.tabs().get(index)?;
        Some(Self::Tab(tab.tree.leaves()))
    }

    /// The panes of `workspace` closed by this target, in tab then tree order; panes
    /// already closed are left out.
    pub fn panes<P>(&self, workspace: &Workspace<P>) -> Vec<PaneId> {
        match self {
            Self::Pane(id) => workspace.tab_of(*id).map(|_| *id).into_iter().collect(),
            Self::Tab(ids) => ids
                .iter()
                .copied()
                .filter(|&id| workspace.tab_of(id).is_some())
                .collect(),
            Self::Window => workspace
                .tabs()
                .iter()
                .flat_map(|tab| tab.tree.leaves())
                .collect(),
        }
    }

    fn kind(&self) -> CloseTarget {
        match self {
            Self::Pane(_) => CloseTarget::Pane,
            Self::Tab(_) => CloseTarget::Tab,
            Self::Window => CloseTarget::Window,
        }
    }
}

/// A close waiting for the user's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmClose {
    pub target: Target,
    /// Foreground programs of the targeted panes, at least one.
    pub programs: Vec<String>,
}

impl ConfirmClose {
    /// The confirmation for closing `target`, given the foreground program of each of
    /// its panes (`None` for a pane whose shell is in the foreground). `None` when no
    /// pane runs a program: the close happens at once.
    pub fn request(
        target: Target,
        programs: impl IntoIterator<Item = Option<String>>,
    ) -> Option<Self> {
        let programs: Vec<String> = programs.into_iter().flatten().collect();
        (!programs.is_empty()).then_some(Self { target, programs })
    }

    /// The question, naming what closes.
    pub fn title(&self, lang: Lang) -> String {
        i18n::text(lang, &Msg::ConfirmCloseTitle(self.target.kind())).into_owned()
    }

    /// The programs still running.
    pub fn message(&self, lang: Lang) -> String {
        i18n::text(lang, &Msg::ProgramsRunning(self.programs.clone())).into_owned()
    }
}

/// The user's answer to a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Close,
    Cancel,
}

/// The answer given by `key`: `Enter` closes, `Escape` cancels; `None` for a key the
/// dialog ignores.
pub fn answer_for(key: &Key) -> Option<Answer> {
    match key {
        Key::Named(NamedKey::Enter) => Some(Answer::Close),
        Key::Named(NamedKey::Escape) => Some(Answer::Cancel),
        _ => None,
    }
}

/// A button of the dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct Button {
    pub rect: Rect,
    pub label: String,
    /// Left edge of the label, centered in the button.
    pub label_x: f32,
    pub answer: Answer,
    /// The answer `Enter` gives, drawn highlighted.
    pub default: bool,
}

/// The dialog placed in a window, in physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct DialogLayout {
    pub rect: Rect,
    pub title: String,
    pub message: String,
    /// Left edge of the title and message lines.
    pub text_x: f32,
    pub title_y: f32,
    pub message_y: f32,
    /// Width the title and message are truncated to.
    pub text_width: f32,
    /// Top of the button labels.
    pub label_y: f32,
    /// Cancel, then Close at the right edge.
    pub buttons: Vec<Button>,
}

/// What the mouse is over in the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogHit {
    Button(Answer),
    Box,
}

impl DialogLayout {
    /// Lays `dialog` out in the center of a `window`, wide enough for its text (up to
    /// the window width), its texts measured by `measure` in pixels.
    pub fn new(
        dialog: &ConfirmClose,
        lang: Lang,
        window: Rect,
        metrics: UiMetrics,
        mut measure: impl FnMut(&str) -> f32,
    ) -> Self {
        let scale = metrics.scale;
        let line = metrics.line_height;
        let (padding, gap) = (PADDING * scale, GAP * scale);
        let title = dialog.title(lang);
        let message = dialog.message(lang);
        let labels = [
            (Answer::Cancel, i18n::text(lang, &Msg::Cancel).into_owned()),
            (Answer::Close, i18n::text(lang, &Msg::Close).into_owned()),
        ];
        let button_width = labels
            .iter()
            .map(|(_, label)| measure(label))
            .fold(0.0, f32::max)
            + 2.0 * BUTTON_PADDING * scale;
        let button_height = (line + 2.0 * BUTTON_VERTICAL_PADDING * scale).round();
        let buttons_width = 2.0 * button_width + gap;

        let content = measure(&title).max(measure(&message)).max(buttons_width);
        let widest = (window.width - 2.0 * MARGIN * scale).max(0.0);
        let width = (content + 2.0 * padding)
            .max(MIN_WIDTH * scale)
            .min(widest)
            .round();
        let height = (2.0 * padding + 2.0 * line + 3.0 * gap + button_height).round();
        let rect = Rect {
            x: (window.x + (window.width - width) / 2.0).round(),
            y: (window.y + (window.height - height) / 2.0).round(),
            width,
            height,
        };

        let title_y = rect.y + padding;
        let message_y = (title_y + line + gap).round();
        let button_y = rect.y + rect.height - padding - button_height;
        let mut right = rect.x + rect.width - padding;
        let mut buttons: Vec<Button> = labels
            .into_iter()
            .rev()
            .map(|(answer, label)| {
                let x = (right - button_width).round();
                right = x - gap;
                let label_x = (x + (button_width - measure(&label)) / 2.0).round();
                Button {
                    rect: Rect {
                        x,
                        y: button_y,
                        width: button_width.round(),
                        height: button_height,
                    },
                    label,
                    label_x,
                    answer,
                    default: answer == Answer::Close,
                }
            })
            .collect();
        buttons.reverse();
        Self {
            rect,
            title,
            message,
            text_x: rect.x + padding,
            title_y,
            message_y,
            text_width: (rect.width - 2.0 * padding).max(0.0),
            label_y: (button_y + (button_height - line) / 2.0).round(),
            buttons,
        }
    }

    /// What is under `(x, y)`; `None` outside the box.
    pub fn hit(&self, x: f32, y: f32) -> Option<DialogHit> {
        if !self.rect.contains(x, y) {
            return None;
        }
        let button = self.buttons.iter().find(|b| b.rect.contains(x, y));
        Some(button.map_or(DialogHit::Box, |b| DialogHit::Button(b.answer)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character 10 px wide.
    fn width(text: &str) -> f32 {
        text.chars().count() as f32 * 10.0
    }

    const METRICS: UiMetrics = UiMetrics {
        line_height: 20.0,
        scale: 1.0,
    };

    fn window(width: f32, height: f32) -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        }
    }

    fn confirm(target: Target, programs: &[&str]) -> ConfirmClose {
        ConfirmClose {
            target,
            programs: programs.iter().map(|&p| p.to_owned()).collect(),
        }
    }

    #[test]
    fn messages_for_one_program() {
        let dialog = confirm(Target::Pane(PaneId(1)), &["vim"]);
        assert_eq!(dialog.title(Lang::En), "Close this pane?");
        assert_eq!(dialog.message(Lang::En), "vim is still running");
        assert_eq!(dialog.title(Lang::Fr), "Fermer ce panneau ?");
        assert_eq!(
            dialog.message(Lang::Fr),
            "vim est toujours en cours d'exécution"
        );
    }

    #[test]
    fn messages_for_several_programs() {
        let dialog = confirm(Target::Window, &["vim", "cargo"]);
        assert_eq!(dialog.title(Lang::En), "Close the window?");
        assert_eq!(
            dialog.message(Lang::En),
            "2 programs are still running: vim, cargo"
        );
        assert_eq!(dialog.title(Lang::Fr), "Fermer la fenêtre ?");
        assert_eq!(
            dialog.message(Lang::Fr),
            "2 programmes sont toujours en cours d'exécution : vim, cargo"
        );
        let tab = confirm(Target::Tab(vec![PaneId(1)]), &["vim", "cargo"]);
        assert_eq!(tab.title(Lang::En), "Close this tab?");
        assert_eq!(tab.title(Lang::Fr), "Fermer cet onglet ?");
    }

    #[test]
    fn a_close_without_program_does_not_ask() {
        assert_eq!(ConfirmClose::request(Target::Window, [None, None]), None);
        assert_eq!(ConfirmClose::request(Target::Pane(PaneId(0)), []), None);
    }

    #[test]
    fn a_close_asks_with_the_running_programs_in_order() {
        let request = ConfirmClose::request(
            Target::Window,
            [Some("vim".into()), None, Some("cargo".into())],
        );
        assert_eq!(request, Some(confirm(Target::Window, &["vim", "cargo"])));
    }

    /// Two tabs: panes 0 and 2 (split) in the first, pane 1 in the second.
    fn workspace() -> Workspace<()> {
        use crate::layout::{CellSize, Rect};
        use crate::workspace::tree::{Axis, Geometry};
        let mut workspace = Workspace::new();
        let first = workspace.allocate_id();
        workspace.new_tab(first, ());
        let second = workspace.allocate_id();
        workspace.new_tab(second, ());
        workspace.select(0);
        let geometry = Geometry {
            area: Rect::from_size(1000, 1000),
            cell: CellSize {
                width: 10.0,
                height: 20.0,
            },
            divider: 1.0,
        };
        workspace
            .split(Axis::Horizontal, geometry, |_| Some(()))
            .expect("split accepted");
        workspace
    }

    #[test]
    fn a_pane_target_is_that_pane_while_it_is_open() {
        let mut workspace = workspace();
        let target = Target::Pane(PaneId(2));
        assert_eq!(target.panes(&workspace), [PaneId(2)]);
        workspace.close_pane(PaneId(2)).unwrap();
        assert!(target.panes(&workspace).is_empty());
    }

    #[test]
    fn a_tab_target_is_its_panes_at_request_time() {
        let mut workspace = workspace();
        let target = Target::tab(&workspace, 0).unwrap();
        assert_eq!(target, Target::Tab(vec![PaneId(0), PaneId(2)]));
        assert_eq!(Target::tab(&workspace, 2), None);
        workspace.close_pane(PaneId(0)).unwrap();
        assert_eq!(target.panes(&workspace), [PaneId(2)]);
    }

    #[test]
    fn the_window_target_is_every_pane() {
        let workspace = workspace();
        assert_eq!(
            Target::Window.panes(&workspace),
            [PaneId(0), PaneId(2), PaneId(1)]
        );
    }

    #[test]
    fn enter_closes_and_escape_cancels() {
        assert_eq!(
            answer_for(&Key::Named(NamedKey::Enter)),
            Some(Answer::Close)
        );
        assert_eq!(
            answer_for(&Key::Named(NamedKey::Escape)),
            Some(Answer::Cancel)
        );
        assert_eq!(answer_for(&Key::Named(NamedKey::Tab)), None);
        assert_eq!(answer_for(&Key::Character("y".into())), None);
    }

    #[test]
    fn the_box_is_centered_with_cancel_then_close_on_the_right() {
        let dialog = confirm(Target::Pane(PaneId(0)), &["vim"]);
        let layout = DialogLayout::new(&dialog, Lang::En, window(1000.0, 600.0), METRICS, width);
        assert_eq!(layout.rect.width, MIN_WIDTH);
        assert_eq!(layout.rect.x, 340.0);
        assert_eq!(
            layout.rect.y,
            ((600.0 - layout.rect.height) / 2.0_f32).round()
        );
        let [cancel, close] = layout.buttons.as_slice() else {
            panic!("two buttons");
        };
        assert_eq!(
            (cancel.answer, cancel.label.as_str()),
            (Answer::Cancel, "Cancel")
        );
        assert_eq!(
            (close.answer, close.label.as_str()),
            (Answer::Close, "Close")
        );
        assert!(close.default && !cancel.default);
        assert_eq!(close.rect.x + close.rect.width, 340.0 + MIN_WIDTH - PADDING);
        assert!(cancel.rect.x + cancel.rect.width < close.rect.x);
        // Labels are centered: "Cancel" is the widest, "Close" sits 5 px in.
        assert_eq!(cancel.label_x, cancel.rect.x + BUTTON_PADDING);
        assert_eq!(close.label_x, close.rect.x + BUTTON_PADDING + 5.0);
        assert!(
            layout
                .rect
                .contains(close.rect.x, close.rect.y + close.rect.height - 1.0)
        );
    }

    #[test]
    fn the_box_grows_with_its_text_up_to_the_window() {
        let programs: Vec<String> = (0..10).map(|i| format!("program{i}")).collect();
        let names: Vec<&str> = programs.iter().map(String::as_str).collect();
        let dialog = confirm(Target::Window, &names);
        let wide = DialogLayout::new(&dialog, Lang::En, window(2000.0, 600.0), METRICS, width);
        let message = width(&dialog.message(Lang::En));
        assert_eq!(wide.rect.width, message + 2.0 * PADDING);
        assert_eq!(wide.text_width, message);
        let narrow = DialogLayout::new(&dialog, Lang::En, window(500.0, 600.0), METRICS, width);
        assert_eq!(narrow.rect.width, 500.0 - 2.0 * MARGIN);
        assert_eq!(narrow.rect.x, MARGIN);
    }

    #[test]
    fn hits_find_the_buttons_and_the_box() {
        let dialog = confirm(Target::Pane(PaneId(0)), &["vim"]);
        let layout = DialogLayout::new(&dialog, Lang::En, window(1000.0, 600.0), METRICS, width);
        let center = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);
        let (x, y) = center(layout.buttons[0].rect);
        assert_eq!(layout.hit(x, y), Some(DialogHit::Button(Answer::Cancel)));
        let (x, y) = center(layout.buttons[1].rect);
        assert_eq!(layout.hit(x, y), Some(DialogHit::Button(Answer::Close)));
        assert_eq!(
            layout.hit(layout.text_x, layout.title_y),
            Some(DialogHit::Box)
        );
        assert_eq!(layout.hit(0.0, 0.0), None);
    }
}
