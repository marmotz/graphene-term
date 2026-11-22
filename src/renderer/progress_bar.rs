//! Drawing of a progress bar (pure, no GPU access): its translucent track and the
//! filled part over it, shared by the tabs, the pane bars and the block cards.

use super::rects::RectInstance;
use super::scene::gpu_color;
use crate::layout::Rect;
use crate::progress::{self, PROGRESS_TRACK_OPACITY, ProgressColors, ProgressView};

/// The track filling `rect`, then the part of it `view` fills, in the color of its
/// state.
pub fn progress_rects(
    rect: Rect,
    view: ProgressView,
    colors: &ProgressColors,
    srgb_target: bool,
) -> [RectInstance; 2] {
    let [r, g, b, _] = gpu_color(colors.track, srgb_target);
    let track = RectInstance::new(
        [rect.x, rect.y],
        [rect.width, rect.height],
        [r, g, b, PROGRESS_TRACK_OPACITY],
    );
    let (x, width) = progress::filled_span(rect.x, rect.width, view);
    let bar = RectInstance::new(
        [x, rect.y],
        [width, rect.height],
        gpu_color(colors.bar(view.state), srgb_target),
    );
    [track, bar]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::ProgressState;
    use alacritty_terminal::vte::ansi::Rgb;

    const RECT: Rect = Rect {
        x: 10.0,
        y: 40.0,
        width: 200.0,
        height: 2.0,
    };

    fn colors() -> ProgressColors {
        let gray = |v| Rgb { r: v, g: v, b: v };
        ProgressColors {
            normal: gray(10),
            error: gray(20),
            paused: gray(30),
            track: gray(255),
        }
    }

    fn view(state: ProgressState, value: u8) -> ProgressView {
        ProgressView {
            state,
            value,
            offset: 0.5,
        }
    }

    #[test]
    fn track_covers_the_band_translucently() {
        let [track, _] = progress_rects(RECT, view(ProgressState::Normal, 50), &colors(), false);
        assert_eq!(track.position, [10.0, 40.0]);
        assert_eq!(track.size, [200.0, 2.0]);
        assert_eq!(track.color, [1.0, 1.0, 1.0, PROGRESS_TRACK_OPACITY]);
    }

    #[test]
    fn bar_fills_up_to_the_value_in_the_state_color() {
        let gray = |v: u8| gpu_color(Rgb { r: v, g: v, b: v }, false);
        for (state, color) in [
            (ProgressState::Normal, gray(10)),
            (ProgressState::Error, gray(20)),
            (ProgressState::Paused, gray(30)),
        ] {
            let [_, bar] = progress_rects(RECT, view(state, 50), &colors(), false);
            assert_eq!(bar.position, [10.0, 40.0]);
            assert_eq!(bar.size, [100.0, 2.0]);
            assert_eq!(bar.color, color);
        }
    }

    #[test]
    fn indeterminate_bar_is_the_animated_segment_in_the_accent() {
        let [_, bar] = progress_rects(
            RECT,
            view(ProgressState::Indeterminate, 0),
            &colors(),
            false,
        );
        assert_eq!(bar.position, [85.0, 40.0]);
        assert_eq!(bar.size, [50.0, 2.0]);
        assert_eq!(bar.color, gpu_color(colors().normal, false));
    }
}
