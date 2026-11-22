//! macOS side of the global hotkey: a Carbon hot key (`RegisterEventHotKey`), whose
//! presses an event handler of the application target receives. The AppKit loop of
//! winit dispatches the Carbon events, on the main thread where all this runs.

use std::ffi::c_void;
use std::ptr;

use super::{HotkeyError, OnPress};

type OSStatus = i32;
type EventTargetRef = *mut c_void;
type EventHandlerRef = *mut c_void;
type EventHandlerCallRef = *mut c_void;
type EventRef = *mut c_void;
type EventHotKeyRef = *mut c_void;
type EventHandlerUPP = extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OSStatus;

#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[repr(C)]
struct EventHotKeyID {
    signature: u32,
    id: u32,
}

/// `noErr`.
const NO_ERR: OSStatus = 0;
/// `eventHotKeyExistsErr` (`CarbonEventsCore.h`).
const HOT_KEY_EXISTS: OSStatus = -9878;
/// `kEventClassKeyboard`, `'keyb'`.
const EVENT_CLASS_KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
/// `kEventHotKeyPressed`.
const EVENT_HOT_KEY_PRESSED: u32 = 5;
/// Signature of the hot key of the application, `'grph'`.
const SIGNATURE: u32 = u32::from_be_bytes(*b"grph");

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> EventTargetRef;
    fn InstallEventHandler(
        target: EventTargetRef,
        handler: EventHandlerUPP,
        type_count: usize,
        types: *const EventTypeSpec,
        user_data: *mut c_void,
        out: *mut EventHandlerRef,
    ) -> OSStatus;
    fn RegisterEventHotKey(
        code: u32,
        modifiers: u32,
        id: EventHotKeyID,
        target: EventTargetRef,
        options: u32,
        out: *mut EventHotKeyRef,
    ) -> OSStatus;
    fn UnregisterEventHotKey(hot_key: EventHotKeyRef) -> OSStatus;
}

/// The hot key of the application, and the handler of its presses.
pub struct Registration {
    /// The press callback, handed to the handler once installed; it then lives as long
    /// as the process.
    on_press: Option<Box<OnPress>>,
    hot_key: Option<EventHotKeyRef>,
}

impl Registration {
    pub fn new(on_press: OnPress) -> Self {
        Self {
            on_press: Some(Box::new(on_press)),
            hot_key: None,
        }
    }

    /// Registers the virtual key `code` with the Carbon `modifiers`, in place of the
    /// current hot key. `text` names the hotkey in errors.
    pub fn register(&mut self, text: &str, code: u32, modifiers: u32) -> Result<(), HotkeyError> {
        self.unregister();
        self.install_handler()?;
        let mut hot_key = ptr::null_mut();
        let id = EventHotKeyID {
            signature: SIGNATURE,
            id: 1,
        };
        // SAFETY: plain Carbon call on the main thread, `hot_key` outlives it.
        let status = unsafe {
            RegisterEventHotKey(
                code,
                modifiers,
                id,
                GetApplicationEventTarget(),
                0,
                &mut hot_key,
            )
        };
        match status {
            NO_ERR => {
                self.hot_key = Some(hot_key);
                Ok(())
            }
            HOT_KEY_EXISTS => Err(HotkeyError::Taken(text.to_owned())),
            status => Err(HotkeyError::Failed(format!(
                "RegisterEventHotKey returned {status}"
            ))),
        }
    }

    /// Releases the current hot key.
    pub fn unregister(&mut self) {
        if let Some(hot_key) = self.hot_key.take() {
            // SAFETY: a hot key `RegisterEventHotKey` returned, released once.
            let status = unsafe { UnregisterEventHotKey(hot_key) };
            if status != NO_ERR {
                log::debug!("UnregisterEventHotKey returned {status}");
            }
        }
    }

    /// Installs the handler of the hot key presses, once.
    fn install_handler(&mut self) -> Result<(), HotkeyError> {
        let Some(on_press) = self.on_press.take() else {
            return Ok(());
        };
        let spec = EventTypeSpec {
            event_class: EVENT_CLASS_KEYBOARD,
            event_kind: EVENT_HOT_KEY_PRESSED,
        };
        let user_data = Box::into_raw(on_press);
        // SAFETY: `spec` outlives the call; `user_data` is never freed, so it stays
        // valid for every call of the handler.
        let status = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                on_hot_key,
                1,
                &spec,
                user_data.cast(),
                ptr::null_mut(),
            )
        };
        if status != NO_ERR {
            // SAFETY: the handler is not installed: the box is still owned here.
            self.on_press = Some(unsafe { Box::from_raw(user_data) });
            return Err(HotkeyError::Failed(format!(
                "InstallEventHandler returned {status}"
            )));
        }
        Ok(())
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.unregister();
    }
}

/// Handler of `kEventHotKeyPressed`: the application has a single hot key.
extern "C" fn on_hot_key(
    _call: EventHandlerCallRef,
    _event: EventRef,
    user_data: *mut c_void,
) -> OSStatus {
    // SAFETY: the `Box<OnPress>` leaked by `install_handler`, never freed.
    let on_press = unsafe { &*user_data.cast::<OnPress>() };
    on_press();
    NO_ERR
}
