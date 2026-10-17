//! The breathing driver: a slow opacity sine for live marks and the model
//! glow.

use mosaic::prelude::Element;
use mosaic::widgets::Tick;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// One breathing cycle: opacity runs from `high` to `low` and back over
/// `period` seconds, easing at both ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Breath {
    pub period: f32,
    pub high: f32,
    pub low: f32,
}

impl Breath {
    /// The live status square: 1 → 0.45 over 2.4 s.
    pub const LIVE: Breath = Breath {
        period: 2.4,
        high: 1.0,
        low: 0.45,
    };
    /// The model glow: 0.45 → 0.6 over 3.6 s (dimmest at rest).
    pub const GLOW: Breath = Breath {
        period: 3.6,
        high: 0.45,
        low: 0.6,
    };

    /// Opacity `seconds` into the cycle. A cosine is the ease-in-out of the
    /// design's two-keyframe loop.
    pub fn at(self, seconds: f32) -> f32 {
        let phase = (seconds / self.period).fract();
        let swing = 0.5 - 0.5 * (std::f32::consts::TAU * phase).cos();
        self.high + (self.low - self.high) * swing
    }
}

/// Repaint interval. A 2.4 s breath changes by under 0.04 opacity per step
/// at this rate, which reads as continuous while keeping the frame rate
/// (and CPU) low.
const STEP: Duration = Duration::from_millis(33);

/// Drives `breath` for as long as `element` is in the tree, repainting it on
/// each step. Paint closures read the current opacity from the returned
/// cell. The driver is dropped when the element is removed.
pub fn breathe(element: &Element, breath: Breath) -> Rc<Cell<f32>> {
    let level = Rc::new(Cell::new(breath.high));
    let elapsed = Cell::new(0.0f32);
    let target = element.clone();
    let shared = level.clone();
    element.animate(move |dt| {
        let seconds = (elapsed.get() + dt.as_secs_f32()) % breath.period;
        elapsed.set(seconds);
        shared.set(breath.at(seconds));
        target.paint_dirty();
        Tick::Sleep(STEP)
    });
    level
}

/// [`breathe`], but only while `keep` holds: the driver stops (and the cell
/// returns to `breath.high`) the first step `keep` is false. Call again to
/// restart.
pub fn breathe_while(
    element: &Element,
    breath: Breath,
    level: Rc<Cell<f32>>,
    keep: impl Fn() -> bool + 'static,
) {
    let elapsed = Cell::new(0.0f32);
    let target = element.clone();
    element.animate(move |dt| {
        if !keep() {
            level.set(breath.high);
            target.paint_dirty();
            return Tick::Done;
        }
        let seconds = (elapsed.get() + dt.as_secs_f32()) % breath.period;
        elapsed.set(seconds);
        level.set(breath.at(seconds));
        target.paint_dirty();
        Tick::Sleep(STEP)
    });
}
