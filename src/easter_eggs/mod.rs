//! Hidden features: secret command palette entries, the ASCII drawing they open, and
//! the hidden theme.

pub mod art;
pub mod secrets;

/// The hidden theme, unlocked from the settings.
pub const HIDDEN_THEME: &str = "phosphor-crt";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hidden_theme_is_a_hidden_built_in_theme() {
        assert!(crate::theme::is_hidden(HIDDEN_THEME));
        assert!(crate::theme::built_in(HIDDEN_THEME).is_some());
    }
}
