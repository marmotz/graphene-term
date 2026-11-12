//! Byte encoding of mouse, focus and alternate scroll events sent to the application
//! (xterm `ctlseqs`). No window, clipboard or `Term` access.

use alacritty_terminal::term::TermMode;
use winit::keyboard::ModifiersState;

/// Highest 1-based coordinate the X10 encoding can carry (`32 + 223 = 255`).
const X10_MAX_COORDINATE: usize = 223;

/// Mouse button reported to the application.
#[allow(dead_code, reason = "reported by the click routing task")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
}

/// Mouse event reported to the application.
#[allow(dead_code, reason = "reported by the click and wheel routing tasks")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEvent {
    Press(Button),
    Release(Button),
    /// Motion with the held button, if any.
    Motion(Option<Button>),
    /// One wheel step: `true` = up.
    Wheel(bool),
}

impl Button {
    fn code(self) -> u8 {
        match self {
            Button::Left => 0,
            Button::Middle => 1,
            Button::Right => 2,
        }
    }
}

/// Bytes for `event` at viewport cell (`column`, `line`), 0-based. `None` when the
/// current mode does not report this event, or when X10 cannot encode the cell.
#[allow(dead_code, reason = "called by the click and wheel routing tasks")]
pub fn encode(
    event: MouseEvent,
    column: usize,
    line: usize,
    mods: ModifiersState,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let reported = match event {
        MouseEvent::Press(_) | MouseEvent::Release(_) | MouseEvent::Wheel(_) => {
            mode.intersects(TermMode::MOUSE_MODE)
        }
        MouseEvent::Motion(Some(_)) => {
            mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION)
        }
        MouseEvent::Motion(None) => mode.contains(TermMode::MOUSE_MOTION),
    };
    if !reported {
        return None;
    }

    let mut code = match event {
        MouseEvent::Press(button) | MouseEvent::Release(button) => button.code(),
        MouseEvent::Motion(Some(button)) => button.code() + 32,
        MouseEvent::Motion(None) => 3 + 32,
        MouseEvent::Wheel(true) => 64,
        MouseEvent::Wheel(false) => 65,
    };
    // Shift (+4) is never set: a Shift event never reaches the application.
    if mods.alt_key() {
        code += 8;
    }
    if mods.control_key() {
        code += 16;
    }

    let x = column + 1;
    let y = line + 1;
    let release = matches!(event, MouseEvent::Release(_));

    if mode.contains(TermMode::SGR_MOUSE) {
        let last = if release { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{x};{y}{last}").into_bytes());
    }

    if x > X10_MAX_COORDINATE || y > X10_MAX_COORDINATE {
        return None;
    }
    // X10 cannot tell which button was released: release is always 3, modifiers kept.
    if release {
        code = (code & !0b11) | 3;
    }
    Some(vec![
        0x1b,
        b'[',
        b'M',
        32 + code,
        32 + x as u8,
        32 + y as u8,
    ])
}

/// `CSI I` / `CSI O` when `FOCUS_IN_OUT` is set.
pub fn focus_report(focused: bool, mode: TermMode) -> Option<&'static [u8]> {
    if !mode.contains(TermMode::FOCUS_IN_OUT) {
        return None;
    }
    Some(if focused { b"\x1b[I" } else { b"\x1b[O" })
}

/// Arrow keys sent for `lines` wheel steps in alternate scroll: up when positive.
pub fn alternate_scroll_bytes(lines: i32, mode: TermMode) -> Vec<u8> {
    let arrow: &[u8] = match (lines > 0, mode.contains(TermMode::APP_CURSOR)) {
        (true, true) => b"\x1bOA",
        (true, false) => b"\x1b[A",
        (false, true) => b"\x1bOB",
        (false, false) => b"\x1b[B",
    };
    arrow.repeat(lines.unsigned_abs() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: ModifiersState = ModifiersState::empty();

    fn sgr(mode: TermMode) -> TermMode {
        mode | TermMode::SGR_MOUSE
    }

    #[test]
    fn sgr_press_and_release_use_real_button() {
        let mode = sgr(TermMode::MOUSE_REPORT_CLICK);
        assert_eq!(
            encode(MouseEvent::Press(Button::Left), 4, 9, NONE, mode),
            Some(b"\x1b[<0;5;10M".to_vec())
        );
        assert_eq!(
            encode(MouseEvent::Release(Button::Right), 4, 9, NONE, mode),
            Some(b"\x1b[<2;5;10m".to_vec())
        );
        assert_eq!(
            encode(MouseEvent::Press(Button::Middle), 0, 0, NONE, mode),
            Some(b"\x1b[<1;1;1M".to_vec())
        );
    }

    #[test]
    fn x10_press_and_release() {
        let mode = TermMode::MOUSE_REPORT_CLICK;
        assert_eq!(
            encode(MouseEvent::Press(Button::Right), 0, 1, NONE, mode),
            Some(vec![0x1b, b'[', b'M', 32 + 2, 33, 34])
        );
        assert_eq!(
            encode(MouseEvent::Release(Button::Right), 0, 1, NONE, mode),
            Some(vec![0x1b, b'[', b'M', 32 + 3, 33, 34])
        );
    }

    #[test]
    fn wheel_up_and_down() {
        let mode = sgr(TermMode::MOUSE_REPORT_CLICK);
        assert_eq!(
            encode(MouseEvent::Wheel(true), 2, 3, NONE, mode),
            Some(b"\x1b[<64;3;4M".to_vec())
        );
        assert_eq!(
            encode(MouseEvent::Wheel(false), 2, 3, NONE, mode),
            Some(b"\x1b[<65;3;4M".to_vec())
        );
    }

    #[test]
    fn nothing_reported_without_mouse_mode() {
        let mode = TermMode::SGR_MOUSE;
        assert_eq!(
            encode(MouseEvent::Press(Button::Left), 0, 0, NONE, mode),
            None
        );
        assert_eq!(
            encode(MouseEvent::Release(Button::Left), 0, 0, NONE, mode),
            None
        );
        assert_eq!(encode(MouseEvent::Wheel(true), 0, 0, NONE, mode), None);
    }

    #[test]
    fn motion_filtered_per_mode() {
        let drag = MouseEvent::Motion(Some(Button::Left));
        let hover = MouseEvent::Motion(None);

        let click = sgr(TermMode::MOUSE_REPORT_CLICK);
        assert_eq!(encode(drag, 0, 0, NONE, click), None);
        assert_eq!(encode(hover, 0, 0, NONE, click), None);

        let button_motion = sgr(TermMode::MOUSE_DRAG);
        assert_eq!(
            encode(drag, 0, 0, NONE, button_motion),
            Some(b"\x1b[<32;1;1M".to_vec())
        );
        assert_eq!(encode(hover, 0, 0, NONE, button_motion), None);

        let any_motion = sgr(TermMode::MOUSE_MOTION);
        assert_eq!(
            encode(drag, 0, 0, NONE, any_motion),
            Some(b"\x1b[<32;1;1M".to_vec())
        );
        assert_eq!(
            encode(hover, 0, 0, NONE, any_motion),
            Some(b"\x1b[<35;1;1M".to_vec())
        );
    }

    #[test]
    fn alt_and_ctrl_bits_never_shift() {
        let mode = sgr(TermMode::MOUSE_REPORT_CLICK);
        let press = MouseEvent::Press(Button::Left);
        assert_eq!(
            encode(press, 0, 0, ModifiersState::ALT, mode),
            Some(b"\x1b[<8;1;1M".to_vec())
        );
        assert_eq!(
            encode(press, 0, 0, ModifiersState::CONTROL, mode),
            Some(b"\x1b[<16;1;1M".to_vec())
        );
        assert_eq!(
            encode(press, 0, 0, ModifiersState::SHIFT, mode),
            Some(b"\x1b[<0;1;1M".to_vec())
        );
        assert_eq!(
            encode(
                press,
                0,
                0,
                ModifiersState::ALT | ModifiersState::CONTROL | ModifiersState::SHIFT,
                mode
            ),
            Some(b"\x1b[<24;1;1M".to_vec())
        );
    }

    #[test]
    fn x10_release_keeps_modifiers() {
        assert_eq!(
            encode(
                MouseEvent::Release(Button::Middle),
                0,
                0,
                ModifiersState::CONTROL,
                TermMode::MOUSE_REPORT_CLICK
            ),
            Some(vec![0x1b, b'[', b'M', 32 + 16 + 3, 33, 33])
        );
    }

    #[test]
    fn x10_coordinate_limit() {
        let mode = TermMode::MOUSE_REPORT_CLICK;
        let press = MouseEvent::Press(Button::Left);
        assert_eq!(
            encode(press, 222, 222, NONE, mode),
            Some(vec![0x1b, b'[', b'M', 32, 255, 255])
        );
        assert_eq!(encode(press, 223, 0, NONE, mode), None);
        assert_eq!(encode(press, 0, 223, NONE, mode), None);
        // SGR has no such limit.
        assert_eq!(
            encode(press, 223, 0, NONE, sgr(mode)),
            Some(b"\x1b[<0;224;1M".to_vec())
        );
    }

    #[test]
    fn focus_report_needs_focus_mode() {
        assert_eq!(focus_report(true, TermMode::empty()), None);
        assert_eq!(focus_report(false, TermMode::empty()), None);
        assert_eq!(
            focus_report(true, TermMode::FOCUS_IN_OUT),
            Some(&b"\x1b[I"[..])
        );
        assert_eq!(
            focus_report(false, TermMode::FOCUS_IN_OUT),
            Some(&b"\x1b[O"[..])
        );
    }

    #[test]
    fn alternate_scroll_arrows() {
        assert_eq!(
            alternate_scroll_bytes(2, TermMode::empty()),
            b"\x1b[A\x1b[A".to_vec()
        );
        assert_eq!(
            alternate_scroll_bytes(-1, TermMode::empty()),
            b"\x1b[B".to_vec()
        );
        assert_eq!(
            alternate_scroll_bytes(1, TermMode::APP_CURSOR),
            b"\x1bOA".to_vec()
        );
        assert_eq!(
            alternate_scroll_bytes(-2, TermMode::APP_CURSOR),
            b"\x1bOB\x1bOB".to_vec()
        );
    }
}
