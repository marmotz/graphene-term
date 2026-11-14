//! Byte encoding of mouse, focus and alternate scroll events sent to the application
//! (xterm `ctlseqs`), and the routing of the mouse between the application and
//! Graphene. No window, clipboard or `Term` access.

use alacritty_terminal::term::TermMode;
use winit::keyboard::ModifiersState;
use winit::window::CursorIcon;

use crate::layout::Hit;
use crate::timeline::BlockId;
use crate::view::{RowSource, ScrollTarget};
use crate::workspace::tree::Axis;

/// Highest 1-based coordinate the X10 encoding can carry (`32 + 223 = 255`).
const X10_MAX_COORDINATE: usize = 223;

/// Mouse button reported to the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
}

/// Mouse event reported to the application.
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

/// Mouse button of a winit event, `None` for the extra buttons (back, forward...).
pub fn button_from(button: winit::event::MouseButton) -> Option<Button> {
    match button {
        winit::event::MouseButton::Left => Some(Button::Left),
        winit::event::MouseButton::Middle => Some(Button::Middle),
        winit::event::MouseButton::Right => Some(Button::Right),
        _ => None,
    }
}

/// Gesture owned by the application: the button pressed while mouse reporting was
/// reserved, kept until its release, and the last cell reported so motion is only sent
/// when the cell changes. The owner is fixed at press: `Shift` held mid-drag changes
/// nothing.
#[derive(Debug, Default)]
pub struct Reporter {
    button: Option<Button>,
    last_cell: Option<(usize, usize)>,
}

impl Reporter {
    /// Whether the application owns the gesture of `button`.
    pub fn owns(&self, button: Button) -> bool {
        self.button == Some(button)
    }

    /// Whether an application gesture is in progress.
    pub fn active(&self) -> bool {
        self.button.is_some()
    }

    /// Starts an application gesture at viewport cell `cell` (column, line).
    pub fn press(
        &mut self,
        button: Button,
        cell: (usize, usize),
        mods: ModifiersState,
        mode: TermMode,
    ) -> Option<Vec<u8>> {
        self.button = Some(button);
        self.last_cell = Some(cell);
        encode(MouseEvent::Press(button), cell.0, cell.1, mods, mode)
    }

    /// Ends the application gesture. Dropped by `encode` if the application left mouse
    /// mode meanwhile.
    pub fn release(
        &mut self,
        button: Button,
        cell: (usize, usize),
        mods: ModifiersState,
        mode: TermMode,
    ) -> Option<Vec<u8>> {
        self.button = None;
        self.last_cell = Some(cell);
        encode(MouseEvent::Release(button), cell.0, cell.1, mods, mode)
    }

    /// Motion to `cell`, with the held application button if any. Nothing when the cell
    /// did not change.
    pub fn motion(
        &mut self,
        cell: (usize, usize),
        mods: ModifiersState,
        mode: TermMode,
    ) -> Option<Vec<u8>> {
        if self.last_cell == Some(cell) {
            return None;
        }
        self.last_cell = Some(cell);
        encode(MouseEvent::Motion(self.button), cell.0, cell.1, mods, mode)
    }
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

/// Bytes sent to the application for `lines` wheel steps (up when positive) at viewport
/// cell (`column`, `line`), 0-based. `None` when the wheel must scroll the history instead.
///
/// In order: mouse mode without `Shift` reports one wheel event per line; the alternate
/// screen with `ALTERNATE_SCROLL` and no mouse mode gets arrow keys; anything else
/// (including `Shift` + wheel in mouse mode) scrolls the history.
pub fn wheel_bytes(
    lines: i32,
    column: usize,
    line: usize,
    mods: ModifiersState,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if mode.intersects(TermMode::MOUSE_MODE) {
        if mods.shift_key() {
            return None;
        }
        let step = encode(MouseEvent::Wheel(lines > 0), column, line, mods, mode)?;
        return Some(step.repeat(lines.unsigned_abs() as usize));
    }
    if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
        return Some(alternate_scroll_bytes(lines, mode));
    }
    None
}

/// Whether a mouse event over `hit` goes to the application when it captures the
/// mouse: over the live grid's rows only, where the application draws (the whole window
/// on the alternate screen). Everywhere else the mouse is Graphene's.
pub fn reports_at(hit: &Hit, alternate: bool) -> bool {
    alternate
        || matches!(
            hit,
            Hit::Cell {
                source: RowSource::Live,
                ..
            }
        )
}

/// What a wheel step over `hit` scrolls, when the application does not take it: the
/// output of the block under the pointer when it can scroll (`scrollable`), otherwise
/// (short block, gap, margin, chevron, blank space) the timeline.
pub fn wheel_target(hit: &Hit, scrollable: impl Fn(BlockId) -> bool) -> ScrollTarget {
    match hit {
        Hit::Cell {
            block: Some(id), ..
        } if scrollable(*id) => ScrollTarget::Block(*id),
        _ => ScrollTarget::Timeline,
    }
}

/// Pointer shape over a divider between panes split along `axis`, or while dragging
/// it: the arrows across the divider.
pub fn divider_icon(axis: Axis) -> CursorIcon {
    match axis {
        Axis::Horizontal => CursorIcon::ColResize,
        Axis::Vertical => CursorIcon::RowResize,
    }
}

/// Pointer shape: the hand over a fold chevron (always Graphene's, even while the
/// application captures the mouse), the arrow while the application captures the
/// mouse, the I-beam (text selection) otherwise.
pub fn pointer_icon(mode: TermMode, over_chevron: bool) -> CursorIcon {
    if over_chevron {
        CursorIcon::Pointer
    } else if mode.intersects(TermMode::MOUSE_MODE) {
        CursorIcon::Default
    } else {
        CursorIcon::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divider_icon_follows_the_axis() {
        assert_eq!(divider_icon(Axis::Horizontal), CursorIcon::ColResize);
        assert_eq!(divider_icon(Axis::Vertical), CursorIcon::RowResize);
    }

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

    #[test]
    fn button_from_maps_main_buttons_only() {
        use winit::event::MouseButton;
        assert_eq!(button_from(MouseButton::Left), Some(Button::Left));
        assert_eq!(button_from(MouseButton::Middle), Some(Button::Middle));
        assert_eq!(button_from(MouseButton::Right), Some(Button::Right));
        assert_eq!(button_from(MouseButton::Back), None);
    }

    #[test]
    fn reporter_press_drag_release() {
        let mode = sgr(TermMode::MOUSE_DRAG);
        let mut r = Reporter::default();
        assert!(!r.active());
        assert_eq!(
            r.press(Button::Left, (1, 2), NONE, mode),
            Some(b"\x1b[<0;2;3M".to_vec())
        );
        assert!(r.owns(Button::Left));
        assert!(!r.owns(Button::Right));
        // Same cell: nothing.
        assert_eq!(r.motion((1, 2), NONE, mode), None);
        assert_eq!(
            r.motion((2, 2), NONE, mode),
            Some(b"\x1b[<32;3;3M".to_vec())
        );
        assert_eq!(
            r.release(Button::Left, (2, 2), NONE, mode),
            Some(b"\x1b[<0;3;3m".to_vec())
        );
        assert!(!r.active());
    }

    #[test]
    fn reporter_motion_without_button_needs_any_motion_mode() {
        let mut r = Reporter::default();
        assert_eq!(r.motion((0, 0), NONE, sgr(TermMode::MOUSE_DRAG)), None);
        assert_eq!(
            r.motion((1, 0), NONE, sgr(TermMode::MOUSE_MOTION)),
            Some(b"\x1b[<35;2;1M".to_vec())
        );
    }

    #[test]
    fn reporter_release_dropped_after_mouse_mode_left() {
        let mut r = Reporter::default();
        r.press(
            Button::Right,
            (0, 0),
            NONE,
            sgr(TermMode::MOUSE_REPORT_CLICK),
        );
        assert_eq!(
            r.release(Button::Right, (0, 0), NONE, TermMode::empty()),
            None
        );
        assert!(!r.active());
    }

    #[test]
    fn reporter_owner_kept_when_shift_pressed_mid_drag() {
        let mode = sgr(TermMode::MOUSE_DRAG);
        let mut r = Reporter::default();
        r.press(Button::Left, (0, 0), NONE, mode);
        // Shift held during the drag: still reported, Shift bit never set.
        assert_eq!(
            r.motion((1, 0), ModifiersState::SHIFT, mode),
            Some(b"\x1b[<32;2;1M".to_vec())
        );
        assert!(r.owns(Button::Left));
    }

    #[test]
    fn wheel_reported_once_per_line_in_mouse_mode() {
        let mode = sgr(TermMode::MOUSE_REPORT_CLICK);
        assert_eq!(
            wheel_bytes(2, 0, 0, NONE, mode),
            Some(b"\x1b[<64;1;1M\x1b[<64;1;1M".to_vec())
        );
        assert_eq!(
            wheel_bytes(-1, 4, 2, NONE, mode),
            Some(b"\x1b[<65;5;3M".to_vec())
        );
    }

    #[test]
    fn wheel_reported_before_alternate_scroll() {
        let mode =
            sgr(TermMode::MOUSE_REPORT_CLICK) | TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
        assert_eq!(
            wheel_bytes(1, 0, 0, NONE, mode),
            Some(b"\x1b[<64;1;1M".to_vec())
        );
    }

    #[test]
    fn shift_wheel_in_mouse_mode_scrolls_history() {
        let mode =
            sgr(TermMode::MOUSE_REPORT_CLICK) | TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
        assert_eq!(wheel_bytes(1, 0, 0, ModifiersState::SHIFT, mode), None);
    }

    #[test]
    fn wheel_arrows_on_alternate_screen_without_mouse_mode() {
        let mode = TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
        assert_eq!(
            wheel_bytes(2, 0, 0, NONE, mode),
            Some(b"\x1b[A\x1b[A".to_vec())
        );
        assert_eq!(
            wheel_bytes(2, 0, 0, ModifiersState::SHIFT, mode),
            Some(b"\x1b[A\x1b[A".to_vec())
        );
    }

    #[test]
    fn wheel_scrolls_history_otherwise() {
        assert_eq!(wheel_bytes(1, 0, 0, NONE, TermMode::empty()), None);
        assert_eq!(wheel_bytes(1, 0, 0, NONE, TermMode::ALT_SCREEN), None);
    }

    #[test]
    fn wheel_out_of_x10_range_scrolls_history() {
        let mode = TermMode::MOUSE_REPORT_CLICK;
        assert_eq!(wheel_bytes(1, 300, 0, NONE, mode), None);
    }

    #[test]
    fn pointer_is_arrow_while_the_application_captures_the_mouse() {
        assert_eq!(pointer_icon(TermMode::empty(), false), CursorIcon::Text);
        assert_eq!(pointer_icon(TermMode::ALT_SCREEN, false), CursorIcon::Text);
        for mode in [
            TermMode::MOUSE_REPORT_CLICK,
            TermMode::MOUSE_DRAG,
            TermMode::MOUSE_MOTION,
            TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE,
        ] {
            assert_eq!(pointer_icon(mode, false), CursorIcon::Default);
        }
    }

    #[test]
    fn pointer_is_a_hand_over_a_fold_chevron_even_while_the_application_captures_the_mouse() {
        assert_eq!(pointer_icon(TermMode::empty(), true), CursorIcon::Pointer);
        assert_eq!(
            pointer_icon(TermMode::MOUSE_REPORT_CLICK, true),
            CursorIcon::Pointer
        );
    }

    fn cell(source: RowSource, block: Option<u64>) -> Hit {
        Hit::Cell {
            column: 0,
            row: 0,
            side: alacritty_terminal::index::Side::Left,
            source,
            line: alacritty_terminal::index::Line(0),
            block: block.map(BlockId),
        }
    }

    #[test]
    fn mouse_is_reported_over_the_live_rows_only() {
        assert!(reports_at(&cell(RowSource::Live, Some(3)), false));
        for hit in [
            cell(RowSource::Header(BlockId(3)), Some(3)),
            cell(RowSource::Output(BlockId(2)), Some(2)),
            cell(RowSource::Summary(BlockId(2)), Some(2)),
            Hit::Margin { row: 0 },
            Hit::Chevron { row: 0 },
            Hit::Gap,
            Hit::Blank,
        ] {
            assert!(!reports_at(&hit, false), "{hit:?}");
        }
    }

    #[test]
    fn mouse_is_reported_over_the_whole_window_on_the_alternate_screen() {
        for hit in [
            cell(RowSource::Live, None),
            Hit::Margin { row: 0 },
            Hit::Blank,
        ] {
            assert!(reports_at(&hit, true), "{hit:?}");
        }
    }

    #[test]
    fn wheel_scrolls_the_block_under_the_pointer_when_it_can_scroll() {
        let scrollable = |id: BlockId| id == BlockId(2);
        for source in [
            RowSource::Header(BlockId(2)),
            RowSource::Output(BlockId(2)),
            RowSource::Live,
        ] {
            assert_eq!(
                wheel_target(&cell(source, Some(2)), scrollable),
                ScrollTarget::Block(BlockId(2))
            );
        }
    }

    #[test]
    fn wheel_scrolls_the_timeline_elsewhere() {
        let scrollable = |id: BlockId| id == BlockId(2);
        for hit in [
            // A short block.
            cell(RowSource::Output(BlockId(1)), Some(1)),
            // The alternate screen: no block.
            cell(RowSource::Live, None),
            Hit::Margin { row: 0 },
            Hit::Chevron { row: 0 },
            Hit::Gap,
            Hit::Blank,
        ] {
            assert_eq!(
                wheel_target(&hit, scrollable),
                ScrollTarget::Timeline,
                "{hit:?}"
            );
        }
    }
}
