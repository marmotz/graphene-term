//! The honeycomb shown over the active pane when the Konami code is typed: graphene's
//! hexagons appear in a wave from the center of the pane, stay a moment, then fade
//! out together.

use std::time::{Duration, Instant};

use crate::layout::Rect;
use crate::renderer::RectInstance;

/// How long the wave takes to reach the farthest hexagons.
const WAVE: Duration = Duration::from_millis(600);
/// How long the hexagons all stay once shown.
const HOLD: Duration = Duration::from_millis(400);
/// How long they take to fade out.
const FADE_OUT: Duration = Duration::from_millis(600);
/// The whole animation.
pub const DURATION: Duration = Duration::from_millis(1600);
/// How long one hexagon takes to appear, within the wave.
const FADE_IN: Duration = Duration::from_millis(200);

/// Side of a hexagon, in logical pixels.
const SIDE: f32 = 24.0;
/// Width of its outline, in logical pixels.
const STROKE: f32 = 1.5;

/// Time between two frames of the animation.
const FRAME: Duration = Duration::from_millis(16);

/// A running honeycomb animation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Honeycomb {
    started: Instant,
    /// Area covered, in physical pixels: the active pane when it started.
    area: Rect,
}

impl Honeycomb {
    pub fn new(started: Instant, area: Rect) -> Self {
        Self { started, area }
    }

    /// The hexagon outlines at `now`, drawn in `color` at the window's `scale`. Pointy
    /// top hexagons pave the area, rows shifted by half a width, the paving centered;
    /// only whole hexagons are kept. Empty once the animation is over.
    pub fn rects(&self, now: Instant, scale: f32, color: [f32; 4]) -> Vec<RectInstance> {
        let elapsed = now.saturating_duration_since(self.started);
        if elapsed >= DURATION {
            return Vec::new();
        }
        let side = SIDE * scale;
        let width = 3.0_f32.sqrt() * side;
        let height = 2.0 * side;
        let step = 1.5 * side;
        let columns = ((self.area.width - width / 2.0) / width).floor();
        let rows = ((self.area.height - side / 2.0) / step).floor();
        if columns < 1.0 || rows < 1.0 {
            return Vec::new();
        }
        let left = self.area.x + (self.area.width - (columns * width + width / 2.0)) / 2.0;
        let top = self.area.y + (self.area.height - (rows * step + side / 2.0)) / 2.0;
        let center = [
            self.area.x + self.area.width / 2.0,
            self.area.y + self.area.height / 2.0,
        ];
        let boxes: Vec<[f32; 2]> = (0..rows as usize)
            .flat_map(|row| {
                let shift = if row % 2 == 1 { width / 2.0 } else { 0.0 };
                (0..columns as usize).map(move |column| {
                    [
                        left + shift + column as f32 * width,
                        top + row as f32 * step,
                    ]
                })
            })
            .collect();
        let distance = |position: &[f32; 2]| {
            let dx = position[0] + width / 2.0 - center[0];
            let dy = position[1] + height / 2.0 - center[1];
            (dx * dx + dy * dy).sqrt()
        };
        let farthest = boxes.iter().map(distance).fold(1.0, f32::max);
        let elapsed = elapsed.as_secs_f32();
        let stroke = (STROKE * scale).max(1.0);
        boxes
            .iter()
            .map(|position| {
                let delay = (WAVE - FADE_IN).as_secs_f32() * distance(position) / farthest;
                let mut color = color;
                color[3] *= alpha(elapsed, delay);
                RectInstance::new(*position, [width, height], color).hexagon(stroke)
            })
            .collect()
    }

    /// When the next frame is due, `None` once the animation is over.
    pub fn deadline(&self, now: Instant) -> Option<Instant> {
        (now.saturating_duration_since(self.started) < DURATION).then(|| now + FRAME)
    }
}

/// Opacity of a hexagon appearing `delay` seconds into the wave, `elapsed` seconds into
/// the animation.
fn alpha(elapsed: f32, delay: f32) -> f32 {
    let fade_out_start = (WAVE + HOLD).as_secs_f32();
    let shown = ((elapsed - delay) / FADE_IN.as_secs_f32()).clamp(0.0, 1.0);
    let left = (1.0 - (elapsed - fade_out_start) / FADE_OUT.as_secs_f32()).clamp(0.0, 1.0);
    shown.min(left)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLOR: [f32; 4] = [0.2, 0.4, 0.6, 1.0];

    fn area() -> Rect {
        Rect {
            x: 10.0,
            y: 20.0,
            width: 400.0,
            height: 300.0,
        }
    }

    #[test]
    fn the_phases_add_up_to_the_duration() {
        assert_eq!(WAVE + HOLD + FADE_OUT, DURATION);
    }

    #[test]
    fn whole_hexagons_pave_the_area() {
        let started = Instant::now();
        let honeycomb = Honeycomb::new(started, area());
        let rects = honeycomb.rects(started + WAVE, 1.0, COLOR);
        // 41.57 px wide: 9 columns plus the half width of the shifted rows. Rows every
        // 36 px, 48 px high: 8 rows.
        assert_eq!(rects.len(), 9 * 8);
        assert_eq!(rects[0].size, [3.0_f32.sqrt() * 24.0, 48.0]);
        let area = area();
        let hexagon = RectInstance::new([0.0; 2], [1.0; 2], COLOR)
            .hexagon(1.0)
            .shape;
        for rect in &rects {
            assert_eq!(rect.shape, hexagon);
            assert!(rect.position[0] >= area.x && rect.position[1] >= area.y);
            assert!(rect.position[0] + rect.size[0] <= area.x + area.width + 0.01);
            assert!(rect.position[1] + rect.size[1] <= area.y + area.height + 0.01);
        }
        // At twice the scale, hexagons twice as large: fewer of them.
        assert_eq!(honeycomb.rects(started + WAVE, 2.0, COLOR).len(), 4 * 3);
    }

    #[test]
    fn an_area_too_small_for_a_hexagon_shows_none() {
        let started = Instant::now();
        let small = Rect {
            width: 30.0,
            ..area()
        };
        assert!(
            Honeycomb::new(started, small)
                .rects(started, 1.0, COLOR)
                .is_empty()
        );
    }

    #[test]
    fn hexagons_are_invisible_at_the_start_and_the_end() {
        let started = Instant::now();
        let honeycomb = Honeycomb::new(started, area());
        let alphas = |at: Duration| -> Vec<f32> {
            honeycomb
                .rects(started + at, 1.0, COLOR)
                .iter()
                .map(|rect| rect.color[3])
                .collect()
        };
        assert!(alphas(Duration::ZERO).iter().all(|&a| a == 0.0));
        // Once the wave is over, all shown in full.
        assert!(alphas(WAVE).iter().all(|&a| a == 1.0));
        assert!(alphas(WAVE + HOLD).iter().all(|&a| a == 1.0));
        // Halfway into the wave, the center ones are shown, the far ones not yet.
        let half = alphas(WAVE / 2);
        assert!(half.contains(&1.0));
        assert!(half.contains(&0.0));
        // Fading out together.
        let fading = alphas(WAVE + HOLD + FADE_OUT / 2);
        assert!(fading.iter().all(|&a| (a - 0.5).abs() < 0.01));
        let last = alphas(DURATION - Duration::from_millis(1));
        assert!(last.iter().all(|&a| a < 0.01));
        assert!(alphas(DURATION).is_empty());
    }

    #[test]
    fn deadline_runs_until_the_end() {
        let started = Instant::now();
        let honeycomb = Honeycomb::new(started, area());
        assert_eq!(honeycomb.deadline(started), Some(started + FRAME));
        let late = started + DURATION - Duration::from_millis(1);
        assert_eq!(honeycomb.deadline(late), Some(late + FRAME));
        assert_eq!(honeycomb.deadline(started + DURATION), None);
    }
}
