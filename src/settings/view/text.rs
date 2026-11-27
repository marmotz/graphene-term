//! How the Settings view writes its texts. egui's `label`, `button`, `heading` and
//! `selectable_label` take any `&str`, so `clippy.toml` forbids them: the view calls
//! the [`Texts`] methods instead, which only take a [`Shown`], built from a
//! [`UiText`]. A string literal is then refused by the compiler, not only by review.

use egui::{Color32, Response, RichText, Ui, WidgetText};

use crate::i18n::UiText;

/// A [`UiText`] with its egui style (weak, strong, small, sized, colored).
#[derive(Debug, Clone)]
pub struct Shown(RichText);

impl From<UiText> for Shown {
    fn from(text: UiText) -> Self {
        Self(RichText::new(text))
    }
}

impl From<UiText> for WidgetText {
    fn from(text: UiText) -> Self {
        RichText::new(text).into()
    }
}

/// Styles of a text, on a [`UiText`] or a [`Shown`].
pub trait Style: Into<Shown> {
    fn weak(self) -> Shown {
        Shown(self.into().0.weak())
    }

    fn strong(self) -> Shown {
        Shown(self.into().0.strong())
    }

    fn small(self) -> Shown {
        Shown(self.into().0.small())
    }

    fn size(self, size: f32) -> Shown {
        Shown(self.into().0.size(size))
    }

    fn color(self, color: Color32) -> Shown {
        Shown(self.into().0.color(color))
    }
}

impl Style for UiText {}
impl Style for Shown {}

/// The egui text widgets, taking a [`Shown`] only.
pub trait Texts {
    fn show_label(&mut self, text: impl Into<Shown>) -> Response;
    fn show_button(&mut self, text: impl Into<Shown>) -> Response;
    fn show_heading(&mut self, text: impl Into<Shown>) -> Response;
    fn show_selectable(&mut self, selected: bool, text: impl Into<Shown>) -> Response;
}

impl Texts for Ui {
    #[expect(clippy::disallowed_methods, reason = "the wrapper of `Ui::label`")]
    fn show_label(&mut self, text: impl Into<Shown>) -> Response {
        self.label(text.into().0)
    }

    #[expect(clippy::disallowed_methods, reason = "the wrapper of `Ui::button`")]
    fn show_button(&mut self, text: impl Into<Shown>) -> Response {
        self.button(text.into().0)
    }

    #[expect(clippy::disallowed_methods, reason = "the wrapper of `Ui::heading`")]
    fn show_heading(&mut self, text: impl Into<Shown>) -> Response {
        self.heading(text.into().0)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the wrapper of `Ui::selectable_label`"
    )]
    fn show_selectable(&mut self, selected: bool, text: impl Into<Shown>) -> Response {
        self.selectable_label(selected, text.into().0)
    }
}
