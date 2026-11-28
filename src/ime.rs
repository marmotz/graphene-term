//! Input method (IME) state of the terminal window: the text being composed, shown at
//! the cursor until committed, and the delivery of the committed text to the panes.
//! Pure: the session hands it the window events and the workspace.

use winit::event::Ime;
use winit::keyboard::ModifiersState;

use crate::pane::Pane;
use crate::workspace::Workspace;

/// Text being composed by the input method, shown at the cursor until committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preedit {
    pub text: String,
    /// Cursor inside `text`, in bytes; `None` hides the cursor.
    pub cursor: Option<(usize, usize)>,
}

/// The composition of the window and the input target `T` it belongs to. The system
/// input method has one composition per window: it always belongs to the current
/// target of the keyboard, and is dropped when that target changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composition<T> {
    current: Option<(Preedit, T)>,
}

impl<T> Default for Composition<T> {
    fn default() -> Self {
        Self { current: None }
    }
}

impl<T: PartialEq> Composition<T> {
    /// The text being composed, `None` outside a composition.
    pub fn preedit(&self) -> Option<&Preedit> {
        self.current.as_ref().map(|(preedit, _)| preedit)
    }

    /// Whether a composition is in progress.
    pub fn is_composing(&self) -> bool {
        self.current.is_some()
    }

    /// Applies an input method event received while `target` has the keyboard.
    /// Returns what changed: the composition to redraw, a text to deliver.
    pub fn event(&mut self, event: Ime, target: T) -> ImeEffect {
        match event {
            Ime::Enabled => ImeEffect::default(),
            Ime::Preedit(text, cursor) => {
                let next = (!text.is_empty()).then_some((Preedit { text, cursor }, target));
                let changed = self.current != next;
                self.current = next;
                ImeEffect {
                    redraw: changed,
                    commit: None,
                }
            }
            Ime::Commit(text) => ImeEffect {
                redraw: self.clear(),
                commit: (!text.is_empty()).then_some(text),
            },
            Ime::Disabled => ImeEffect {
                redraw: self.clear(),
                commit: None,
            },
        }
    }

    /// Drops the composition. Returns whether there was one.
    pub fn clear(&mut self) -> bool {
        self.current.take().is_some()
    }

    /// Drops the composition when it belongs to another target than `target`, the one
    /// that has the keyboard now. Returns whether it was dropped.
    pub fn follow(&mut self, target: &T) -> bool {
        if self
            .current
            .as_ref()
            .is_some_and(|(_, owner)| owner != target)
        {
            self.current = None;
            return true;
        }
        false
    }
}

/// What an input method event changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImeEffect {
    /// The composition shown changed.
    pub redraw: bool,
    /// Text committed by the input method, to deliver as typed text.
    pub commit: Option<String>,
}

/// A pane that receives the text committed by the input method.
pub trait TextTarget {
    /// Whether the pane is in rich mode (the input editor at the prompt).
    fn rich(&self, modifiers: ModifiersState) -> bool;
    /// Replaces the selection of the prompt's input with `typed`; `false` when there is
    /// no such selection.
    fn replace_selection(&mut self, typed: &[u8]) -> bool;
    /// Sends `bytes` as typed on the keyboard.
    fn type_bytes(&mut self, bytes: Vec<u8>);
}

impl TextTarget for Pane {
    fn rich(&self, modifiers: ModifiersState) -> bool {
        self.rich_mode(modifiers)
    }

    fn replace_selection(&mut self, typed: &[u8]) -> bool {
        Pane::replace_selection(self, typed)
    }

    fn type_bytes(&mut self, bytes: Vec<u8>) {
        Pane::type_bytes(self, bytes);
    }
}

/// Delivers `text`, committed by the input method, to every input target of the
/// workspace (the active pane, and the broadcast set): at the prompt it replaces the
/// selection of the input first, otherwise it is typed as UTF-8. It never goes through
/// the shortcuts or the key encoding: a committed text is never a command.
pub fn commit_to_panes<P: TextTarget>(
    workspace: &mut Workspace<P>,
    text: &str,
    modifiers: ModifiersState,
) {
    if text.is_empty() {
        return;
    }
    for id in workspace.input_targets() {
        let Some(pane) = workspace.pane_mut(id) else {
            continue;
        };
        let replaced = pane.rich(modifiers) && pane.replace_selection(text.as_bytes());
        if !replaced {
            pane.type_bytes(text.as_bytes().to_vec());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{CellSize, Rect};
    use crate::workspace::DIVIDER_THICKNESS;
    use crate::workspace::tree::{Axis, Geometry};

    fn preedit(text: &str, cursor: Option<(usize, usize)>) -> Ime {
        Ime::Preedit(text.to_owned(), cursor)
    }

    #[test]
    fn a_preedit_starts_and_updates_the_composition() {
        let mut composition = Composition::default();
        assert!(!composition.is_composing());

        let effect = composition.event(preedit("ni", Some((2, 2))), 1);
        assert_eq!(
            effect,
            ImeEffect {
                redraw: true,
                commit: None
            }
        );
        assert_eq!(
            composition.preedit(),
            Some(&Preedit {
                text: "ni".to_owned(),
                cursor: Some((2, 2)),
            })
        );

        let effect = composition.event(preedit("你", None), 1);
        assert!(effect.redraw);
        assert_eq!(composition.preedit().map(|p| p.text.as_str()), Some("你"));
        assert_eq!(composition.preedit().and_then(|p| p.cursor), None);
    }

    #[test]
    fn the_same_preedit_again_needs_no_redraw() {
        let mut composition = Composition::default();
        composition.event(preedit("ka", Some((2, 2))), 1);
        assert!(!composition.event(preedit("ka", Some((2, 2))), 1).redraw);
    }

    #[test]
    fn an_empty_preedit_ends_the_composition() {
        let mut composition = Composition::default();
        composition.event(preedit("ka", Some((2, 2))), 1);
        assert!(composition.event(preedit("", None), 1).redraw);
        assert!(!composition.is_composing());
        // Already empty: nothing to redraw.
        assert!(!composition.event(preedit("", None), 1).redraw);
    }

    #[test]
    fn a_commit_ends_the_composition_and_hands_the_text() {
        let mut composition = Composition::default();
        composition.event(preedit("かんじ", Some((9, 9))), 1);
        let effect = composition.event(Ime::Commit("漢字".to_owned()), 1);
        assert_eq!(
            effect,
            ImeEffect {
                redraw: true,
                commit: Some("漢字".to_owned()),
            }
        );
        assert!(!composition.is_composing());
    }

    #[test]
    fn a_commit_without_composition_still_hands_the_text() {
        let mut composition = Composition::<u8>::default();
        let effect = composition.event(Ime::Commit("é".to_owned()), 1);
        assert_eq!(
            effect,
            ImeEffect {
                redraw: false,
                commit: Some("é".to_owned()),
            }
        );
        assert_eq!(
            composition.event(Ime::Commit(String::new()), 1).commit,
            None
        );
    }

    #[test]
    fn disabling_the_input_method_drops_the_composition() {
        let mut composition = Composition::default();
        composition.event(preedit("ka", None), 1);
        assert!(composition.event(Ime::Disabled, 1).redraw);
        assert!(!composition.is_composing());
        assert_eq!(composition.event(Ime::Enabled, 1), ImeEffect::default());
    }

    #[test]
    fn the_composition_is_dropped_when_the_target_changes() {
        let mut composition = Composition::default();
        composition.event(preedit("ka", None), 1);
        assert!(!composition.follow(&1));
        assert!(composition.is_composing());
        assert!(composition.follow(&2));
        assert!(!composition.is_composing());
        assert!(!composition.follow(&3));
    }

    #[test]
    fn clearing_tells_whether_there_was_a_composition() {
        let mut composition = Composition::default();
        assert!(!composition.clear());
        composition.event(preedit("ka", None), 1);
        assert!(composition.clear());
        assert!(!composition.is_composing());
    }

    #[derive(Debug, Default)]
    struct Fake {
        rich: bool,
        selection: bool,
        typed: Vec<Vec<u8>>,
        replaced: Vec<Vec<u8>>,
    }

    impl TextTarget for Fake {
        fn rich(&self, _: ModifiersState) -> bool {
            self.rich
        }
        fn replace_selection(&mut self, typed: &[u8]) -> bool {
            if !self.selection {
                return false;
            }
            self.replaced.push(typed.to_vec());
            true
        }
        fn type_bytes(&mut self, bytes: Vec<u8>) {
            self.typed.push(bytes);
        }
    }

    fn geometry() -> Geometry {
        Geometry {
            area: Rect::from_size(800, 600),
            cell: CellSize {
                width: 10.0,
                height: 20.0,
            },
            divider: DIVIDER_THICKNESS,
            title_bar: 0.0,
        }
    }

    #[test]
    fn committed_text_is_typed_in_the_active_pane_only() {
        let mut workspace = Workspace::new();
        let first = workspace.allocate_id();
        workspace.new_tab(first, Fake::default());
        let second = workspace
            .split(Axis::Horizontal, geometry(), |_| Some(Fake::default()))
            .unwrap();

        commit_to_panes(&mut workspace, "漢字", ModifiersState::empty());

        assert_eq!(
            workspace.pane(second).unwrap().typed,
            vec!["漢字".as_bytes().to_vec()]
        );
        assert!(workspace.pane(first).unwrap().typed.is_empty());
    }

    #[test]
    fn committed_text_is_broadcast_to_every_target() {
        let mut workspace = Workspace::new();
        let first = workspace.allocate_id();
        workspace.new_tab(first, Fake::default());
        let second = workspace
            .split(Axis::Horizontal, geometry(), |_| Some(Fake::default()))
            .unwrap();
        assert!(workspace.toggle_broadcast_all());

        commit_to_panes(&mut workspace, "é", ModifiersState::empty());

        for id in [first, second] {
            assert_eq!(
                workspace.pane(id).unwrap().typed,
                vec!["é".as_bytes().to_vec()],
                "{id:?}"
            );
        }
    }

    #[test]
    fn at_the_prompt_committed_text_replaces_the_selection() {
        let mut workspace = Workspace::new();
        let id = workspace.allocate_id();
        workspace.new_tab(
            id,
            Fake {
                rich: true,
                selection: true,
                ..Fake::default()
            },
        );

        commit_to_panes(&mut workspace, "日本", ModifiersState::empty());

        let pane = workspace.pane(id).unwrap();
        assert_eq!(pane.replaced, vec!["日本".as_bytes().to_vec()]);
        assert!(pane.typed.is_empty());
    }

    #[test]
    fn at_the_prompt_without_selection_committed_text_is_typed() {
        let mut workspace = Workspace::new();
        let id = workspace.allocate_id();
        workspace.new_tab(
            id,
            Fake {
                rich: true,
                ..Fake::default()
            },
        );

        commit_to_panes(&mut workspace, "日本", ModifiersState::empty());

        let pane = workspace.pane(id).unwrap();
        assert!(pane.replaced.is_empty());
        assert_eq!(pane.typed, vec!["日本".as_bytes().to_vec()]);
    }

    #[test]
    fn outside_the_prompt_a_selection_is_not_replaced() {
        let mut workspace = Workspace::new();
        let id = workspace.allocate_id();
        workspace.new_tab(
            id,
            Fake {
                selection: true,
                ..Fake::default()
            },
        );

        commit_to_panes(&mut workspace, "a", ModifiersState::empty());

        let pane = workspace.pane(id).unwrap();
        assert!(pane.replaced.is_empty());
        assert_eq!(pane.typed, vec![b"a".to_vec()]);
    }

    #[test]
    fn an_empty_commit_types_nothing() {
        let mut workspace = Workspace::new();
        let id = workspace.allocate_id();
        workspace.new_tab(id, Fake::default());
        commit_to_panes(&mut workspace, "", ModifiersState::empty());
        assert!(workspace.pane(id).unwrap().typed.is_empty());
    }
}
