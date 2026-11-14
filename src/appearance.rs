//! Light/dark: follows the system appearance through the XDG desktop portal.
//!
//! winit 0.30 does not report the theme on X11 or Wayland, so the `color-scheme` setting
//! of `org.freedesktop.portal.Settings` is read, then watched through its
//! `SettingChanged` signal, on a thread of its own.

use futures_util::StreamExt;
use zbus::Variant;
use zbus::wire::OwnedVariant;

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const NAMESPACE: &str = "org.freedesktop.appearance";
const KEY: &str = "color-scheme";

/// The system appearance. Without portal, or without preference, it is `Dark`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Appearance {
    Light,
    #[default]
    Dark,
}

impl Appearance {
    /// The appearance of a portal `color-scheme` value: 1 dark, 2 light, 0 (no
    /// preference) or anything else dark.
    pub fn from_color_scheme(value: u32) -> Self {
        match value {
            2 => Self::Light,
            _ => Self::Dark,
        }
    }
}

/// The `u32` inside a setting value. `Read`, the method of portals older than
/// `ReadOne`, wraps it in one more variant.
fn color_scheme(value: Variant<'_>) -> Option<u32> {
    match value {
        Variant::Variant(inner) => color_scheme(*inner),
        Variant::U32(value) => Some(value),
        _ => None,
    }
}

/// Starts the thread that reports the system appearance: once at startup, then on each
/// change. Without portal, nothing is reported and the default `Dark` stays.
pub fn spawn(on_change: impl Fn(Appearance) + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("appearance".into())
        .spawn(move || {
            if let Err(error) = zbus::block_on(watch(&on_change)) {
                log::info!("system appearance unavailable ({error}): dark theme");
            }
        });
    if let Err(error) = spawned {
        log::warn!("cannot start the appearance watcher: {error}");
    }
}

/// Reads the current appearance, then reports each change until the bus closes.
async fn watch(on_change: &impl Fn(Appearance)) -> zbus::Result<()> {
    let connection = zbus::Connection::session().await?;
    let proxy = zbus::Proxy::new(&connection, PORTAL, PORTAL_PATH, SETTINGS).await?;
    // Subscribed before the first read, so a change in between is not missed.
    let mut changes = proxy
        .receive_signal_with_args("SettingChanged", &[(0, NAMESPACE), (1, KEY)])
        .await?;

    let value: OwnedVariant = match proxy.call("ReadOne", &(NAMESPACE, KEY)).await {
        Ok(value) => value,
        Err(_) => proxy.call("Read", &(NAMESPACE, KEY)).await?,
    };
    report(value.into(), on_change);

    while let Some(message) = changes.next().await {
        match message
            .body()
            .deserialize::<(String, String, OwnedVariant)>()
        {
            Ok((namespace, key, value)) if namespace == NAMESPACE && key == KEY => {
                report(value.into(), on_change);
            }
            Ok(_) => {}
            Err(error) => log::warn!("unreadable appearance change: {error}"),
        }
    }
    Ok(())
}

fn report(value: Variant<'_>, on_change: &impl Fn(Appearance)) {
    match color_scheme(value) {
        Some(value) => on_change(Appearance::from_color_scheme(value)),
        None => log::warn!("unexpected type for the {NAMESPACE} {KEY} setting"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_scheme_values() {
        assert_eq!(Appearance::from_color_scheme(0), Appearance::Dark);
        assert_eq!(Appearance::from_color_scheme(1), Appearance::Dark);
        assert_eq!(Appearance::from_color_scheme(2), Appearance::Light);
        assert_eq!(Appearance::from_color_scheme(7), Appearance::Dark);
    }

    #[test]
    fn the_default_is_dark() {
        assert_eq!(Appearance::default(), Appearance::Dark);
    }

    #[test]
    fn color_scheme_is_read_from_a_plain_or_nested_variant() {
        assert_eq!(color_scheme(Variant::U32(2)), Some(2));
        let nested = Variant::Variant(Box::new(Variant::Variant(Box::new(Variant::U32(1)))));
        assert_eq!(color_scheme(nested), Some(1));
        assert_eq!(color_scheme(Variant::Bool(true)), None);
    }

    /// Needs a session bus with the desktop portal: `cargo test -- --ignored portal`.
    #[test]
    #[ignore = "needs the XDG desktop portal"]
    fn portal_reports_the_current_appearance() {
        let (sender, receiver) = std::sync::mpsc::channel();
        spawn(move |appearance| {
            let _ = sender.send(appearance);
        });
        let appearance = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("no appearance from the portal");
        eprintln!("system appearance: {appearance:?}");
    }
}
