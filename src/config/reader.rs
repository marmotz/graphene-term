//! Reading of the configuration tables into a [], reporting invalid and unknown keys.

use super::*;

pub(super) struct Reader<'a> {
    pub(super) text: &'a str,
    pub(super) file: &'a Path,
    pub(super) errors: Vec<ConfigError>,
    pub(super) unknown_keys: Vec<UnknownConfigKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnknownConfigKey {
    pub section: Option<String>,
    pub key: String,
    pub path: String,
    pub whole_section: bool,
}

impl Reader<'_> {
    pub(super) fn unknown_key(
        &mut self,
        section: Option<&str>,
        key: &str,
        whole_section: bool,
        span: Option<Range<usize>>,
        message: String,
    ) {
        let path = section.map_or_else(|| key.to_owned(), |section| format!("{section}.{key}"));
        let segments: Vec<&str> = section.into_iter().chain([key]).collect();
        self.unknown_keys.push(UnknownConfigKey {
            section: section.map(str::to_owned),
            key: key.to_owned(),
            path,
            whole_section,
        });
        self.error(
            span,
            Some(key_path(&segments)),
            ConfigErrorKind::UnknownKey,
            message,
        );
    }

    pub(super) fn error(
        &mut self,
        span: Option<Range<usize>>,
        key: Option<String>,
        kind: ConfigErrorKind,
        message: String,
    ) {
        let line = span.map(|span| {
            let end = span.start.min(self.text.len());
            self.text.as_bytes()[..end]
                .iter()
                .filter(|&&b| b == b'\n')
                .count()
                + 1
        });
        self.errors.push(ConfigError {
            file: self.file.to_path_buf(),
            line,
            key,
            kind,
            message,
        });
    }

    /// Deserializes `value` as `T`, validates it, then stores it in `target`; on error
    /// `target` gets `previous`.
    pub(super) fn set<T: DeserializeOwned, U: Clone>(
        &mut self,
        value: &Spanned<DeValue<'_>>,
        key: &[&str],
        path: &str,
        target: &mut U,
        previous: &U,
        check: impl FnOnce(T) -> Checked<U>,
    ) {
        let checked = match T::deserialize(ValueDeserializer::from(value.clone())) {
            Ok(raw) => check(raw),
            Err(error) => Checked::Invalid(error.message().trim_end().to_owned()),
        };
        match checked {
            Checked::Valid(v) => *target = v,
            Checked::Clamped(v, message) => {
                self.error(
                    Some(value.span()),
                    Some(key_path(key)),
                    ConfigErrorKind::InvalidValue,
                    format!("`{path}`: {message}"),
                );
                *target = v;
            }
            Checked::Invalid(message) => {
                self.error(
                    Some(value.span()),
                    Some(key_path(key)),
                    ConfigErrorKind::InvalidValue,
                    format!("invalid value for `{path}`: {message}"),
                );
                *target = previous.clone();
            }
        }
    }

    pub(super) fn read_section_key(
        &mut self,
        section: &str,
        key: &Spanned<std::borrow::Cow<'_, str>>,
        value: &Spanned<DeValue<'_>>,
        config: &mut Config,
        previous: &Config,
    ) {
        let name = key.get_ref().as_ref();
        let path = format!("{section}.{name}");
        let path = path.as_str();
        let segments = [section, name];
        let key_segments = segments.as_slice();
        match (section, name) {
            ("font", "family") => self.set(value, key_segments, path, &mut config.font.family, &previous.font.family, |family: String| {
                Checked::Valid(Some(family).filter(|f| !f.is_empty()))
            }),
            ("font", "size") => self.set(value, key_segments, path, &mut config.font.size, &previous.font.size, |size: f32| {
                if !size.is_finite() {
                    Checked::Invalid("not a number".to_owned())
                } else if (MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&size) {
                    Checked::Valid(size)
                } else {
                    let clamped = size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
                    Checked::Clamped(clamped, format!("{size} is out of {MIN_FONT_SIZE}..={MAX_FONT_SIZE}, using {clamped}"))
                }
            }),
            ("font", "ligatures") => self.set(value, key_segments, path, &mut config.font.ligatures, &previous.font.ligatures, Checked::Valid),
            ("font", "features") => self.set(value, key_segments, path, &mut config.font.features, &previous.font.features, |features: String| {
                match FontFeatureList::parse(&features) {
                    Ok(features) => Checked::Valid(features),
                    Err(message) => Checked::Invalid(message),
                }
            }),
            ("window", "startup_mode") => self.set(value, key_segments, path, &mut config.window.startup_mode, &previous.window.startup_mode, Checked::Valid),
            ("window", "width") => self.set(value, key_segments, path, &mut config.window.width, &previous.window.width, positive_size),
            ("window", "height") => self.set(value, key_segments, path, &mut config.window.height, &previous.window.height, positive_size),
            ("window", "padding") => self.set(value, key_segments, path, &mut config.window.padding, &previous.window.padding, |padding: f32| {
                if padding.is_finite() && padding >= 0.0 {
                    Checked::Valid(padding)
                } else {
                    Checked::Invalid("must be a non-negative number".to_owned())
                }
            }),
            ("window", "progress") => self.set(value, key_segments, path, &mut config.window.progress, &previous.window.progress, Checked::Valid),
            ("window", "background_opacity") => self.set(value, key_segments, path, &mut config.window.background_opacity, &previous.window.background_opacity, |opacity: f32| {
                if !opacity.is_finite() {
                    Checked::Invalid("not a number".to_owned())
                } else if (0.0..=1.0).contains(&opacity) {
                    Checked::Valid(opacity)
                } else {
                    let clamped = opacity.clamp(0.0, 1.0);
                    Checked::Clamped(clamped, format!("{opacity} is out of 0.0..=1.0, using {clamped}"))
                }
            }),
            ("scrollback", "lines") => self.set(value, key_segments, path, &mut config.scrollback_lines, &previous.scrollback_lines, |lines: usize| {
                if lines <= MAX_SCROLLBACK_LINES {
                    Checked::Valid(lines)
                } else {
                    Checked::Clamped(MAX_SCROLLBACK_LINES, format!("{lines} is above the maximum, using {MAX_SCROLLBACK_LINES}"))
                }
            }),
            ("cursor", "shape") => self.set(value, key_segments, path, &mut config.cursor.shape, &previous.cursor.shape, Checked::Valid),
            ("cursor", "blinking") => self.set(value, key_segments, path, &mut config.cursor.blinking, &previous.cursor.blinking, Checked::Valid),
            ("selection", "word_separators") => self.set(value, key_segments, path, &mut config.word_separators, &previous.word_separators, Checked::Valid),
            ("tabs", "position") => self.set(value, key_segments, path, &mut config.tab_position, &previous.tab_position, Checked::Valid),
            ("theme", "name") => self.set(value, key_segments, path, &mut config.theme.name, &previous.theme.name, non_empty),
            ("theme", "light") => self.set(value, key_segments, path, &mut config.theme.light, &previous.theme.light, |name: String| non_empty(name).map(Some)),
            ("theme", "dark") => self.set(value, key_segments, path, &mut config.theme.dark, &previous.theme.dark, |name: String| non_empty(name).map(Some)),
            // Unknown names are dropped without an error, to reveal nothing.
            ("theme", "unlocked") => self.set(value, key_segments, path, &mut config.theme.unlocked, &previous.theme.unlocked, |names: Vec<String>| {
                Checked::Valid(names.into_iter().filter(|name| crate::theme::is_hidden(name)).collect())
            }),
            ("updates", "check") => self.set(value, key_segments, path, &mut config.updates.check, &previous.updates.check, Checked::Valid),
            ("updates", "interval") => self.set(value, key_segments, path, &mut config.updates.interval, &previous.updates.interval, |interval: String| {
                match UpdateInterval::parse(&interval) {
                    Some(interval) => Checked::Valid(interval),
                    None => {
                        let names: Vec<_> = UpdateInterval::VALUES.iter().map(|(name, _)| format!("\"{name}\"")).collect();
                        Checked::Clamped(UpdateInterval::Auto, format!("invalid value \"{interval}\", expected one of {}; using \"auto\"", names.join(", ")))
                    }
                }
            }),
            ("notifications", "program") => self.set(value, key_segments, path, &mut config.notifications.program, &previous.notifications.program, Checked::Valid),
            ("notifications", "command_finished") => self.set(value, key_segments, path, &mut config.notifications.command_finished, &previous.notifications.command_finished, Checked::Valid),
            ("notifications", "command_threshold") => self.set(value, key_segments, path, &mut config.notifications.command_threshold, &previous.notifications.command_threshold, |seconds: u64| {
                if seconds >= 1 {
                    Checked::Valid(seconds)
                } else {
                    Checked::Invalid("must be at least 1".to_owned())
                }
            }),
            ("notifications", "bell") => self.set(value, key_segments, path, &mut config.notifications.bell, &previous.notifications.bell, Checked::Valid),
            ("quick_terminal", "hotkey") => self.set(value, key_segments, path, &mut config.quick_terminal.hotkey, &previous.quick_terminal.hotkey, |hotkey: String| {
                if hotkey.is_empty() {
                    return Checked::Valid(hotkey);
                }
                match keybindings::Shortcut::parse(&hotkey) {
                    Ok(_) => Checked::Valid(hotkey),
                    Err(message) => Checked::Invalid(message),
                }
            }),
            ("quick_terminal", "edge") => self.set(value, key_segments, path, &mut config.quick_terminal.edge, &previous.quick_terminal.edge, Checked::Valid),
            ("quick_terminal", "size") => self.set(value, key_segments, path, &mut config.quick_terminal.size, &previous.quick_terminal.size, |size: u32| {
                if (MIN_QUICK_TERMINAL_SIZE..=MAX_QUICK_TERMINAL_SIZE).contains(&size) {
                    Checked::Valid(size)
                } else {
                    let clamped = size.clamp(MIN_QUICK_TERMINAL_SIZE, MAX_QUICK_TERMINAL_SIZE);
                    Checked::Clamped(clamped, format!("{size} is out of {MIN_QUICK_TERMINAL_SIZE}..={MAX_QUICK_TERMINAL_SIZE}, using {clamped}"))
                }
            }),
            ("quick_terminal", "screen") => self.set(value, key_segments, path, &mut config.quick_terminal.screen, &previous.quick_terminal.screen, |screen: String| {
                match QuickTerminalScreen::parse(&screen) {
                    Ok(screen) => Checked::Valid(screen),
                    Err(message) => Checked::Invalid(message),
                }
            }),
            ("quick_terminal", "animation_ms") => self.set(value, key_segments, path, &mut config.quick_terminal.animation_ms, &previous.quick_terminal.animation_ms, |ms: u32| {
                if ms <= MAX_QUICK_TERMINAL_ANIMATION_MS {
                    Checked::Valid(ms)
                } else {
                    Checked::Clamped(MAX_QUICK_TERMINAL_ANIMATION_MS, format!("{ms} is above the maximum, using {MAX_QUICK_TERMINAL_ANIMATION_MS}"))
                }
            }),
            ("quick_terminal", "hide_on_focus_loss") => self.set(value, key_segments, path, &mut config.quick_terminal.hide_on_focus_loss, &previous.quick_terminal.hide_on_focus_loss, Checked::Valid),
            ("keybindings", keys) => {
                let previous_action = previous.keybindings.get(keys).cloned();
                let mut action = previous_action.clone();
                self.set(value, key_segments, path, &mut action, &previous_action, |action: String| {
                    match keybindings::parse_binding(keys, &action) {
                        Ok(_) => Checked::Valid(Some(action)),
                        Err(message) => Checked::Invalid(message),
                    }
                });
                if let Some(action) = action {
                    config.keybindings.insert(keys.to_owned(), action);
                }
            }
            _ => self.unknown_key(
                Some(section),
                name,
                matches!(value.get_ref(), DeValue::Table(_)),
                Some(key.span()),
                format!("unknown key `{path}`"),
            ),
        }
    }
}

/// Result of validating a value read from the file.
pub(super) enum Checked<T> {
    Valid(T),
    /// Out of range: brought back into range, with an error.
    Clamped(T, String),
    /// Rejected: the previous value stays, with an error.
    Invalid(String),
}

impl<T> Checked<T> {
    pub(super) fn map<U>(self, f: impl FnOnce(T) -> U) -> Checked<U> {
        match self {
            Checked::Valid(v) => Checked::Valid(f(v)),
            Checked::Clamped(v, message) => Checked::Clamped(f(v), message),
            Checked::Invalid(message) => Checked::Invalid(message),
        }
    }
}

pub(super) fn positive_size(size: u32) -> Checked<u32> {
    if size > 0 {
        Checked::Valid(size)
    } else {
        Checked::Invalid("must be greater than 0".to_owned())
    }
}

pub(super) fn non_empty(name: String) -> Checked<String> {
    if name.is_empty() {
        Checked::Invalid("must not be empty".to_owned())
    } else {
        Checked::Valid(name)
    }
}
