//! Search bar of a pane (pure, no GPU access): a compact strip floating at the top
//! right of the pane content, over its rows, holding the search field, the match
//! counter, the case, regex and whole word toggles, the zone and status selectors, the
//! buttons going to the older and the more recent match, and the close button.
//!
//! A narrow pane drops the selectors first, then the toggles, so the field keeps room
//! for its text; the counter and the buttons always stay.

use super::tab_bar::UiMetrics;
use crate::i18n::UiText;
use crate::layout::Rect;
use crate::terminal::PaneId;

/// Widest bar, in logical pixels.
pub const MAX_WIDTH: f32 = 420.0;
/// Gap between the bar and the edges of the pane content, in logical pixels.
pub const MARGIN: f32 = 6.0;
/// Room above and below a line of text in the bar, in logical pixels.
pub const ROW_PADDING: f32 = 3.0;
/// Room on each side of a label in its button or field, in logical pixels.
pub const TEXT_PADDING: f32 = 5.0;
/// Narrowest field kept before the toggles and selectors are dropped, in logical pixels.
pub const MIN_FIELD: f32 = 90.0;

/// Labels of the toggles and buttons, the same in every language.
pub const CASE_LABEL: &str = "Aa";
pub const REGEX_LABEL: &str = ".*";
pub const WORD_LABEL: &str = "ab";
pub const OLDER_LABEL: &str = "↑";
pub const NEWER_LABEL: &str = "↓";
pub const CLOSE_LABEL: &str = "×";

/// What the mouse is over in a search bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchBarHit {
    Field,
    CaseSensitive,
    Regex,
    WholeWord,
    Zone,
    Status,
    /// The button going to the older match, like `Enter`.
    Older,
    /// The button going to the more recent match, like `Shift+Enter`.
    Newer,
    Close,
    /// The rest of the bar: the counter, the gaps.
    Bar,
}

/// What the bar shows, read from the pane's search state.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchBarView {
    pub text: String,
    /// Byte offset of the caret in `text`.
    pub caret: usize,
    /// The whole text is selected.
    pub selected: bool,
    /// The bar has the keyboard: its caret is shown.
    pub focused: bool,
    /// Shown in the empty field.
    pub placeholder: UiText,
    /// Match counter, or the invalid regex message.
    pub counter: UiText,
    /// The query is not a valid regex: the counter and the border take the error color.
    pub invalid: bool,
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
    /// Labels of the zone and status selectors.
    pub zone: UiText,
    pub status: UiText,
    /// The selectors filter something: the history is split into blocks. Greyed out
    /// otherwise.
    pub filters: bool,
    /// A selector is not at its default: it is highlighted like an active toggle.
    pub zone_active: bool,
    pub status_active: bool,
}

/// A toggle, a selector or a button of the bar.
#[derive(Debug, Clone, PartialEq)]
pub struct BarButton {
    pub hit: SearchBarHit,
    pub rect: Rect,
    pub label: String,
    /// Left edge of the label.
    pub label_x: f32,
    /// Highlighted: an active toggle or a selector off its default.
    pub active: bool,
    /// Greyed out and inert: a selector without blocks to filter.
    pub enabled: bool,
}

/// The search bar of one pane, in physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchBarLayout {
    pub pane: PaneId,
    pub rect: Rect,
    /// The text field.
    pub field: Rect,
    /// Text shown in the field: its end when it does not fit before the caret, or the
    /// placeholder.
    pub text: String,
    pub placeholder: bool,
    pub text_x: f32,
    /// Top of every line of text in the bar.
    pub text_y: f32,
    pub text_width: f32,
    /// The text cursor, while the bar has the keyboard.
    pub caret: Option<Rect>,
    /// Highlight behind a selected text.
    pub selection: Option<Rect>,
    pub counter: String,
    pub counter_x: f32,
    pub invalid: bool,
    pub focused: bool,
    pub buttons: Vec<BarButton>,
}

impl SearchBarLayout {
    /// The bar of `pane` showing `view`, at the top right of `content`, the pane's
    /// content under its title bar. Texts are measured by `measure` in pixels.
    pub fn new(
        pane: PaneId,
        content: Rect,
        view: &SearchBarView,
        metrics: UiMetrics,
        mut measure: impl FnMut(&str) -> f32,
    ) -> Self {
        let scale = metrics.scale;
        let line = metrics.line_height;
        let (margin, row_padding, text_padding) =
            (MARGIN * scale, ROW_PADDING * scale, TEXT_PADDING * scale);
        let width = (MAX_WIDTH * scale)
            .min(content.width - 2.0 * margin)
            .max(0.0)
            .round();
        let height = (line + 2.0 * row_padding).round();
        let rect = Rect {
            x: (content.x + content.width - margin - width).round(),
            y: (content.y + margin).round(),
            width,
            height,
        };
        let text_y = rect.y + row_padding;

        let mut button = |hit, label: &str, active, enabled| {
            let label_width = measure(label);
            let width = (label_width + 2.0 * text_padding).max(line).round();
            (hit, label.to_owned(), active, enabled, width, label_width)
        };
        let fixed = [
            button(SearchBarHit::Older, OLDER_LABEL, false, true),
            button(SearchBarHit::Newer, NEWER_LABEL, false, true),
            button(SearchBarHit::Close, CLOSE_LABEL, false, true),
        ];
        let toggles = [
            button(
                SearchBarHit::CaseSensitive,
                CASE_LABEL,
                view.case_sensitive,
                true,
            ),
            button(SearchBarHit::Regex, REGEX_LABEL, view.regex, true),
            button(SearchBarHit::WholeWord, WORD_LABEL, view.whole_word, true),
        ];
        let selectors = [
            button(
                SearchBarHit::Zone,
                &view.zone,
                view.zone_active && view.filters,
                view.filters,
            ),
            button(
                SearchBarHit::Status,
                &view.status,
                view.status_active && view.filters,
                view.filters,
            ),
        ];
        let counter_width = if view.counter.is_empty() {
            0.0
        } else {
            (measure(&view.counter) + 2.0 * text_padding).round()
        };
        let sum = |group: &[(SearchBarHit, String, bool, bool, f32, f32)]| -> f32 {
            group.iter().map(|b| b.4).sum()
        };
        let min_field = MIN_FIELD * scale;
        let with_fixed = sum(&fixed) + counter_width;
        let mut shown: Vec<_> = Vec::new();
        let with_toggles = with_fixed + sum(&toggles);
        let with_all = with_toggles + sum(&selectors);
        if width - with_all >= min_field {
            shown.extend(toggles);
            shown.extend(selectors);
        } else if width - with_toggles >= min_field {
            shown.extend(toggles);
        }
        shown.extend(fixed);

        let used = counter_width + sum(&shown);
        let field = Rect {
            x: rect.x,
            y: rect.y,
            width: (width - used).max(0.0),
            height,
        };
        let counter_x = field.x + field.width + text_padding;
        let mut x = field.x + field.width + counter_width;
        let buttons = shown
            .into_iter()
            .map(|(hit, label, active, enabled, button_width, label_width)| {
                let rect = Rect {
                    x,
                    y: rect.y,
                    width: button_width,
                    height,
                };
                x += button_width;
                BarButton {
                    hit,
                    label_x: (rect.x + (button_width - label_width) / 2.0).round(),
                    rect,
                    label,
                    active,
                    enabled,
                }
            })
            .collect();

        let text_x = field.x + text_padding;
        let text_width = (field.width - 2.0 * text_padding).max(0.0);
        let placeholder = view.text.is_empty();
        let (text, caret_offset) = if placeholder {
            (view.placeholder.to_string(), 0.0)
        } else {
            let caret = view.caret.min(view.text.len());
            // Drop characters from the start until the text before the caret fits.
            let mut start = 0;
            while start < caret && measure(&view.text[start..caret]) > text_width {
                start += view.text[start..].chars().next().map_or(1, char::len_utf8);
            }
            let shown = view.text[start..].to_owned();
            let offset = measure(&view.text[start..caret]).min(text_width);
            (shown, offset)
        };
        let caret = view.focused.then(|| Rect {
            x: (text_x + caret_offset).round(),
            y: text_y,
            width: scale.round().max(1.0),
            height: line,
        });
        let selection = (view.selected && !placeholder).then(|| Rect {
            x: text_x,
            y: text_y,
            width: measure(&text).min(text_width).round(),
            height: line,
        });
        Self {
            pane,
            rect,
            field,
            text,
            placeholder,
            text_x,
            text_y,
            text_width,
            caret,
            selection,
            counter: view.counter.to_string(),
            counter_x,
            invalid: view.invalid,
            focused: view.focused,
            buttons,
        }
    }

    /// What is under the pixel `(x, y)`: a button, the field, the rest of the bar. A
    /// greyed out selector is the bar.
    pub fn hit(&self, x: f32, y: f32) -> Option<SearchBarHit> {
        if !self.rect.contains(x, y) {
            return None;
        }
        if self.field.contains(x, y) {
            return Some(SearchBarHit::Field);
        }
        let hit = self
            .buttons
            .iter()
            .find(|button| button.rect.contains(x, y))
            .filter(|button| button.enabled)
            .map_or(SearchBarHit::Bar, |button| button.hit);
        Some(hit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const METRICS: UiMetrics = UiMetrics {
        line_height: 18.0,
        scale: 1.0,
    };

    /// Ten pixels per character.
    fn measure(text: &str) -> f32 {
        10.0 * text.chars().count() as f32
    }

    fn content(width: f32) -> Rect {
        Rect {
            x: 100.0,
            y: 80.0,
            width,
            height: 300.0,
        }
    }

    fn view() -> SearchBarView {
        SearchBarView {
            text: "error".to_owned(),
            caret: 5,
            selected: false,
            focused: true,
            placeholder: UiText::user("Search"),
            counter: UiText::user("2/7"),
            invalid: false,
            case_sensitive: true,
            regex: false,
            whole_word: false,
            zone: UiText::user("All"),
            status: UiText::user("Any"),
            filters: true,
            zone_active: false,
            status_active: false,
        }
    }

    fn hits(layout: &SearchBarLayout) -> Vec<SearchBarHit> {
        layout.buttons.iter().map(|button| button.hit).collect()
    }

    fn bar(width: f32, view: &SearchBarView) -> SearchBarLayout {
        SearchBarLayout::new(PaneId(1), content(width), view, METRICS, measure)
    }

    #[test]
    fn a_wide_bar_sits_at_the_top_right_of_the_content_with_every_control() {
        let layout = bar(1000.0, &view());
        assert_eq!(
            layout.rect,
            Rect {
                x: 100.0 + 1000.0 - 6.0 - 420.0,
                y: 86.0,
                width: 420.0,
                height: 24.0,
            }
        );
        assert_eq!(
            hits(&layout),
            [
                SearchBarHit::CaseSensitive,
                SearchBarHit::Regex,
                SearchBarHit::WholeWord,
                SearchBarHit::Zone,
                SearchBarHit::Status,
                SearchBarHit::Older,
                SearchBarHit::Newer,
                SearchBarHit::Close,
            ]
        );
        // The controls fill the bar after the field and the counter, without overlap.
        let last = layout.buttons.last().expect("buttons");
        assert_eq!(
            last.rect.x + last.rect.width,
            layout.rect.x + layout.rect.width
        );
        for pair in layout.buttons.windows(2) {
            assert_eq!(pair[0].rect.x + pair[0].rect.width, pair[1].rect.x);
        }
        assert_eq!(layout.field.x, layout.rect.x);
        assert!(layout.counter_x > layout.field.x + layout.field.width);
        assert!(layout.buttons[0].active, "case toggle is on");
        assert!(!layout.buttons[1].active);
        assert_eq!(layout.text, "error");
        assert_eq!(
            layout.caret.map(|caret| caret.x),
            Some(layout.text_x + 50.0)
        );
    }

    #[test]
    fn a_narrow_bar_drops_the_selectors_then_the_toggles() {
        // Buttons are 20 (one character), 30 or 40 wide, the counter 40.
        let fixed = 3.0 * 20.0 + 40.0;
        let toggles = 3.0 * 30.0;
        let selectors = 2.0 * 40.0;
        let width = |bar: f32| bar + 2.0 * MARGIN;

        let all = bar(width(fixed + toggles + selectors + MIN_FIELD), &view());
        assert_eq!(hits(&all).len(), 8);

        let no_selectors = bar(width(fixed + toggles + MIN_FIELD), &view());
        assert_eq!(
            hits(&no_selectors),
            [
                SearchBarHit::CaseSensitive,
                SearchBarHit::Regex,
                SearchBarHit::WholeWord,
                SearchBarHit::Older,
                SearchBarHit::Newer,
                SearchBarHit::Close,
            ]
        );
        let narrow = bar(width(fixed + MIN_FIELD), &view());
        assert_eq!(
            hits(&narrow),
            [
                SearchBarHit::Older,
                SearchBarHit::Newer,
                SearchBarHit::Close
            ]
        );
        assert_eq!(narrow.field.width, MIN_FIELD);
        // Narrower still, the field shrinks.
        let tiny = bar(width(fixed + 10.0), &view());
        assert_eq!(tiny.field.width, 10.0);
    }

    #[test]
    fn hit_finds_the_field_the_buttons_and_the_rest_of_the_bar() {
        let layout = bar(1000.0, &view());
        let center = |rect: Rect| (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        let (x, y) = center(layout.field);
        assert_eq!(layout.hit(x, y), Some(SearchBarHit::Field));
        for button in &layout.buttons {
            let (x, y) = center(button.rect);
            assert_eq!(layout.hit(x, y), Some(button.hit));
        }
        // The counter is part of the bar.
        assert_eq!(
            layout.hit(layout.counter_x + 1.0, layout.text_y),
            Some(SearchBarHit::Bar)
        );
        assert_eq!(layout.hit(layout.rect.x - 1.0, layout.rect.y), None);
        assert_eq!(
            layout.hit(layout.rect.x, layout.rect.y + layout.rect.height),
            None
        );
    }

    #[test]
    fn selectors_are_greyed_out_without_blocks() {
        let view = SearchBarView {
            filters: false,
            zone_active: true,
            ..view()
        };
        let layout = bar(1000.0, &view);
        let zone = layout
            .buttons
            .iter()
            .find(|button| button.hit == SearchBarHit::Zone)
            .expect("zone selector");
        assert!(!zone.enabled);
        assert!(!zone.active);
        let (x, y) = (zone.rect.x + 1.0, zone.rect.y + 1.0);
        assert_eq!(layout.hit(x, y), Some(SearchBarHit::Bar));
    }

    #[test]
    fn the_field_shows_the_placeholder_then_the_end_of_a_long_text() {
        let empty = SearchBarView {
            text: String::new(),
            caret: 0,
            ..view()
        };
        let layout = bar(1000.0, &empty);
        assert!(layout.placeholder);
        assert_eq!(layout.text, "Search");
        assert_eq!(layout.caret.map(|caret| caret.x), Some(layout.text_x));

        let long = "x".repeat(40);
        let view = SearchBarView {
            caret: long.len(),
            text: long,
            ..view()
        };
        let layout = bar(1000.0, &view);
        let fits = (layout.text_width / 10.0) as usize;
        assert_eq!(layout.text.len(), fits);
        let caret = layout.caret.expect("caret");
        assert!(caret.x <= layout.text_x + layout.text_width);
    }

    #[test]
    fn the_caret_shows_only_with_the_keyboard_and_a_selection_is_highlighted() {
        let unfocused = SearchBarView {
            focused: false,
            ..view()
        };
        assert_eq!(bar(1000.0, &unfocused).caret, None);
        let selected = SearchBarView {
            selected: true,
            ..view()
        };
        let layout = bar(1000.0, &selected);
        assert_eq!(layout.selection.map(|s| s.width), Some(50.0));
        assert_eq!(bar(1000.0, &view()).selection, None);
    }
}
