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
    ) {
        let path = section.map_or_else(|| key.to_owned(), |section| format!("{section}.{key}"));
        let segments: Vec<&str> = section.into_iter().chain([key]).collect();
        self.unknown_keys.push(UnknownConfigKey {
            section: section.map(str::to_owned),
            key: key.to_owned(),
            path: path.clone(),
            whole_section,
        });
        self.error(
            span,
            Some(key_path(&segments)),
            ConfigErrorKind::UnknownKey,
            ConfigMsg::UnknownKey(path),
        );
    }

    pub(super) fn error(
        &mut self,
        span: Option<Range<usize>>,
        key: Option<String>,
        kind: ConfigErrorKind,
        detail: ConfigMsg,
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
            detail: Box::new(detail),
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
            Err(error) => {
                Checked::Invalid(ValueError::Detail(error.message().trim_end().to_owned()))
            }
        };
        match checked {
            Checked::Valid(v) => *target = v,
            Checked::Clamped(v, clamp) => {
                self.error(
                    Some(value.span()),
                    Some(key_path(key)),
                    ConfigErrorKind::InvalidValue,
                    clamp.at(path),
                );
                *target = v;
            }
            Checked::Invalid(reason) => {
                self.error(
                    Some(value.span()),
                    Some(key_path(key)),
                    ConfigErrorKind::InvalidValue,
                    ConfigMsg::InvalidValue {
                        path: path.to_owned(),
                        reason,
                    },
                );
                *target = previous.clone();
            }
        }
    }

    // One line per key, as a table: rustfmt would spread each over several.
    #[rustfmt::skip]
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
                    Checked::Invalid(ValueError::NotANumber)
                } else if (MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&size) {
                    Checked::Valid(size)
                } else {
                    let clamped = size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
                    Checked::Clamped(clamped, Clamp::out_of_range(number(size), number(MIN_FONT_SIZE), number(MAX_FONT_SIZE), number(clamped)))
                }
            }),
            ("font", "ligatures") => self.set(value, key_segments, path, &mut config.font.ligatures, &previous.font.ligatures, Checked::Valid),
            ("font", "features") => self.set(value, key_segments, path, &mut config.font.features, &previous.font.features, |features: String| {
                match FontFeatureList::parse(&features) {
                    Ok(features) => Checked::Valid(features),
                    Err(reason) => Checked::Invalid(reason),
                }
            }),
            ("window", "startup_mode") => self.set(value, key_segments, path, &mut config.window.startup_mode, &previous.window.startup_mode, Checked::Valid),
            ("window", "width") => self.set(value, key_segments, path, &mut config.window.width, &previous.window.width, positive_size),
            ("window", "height") => self.set(value, key_segments, path, &mut config.window.height, &previous.window.height, positive_size),
            ("window", "padding") => self.set(value, key_segments, path, &mut config.window.padding, &previous.window.padding, |padding: f32| {
                if padding.is_finite() && padding >= 0.0 {
                    Checked::Valid(padding)
                } else {
                    Checked::Invalid(ValueError::NotNegative)
                }
            }),
            ("window", "progress") => self.set(value, key_segments, path, &mut config.window.progress, &previous.window.progress, Checked::Valid),
            ("window", "restore_session") => self.set(value, key_segments, path, &mut config.window.restore_session, &previous.window.restore_session, Checked::Valid),
            ("window", "background_opacity") => self.set(value, key_segments, path, &mut config.window.background_opacity, &previous.window.background_opacity, |opacity: f32| {
                if !opacity.is_finite() {
                    Checked::Invalid(ValueError::NotANumber)
                } else if (0.0..=1.0).contains(&opacity) {
                    Checked::Valid(opacity)
                } else {
                    let clamped = opacity.clamp(0.0, 1.0);
                    Checked::Clamped(clamped, Clamp::out_of_range(number(opacity), 0.0, 1.0, number(clamped)))
                }
            }),
            ("scrollback", "lines") => self.set(value, key_segments, path, &mut config.scrollback_lines, &previous.scrollback_lines, |lines: usize| {
                if lines <= MAX_SCROLLBACK_LINES {
                    Checked::Valid(lines)
                } else {
                    Checked::Clamped(MAX_SCROLLBACK_LINES, Clamp::AboveMaximum { value: lines as f64, max: MAX_SCROLLBACK_LINES as f64 })
                }
            }),
            ("cursor", "shape") => self.set(value, key_segments, path, &mut config.cursor.shape, &previous.cursor.shape, Checked::Valid),
            ("cursor", "blinking") => self.set(value, key_segments, path, &mut config.cursor.blinking, &previous.cursor.blinking, Checked::Valid),
            ("keyboard", "ime") => self.set(value, key_segments, path, &mut config.keyboard.ime, &previous.keyboard.ime, Checked::Valid),
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
                        Checked::Clamped(UpdateInterval::Auto, Clamp::UnknownInterval { value: interval, expected: names.join(", ") })
                    }
                }
            }),
            ("notifications", "program") => self.set(value, key_segments, path, &mut config.notifications.program, &previous.notifications.program, Checked::Valid),
            ("notifications", "command_finished") => self.set(value, key_segments, path, &mut config.notifications.command_finished, &previous.notifications.command_finished, Checked::Valid),
            ("notifications", "command_threshold") => self.set(value, key_segments, path, &mut config.notifications.command_threshold, &previous.notifications.command_threshold, |seconds: u64| {
                if seconds >= 1 {
                    Checked::Valid(seconds)
                } else {
                    Checked::Invalid(ValueError::AtLeast(1.0))
                }
            }),
            ("notifications", "bell") => self.set(value, key_segments, path, &mut config.notifications.bell, &previous.notifications.bell, Checked::Valid),
            ("quick_terminal", "enabled") => self.set(value, key_segments, path, &mut config.quick_terminal.enabled, &previous.quick_terminal.enabled, Checked::Valid),
            ("quick_terminal", "hotkey") => self.set(value, key_segments, path, &mut config.quick_terminal.hotkey, &previous.quick_terminal.hotkey, |hotkey: String| {
                if hotkey.is_empty() {
                    return Checked::Valid(hotkey);
                }
                match keybindings::Shortcut::parse(&hotkey) {
                    Ok(_) => Checked::Valid(hotkey),
                    Err(reason) => Checked::Invalid(reason),
                }
            }),
            ("quick_terminal", "edge") => self.set(value, key_segments, path, &mut config.quick_terminal.edge, &previous.quick_terminal.edge, Checked::Valid),
            ("quick_terminal", "size") => self.set(value, key_segments, path, &mut config.quick_terminal.size, &previous.quick_terminal.size, |size: u32| {
                if (MIN_QUICK_TERMINAL_SIZE..=MAX_QUICK_TERMINAL_SIZE).contains(&size) {
                    Checked::Valid(size)
                } else {
                    let clamped = size.clamp(MIN_QUICK_TERMINAL_SIZE, MAX_QUICK_TERMINAL_SIZE);
                    Checked::Clamped(clamped, Clamp::out_of_range(f64::from(size), f64::from(MIN_QUICK_TERMINAL_SIZE), f64::from(MAX_QUICK_TERMINAL_SIZE), f64::from(clamped)))
                }
            }),
            ("quick_terminal", "screen") => self.set(value, key_segments, path, &mut config.quick_terminal.screen, &previous.quick_terminal.screen, |screen: String| {
                match QuickTerminalScreen::parse(&screen) {
                    Ok(screen) => Checked::Valid(screen),
                    Err(reason) => Checked::Invalid(reason),
                }
            }),
            ("quick_terminal", "animation_ms") => self.set(value, key_segments, path, &mut config.quick_terminal.animation_ms, &previous.quick_terminal.animation_ms, |ms: u32| {
                if ms <= MAX_QUICK_TERMINAL_ANIMATION_MS {
                    Checked::Valid(ms)
                } else {
                    Checked::Clamped(MAX_QUICK_TERMINAL_ANIMATION_MS, Clamp::AboveMaximum { value: f64::from(ms), max: f64::from(MAX_QUICK_TERMINAL_ANIMATION_MS) })
                }
            }),
            ("quick_terminal", "hide_on_focus_loss") => self.set(value, key_segments, path, &mut config.quick_terminal.hide_on_focus_loss, &previous.quick_terminal.hide_on_focus_loss, Checked::Valid),
            ("quick_terminal", "autostart") => self.set(value, key_segments, path, &mut config.quick_terminal.autostart, &previous.quick_terminal.autostart, Checked::Valid),
            ("keybindings", keys) => {
                let previous_action = previous.keybindings.get(keys).cloned();
                let mut action = previous_action.clone();
                self.set(value, key_segments, path, &mut action, &previous_action, |action: String| {
                    match keybindings::parse_binding(keys, &action) {
                        Ok(_) => Checked::Valid(Some(action)),
                        Err(reason) => Checked::Invalid(reason),
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
            ),
        }
    }
}

/// Result of validating a value read from the file.
pub(super) enum Checked<T> {
    Valid(T),
    /// Out of range: brought back into range, with an error.
    Clamped(T, Clamp),
    /// Rejected: the previous value stays, with an error.
    Invalid(ValueError),
}

/// How a value was brought back into range ([`Checked::Clamped`]).
pub(super) enum Clamp {
    OutOfRange {
        value: f64,
        min: f64,
        max: f64,
        used: f64,
    },
    AboveMaximum {
        value: f64,
        max: f64,
    },
    UnknownInterval {
        value: String,
        expected: String,
    },
}

impl Clamp {
    fn out_of_range(value: f64, min: f64, max: f64, used: f64) -> Self {
        Self::OutOfRange {
            value,
            min,
            max,
            used,
        }
    }

    /// The error of the key `path`.
    fn at(self, path: &str) -> ConfigMsg {
        let path = path.to_owned();
        match self {
            Self::OutOfRange {
                value,
                min,
                max,
                used,
            } => ConfigMsg::OutOfRange {
                path,
                value,
                min,
                max,
                used,
            },
            Self::AboveMaximum { value, max } => ConfigMsg::AboveMaximum { path, value, max },
            Self::UnknownInterval { value, expected } => ConfigMsg::UnknownInterval {
                path,
                value,
                expected,
            },
        }
    }
}

/// `value` with the digits it is written with: `0.9_f32` is `0.9`, not the
/// `0.8999999761581421` of its exact `f64`.
fn number(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(f64::from(value))
}

impl<T> Checked<T> {
    pub(super) fn map<U>(self, f: impl FnOnce(T) -> U) -> Checked<U> {
        match self {
            Checked::Valid(v) => Checked::Valid(f(v)),
            Checked::Clamped(v, clamp) => Checked::Clamped(f(v), clamp),
            Checked::Invalid(reason) => Checked::Invalid(reason),
        }
    }
}

pub(super) fn positive_size(size: u32) -> Checked<u32> {
    if size > 0 {
        Checked::Valid(size)
    } else {
        Checked::Invalid(ValueError::GreaterThanZero)
    }
}

pub(super) fn non_empty(name: String) -> Checked<String> {
    if name.is_empty() {
        Checked::Invalid(ValueError::NotEmpty)
    } else {
        Checked::Valid(name)
    }
}
