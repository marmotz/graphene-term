//! macOS side of the quick terminal: the mouse cursor, the visible part of a screen and
//! the Spaces behavior of the window, read and set through AppKit on the main thread.

use objc2::rc::Retained;
use objc2_app_kit::{NSEvent, NSScreen, NSView, NSWindowCollectionBehavior};
use objc2_foundation::NSRect;
use wgpu::rwh::{HasWindowHandle, RawWindowHandle};
use winit::monitor::MonitorHandle;
use winit::platform::macos::MonitorHandleExtMacOS;
use winit::window::Window;

use super::Insets;

/// The `NSScreen` of `monitor`.
fn ns_screen(monitor: &MonitorHandle) -> Option<Retained<NSScreen>> {
    let screen = monitor.ns_screen()?;
    // SAFETY: winit hands the pointer of a live `NSScreen`, retained here.
    unsafe { Retained::retain(screen.cast::<NSScreen>()) }
}

fn max_x(rect: NSRect) -> f64 {
    rect.origin.x + rect.size.width
}

fn max_y(rect: NSRect) -> f64 {
    rect.origin.y + rect.size.height
}

/// Position of the mouse cursor in the physical pixels of `monitors`, as winit counts
/// them (from the top-left corner). `None` when it is on none of them.
pub fn cursor_position(monitors: &[MonitorHandle]) -> Option<(i32, i32)> {
    // AppKit counts from the bottom-left corner of the primary screen, in points.
    let point = NSEvent::mouseLocation();
    monitors.iter().find_map(|monitor| {
        let frame = ns_screen(monitor)?.frame();
        let inside = point.x >= frame.origin.x
            && point.x < max_x(frame)
            && point.y > frame.origin.y
            && point.y <= max_y(frame);
        if !inside {
            return None;
        }
        let scale = monitor.scale_factor();
        let origin = monitor.position();
        Some((
            origin.x + ((point.x - frame.origin.x) * scale) as i32,
            origin.y + ((max_y(frame) - point.y) * scale) as i32,
        ))
    })
}

/// Space the menu bar and the Dock take on each side of `monitor`, in its physical
/// pixels.
pub fn insets(monitor: &MonitorHandle) -> Insets {
    let Some(screen) = ns_screen(monitor) else {
        return Insets::default();
    };
    let (frame, visible) = (screen.frame(), screen.visibleFrame());
    let scale = monitor.scale_factor();
    let pixels = |points: f64| (points.max(0.0) * scale).round() as u32;
    Insets {
        top: pixels(max_y(frame) - max_y(visible)),
        bottom: pixels(visible.origin.y - frame.origin.y),
        left: pixels(visible.origin.x - frame.origin.x),
        right: pixels(max_x(frame) - max_x(visible)),
    }
}

/// Shows `window` on the current Space and above full screen applications.
pub fn join_all_spaces(window: &Window) {
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // SAFETY: the view of a live winit window, used on the main thread.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    if let Some(ns_window) = view.window() {
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
    }
}
