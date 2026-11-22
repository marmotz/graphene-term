//! What the settings window shows and writes: field values from the applied
//! configuration, errors routed by key, the locked state of an unreadable file, and the
//! changes an edit makes to `config.toml`. Pure; drawn by `view`.

use std::path::PathBuf;

use alacritty_terminal::vte::ansi::Rgb;
use cosmic_text::fontdb;

use crate::appearance::Appearance;
use crate::config::{
    self, Config, ConfigError, ConfigErrorKind, CursorShape, Language, MAX_FONT_SIZE,
    MAX_SCROLLBACK_LINES, MIN_FONT_SIZE, StartupMode, TabPosition, UpdateInterval,
};
use crate::i18n::{Msg, SettingsField as Field, SettingsSection as Section};
use crate::input::Action;
use crate::keybindings::{self, Keybindings, NONE_ACTION, Shortcut};
use crate::palette::Palette;
use crate::theme::{self, ColorKey, NameError};

/// Errors of the last load of the configuration file and of its theme.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadErrors {
    pub config: Vec<ConfigError>,
    pub theme: Vec<ConfigError>,
}

/// How a field is edited.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Widget {
    /// One value of [`SettingsModel::choices`], written on change.
    Choice,
    /// A number typed in a text field, written on `Enter` or focus loss.
    Number(NumberRange),
    /// A slider, written when the drag ends and on each keyboard step.
    Slider { min: f64, max: f64, step: f64 },
    /// A checkbox, written on change.
    Toggle,
    /// Free text, written on `Enter` or focus loss.
    Text,
}

/// Accepted values of a number field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberRange {
    pub min: f64,
    pub max: Option<f64>,
    pub integer: bool,
}

/// An entry of a choice field: the value written to the file, and a label when the
/// value itself is not shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub value: String,
    pub label: Option<Msg>,
}

/// What the user did to a field.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Choice(String),
    Toggle(bool),
    Text(String),
    Number(f64),
}

/// A value written to the file.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Integer(i64),
    Float(f64),
    Text(String),
    /// An array of strings.
    List(Vec<String>),
}

impl From<Value> for toml_edit::Value {
    fn from(value: Value) -> Self {
        match value {
            Value::Bool(value) => value.into(),
            Value::Integer(value) => value.into(),
            Value::Float(value) => value.into(),
            Value::Text(value) => value.into(),
            Value::List(items) => toml_edit::Value::Array(items.into_iter().collect()),
        }
    }
}

/// A change to `config.toml`, by raw key segments (`["font", "size"]`).
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Set(Vec<String>, Value),
    Remove(Vec<String>),
}

/// A change to a theme file, made on the theme being edited
/// ([`SettingsModel::edited_theme`]).
#[derive(Debug, Clone, PartialEq)]
pub enum ThemeOp {
    /// Sets one color; `None` empties it (only `selection_foreground`).
    SetColor(ColorKey, Option<Rgb>),
    /// Copies the theme to a new user theme of this name.
    Duplicate(String),
    /// Renames the user theme.
    Rename(String),
    /// Deletes the user theme.
    Delete,
}

/// Why a typed value is not written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    NotANumber,
    NotAWholeNumber,
    AtLeast(String),
    AtMost(String),
}

impl InputError {
    pub fn msg(&self) -> Msg {
        match self {
            InputError::NotANumber => Msg::NotANumber,
            InputError::NotAWholeNumber => Msg::NotAWholeNumber,
            InputError::AtLeast(min) => Msg::AtLeast(min.clone()),
            InputError::AtMost(max) => Msg::AtMost(max.clone()),
        }
    }
}

/// Section of the file holding the keys of each settings section.
const SECTION_KEYS: [(&str, Section); 11] = [
    ("language", Section::Language),
    ("font", Section::Font),
    ("window", Section::Window),
    ("scrollback", Section::Scrollback),
    ("cursor", Section::Cursor),
    ("selection", Section::Selection),
    ("tabs", Section::Tabs),
    ("theme", Section::Theme),
    ("keybindings", Section::KeyboardShortcuts),
    ("notifications", Section::Notifications),
    ("updates", Section::Updates),
];

/// Fields of `section`, in display order.
pub fn section_fields(section: Section) -> &'static [Field] {
    match section {
        Section::Font => &[
            Field::FontFamily,
            Field::FontSize,
            Field::FontLigatures,
            Field::FontFeatures,
        ],
        Section::Window => &[
            Field::StartupMode,
            Field::WindowWidth,
            Field::WindowHeight,
            Field::Padding,
            Field::BackgroundOpacity,
        ],
        Section::Scrollback => &[Field::ScrollbackLines],
        Section::Cursor => &[Field::CursorShape, Field::CursorBlinking],
        Section::Selection => &[Field::WordSeparators],
        Section::Tabs => &[Field::TabPosition],
        Section::Language => &[Field::Language],
        Section::Theme => &[
            Field::ThemeFollow,
            Field::ThemeName,
            Field::ThemeLight,
            Field::ThemeDark,
        ],
        Section::KeyboardShortcuts => &[],
        Section::Notifications => &[
            Field::NotificationsProgram,
            Field::NotificationsCommandFinished,
            Field::NotificationsCommandThreshold,
            Field::NotificationsBell,
        ],
        Section::Updates => &[Field::UpdatesCheck, Field::UpdatesInterval],
    }
}

/// Key of `field` in the file; `None` for the appearance toggle, which has none.
fn path(field: Field) -> Option<&'static [&'static str]> {
    Some(match field {
        Field::Language => &["language"],
        Field::FontFamily => &["font", "family"],
        Field::FontSize => &["font", "size"],
        Field::FontLigatures => &["font", "ligatures"],
        Field::FontFeatures => &["font", "features"],
        Field::StartupMode => &["window", "startup_mode"],
        Field::WindowWidth => &["window", "width"],
        Field::WindowHeight => &["window", "height"],
        Field::Padding => &["window", "padding"],
        Field::BackgroundOpacity => &["window", "background_opacity"],
        Field::ScrollbackLines => &["scrollback", "lines"],
        Field::CursorShape => &["cursor", "shape"],
        Field::CursorBlinking => &["cursor", "blinking"],
        Field::WordSeparators => &["selection", "word_separators"],
        Field::TabPosition => &["tabs", "position"],
        Field::ThemeFollow => return None,
        Field::ThemeName => &["theme", "name"],
        Field::ThemeLight => &["theme", "light"],
        Field::ThemeDark => &["theme", "dark"],
        Field::UpdatesCheck => &["updates", "check"],
        Field::UpdatesInterval => &["updates", "interval"],
        Field::NotificationsProgram => &["notifications", "program"],
        Field::NotificationsCommandFinished => &["notifications", "command_finished"],
        Field::NotificationsCommandThreshold => &["notifications", "command_threshold"],
        Field::NotificationsBell => &["notifications", "bell"],
    })
}

fn owned(path: &[&str]) -> Vec<String> {
    path.iter().map(|segment| (*segment).to_owned()).collect()
}

/// How `field` is edited.
pub fn widget(field: Field) -> Widget {
    let window_size = NumberRange {
        min: 1.0,
        max: Some(f64::from(u32::MAX)),
        integer: true,
    };
    match field {
        Field::Language
        | Field::FontFamily
        | Field::StartupMode
        | Field::CursorShape
        | Field::TabPosition
        | Field::ThemeName
        | Field::ThemeLight
        | Field::ThemeDark
        | Field::UpdatesInterval => Widget::Choice,
        Field::FontSize => Widget::Number(NumberRange {
            min: f64::from(MIN_FONT_SIZE),
            max: Some(f64::from(MAX_FONT_SIZE)),
            integer: false,
        }),
        Field::WindowWidth | Field::WindowHeight => Widget::Number(window_size),
        Field::Padding => Widget::Number(NumberRange {
            min: 0.0,
            max: None,
            integer: false,
        }),
        Field::ScrollbackLines => Widget::Number(NumberRange {
            min: 0.0,
            max: Some(MAX_SCROLLBACK_LINES as f64),
            integer: true,
        }),
        Field::BackgroundOpacity => Widget::Slider {
            min: 0.0,
            max: 1.0,
            step: 0.01,
        },
        Field::NotificationsCommandThreshold => Widget::Number(NumberRange {
            min: 1.0,
            max: Some(f64::from(u32::MAX)),
            integer: true,
        }),
        Field::FontLigatures
        | Field::CursorBlinking
        | Field::UpdatesCheck
        | Field::ThemeFollow
        | Field::NotificationsProgram
        | Field::NotificationsCommandFinished
        | Field::NotificationsBell => Widget::Toggle,
        Field::WordSeparators | Field::FontFeatures => Widget::Text,
    }
}

fn name_of<T: PartialEq + Copy>(values: &[(&'static str, T)], value: T) -> &'static str {
    values
        .iter()
        .find(|(_, v)| *v == value)
        .map_or("", |(name, _)| name)
}

fn names<T>(values: &[(&'static str, T)]) -> Vec<Choice> {
    values
        .iter()
        .map(|(name, _)| Choice {
            value: (*name).to_owned(),
            label: None,
        })
        .collect()
}

/// Monospace families of `db`, sorted, each once.
pub fn monospace_families(db: &fontdb::Database) -> Vec<String> {
    let mut families: Vec<String> = db
        .faces()
        .filter(|face| face.monospaced)
        .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
        .collect();
    families.sort();
    families.dedup();
    families
}

/// Parses a typed number of `range`.
fn parse_number(text: &str, range: NumberRange) -> Result<Value, InputError> {
    let number: f64 = text
        .trim()
        .parse()
        .ok()
        .filter(|n: &f64| n.is_finite())
        .ok_or(InputError::NotANumber)?;
    if range.integer && number.fract() != 0.0 {
        return Err(InputError::NotAWholeNumber);
    }
    if number < range.min {
        return Err(InputError::AtLeast(range.min.to_string()));
    }
    if let Some(max) = range.max
        && number > max
    {
        return Err(InputError::AtMost(max.to_string()));
    }
    Ok(if range.integer {
        Value::Integer(number as i64)
    } else {
        Value::Float(number)
    })
}

/// Whether writing `new` over `current` changes nothing. Floats are compared at the
/// precision the configuration keeps them (`f32`).
fn same(current: &Value, new: &Value) -> bool {
    match (current, new) {
        (Value::Float(a), Value::Float(b)) => *a as f32 == *b as f32,
        (a, b) => a == b,
    }
}

/// The settings window's state.
#[derive(Debug, Clone)]
pub struct SettingsModel {
    config: Config,
    errors: LoadErrors,
    /// Error of the last write, cleared by the next write or reload.
    write_error: Option<String>,
    theme_choices: Vec<String>,
    font_families: Vec<String>,
    /// The shortcut table of `config`.
    keybindings: Keybindings,
    /// The config directory, holding the user themes; `None` for built-in themes only.
    dir: Option<PathBuf>,
    /// The system appearance, which picks the active theme when following it.
    appearance: Appearance,
    /// The theme shown in the theme editor: the active theme when the window opens.
    edited_theme: String,
    /// The colors of `edited_theme`, read again on each reload.
    edited_palette: Palette,
    /// Whether `edited_theme` has no user file, and so cannot be changed.
    edited_is_built_in: bool,
}

impl SettingsModel {
    /// The model of the applied `config` and the `errors` of its last load, with the
    /// user themes of `dir` under the system `appearance`. The theme editor shows the
    /// active theme.
    pub fn new(
        config: &Config,
        errors: &LoadErrors,
        dir: Option<PathBuf>,
        appearance: Appearance,
    ) -> Self {
        let edited_theme = config.theme.active(appearance).to_owned();
        let mut model = Self {
            config: config.clone(),
            errors: errors.clone(),
            write_error: None,
            theme_choices: theme::available(dir.as_deref(), &config.theme.unlocked),
            font_families: Vec::new(),
            keybindings: Keybindings::new(&config.keybindings),
            dir,
            appearance,
            edited_theme,
            edited_palette: theme::default_palette().clone(),
            edited_is_built_in: true,
        };
        model.reload_edited_theme();
        model
    }

    /// Takes a reloaded configuration, its errors and the system appearance.
    pub fn refresh(&mut self, config: &Config, errors: &LoadErrors, appearance: Appearance) {
        self.config = config.clone();
        self.errors = errors.clone();
        self.appearance = appearance;
        self.write_error = None;
        self.keybindings = Keybindings::new(&self.config.keybindings);
        self.reload_edited_theme();
    }

    /// Takes the configuration parsed from the file just written, before the reload
    /// reports it; the theme errors stay those of the last load.
    pub fn written(&mut self, config: Config, config_errors: Vec<ConfigError>) {
        self.config = config;
        self.errors.config = config_errors;
        self.write_error = None;
        self.keybindings = Keybindings::new(&self.config.keybindings);
        self.reload_edited_theme();
    }

    /// The theme the configuration loads under the current appearance.
    pub fn active_theme(&self) -> &str {
        self.config.theme.active(self.appearance)
    }

    /// The colors of the active theme, as the reload loads them.
    pub fn active_palette(&self) -> Palette {
        theme::load(self.active_theme(), self.dir.as_deref()).0
    }

    /// The colors to show in the terminal window while `picked` is chosen for its key:
    /// the edited palette with that color, only when the edited theme is the active one.
    pub fn live_preview(&self, picked: Option<(ColorKey, Rgb)>) -> Option<Palette> {
        let (key, color) = picked?;
        if self.edited_theme != self.active_theme() {
            return None;
        }
        let mut palette = self.edited_palette.clone();
        key.set(&mut palette, Some(color));
        Some(palette)
    }

    /// The theme shown in the theme editor.
    pub fn edited_theme(&self) -> &str {
        &self.edited_theme
    }

    /// The colors of the edited theme.
    pub fn edited_palette(&self) -> &Palette {
        &self.edited_palette
    }

    /// Whether the edited theme is built in (no user file): it cannot be changed, only
    /// duplicated.
    pub fn edited_is_built_in(&self) -> bool {
        self.edited_is_built_in
    }

    /// Shows `name` in the theme editor, independently of the active theme.
    pub fn edit_theme(&mut self, name: &str) {
        name.clone_into(&mut self.edited_theme);
        self.reload_edited_theme();
    }

    /// Reads the edited theme again; one that no longer exists (deleted or renamed)
    /// gives way to the active theme.
    fn reload_edited_theme(&mut self) {
        let dir = self.dir.as_deref();
        if !theme::available(dir, &self.config.theme.unlocked).contains(&self.edited_theme) {
            self.edited_theme = self.config.theme.active(self.appearance).to_owned();
        }
        self.edited_palette = theme::load(&self.edited_theme, dir).0;
        self.edited_is_built_in = !dir
            .and_then(|dir| theme::user_file(dir, &self.edited_theme))
            .is_some_and(|file| file.is_file());
    }

    /// Checks `op` on the edited theme and returns the changes it makes to
    /// `config.toml`, to write before `op` itself:
    /// - duplicating points the active theme's key to the copy;
    /// - renaming moves each of `theme.name`, `theme.light` and `theme.dark` naming the
    ///   edited theme to the new name;
    /// - deleting sets those keys back to `graphene-night`.
    ///
    /// The new name of a copy or a rename must be free ([`theme::valid_name`]).
    pub fn theme_op(&self, op: ThemeOp) -> Result<(Vec<Change>, ThemeOp), NameError> {
        let changes = match &op {
            ThemeOp::SetColor(..) => Vec::new(),
            ThemeOp::Duplicate(name) => {
                theme::valid_name(name, self.dir.as_deref())?;
                let key = self.config.theme.active_key(self.appearance);
                vec![Change::Set(owned(key), Value::Text(name.clone()))]
            }
            ThemeOp::Rename(name) => {
                theme::valid_name(name, self.dir.as_deref())?;
                self.theme_keys_naming_edited(name)
            }
            ThemeOp::Delete => self.theme_keys_naming_edited(theme::DEFAULT_THEME),
        };
        Ok((changes, op))
    }

    /// Sets to `name` each of `theme.name`, `theme.light` and `theme.dark` that names
    /// the edited theme.
    fn theme_keys_naming_edited(&self, name: &str) -> Vec<Change> {
        let theme = &self.config.theme;
        [
            ("name", Some(&theme.name)),
            ("light", theme.light.as_ref()),
            ("dark", theme.dark.as_ref()),
        ]
        .into_iter()
        .filter(|(_, value)| value.is_some_and(|value| *value == self.edited_theme))
        .map(|(key, _)| Change::Set(owned(&["theme", key]), Value::Text(name.to_owned())))
        .collect()
    }

    /// Records a failed write.
    pub fn write_failed(&mut self, error: &ConfigError) {
        self.write_error = Some(error.to_string());
    }

    /// The user and built-in themes.
    pub fn theme_choices(&self) -> &[String] {
        &self.theme_choices
    }

    pub fn set_theme_choices(&mut self, choices: Vec<String>) {
        self.theme_choices = choices;
    }

    pub fn set_font_families(&mut self, families: Vec<String>) {
        self.font_families = families;
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The changes unlocking the hidden theme and making it the active one: adds it to
    /// `theme.unlocked` unless it already is there, and sets the key of the active theme
    /// (`theme.name`, or `theme.light` / `theme.dark` when following the appearance).
    pub fn unlock_hidden_theme(&self) -> Vec<Change> {
        let hidden = crate::easter_eggs::HIDDEN_THEME;
        let theme = &self.config.theme;
        let mut changes = Vec::new();
        if !theme.unlocked.iter().any(|name| name == hidden) {
            let mut unlocked = theme.unlocked.clone();
            unlocked.push(hidden.to_owned());
            changes.push(Change::Set(
                owned(&["theme", "unlocked"]),
                Value::List(unlocked),
            ));
        }
        let key = theme.active_key(self.appearance);
        changes.push(Change::Set(owned(key), Value::Text(hidden.to_owned())));
        changes
    }

    /// The syntax or read error that keeps `section` from being edited, if any: one of
    /// `keybindings.toml` locks the keyboard shortcuts, one of `config.toml` the others.
    pub fn locked(&self, section: Section) -> Option<&ConfigError> {
        let shortcuts = section == Section::KeyboardShortcuts;
        self.errors.config.iter().find(|error| {
            matches!(error.kind, ConfigErrorKind::Syntax | ConfigErrorKind::Read)
                && in_keybindings_file(error) == shortcuts
        })
    }

    pub fn write_error(&self) -> Option<&str> {
        self.write_error.as_deref()
    }

    /// Whether `field` is shown: the theme name without the appearance toggle, the
    /// light and dark themes with it.
    pub fn visible(&self, field: Field) -> bool {
        let follows = self.config.theme.follows_appearance();
        match field {
            Field::ThemeName => !follows,
            Field::ThemeLight | Field::ThemeDark => follows,
            _ => true,
        }
    }

    /// Current value of a field that has a key.
    fn value(&self, field: Field) -> Value {
        let c = &self.config;
        let text = |name: &str| Value::Text(name.to_owned());
        match field {
            Field::Language => text(name_of(&Language::VALUES, c.language)),
            Field::FontFamily => text(c.font.family.as_deref().unwrap_or_default()),
            Field::FontSize => Value::Float(f64::from(c.font.size)),
            Field::FontLigatures => Value::Bool(c.font.ligatures),
            Field::FontFeatures => text(&c.font.features.to_string()),
            Field::StartupMode => text(name_of(&StartupMode::VALUES, c.window.startup_mode)),
            Field::WindowWidth => Value::Integer(i64::from(c.window.width)),
            Field::WindowHeight => Value::Integer(i64::from(c.window.height)),
            Field::Padding => Value::Float(f64::from(c.window.padding)),
            Field::BackgroundOpacity => Value::Float(f64::from(c.window.background_opacity)),
            Field::ScrollbackLines => Value::Integer(c.scrollback_lines as i64),
            Field::CursorShape => text(name_of(&CursorShape::VALUES, c.cursor.shape)),
            Field::CursorBlinking => Value::Bool(c.cursor.blinking),
            Field::WordSeparators => text(&c.word_separators),
            Field::TabPosition => text(name_of(&TabPosition::VALUES, c.tab_position)),
            Field::ThemeFollow => Value::Bool(c.theme.follows_appearance()),
            Field::ThemeName => text(&c.theme.name),
            Field::ThemeLight => text(c.theme.light.as_deref().unwrap_or(&c.theme.name)),
            Field::ThemeDark => text(c.theme.dark.as_deref().unwrap_or(&c.theme.name)),
            Field::UpdatesCheck => Value::Bool(c.updates.check),
            Field::UpdatesInterval => text(name_of(&UpdateInterval::VALUES, c.updates.interval)),
            Field::NotificationsProgram => Value::Bool(c.notifications.program),
            Field::NotificationsCommandFinished => Value::Bool(c.notifications.command_finished),
            Field::NotificationsCommandThreshold => {
                Value::Integer(i64::try_from(c.notifications.command_threshold).unwrap_or(i64::MAX))
            }
            Field::NotificationsBell => Value::Bool(c.notifications.bell),
        }
    }

    /// Text of a choice, text or number field.
    pub fn text(&self, field: Field) -> String {
        match self.value(field) {
            Value::Text(text) => text,
            Value::Integer(n) => n.to_string(),
            Value::Float(n) => (n as f32).to_string(),
            Value::Bool(_) | Value::List(_) => String::new(),
        }
    }

    /// State of a toggle field.
    pub fn on(&self, field: Field) -> bool {
        self.value(field) == Value::Bool(true)
    }

    /// Value of a slider field.
    pub fn number(&self, field: Field) -> f64 {
        match self.value(field) {
            Value::Float(n) => n,
            Value::Integer(n) => n as f64,
            Value::Bool(_) | Value::Text(_) | Value::List(_) => 0.0,
        }
    }

    /// Entries of a choice field; the current value is kept when it is not one of them
    /// (a theme that no longer exists, a family not installed).
    pub fn choices(&self, field: Field) -> Vec<Choice> {
        let named = |names: &[String]| -> Vec<Choice> {
            names
                .iter()
                .map(|name| Choice {
                    value: name.clone(),
                    label: None,
                })
                .collect()
        };
        let mut choices = match field {
            Field::Language => names(&Language::VALUES),
            Field::FontFamily => {
                let mut choices = vec![Choice {
                    value: String::new(),
                    label: Some(Msg::SystemMonospace),
                }];
                choices.extend(named(&self.font_families));
                choices
            }
            Field::StartupMode => names(&StartupMode::VALUES),
            Field::CursorShape => names(&CursorShape::VALUES),
            Field::TabPosition => names(&TabPosition::VALUES),
            Field::ThemeName | Field::ThemeLight | Field::ThemeDark => named(&self.theme_choices),
            Field::UpdatesInterval => names(&UpdateInterval::VALUES),
            _ => return Vec::new(),
        };
        let current = self.text(field);
        if !choices.iter().any(|choice| choice.value == current) {
            choices.push(Choice {
                value: current,
                label: None,
            });
        }
        choices
    }

    /// The changes `input` makes to the file; empty when it changes nothing.
    pub fn edit(&self, field: Field, input: Input) -> Result<Vec<Change>, InputError> {
        if field == Field::ThemeFollow {
            return Ok(self.follow_appearance(input == Input::Toggle(true)));
        }
        let Some(path) = path(field) else {
            return Ok(Vec::new());
        };
        let new = match (widget(field), input) {
            (Widget::Choice, Input::Choice(value)) | (Widget::Text, Input::Text(value)) => {
                Value::Text(value)
            }
            (Widget::Toggle, Input::Toggle(on)) => Value::Bool(on),
            (Widget::Number(range), Input::Text(text)) => parse_number(&text, range)?,
            (Widget::Slider { min, max, step }, Input::Number(number)) => {
                Value::Float(((number / step).round() * step).clamp(min, max))
            }
            _ => return Ok(Vec::new()),
        };
        if same(&self.value(field), &new) {
            return Ok(Vec::new());
        }
        Ok(vec![Change::Set(owned(path), new)])
    }

    /// Turning the appearance toggle on sets `light` and `dark` (to the current theme
    /// when unset); turning it off removes both, leaving `name`.
    fn follow_appearance(&self, on: bool) -> Vec<Change> {
        let theme = &self.config.theme;
        if on == theme.follows_appearance()
            && (on || (theme.light.is_none() && theme.dark.is_none()))
        {
            return Vec::new();
        }
        let light = owned(&["theme", "light"]);
        let dark = owned(&["theme", "dark"]);
        if on {
            let pick = |name: &Option<String>| {
                Value::Text(name.clone().unwrap_or_else(|| theme.name.clone()))
            };
            vec![
                Change::Set(light, pick(&theme.light)),
                Change::Set(dark, pick(&theme.dark)),
            ]
        } else {
            vec![Change::Remove(light), Change::Remove(dark)]
        }
    }

    /// Messages of the configuration errors of `field`.
    pub fn field_errors(&self, field: Field) -> Vec<&str> {
        let Some(path) = path(field).map(config::key_path) else {
            return Vec::new();
        };
        self.value_errors()
            .filter(|error| error.key.as_deref() == Some(path.as_str()))
            .map(|error| error.message.as_str())
            .collect()
    }

    /// Messages of the configuration errors of `section` tied to no field (a section
    /// that is not a table, a keybinding).
    pub fn section_errors(&self, section: Section) -> Vec<&str> {
        self.value_errors()
            .filter(|error| {
                let key = error.key.as_deref().unwrap_or_default();
                field_of_key(key).is_none() && section_of_key(key) == Some(section)
            })
            .map(|error| error.message.as_str())
            .collect()
    }

    /// Errors of the theme file and theme name, shown in the Theme section.
    pub fn theme_errors(&self) -> Vec<String> {
        self.errors.theme.iter().map(ToString::to_string).collect()
    }

    /// Unknown keys of the file, with their line.
    pub fn unknown_keys(&self) -> Vec<(&str, Option<usize>)> {
        self.errors
            .config
            .iter()
            .filter(|error| error.kind == ConfigErrorKind::UnknownKey)
            .map(|error| (error.key.as_deref().unwrap_or_default(), error.line))
            .collect()
    }

    /// Value errors tied to neither a field nor a section.
    pub fn other_errors(&self) -> Vec<String> {
        self.value_errors()
            .filter(|error| {
                let key = error.key.as_deref().unwrap_or_default();
                field_of_key(key).is_none() && section_of_key(key).is_none()
            })
            .map(ToString::to_string)
            .collect()
    }

    /// One row per action of the keyboard shortcuts section, with its shortcuts: the
    /// palette's actions, the palette, the first nine tabs, then any other action the
    /// file binds (`go_to_tab_12`).
    pub fn shortcut_rows(&self) -> Vec<(Action, Vec<Shortcut>)> {
        let mut actions: Vec<Action> = Action::ALL
            .into_iter()
            .chain([Action::CommandPalette])
            .chain((1..=9).map(Action::GoToTab))
            .collect();
        for action in self.config.keybindings.values() {
            if let Some(action) = Action::from_id(action)
                && !actions.contains(&action)
            {
                actions.push(action);
            }
        }
        actions
            .into_iter()
            .map(|action| (action, self.keybindings.shortcuts(action)))
            .collect()
    }

    /// The action `shortcut` triggers now.
    pub fn bound(&self, shortcut: Shortcut) -> Option<Action> {
        self.keybindings.bound(shortcut)
    }

    /// Binds `shortcut` to `action`; `Err` with the action it is already bound to, when
    /// another one (see [`Self::replace_shortcut`]).
    pub fn add_shortcut(&self, action: Action, shortcut: Shortcut) -> Result<Vec<Change>, Action> {
        match self.bound(shortcut) {
            Some(bound) if bound == action => Ok(Vec::new()),
            Some(other) => Err(other),
            None => Ok(self.replace_shortcut(action, shortcut)),
        }
    }

    /// Binds `shortcut` to `action`, over any action it is bound to: the entries of the
    /// file naming it are replaced by `"<label>" = "<action>"`, or just removed when
    /// that is its default.
    pub fn replace_shortcut(&self, action: Action, shortcut: Shortcut) -> Vec<Change> {
        if self.bound(shortcut) == Some(action) {
            return Vec::new();
        }
        let label = shortcut.label();
        let mut changes = self.remove_entries(shortcut, &label);
        if keybindings::default_action(shortcut) == Some(action) {
            if self.config.keybindings.contains_key(&label) {
                changes.push(Change::Remove(keybinding(&label)));
            }
        } else {
            changes.push(Change::Set(keybinding(&label), Value::Text(action.id())));
        }
        changes
    }

    /// Unbinds `shortcut` from `action`: its entries in the file are removed, and a
    /// default shortcut of `action` is set to `"none"`.
    pub fn remove_shortcut(&self, action: Action, shortcut: Shortcut) -> Vec<Change> {
        if self.bound(shortcut) != Some(action) {
            return Vec::new();
        }
        let label = shortcut.label();
        let mut changes = self.remove_entries(shortcut, &label);
        if keybindings::default_action(shortcut) == Some(action) {
            changes.push(Change::Set(
                keybinding(&label),
                Value::Text(NONE_ACTION.to_owned()),
            ));
        } else if self.config.keybindings.contains_key(&label) {
            changes.push(Change::Remove(keybinding(&label)));
        }
        changes
    }

    /// Removals of the `[keybindings]` entries naming `shortcut` but written another
    /// way than `label` (`ctrl+shift+c`), so a single entry is left to set.
    fn remove_entries(&self, shortcut: Shortcut, label: &str) -> Vec<Change> {
        self.config
            .keybindings
            .keys()
            .filter(|keys| keys.as_str() != label && Shortcut::parse(keys) == Ok(shortcut))
            .map(|keys| Change::Remove(keybinding(keys)))
            .collect()
    }

    fn value_errors(&self) -> impl Iterator<Item = &ConfigError> {
        self.errors.config.iter().filter(|error| {
            matches!(
                error.kind,
                ConfigErrorKind::InvalidValue | ConfigErrorKind::Write
            )
        })
    }
}

/// Whether `error` was found in `keybindings.toml`.
fn in_keybindings_file(error: &ConfigError) -> bool {
    error
        .file
        .file_name()
        .is_some_and(|name| name == config::KEYBINDINGS_FILE)
}

/// Raw path of a `[keybindings]` entry.
fn keybinding(keys: &str) -> Vec<String> {
    vec!["keybindings".to_owned(), keys.to_owned()]
}

fn field_of_key(key: &str) -> Option<Field> {
    Field::ALL
        .into_iter()
        .find(|&field| path(field).is_some_and(|path| config::key_path(path) == key))
}

/// Section of a key path, by its first segment.
fn section_of_key(key: &str) -> Option<Section> {
    let first = key.split('.').next()?;
    SECTION_KEYS
        .iter()
        .find(|(name, _)| *name == first)
        .map(|&(_, section)| section)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::config::{ConfigError, ConfigErrorKind};

    fn file() -> PathBuf {
        PathBuf::from("/home/user/.config/graphene/config.toml")
    }

    /// `text` read as `config.toml` and `keybindings.toml` at once: both files share the
    /// same TOML here, each reading only its own part.
    fn parse_both(text: &str, previous: &Config) -> (Config, Vec<ConfigError>) {
        let (mut config, mut errors) = Config::parse(text, previous, &file());
        errors.retain(|error| error.kind != ConfigErrorKind::Moved);
        let (keybindings, keybinding_errors, _) =
            Config::parse_keybindings(text, previous, &file());
        config.keybindings = keybindings;
        errors.extend(
            keybinding_errors
                .into_iter()
                .filter(|error| error.kind != ConfigErrorKind::UnknownKey),
        );
        errors.sort_by_key(|error| error.line);
        (config, errors)
    }

    /// The model of `text` parsed over the defaults.
    fn model(text: &str) -> SettingsModel {
        let (config, errors) = parse_both(text, &Config::default());
        SettingsModel::new(
            &config,
            &LoadErrors {
                config: errors,
                theme: Vec::new(),
            },
            None,
            Appearance::Dark,
        )
    }

    fn set(path: &[&str], value: Value) -> Vec<Change> {
        vec![Change::Set(owned(path), value)]
    }

    fn text(value: &str) -> Value {
        Value::Text(value.to_owned())
    }

    #[test]
    fn every_field_is_in_one_section() {
        for field in Field::ALL {
            let sections: Vec<_> = Section::ALL
                .into_iter()
                .filter(|&section| section_fields(section).contains(&field))
                .collect();
            assert_eq!(sections.len(), 1, "{field:?}");
        }
    }

    #[test]
    fn every_key_of_a_field_is_read_by_the_configuration() {
        // A key the parser does not know would be reported as unknown.
        for field in Field::ALL {
            let Some(path) = path(field) else { continue };
            let m = model(&format!("{} = 1", config::key_path(path)));
            assert!(m.unknown_keys().is_empty(), "{field:?}");
        }
    }

    #[test]
    fn fields_show_the_configured_values() {
        let m = model(
            r#"
language = "fr"
[font]
family = "Fira Code"
size = 16.5
[window]
startup_mode = "maximized"
width = 1200
height = 700
padding = 4.0
background_opacity = 0.9
[scrollback]
lines = 500
[cursor]
shape = "beam"
blinking = true
[selection]
word_separators = " ,"
[tabs]
position = "left"
[theme]
name = "gruvbox-dark"
[updates]
check = false
interval = "1w"
[notifications]
program = false
command_finished = false
command_threshold = 25
bell = true
"#,
        );
        let texts = [
            (Field::Language, "fr"),
            (Field::FontFamily, "Fira Code"),
            (Field::FontSize, "16.5"),
            (Field::StartupMode, "maximized"),
            (Field::WindowWidth, "1200"),
            (Field::WindowHeight, "700"),
            (Field::Padding, "4"),
            (Field::ScrollbackLines, "500"),
            (Field::CursorShape, "beam"),
            (Field::WordSeparators, " ,"),
            (Field::TabPosition, "left"),
            (Field::ThemeName, "gruvbox-dark"),
            (Field::UpdatesInterval, "1w"),
            (Field::NotificationsCommandThreshold, "25"),
        ];
        for (field, expected) in texts {
            assert_eq!(m.text(field), expected, "{field:?}");
        }
        assert!((m.number(Field::BackgroundOpacity) - 0.9).abs() < 1e-6);
        assert!(m.on(Field::CursorBlinking));
        assert!(!m.on(Field::UpdatesCheck));
        assert!(!m.on(Field::ThemeFollow));
        assert!(!m.on(Field::NotificationsProgram));
        assert!(!m.on(Field::NotificationsCommandFinished));
        assert!(m.on(Field::NotificationsBell));
        assert!(m.locked(Section::Font).is_none());
    }

    #[test]
    fn defaults_show_the_built_in_values() {
        let m = model("");
        assert_eq!(m.text(Field::FontFamily), "");
        assert_eq!(m.text(Field::FontSize), "14");
        assert_eq!(m.text(Field::Language), "auto");
        assert_eq!(m.text(Field::UpdatesInterval), "auto");
        assert!(m.on(Field::UpdatesCheck));
        assert!(m.on(Field::NotificationsProgram));
        assert!(m.on(Field::NotificationsCommandFinished));
        assert_eq!(m.text(Field::NotificationsCommandThreshold), "10");
        assert!(!m.on(Field::NotificationsBell));
    }

    #[test]
    fn font_section_has_the_ligature_toggle_and_the_features_field() {
        assert_eq!(
            section_fields(Section::Font),
            &[
                Field::FontFamily,
                Field::FontSize,
                Field::FontLigatures,
                Field::FontFeatures,
            ]
        );
        assert_eq!(path(Field::FontLigatures), Some(&["font", "ligatures"][..]));
        assert_eq!(path(Field::FontFeatures), Some(&["font", "features"][..]));
        assert_eq!(widget(Field::FontLigatures), Widget::Toggle);
        assert_eq!(widget(Field::FontFeatures), Widget::Text);
    }

    #[test]
    fn font_ligatures_and_features_show_the_configured_values() {
        let defaults = model("");
        assert!(defaults.on(Field::FontLigatures));
        assert_eq!(defaults.text(Field::FontFeatures), "");

        // The features are shown in their canonical form.
        let m = model("[font]\nligatures = false\nfeatures = \"+ss01, calt=0 cv05=2\"");
        assert!(!m.on(Field::FontLigatures));
        assert_eq!(m.text(Field::FontFeatures), "ss01 -calt cv05=2");
    }

    #[test]
    fn font_ligatures_and_features_write_their_key() {
        let m = model("");
        assert_eq!(
            m.edit(Field::FontLigatures, Input::Toggle(false)),
            Ok(set(&["font", "ligatures"], Value::Bool(false)))
        );
        assert_eq!(
            m.edit(Field::FontLigatures, Input::Toggle(true)),
            Ok(Vec::new())
        );
        assert_eq!(
            m.edit(Field::FontFeatures, Input::Text("ss01 -calt".into())),
            Ok(set(&["font", "features"], text("ss01 -calt")))
        );
        assert_eq!(
            m.edit(Field::FontFeatures, Input::Text(String::new())),
            Ok(Vec::new())
        );
    }

    #[test]
    fn an_invalid_feature_is_an_error_of_the_features_field() {
        let m = model("[font]\nfeatures = \"ss01 toolong\"");
        let errors = m.field_errors(Field::FontFeatures);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("toolong"), "{errors:?}");
        assert!(m.field_errors(Field::FontLigatures).is_empty());
        assert!(m.section_errors(Section::Font).is_empty());
        assert!(m.other_errors().is_empty());
    }

    #[test]
    fn notification_fields_write_their_key() {
        let m = model("");
        assert_eq!(
            m.edit(Field::NotificationsProgram, Input::Toggle(false)),
            Ok(set(&["notifications", "program"], Value::Bool(false)))
        );
        assert_eq!(
            m.edit(Field::NotificationsCommandFinished, Input::Toggle(false)),
            Ok(set(
                &["notifications", "command_finished"],
                Value::Bool(false)
            ))
        );
        assert_eq!(
            m.edit(Field::NotificationsBell, Input::Toggle(true)),
            Ok(set(&["notifications", "bell"], Value::Bool(true)))
        );
        assert_eq!(
            m.edit(
                Field::NotificationsCommandThreshold,
                Input::Text("30".into())
            ),
            Ok(set(
                &["notifications", "command_threshold"],
                Value::Integer(30)
            ))
        );
        assert_eq!(
            m.edit(
                Field::NotificationsCommandThreshold,
                Input::Text("10".into())
            ),
            Ok(Vec::new())
        );
        assert_eq!(
            m.edit(
                Field::NotificationsCommandThreshold,
                Input::Text("0".into())
            ),
            Err(InputError::AtLeast("1".into()))
        );
        assert_eq!(
            m.edit(
                Field::NotificationsCommandThreshold,
                Input::Text("2.5".into())
            ),
            Err(InputError::NotAWholeNumber)
        );
        assert_eq!(
            section_fields(Section::Notifications),
            &[
                Field::NotificationsProgram,
                Field::NotificationsCommandFinished,
                Field::NotificationsCommandThreshold,
                Field::NotificationsBell,
            ]
        );
    }

    #[test]
    fn choice_toggle_and_text_edits_write_their_key() {
        let m = model("");
        assert_eq!(
            m.edit(Field::CursorShape, Input::Choice("beam".into())),
            Ok(set(&["cursor", "shape"], text("beam")))
        );
        assert_eq!(
            m.edit(Field::Language, Input::Choice("fr".into())),
            Ok(set(&["language"], text("fr")))
        );
        assert_eq!(
            m.edit(Field::FontFamily, Input::Choice("Fira Code".into())),
            Ok(set(&["font", "family"], text("Fira Code")))
        );
        assert_eq!(
            m.edit(Field::CursorBlinking, Input::Toggle(true)),
            Ok(set(&["cursor", "blinking"], Value::Bool(true)))
        );
        assert_eq!(
            m.edit(Field::UpdatesCheck, Input::Toggle(false)),
            Ok(set(&["updates", "check"], Value::Bool(false)))
        );
        assert_eq!(
            m.edit(Field::WordSeparators, Input::Text(" ;".into())),
            Ok(set(&["selection", "word_separators"], text(" ;")))
        );
        assert_eq!(
            m.edit(Field::ThemeName, Input::Choice("gruvbox-dark".into())),
            Ok(set(&["theme", "name"], text("gruvbox-dark")))
        );
        assert_eq!(
            m.edit(Field::UpdatesInterval, Input::Choice("6h".into())),
            Ok(set(&["updates", "interval"], text("6h")))
        );
    }

    #[test]
    fn number_edits_write_their_key() {
        let m = model("");
        assert_eq!(
            m.edit(Field::FontSize, Input::Text(" 16 ".into())),
            Ok(set(&["font", "size"], Value::Float(16.0)))
        );
        assert_eq!(
            m.edit(Field::WindowWidth, Input::Text("1280".into())),
            Ok(set(&["window", "width"], Value::Integer(1280)))
        );
        assert_eq!(
            m.edit(Field::Padding, Input::Text("0".into())),
            Ok(set(&["window", "padding"], Value::Float(0.0)))
        );
        assert_eq!(
            m.edit(Field::ScrollbackLines, Input::Text("100000".into())),
            Ok(set(&["scrollback", "lines"], Value::Integer(100_000)))
        );
    }

    #[test]
    fn slider_edits_are_rounded_to_their_step() {
        let m = model("");
        assert_eq!(
            m.edit(Field::BackgroundOpacity, Input::Number(0.854_321)),
            Ok(set(&["window", "background_opacity"], Value::Float(0.85)))
        );
        assert_eq!(
            m.edit(Field::BackgroundOpacity, Input::Number(1.2)),
            Ok(Vec::new()),
            "clamped to 1.0, the current value"
        );
    }

    #[test]
    fn unchanged_values_write_nothing() {
        let m = model("[font]\nsize = 16.0");
        assert_eq!(
            m.edit(Field::FontSize, Input::Text("16".into())),
            Ok(vec![])
        );
        assert_eq!(
            m.edit(Field::CursorShape, Input::Choice("block".into())),
            Ok(vec![])
        );
        assert_eq!(
            m.edit(Field::CursorBlinking, Input::Toggle(false)),
            Ok(vec![])
        );
        assert_eq!(
            m.edit(Field::BackgroundOpacity, Input::Number(1.0)),
            Ok(vec![])
        );
    }

    #[test]
    fn numbers_out_of_range_are_rejected() {
        let m = model("");
        let cases = [
            (Field::FontSize, "5.5", InputError::AtLeast("6".into())),
            (Field::FontSize, "73", InputError::AtMost("72".into())),
            (Field::FontSize, "big", InputError::NotANumber),
            (Field::FontSize, "NaN", InputError::NotANumber),
            (Field::FontSize, "", InputError::NotANumber),
            (Field::WindowWidth, "0", InputError::AtLeast("1".into())),
            (Field::WindowHeight, "600.5", InputError::NotAWholeNumber),
            (
                Field::WindowWidth,
                "5000000000",
                InputError::AtMost(u32::MAX.to_string()),
            ),
            (Field::Padding, "-1", InputError::AtLeast("0".into())),
            (
                Field::ScrollbackLines,
                "100001",
                InputError::AtMost("100000".into()),
            ),
        ];
        for (field, input, error) in cases {
            assert_eq!(
                m.edit(field, Input::Text(input.into())),
                Err(error),
                "{field:?} {input}"
            );
        }
    }

    #[test]
    fn errors_go_under_their_field() {
        let m = model(
            "[font]\nsize = 100.0\n[window]\nwidth = 0\n[keybindings]\n\"Ctrl+X\" = \"nope\"",
        );
        assert_eq!(m.field_errors(Field::FontSize).len(), 1);
        assert!(m.field_errors(Field::FontSize)[0].contains("font.size"));
        assert_eq!(m.field_errors(Field::WindowWidth).len(), 1);
        assert!(m.field_errors(Field::WindowHeight).is_empty());
        // A keybinding has no field: its error is shown in its section.
        assert_eq!(m.section_errors(Section::KeyboardShortcuts).len(), 1);
        assert!(m.section_errors(Section::Font).is_empty());
        assert!(m.other_errors().is_empty());
        assert!(m.unknown_keys().is_empty());
    }

    #[test]
    fn a_section_that_is_not_a_table_is_a_section_error() {
        let m = model("font = 3");
        assert_eq!(m.section_errors(Section::Font), ["`font` must be a table"]);
        assert!(m.field_errors(Field::FontSize).is_empty());
    }

    #[test]
    fn unknown_keys_are_listed_with_their_line() {
        let m = model("\nfoo = 1\n[font]\nweight = 3");
        assert_eq!(
            m.unknown_keys(),
            [("foo", Some(2)), ("font.weight", Some(4))]
        );
        assert!(m.field_errors(Field::FontFamily).is_empty());
        assert!(m.section_errors(Section::Font).is_empty());
    }

    #[test]
    fn theme_errors_are_kept_apart() {
        let theme_error = ConfigError {
            file: PathBuf::from("/c/themes/mine.toml"),
            line: Some(3),
            key: Some("colors.foreground".into()),
            kind: ConfigErrorKind::InvalidValue,
            message: "invalid color".into(),
        };
        let m = SettingsModel::new(
            &Config::default(),
            &LoadErrors {
                config: Vec::new(),
                theme: vec![theme_error],
            },
            None,
            Appearance::Dark,
        );
        assert_eq!(m.theme_errors(), ["mine.toml:3: invalid color"]);
        assert!(m.other_errors().is_empty());
        assert!(m.section_errors(Section::Theme).is_empty());
    }

    #[test]
    fn a_syntax_error_locks_until_a_valid_reload() {
        let mut m = model("[font]\nsize = 16.0");
        let previous = m.config().clone();
        let (config, errors) = Config::parse("[font\n", &previous, &file());
        m.refresh(
            &config,
            &LoadErrors {
                config: errors,
                theme: Vec::new(),
            },
            Appearance::Dark,
        );
        let locked = m.locked(Section::Font).expect("locked by the syntax error");
        assert_eq!(locked.kind, ConfigErrorKind::Syntax);
        assert_eq!(locked.line, Some(1));
        // The previous values stay shown.
        assert_eq!(m.text(Field::FontSize), "16");

        let (config, errors) = Config::parse("[font]\nsize = 18.0", &previous, &file());
        m.refresh(
            &config,
            &LoadErrors {
                config: errors,
                theme: Vec::new(),
            },
            Appearance::Dark,
        );
        assert!(m.locked(Section::Font).is_none());
        assert_eq!(m.text(Field::FontSize), "18");
    }

    #[test]
    fn a_read_error_locks() {
        let m = SettingsModel::new(
            &Config::default(),
            &LoadErrors {
                config: vec![ConfigError::read(file(), "denied".into())],
                theme: Vec::new(),
            },
            None,
            Appearance::Dark,
        );
        assert!(m.locked(Section::Font).is_some());
        assert!(m.locked(Section::KeyboardShortcuts).is_none());
    }

    #[test]
    fn a_file_locks_only_its_own_sections() {
        let keybindings_error = ConfigError::read(
            PathBuf::from("/home/user/.config/graphene/keybindings.toml"),
            "denied".into(),
        );
        let m = SettingsModel::new(
            &Config::default(),
            &LoadErrors {
                config: vec![keybindings_error],
                theme: Vec::new(),
            },
            None,
            Appearance::Dark,
        );
        assert!(m.locked(Section::KeyboardShortcuts).is_some());
        assert!(m.locked(Section::Font).is_none());
        assert!(m.locked(Section::Theme).is_none());

        let m = SettingsModel::new(
            &Config::default(),
            &LoadErrors {
                config: vec![ConfigError::read(file(), "denied".into())],
                theme: Vec::new(),
            },
            None,
            Appearance::Dark,
        );
        assert!(m.locked(Section::KeyboardShortcuts).is_none());
        assert!(m.locked(Section::Font).is_some());
        assert!(m.locked(Section::Theme).is_some());
    }

    #[test]
    fn following_the_appearance_writes_or_removes_light_and_dark() {
        let m = model("[theme]\nname = \"gruvbox-dark\"");
        assert!(m.visible(Field::ThemeName));
        assert!(!m.visible(Field::ThemeLight));
        assert_eq!(
            m.edit(Field::ThemeFollow, Input::Toggle(true)),
            Ok(vec![
                Change::Set(owned(&["theme", "light"]), text("gruvbox-dark")),
                Change::Set(owned(&["theme", "dark"]), text("gruvbox-dark")),
            ])
        );
        assert_eq!(m.edit(Field::ThemeFollow, Input::Toggle(false)), Ok(vec![]));

        let m = model("[theme]\nlight = \"graphene-day\"\ndark = \"graphene-night\"");
        assert!(m.on(Field::ThemeFollow));
        assert!(!m.visible(Field::ThemeName));
        assert!(m.visible(Field::ThemeDark));
        assert_eq!(m.text(Field::ThemeLight), "graphene-day");
        assert_eq!(
            m.edit(Field::ThemeFollow, Input::Toggle(false)),
            Ok(vec![
                Change::Remove(owned(&["theme", "light"])),
                Change::Remove(owned(&["theme", "dark"])),
            ])
        );
        assert_eq!(m.edit(Field::ThemeFollow, Input::Toggle(true)), Ok(vec![]));
        assert_eq!(
            m.edit(Field::ThemeLight, Input::Choice("gruvbox-light".into())),
            Ok(set(&["theme", "light"], text("gruvbox-light")))
        );
    }

    #[test]
    fn half_set_appearance_themes_are_completed_or_removed() {
        let m = model("[theme]\nname = \"gruvbox-dark\"\nlight = \"graphene-day\"");
        assert!(!m.on(Field::ThemeFollow));
        assert_eq!(
            m.edit(Field::ThemeFollow, Input::Toggle(true)),
            Ok(vec![
                Change::Set(owned(&["theme", "light"]), text("graphene-day")),
                Change::Set(owned(&["theme", "dark"]), text("gruvbox-dark")),
            ])
        );
        assert_eq!(
            m.edit(Field::ThemeFollow, Input::Toggle(false)),
            Ok(vec![
                Change::Remove(owned(&["theme", "light"])),
                Change::Remove(owned(&["theme", "dark"])),
            ])
        );
    }

    #[test]
    fn choices_keep_the_current_value() {
        let mut m = model("[theme]\nname = \"gone\"\n[font]\nfamily = \"Missing Mono\"");
        m.set_font_families(vec!["Fira Code".into()]);
        let values = |field| -> Vec<String> {
            m.choices(field)
                .into_iter()
                .map(|choice| choice.value)
                .collect()
        };
        assert_eq!(values(Field::FontFamily), ["", "Fira Code", "Missing Mono"]);
        assert_eq!(
            m.choices(Field::FontFamily)[0].label,
            Some(Msg::SystemMonospace)
        );
        assert_eq!(
            values(Field::ThemeName).last().map(String::as_str),
            Some("gone")
        );
        assert_eq!(values(Field::CursorShape), ["block", "beam", "underline"]);
        assert_eq!(values(Field::Language), ["auto", "en", "fr"]);
        assert_eq!(
            values(Field::UpdatesInterval).len(),
            UpdateInterval::VALUES.len()
        );
    }

    #[test]
    fn monospace_families_of_an_empty_database() {
        assert!(monospace_families(&fontdb::Database::new()).is_empty());
    }

    #[test]
    fn written_configuration_replaces_the_values_and_clears_the_write_error() {
        let mut m = model("");
        m.write_failed(&ConfigError {
            file: file(),
            line: None,
            key: None,
            kind: ConfigErrorKind::Write,
            message: "cannot write the file: denied".into(),
        });
        assert_eq!(
            m.write_error(),
            Some("config.toml: cannot write the file: denied")
        );
        let (config, errors) = Config::parse("[font]\nsize = 20.0", m.config(), &file());
        m.written(config, errors);
        assert_eq!(m.text(Field::FontSize), "20");
        assert_eq!(m.write_error(), None);
    }

    fn shortcut(label: &str) -> Shortcut {
        Shortcut::parse(label).unwrap()
    }

    fn binding(keys: &str) -> Vec<String> {
        keybinding(keys)
    }

    /// The file `text` once `changes` are applied, and its model.
    fn apply(text: &str, changes: &[Change]) -> (String, SettingsModel) {
        let mut edit = crate::config_edit::ConfigEdit::parse(text, &file()).unwrap();
        for change in changes {
            match change {
                Change::Set(path, value) => {
                    let path: Vec<&str> = path.iter().map(String::as_str).collect();
                    edit.set(&path, value.clone()).unwrap();
                }
                Change::Remove(path) => {
                    let path: Vec<&str> = path.iter().map(String::as_str).collect();
                    edit.remove(&path);
                }
            }
        }
        let text = edit.to_string();
        let m = model(&text);
        assert_eq!(m.errors.config, vec![], "{text}");
        (text, m)
    }

    fn labels(m: &SettingsModel, action: Action) -> Vec<String> {
        m.shortcut_rows()
            .into_iter()
            .find(|(a, _)| *a == action)
            .map(|(_, shortcuts)| shortcuts.into_iter().map(Shortcut::label).collect())
            .unwrap_or_default()
    }

    #[test]
    fn shortcut_rows_show_the_defaults_with_the_file_applied() {
        let m = model("");
        let rows = m.shortcut_rows();
        let actions: Vec<Action> = rows.iter().map(|(action, _)| *action).collect();
        assert_eq!(&actions[..Action::ALL.len()], &Action::ALL);
        assert!(actions.contains(&Action::CommandPalette));
        assert!(actions.contains(&Action::GoToTab(9)));
        assert!(!actions.contains(&Action::GoToTab(10)));
        assert_eq!(labels(&m, Action::Copy), ["Ctrl+Shift+C", "Copy"]);
        assert!(labels(&m, Action::EqualisePanes).is_empty());
        assert_eq!(labels(&m, Action::GoToTab(1)), ["Alt+1"]);

        let m = model(
            "[keybindings]\n\"Ctrl+Alt+C\" = \"copy\"\n\"Copy\" = \"none\"\n\"Alt+0\" = \"go_to_tab_10\"\n",
        );
        assert_eq!(labels(&m, Action::Copy), ["Ctrl+Shift+C", "Ctrl+Alt+C"]);
        assert_eq!(labels(&m, Action::GoToTab(10)), ["Alt+0"]);
        assert_eq!(m.bound(shortcut("Copy")), None);
    }

    #[test]
    fn removing_a_default_shortcut_writes_none() {
        let m = model("");
        let changes = m.remove_shortcut(Action::Copy, shortcut("Ctrl+Shift+C"));
        assert_eq!(changes, set(&["keybindings", "Ctrl+Shift+C"], text("none")));
        let (_, m) = apply("", &changes);
        assert_eq!(labels(&m, Action::Copy), ["Copy"]);
        assert_eq!(m.bound(shortcut("Ctrl+Shift+C")), None);
    }

    #[test]
    fn removing_a_configured_shortcut_deletes_its_entry() {
        let file = "# mine\n[keybindings]\n\"Ctrl+Alt+T\" = \"new_tab\"\n\"F7\" = \"copy\"\n";
        let m = model(file);
        let changes = m.remove_shortcut(Action::NewTab, shortcut("Ctrl+Alt+T"));
        assert_eq!(changes, vec![Change::Remove(binding("Ctrl+Alt+T"))]);
        let (text, m) = apply(file, &changes);
        assert!(text.starts_with("# mine\n"), "{text}");
        assert_eq!(labels(&m, Action::NewTab), ["Ctrl+Shift+T"]);
        assert_eq!(labels(&m, Action::Copy), ["Ctrl+Shift+C", "Copy", "F7"]);
        // A shortcut not bound to the action changes nothing.
        assert_eq!(m.remove_shortcut(Action::Paste, shortcut("F7")), vec![]);
    }

    #[test]
    fn removing_an_entry_written_another_way_removes_every_spelling() {
        let file = "[keybindings]\n\"ctrl+shift+c\" = \"copy\"\n\"ctrl+alt+x\" = \"paste\"\n";
        let m = model(file);
        // The default `Ctrl+Shift+C`, also configured in lowercase.
        let changes = m.remove_shortcut(Action::Copy, shortcut("Ctrl+Shift+C"));
        let (text, m) = apply(file, &changes);
        assert!(!text.contains("ctrl+shift+c"), "{text}");
        assert_eq!(labels(&m, Action::Copy), ["Copy"]);
        let changes = m.remove_shortcut(Action::Paste, shortcut("Ctrl+Alt+X"));
        assert_eq!(changes, vec![Change::Remove(binding("ctrl+alt+x"))]);
    }

    #[test]
    fn adding_a_shortcut_writes_its_label_and_action() {
        let m = model("");
        let changes = m.add_shortcut(Action::EqualisePanes, shortcut("Ctrl+Alt+E"));
        assert_eq!(
            changes,
            Ok(set(&["keybindings", "Ctrl+Alt+E"], text("equalise_panes")))
        );
        let (_, m) = apply("", &changes.unwrap());
        assert_eq!(labels(&m, Action::EqualisePanes), ["Ctrl+Alt+E"]);
        // Already bound to the action: nothing to write.
        assert_eq!(
            m.add_shortcut(Action::EqualisePanes, shortcut("Ctrl+Alt+E")),
            Ok(vec![])
        );
    }

    #[test]
    fn restoring_a_removed_default_deletes_its_none_entry() {
        let file = "[keybindings]\n\"Ctrl+Shift+C\" = \"none\"\n";
        let m = model(file);
        let changes = m
            .add_shortcut(Action::Copy, shortcut("Ctrl+Shift+C"))
            .unwrap();
        assert_eq!(changes, vec![Change::Remove(binding("Ctrl+Shift+C"))]);
        let (_, m) = apply(file, &changes);
        assert_eq!(labels(&m, Action::Copy), ["Ctrl+Shift+C", "Copy"]);
    }

    #[test]
    fn a_shortcut_bound_to_another_action_is_a_conflict_until_replaced() {
        let m = model("");
        let taken = shortcut("Ctrl+Shift+T");
        assert_eq!(m.add_shortcut(Action::NextTab, taken), Err(Action::NewTab));
        let changes = m.replace_shortcut(Action::NextTab, taken);
        assert_eq!(
            changes,
            set(&["keybindings", "Ctrl+Shift+T"], text("next_tab"))
        );
        let (_, m) = apply("", &changes);
        assert_eq!(m.bound(taken), Some(Action::NextTab));
        assert!(labels(&m, Action::NewTab).is_empty());
        assert_eq!(
            labels(&m, Action::NextTab),
            ["Ctrl+PageDown", "Ctrl+Shift+T"]
        );
    }

    #[test]
    fn shortcuts_follow_a_written_configuration() {
        let mut m = model("");
        let (config, errors) = parse_both(
            "[keybindings]\n\"F7\" = \"detach_pane\"\n",
            &Config::default(),
        );
        m.written(config, errors);
        assert_eq!(labels(&m, Action::DetachPane), ["F7"]);
        m.refresh(&Config::default(), &LoadErrors::default(), Appearance::Dark);
        assert!(labels(&m, Action::DetachPane).is_empty());
    }

    /// A config directory of its own, holding the user themes `names` (each a copy of
    /// `graphene-day` with another background).
    fn theme_dir(test: &str, names: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "graphene-settings-model-{test}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(theme::THEMES_DIR)).unwrap();
        for name in names {
            std::fs::write(
                theme::user_file(&dir, name).unwrap(),
                "[colors]\nbackground = \"#123456\"\n",
            )
            .unwrap();
        }
        dir
    }

    /// The model of `text` with the user themes of `dir`, under `appearance`.
    fn theme_model(text: &str, dir: &Path, appearance: Appearance) -> SettingsModel {
        let (config, errors) = parse_both(text, &Config::default());
        SettingsModel::new(
            &config,
            &LoadErrors {
                config: errors,
                theme: Vec::new(),
            },
            Some(dir.to_path_buf()),
            appearance,
        )
    }

    const FOLLOWING: &str = "[theme]\nname = \"mine\"\nlight = \"mine\"\ndark = \"other\"\n";

    #[test]
    fn the_editor_shows_the_active_theme() {
        let dir = theme_dir("active", &["mine", "other"]);
        let m = theme_model(FOLLOWING, &dir, Appearance::Dark);
        assert_eq!(m.active_theme(), "other");
        assert_eq!(m.edited_theme(), "other");
        assert!(!m.edited_is_built_in());
        assert_eq!(
            m.edited_palette().background,
            Rgb {
                r: 0x12,
                g: 0x34,
                b: 0x56
            }
        );

        let m = theme_model("", &dir, Appearance::Light);
        assert_eq!(m.edited_theme(), theme::DEFAULT_THEME);
        assert!(m.edited_is_built_in());
        assert_eq!(m.edited_palette(), theme::default_palette());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_edited_theme_is_independent_of_the_active_theme() {
        let dir = theme_dir("independent", &["mine"]);
        let mut m = theme_model("", &dir, Appearance::Dark);
        m.edit_theme("mine");
        assert_eq!(m.edited_theme(), "mine");
        assert_eq!(m.active_theme(), theme::DEFAULT_THEME);
        assert!(!m.edited_is_built_in());

        // A reload keeps it and reads its colors again.
        std::fs::write(
            theme::user_file(&dir, "mine").unwrap(),
            "[colors]\nbackground = \"#654321\"\n",
        )
        .unwrap();
        m.refresh(&Config::default(), &LoadErrors::default(), Appearance::Dark);
        assert_eq!(m.edited_theme(), "mine");
        assert_eq!(
            m.edited_palette().background,
            Rgb {
                r: 0x65,
                g: 0x43,
                b: 0x21
            }
        );

        m.edit_theme("graphene-day");
        assert!(m.edited_is_built_in());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_removed_edited_theme_falls_back_to_the_active_theme() {
        let dir = theme_dir("fallback", &["mine", "other"]);
        let mut m = theme_model("[theme]\nname = \"other\"\n", &dir, Appearance::Dark);
        m.edit_theme("mine");
        std::fs::remove_file(theme::user_file(&dir, "mine").unwrap()).unwrap();
        let config = m.config().clone();
        m.written(config, Vec::new());
        assert_eq!(m.edited_theme(), "other");
        assert!(!m.edited_is_built_in());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn set_color_changes_no_configuration() {
        let dir = theme_dir("set-color", &["mine"]);
        let m = theme_model("", &dir, Appearance::Dark);
        let op = ThemeOp::SetColor(ColorKey::SelectionForeground, None);
        assert_eq!(m.theme_op(op.clone()), Ok((Vec::new(), op)));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn duplicating_points_the_active_key_to_the_copy() {
        let dir = theme_dir("duplicate", &["mine", "other"]);
        let copy = ThemeOp::Duplicate("copy".to_owned());

        let m = theme_model("", &dir, Appearance::Dark);
        assert_eq!(
            m.theme_op(copy.clone()),
            Ok((set(&["theme", "name"], text("copy")), copy.clone()))
        );
        for (appearance, key) in [(Appearance::Light, "light"), (Appearance::Dark, "dark")] {
            let m = theme_model(FOLLOWING, &dir, appearance);
            assert_eq!(
                m.theme_op(copy.clone()),
                Ok((set(&["theme", key], text("copy")), copy.clone()))
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn renaming_moves_every_key_naming_the_edited_theme() {
        let dir = theme_dir("rename", &["mine", "other"]);
        let mut m = theme_model(FOLLOWING, &dir, Appearance::Dark);
        m.edit_theme("mine");
        let op = ThemeOp::Rename("renamed".to_owned());
        let (changes, returned) = m.theme_op(op.clone()).unwrap();
        assert_eq!(returned, op);
        assert_eq!(
            changes,
            [
                Change::Set(owned(&["theme", "name"]), text("renamed")),
                Change::Set(owned(&["theme", "light"]), text("renamed")),
            ]
        );

        // A theme no key names changes no configuration.
        let mut m = theme_model("", &dir, Appearance::Dark);
        m.edit_theme("other");
        assert_eq!(m.theme_op(op.clone()), Ok((Vec::new(), op)));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn deleting_sets_the_keys_naming_the_edited_theme_back_to_the_default() {
        let dir = theme_dir("delete", &["mine", "other"]);
        let m = theme_model(FOLLOWING, &dir, Appearance::Dark);
        assert_eq!(m.edited_theme(), "other");
        assert_eq!(
            m.theme_op(ThemeOp::Delete),
            Ok((
                set(&["theme", "dark"], text(theme::DEFAULT_THEME)),
                ThemeOp::Delete
            ))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn new_names_must_be_valid_and_free() {
        let dir = theme_dir("names", &["mine"]);
        let mut m = theme_model("", &dir, Appearance::Dark);
        m.edit_theme("mine");
        for op in [ThemeOp::Duplicate, ThemeOp::Rename] {
            for (name, error) in [
                ("", NameError::Empty),
                ("a/b", NameError::Invalid),
                (".hidden", NameError::Invalid),
                ("mine", NameError::Taken),
                ("graphene-day", NameError::Taken),
            ] {
                assert_eq!(m.theme_op(op(name.to_owned())), Err(error), "{name:?}");
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    const GREEN: Rgb = Rgb {
        r: 0,
        g: 0xff,
        b: 0,
    };

    #[test]
    fn the_picked_color_of_the_active_theme_is_previewed() {
        let dir = theme_dir("preview", &["mine"]);
        let m = theme_model("[theme]\nname = \"mine\"\n", &dir, Appearance::Dark);
        assert_eq!(m.live_preview(None), None);
        let preview = m
            .live_preview(Some((ColorKey::Cursor, GREEN)))
            .expect("the edited theme is active");
        let mut expected = m.edited_palette().clone();
        expected.cursor = GREEN;
        assert_eq!(preview, expected);

        // Once written, the reload loads the same colors.
        crate::theme_edit::set_color(&dir, "mine", ColorKey::Cursor, Some(GREEN)).unwrap();
        assert_eq!(m.active_palette(), preview);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_theme_that_is_not_active_is_not_previewed() {
        let dir = theme_dir("no-preview", &["mine", "other"]);
        let mut m = theme_model("[theme]\nname = \"other\"\n", &dir, Appearance::Dark);
        m.edit_theme("mine");
        assert_eq!(m.live_preview(Some((ColorKey::Cursor, GREEN))), None);
        assert_eq!(m.active_palette(), theme::load("other", Some(&dir)).0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
